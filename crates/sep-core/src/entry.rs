use std::collections::BTreeSet;

use crate::fetch::{FetchError, FetchOptions, FetchResult, Fetcher};
use crate::library::{FileMeta, HOST, Library, LibraryError, PlaceError};
use crate::model::{AttachmentKind, Block, EntryDoc, Inline};
use crate::parse::metadata::is_edition;
use crate::parse::{
    AttachmentOptions, ParseOptions, extract_macros, parse_attachment, parse_entry,
};
use crate::sections::ContentKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryTarget {
    pub slug: String,
    pub edition: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EntryError {
    #[error("not an SEP entry URL: {0}")]
    NotEntryUrl(String),
    #[error("\"{0}\" does not look like an SEP entry slug or URL")]
    BadSlug(String),
    #[error("\"{0}\" does not look like an SEP edition (live, or one such as sum2026)")]
    BadEdition(String),
}

fn segment_after<'a>(path: &'a str, marker: &str) -> Option<&'a str> {
    path.match_indices(marker).find_map(|(at, _)| {
        let rest = &path[at + marker.len()..];
        let end = rest.find('/').unwrap_or(rest.len());
        (end > 0).then(|| &rest[..end])
    })
}

pub fn normalize_entry(input: &str, edition: &str) -> Result<EntryTarget, EntryError> {
    let mut slug = input.trim().to_owned();
    let mut edition = edition.to_owned();
    if edition != "live" && !is_edition(&edition) {
        return Err(EntryError::BadEdition(edition));
    }

    let lower = slug.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let parsed =
            url::Url::parse(&slug).map_err(|_| EntryError::NotEntryUrl(input.to_owned()))?;
        let sep_host = HOST.split_once("://").map_or(HOST, |(_, host)| host);
        if parsed.host_str() != Some(sep_host) || parsed.port().is_some() {
            return Err(EntryError::NotEntryUrl(input.to_owned()));
        }
        let path = parsed.path();
        let found = segment_after(path, "/entries/")
            .ok_or_else(|| EntryError::NotEntryUrl(input.to_owned()))?;
        let archived = segment_after(path, "/archives/")
            .filter(|ed| path.contains(&format!("/archives/{ed}/")));
        if let Some(archived) = archived
            && edition == "live"
        {
            if !is_edition(archived) {
                return Err(EntryError::NotEntryUrl(input.to_owned()));
            }
            edition = archived.to_owned();
        }
        slug = found.to_owned();
    }

    let mut chars = slug.chars();
    let well_formed = matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !well_formed {
        return Err(EntryError::BadSlug(input.to_owned()));
    }

    let url = if edition == "live" {
        format!("{HOST}/entries/{slug}/")
    } else {
        format!("{HOST}/archives/{edition}/entries/{slug}/")
    };
    Ok(EntryTarget { slug, edition, url })
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error(transparent)]
    Place(#[from] PlaceError),
    #[error(transparent)]
    Fetch(#[from] FetchError),
    #[error("{url} is not in the library")]
    NotHeld { url: String },
    #[error("{url} is not in the library at {dir}\n  fetch it first: typesep fetch {slug}")]
    NotInLibrary {
        url: String,
        dir: String,
        slug: String,
    },
    #[error("HTTP 404 for {url} (remembered{when}; --refresh asks the server again)")]
    Remembered { url: String, when: String },
    #[error("server answered 304 for {url} but the library has no copy")]
    NotModifiedWithoutCopy { url: String },
}

pub struct LoadOptions<'a> {
    pub content: &'a BTreeSet<ContentKey>,
    pub log: &'a mut dyn FnMut(&str),
}

pub struct LoadedPage {
    pub text: String,
    pub meta: Option<FileMeta>,
    pub from_cache: bool,
    pub revalidated: bool,
}

pub struct Source<'a> {
    pub library: &'a Library,
    pub fetcher: Option<&'a mut Fetcher>,
    pub refresh: bool,
    pub check_updates: bool,
}

impl<'a> Source<'a> {
    pub fn offline(library: &'a Library) -> Self {
        Self {
            library,
            fetcher: None,
            refresh: false,
            check_updates: false,
        }
    }

    pub fn fetching(library: &'a Library, fetcher: &'a mut Fetcher) -> Self {
        Self {
            library,
            fetcher: Some(fetcher),
            refresh: false,
            check_updates: false,
        }
    }

    pub fn with_refresh(mut self, refresh: bool) -> Self {
        self.refresh = refresh;
        self
    }

    pub fn with_check_updates(mut self, check: bool) -> Self {
        self.check_updates = check;
        self
    }

    pub fn writes(&self) -> bool {
        self.fetcher.is_some()
    }

    pub fn page(&mut self, url: &str, refresh: bool) -> Result<LoadedPage, LoadError> {
        let meta = self.library.read_meta(url)?;
        let mut cached = self.library.read_body(url)?;

        if !refresh && let Some(text) = cached.take() {
            return Ok(LoadedPage {
                text,
                meta,
                from_cache: true,
                revalidated: false,
            });
        }
        if cached.is_none() && meta.as_ref().is_some_and(|m| m.missing) && !refresh {
            let when = meta
                .as_ref()
                .and_then(|m| m.fetched_at.as_deref())
                .map(|stamp| format!(" on {}", stamp.chars().take(10).collect::<String>()))
                .unwrap_or_default();
            return Err(LoadError::Remembered {
                url: url.to_owned(),
                when,
            });
        }

        let Some(fetcher) = self.fetcher.as_deref_mut() else {
            return Err(LoadError::NotHeld {
                url: url.to_owned(),
            });
        };
        let conditional = if refresh {
            FetchOptions {
                etag: meta.as_ref().and_then(|m| m.etag.as_deref()),
                last_modified: meta.as_ref().and_then(|m| m.last_modified.as_deref()),
            }
        } else {
            FetchOptions::default()
        };
        let answer = fetcher.get(url, conditional);
        let fetched = match answer {
            Ok(result) => result,
            Err(err) => {
                if err.is_gone() {
                    self.library.mark_missing(url)?;
                }
                return Err(err.into());
            }
        };

        match fetched {
            FetchResult::NotModified => match cached {
                Some(text) => Ok(LoadedPage {
                    text,
                    meta,
                    from_cache: true,
                    revalidated: true,
                }),
                None => Err(LoadError::NotModifiedWithoutCopy {
                    url: url.to_owned(),
                }),
            },
            FetchResult::Fetched(res) => {
                self.library.write(
                    url,
                    &res.body,
                    res.etag.as_deref(),
                    res.last_modified.as_deref(),
                )?;
                Ok(LoadedPage {
                    text: String::from_utf8_lossy(&res.body).into_owned(),
                    meta: self.library.read_meta(url)?,
                    from_cache: false,
                    revalidated: false,
                })
            }
        }
    }
}

fn macros_url(page_url: &str) -> Option<String> {
    url::Url::parse(page_url)
        .ok()?
        .join("local.js")
        .ok()
        .map(|u| u.to_string())
}

#[derive(Debug)]
pub struct LoadedEntry {
    pub doc: EntryDoc,
    pub from_cache: bool,
    pub previous_revision: Option<String>,
}

pub fn load_entry(
    source: &mut Source<'_>,
    target: &EntryTarget,
    opts: LoadOptions<'_>,
) -> Result<LoadedEntry, LoadError> {
    let previous_revision = source
        .library
        .entry_meta(&target.url)?
        .map(|record| record.revision);

    let refresh_page = source.refresh || source.check_updates;
    let page = source
        .page(&target.url, refresh_page)
        .map_err(|err| match err {
            LoadError::NotHeld { url } => LoadError::NotInLibrary {
                url,
                dir: source.library.dir().display().to_string(),
                slug: target.slug.clone(),
            },
            other => other,
        })?;
    let fetched_at: String = page
        .meta
        .as_ref()
        .and_then(|meta| meta.fetched_at.as_deref())
        .map(|stamp| stamp.chars().take(10).collect())
        .unwrap_or_default();

    let mut doc = parse_entry(
        &page.text,
        &ParseOptions {
            source_url: &target.url,
            slug: &target.slug,
            edition: if target.edition == "live" {
                ""
            } else {
                &target.edition
            },
            fetched_at: &fetched_at,
        },
    );
    if source.writes() {
        source
            .library
            .record_entry(&target.url, &doc.meta.title, &doc.meta.revision)?;
    }

    let mut macro_sources: BTreeSet<String> = BTreeSet::new();
    let mut add_macros =
        |source: &mut Source<'_>, doc: &mut EntryDoc, page_url: &str, log: &mut dyn FnMut(&str)| {
            let Some(url) = macros_url(page_url) else {
                return;
            };
            if !macro_sources.insert(url.clone()) {
                return;
            }
            match source.page(&url, source.refresh) {
                Ok(page) => doc.meta.macros.extend(extract_macros(&page.text)),
                Err(LoadError::NotHeld { .. } | LoadError::Remembered { .. }) => {}
                Err(LoadError::Fetch(err)) if err.is_gone() => {}
                Err(err) => log(&format!("skipping {url}: {err}")),
            }
        };
    add_macros(source, &mut doc, &target.url, opts.log);

    let wanted = [
        (
            ContentKey::Notes,
            AttachmentKind::Notes,
            doc.links.notes.clone(),
        ),
        (
            ContentKey::Supplements,
            AttachmentKind::Supplement,
            doc.links.supplements.clone(),
        ),
    ];
    for (key, kind, urls) in wanted {
        if !opts.content.contains(&key) {
            continue;
        }
        for url in urls {
            match source.page(&url, source.refresh) {
                Ok(attached) => {
                    doc.attachments.push(parse_attachment(
                        &attached.text,
                        &AttachmentOptions {
                            source_url: &url,
                            kind,
                        },
                    ));
                    add_macros(source, &mut doc, &url, opts.log);
                }
                Err(err) => (opts.log)(&format!("skipping {url}: {err}")),
            }
        }
    }

    Ok(LoadedEntry {
        doc,
        from_cache: page.from_cache,
        previous_revision,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    New,
    Revised { previous: String },
    Unchanged,
}

impl UpdateStatus {
    pub fn of(entry: &LoadedEntry) -> Self {
        match &entry.previous_revision {
            None => UpdateStatus::New,
            Some(previous) if *previous != entry.doc.meta.revision => UpdateStatus::Revised {
                previous: previous.clone(),
            },
            Some(_) => UpdateStatus::Unchanged,
        }
    }

    pub fn changed(&self) -> bool {
        !matches!(self, UpdateStatus::Unchanged)
    }
}

impl std::fmt::Display for UpdateStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdateStatus::New => write!(f, "new"),
            UpdateStatus::Revised { previous } => write!(f, "revised (was {previous})"),
            UpdateStatus::Unchanged => write!(f, "unchanged"),
        }
    }
}

pub fn related_entry_urls(doc: &EntryDoc) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for block in doc.sections.related_entries.iter().flatten() {
        links_in_block(block, &mut |href| {
            if href.contains("/entries/") && seen.insert(href.to_owned()) {
                out.push(href.to_owned());
            }
        });
    }
    out
}

fn links_in_block(block: &Block, found: &mut impl FnMut(&str)) {
    match block {
        Block::Para { children, .. }
        | Block::Pre { children }
        | Block::Heading { children, .. } => links_in_inlines(children, found),
        Block::List(list) => {
            for item in &list.items {
                for block in item {
                    links_in_block(block, found);
                }
            }
            for label in list.labels.iter().flatten() {
                links_in_inlines(label, found);
            }
        }
        Block::Blockquote { children } => {
            for block in children {
                links_in_block(block, found);
            }
        }
        Block::Table(table) => {
            for caption in table.caption.iter() {
                links_in_inlines(caption, found);
            }
            for row in &table.rows {
                for cell in row {
                    links_in_inlines(&cell.children, found);
                }
            }
        }
        Block::Figure(figure) => {
            for caption in figure.caption.iter() {
                links_in_inlines(caption, found);
            }
        }
        Block::Rule | Block::Anchor { .. } => {}
    }
}

fn links_in_inlines(nodes: &[Inline], found: &mut impl FnMut(&str)) {
    for node in nodes {
        match node {
            Inline::Link {
                href,
                internal,
                children,
            } => {
                if !internal {
                    found(href);
                }
                links_in_inlines(children, found);
            }
            Inline::Emph { children }
            | Inline::Strong { children }
            | Inline::Sub { children }
            | Inline::Sup { children }
            | Inline::Code { children } => links_in_inlines(children, found),
            Inline::Figure(figure) => {
                for caption in figure.caption.iter() {
                    links_in_inlines(caption, found);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::ALL_CONTENT_KEYS;

    fn target(slug: &str, edition: &str, url: &str) -> EntryTarget {
        EntryTarget {
            slug: slug.into(),
            edition: edition.into(),
            url: url.into(),
        }
    }

    #[test]
    fn slugs_and_urls_resolve_to_the_same_target() {
        let tm = "https://plato.stanford.edu/entries/turing-machine/";
        assert_eq!(
            normalize_entry("turing-machine", "live").unwrap(),
            target("turing-machine", "live", tm)
        );
        assert_eq!(
            normalize_entry("  turing-machine ", "live").unwrap(),
            target("turing-machine", "live", tm)
        );
        assert_eq!(
            normalize_entry(tm, "live").unwrap(),
            target("turing-machine", "live", tm)
        );
        assert_eq!(
            normalize_entry(
                "https://plato.stanford.edu/entries/turing-machine/notes.html#3",
                "live"
            )
            .unwrap(),
            target("turing-machine", "live", tm)
        );
        assert_eq!(
            normalize_entry("HTTP://plato.stanford.edu/entries/turing-machine", "live").unwrap(),
            target("turing-machine", "live", tm),
            "the scheme test is case-insensitive"
        );
        assert_eq!(
            normalize_entry("turing-machine", "sum2026").unwrap(),
            target(
                "turing-machine",
                "sum2026",
                "https://plato.stanford.edu/archives/sum2026/entries/turing-machine/"
            )
        );
        assert_eq!(
            normalize_entry(
                "https://plato.stanford.edu/archives/win2020/entries/turing-machine/",
                "live"
            )
            .unwrap(),
            target(
                "turing-machine",
                "win2020",
                "https://plato.stanford.edu/archives/win2020/entries/turing-machine/"
            ),
            "an archive URL names its edition"
        );
        assert_eq!(
            normalize_entry(
                "https://plato.stanford.edu/archives/win2020/entries/turing-machine/",
                "sum2026"
            )
            .unwrap()
            .edition,
            "sum2026",
            "an edition given explicitly wins"
        );
        assert_eq!(
            normalize_entry("Turing-Machine", "live").unwrap().slug,
            "Turing-Machine",
            "case is kept; the check is case-insensitive"
        );

        assert_eq!(
            normalize_entry("https://plato.stanford.edu/contents.html", "live"),
            Err(EntryError::NotEntryUrl(
                "https://plato.stanford.edu/contents.html".into()
            ))
        );
        assert_eq!(
            normalize_entry("https://", "live"),
            Err(EntryError::NotEntryUrl("https://".into()))
        );
        assert_eq!(
            normalize_entry("-x", "live"),
            Err(EntryError::BadSlug("-x".into()))
        );
        assert_eq!(
            normalize_entry("a b", "live"),
            Err(EntryError::BadSlug("a b".into()))
        );
        assert_eq!(
            normalize_entry("", "live"),
            Err(EntryError::BadSlug("".into()))
        );
        assert_eq!(
            normalize_entry("turing-machine", "sum2026/../../entries/other"),
            Err(EntryError::BadEdition("sum2026/../../entries/other".into()))
        );
        assert_eq!(
            normalize_entry("turing-machine", "summer"),
            Err(EntryError::BadEdition("summer".into()))
        );
        for other in [
            "https://example.com/entries/turing-machine/",
            "https://plato.stanford.edu.example.com/entries/turing-machine/",
            "https://plato.stanford.edu:8443/entries/turing-machine/",
            "https://plato.stanford.edu/archives/x/entries/turing-machine/",
        ] {
            assert_eq!(
                normalize_entry(other, "live"),
                Err(EntryError::NotEntryUrl(other.into()))
            );
        }
    }

    const TM: &str = "https://plato.stanford.edu/entries/turing-machine/";

    const PAGE: &str = "<h1>Turing Machines</h1><div id=\"pubinfo\">First published Mon Sep 24, 2018; \
substantive revision Wed May 21, 2025</div><div id=\"main-text\"><p>See\
<sup><a href=\"notes.html#note-1\">[1]</a></sup>.</p></div>";

    fn answers() -> Vec<Result<crate::fetch::RawResponse, String>> {
        vec![
            Ok(crate::fetch::testing::ok(TM, PAGE)),
            Ok(crate::fetch::testing::status(404, "Not Found")),
            Ok(crate::fetch::testing::ok(
                &format!("{TM}notes.html"),
                "<div id=\"aueditable\"><h1>Notes to Turing Machines</h1><div id=\"note-1\"><p>[1] A note.</p></div></div>",
            )),
        ]
    }

    #[test]
    fn an_entry_the_library_lacks_is_fetched_and_kept() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        library.init().unwrap();
        let target = normalize_entry("turing-machine", "live").unwrap();
        let all: BTreeSet<ContentKey> = ALL_CONTENT_KEYS.into_iter().collect();
        let mut harness = crate::fetch::testing::Harness::new(answers());

        let loaded = {
            let mut source = Source::fetching(&library, &mut harness.fetcher);
            load_entry(
                &mut source,
                &target,
                LoadOptions {
                    content: &all,
                    log: &mut |_| {},
                },
            )
            .unwrap()
        };
        assert_eq!(loaded.doc.meta.title, "Turing Machines");
        assert_eq!(loaded.doc.attachments.len(), 1, "the notes page came too");
        assert!(!loaded.from_cache);
        assert_eq!(loaded.previous_revision, None, "the entry is new here");
        assert_eq!(UpdateStatus::of(&loaded), UpdateStatus::New);

        assert_eq!(library.read_body(TM).unwrap().as_deref(), Some(PAGE));
        assert!(library.has(&format!("{TM}notes.html")).unwrap());
        assert!(
            library.is_missing(&format!("{TM}local.js")).unwrap(),
            "a 404 is remembered, so it costs one request in all"
        );
        let record = library.entry_meta(TM).unwrap().expect("a recorded entry");
        assert_eq!(record.revision, "Wed May 21, 2025");
        assert_eq!(record.title.as_deref(), Some("Turing Machines"));
        assert_eq!(
            harness.urls(),
            [
                TM.to_owned(),
                format!("{TM}local.js"),
                format!("{TM}notes.html")
            ]
        );

        let before = harness.urls().len();
        let again = {
            let mut source = Source::fetching(&library, &mut harness.fetcher);
            load_entry(
                &mut source,
                &target,
                LoadOptions {
                    content: &all,
                    log: &mut |_| {},
                },
            )
            .unwrap()
        };
        assert!(again.from_cache);
        assert_eq!(harness.urls().len(), before, "no request was made");
        assert_eq!(
            again.previous_revision.as_deref(),
            Some("Wed May 21, 2025"),
            "what the first load recorded"
        );
        assert_eq!(UpdateStatus::of(&again), UpdateStatus::Unchanged);
    }

    #[test]
    fn a_macro_file_that_cannot_be_fetched_is_said_and_one_that_is_absent_is_not() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        library.init().unwrap();
        let target = normalize_entry("turing-machine", "live").unwrap();
        let none: BTreeSet<ContentKey> = BTreeSet::new();
        let mut logged: Vec<String> = Vec::new();

        let mut harness = crate::fetch::testing::Harness::new(vec![
            Ok(crate::fetch::testing::ok(TM, PAGE)),
            Err("timed out".into()),
        ]);
        {
            let mut source = Source::fetching(&library, &mut harness.fetcher);
            load_entry(
                &mut source,
                &target,
                LoadOptions {
                    content: &none,
                    log: &mut |m| logged.push(m.to_owned()),
                },
            )
            .unwrap();
        }
        assert_eq!(logged.len(), 1, "{logged:?}");
        assert!(
            logged[0].starts_with(&format!("skipping {TM}local.js: could not fetch")),
            "{}",
            logged[0]
        );

        logged.clear();
        library.mark_missing(&format!("{TM}local.js")).unwrap();
        load_entry(
            &mut Source::offline(&library),
            &target,
            LoadOptions {
                content: &none,
                log: &mut |m| logged.push(m.to_owned()),
            },
        )
        .unwrap();
        assert!(
            logged.is_empty(),
            "most entries have no local.js: {logged:?}"
        );
    }

    #[test]
    fn a_revalidated_page_keeps_its_copy_and_a_revised_one_replaces_it() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        library.init().unwrap();
        library
            .write(TM, PAGE.as_bytes(), Some("\"v1\""), None)
            .unwrap();
        library
            .record_entry(TM, "Turing Machines", "Wed May 21, 2025")
            .unwrap();
        let target = normalize_entry("turing-machine", "live").unwrap();
        let none: BTreeSet<ContentKey> = BTreeSet::new();

        let mut harness = crate::fetch::testing::Harness::new(vec![Ok(
            crate::fetch::testing::status(304, "Not Modified"),
        )]);
        let loaded = {
            let mut source =
                Source::fetching(&library, &mut harness.fetcher).with_check_updates(true);
            load_entry(
                &mut source,
                &target,
                LoadOptions {
                    content: &none,
                    log: &mut |_| {},
                },
            )
            .unwrap()
        };
        assert!(loaded.from_cache);
        assert_eq!(UpdateStatus::of(&loaded), UpdateStatus::Unchanged);
        assert_eq!(
            harness.asked.borrow()[0].header("if-none-match"),
            Some("\"v1\"")
        );

        let revised = PAGE.replace("Wed May 21, 2025", "Thu Sep 10, 2026");
        let mut harness =
            crate::fetch::testing::Harness::new(vec![Ok(crate::fetch::testing::ok(TM, &revised))]);
        let loaded = {
            let mut source =
                Source::fetching(&library, &mut harness.fetcher).with_check_updates(true);
            load_entry(
                &mut source,
                &target,
                LoadOptions {
                    content: &none,
                    log: &mut |_| {},
                },
            )
            .unwrap()
        };
        assert_eq!(
            UpdateStatus::of(&loaded),
            UpdateStatus::Revised {
                previous: "Wed May 21, 2025".into()
            }
        );
        assert_eq!(
            UpdateStatus::of(&loaded).to_string(),
            "revised (was Wed May 21, 2025)"
        );
        assert_eq!(library.read_body(TM).unwrap().as_deref(), Some(&*revised));
        let record = library.entry_meta(TM).unwrap().unwrap();
        assert_eq!(record.revision, "Thu Sep 10, 2026");
        assert_eq!(
            record.previous_revision.as_deref(),
            Some("Wed May 21, 2025")
        );
    }

    #[test]
    fn a_source_that_does_not_fetch_makes_no_request_and_writes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        library.init().unwrap();
        library.write(TM, PAGE.as_bytes(), None, None).unwrap();
        let target = normalize_entry("turing-machine", "live").unwrap();
        let all: BTreeSet<ContentKey> = ALL_CONTENT_KEYS.into_iter().collect();
        let mut skipped: Vec<String> = Vec::new();

        let loaded = load_entry(
            &mut Source::offline(&library),
            &target,
            LoadOptions {
                content: &all,
                log: &mut |m| skipped.push(m.to_owned()),
            },
        )
        .unwrap();
        assert_eq!(loaded.doc.attachments.len(), 0);
        assert_eq!(
            skipped,
            [format!(
                "skipping {TM}notes.html: {TM}notes.html is not in the library"
            )]
        );
        assert!(
            library.entry_meta(TM).unwrap().is_none(),
            "a dump reads the library and leaves it as it found it"
        );
    }

    #[test]
    fn related_entries_are_where_follow_related_gets_its_queue() {
        let html = format!(
            "<h1>T</h1><div id=\"main-text\"><p>x</p></div>\
             <div id=\"related-entries\"><p><a href=\"{TM}\">Turing machines</a>, \
             <a href=\"../computability/\">computability</a>, \
             <a href=\"#sec-1\">above</a>, \
             <a href=\"https://example.com/\">elsewhere</a>, \
             <a href=\"{TM}\">again</a></p></div>"
        );
        let doc = crate::parse::parse_entry(
            &html,
            &ParseOptions {
                source_url: "https://plato.stanford.edu/entries/computer-science/",
                slug: "computer-science",
                edition: "",
                fetched_at: "",
            },
        );
        assert_eq!(
            related_entry_urls(&doc),
            [
                TM.to_owned(),
                "https://plato.stanford.edu/entries/computability/".to_owned()
            ],
            "entries only, each once, and never a fragment or another site"
        );
    }

    #[test]
    fn figures_are_fetched_with_the_entry_and_a_missing_one_is_remembered() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        library.init().unwrap();
        let mut doc = EntryDoc::default();
        doc.body
            .push(crate::model::Block::Figure(crate::model::Figure {
                src: format!("{TM}S1.svg"),
                alt: String::new(),
                caption: None,
                id: None,
                inline: None,
            }));
        doc.body
            .push(crate::model::Block::Figure(crate::model::Figure {
                src: format!("{TM}gone.png"),
                alt: String::new(),
                caption: None,
                id: None,
                inline: None,
            }));

        let mut harness = crate::fetch::testing::Harness::new(vec![
            Ok(crate::fetch::testing::ok(&format!("{TM}S1.svg"), "<svg/>")),
            Ok(crate::fetch::testing::status(404, "Not Found")),
        ]);
        let pass = {
            let mut source = Source::fetching(&library, &mut harness.fetcher);
            crate::figures::fetch_figures(&doc, &mut source, &mut |_| {})
        };
        assert_eq!(pass.present, [format!("{TM}S1.svg")]);
        assert_eq!(pass.fetched, 1);
        assert_eq!(pass.missing, [format!("{TM}gone.png")]);
        assert!(pass.failed.is_empty());
        assert!(library.has(&format!("{TM}S1.svg")).unwrap());
        assert!(library.is_missing(&format!("{TM}gone.png")).unwrap());

        let before = harness.urls().len();
        let pass = {
            let mut source = Source::fetching(&library, &mut harness.fetcher);
            crate::figures::fetch_figures(&doc, &mut source, &mut |_| {})
        };
        assert_eq!(harness.urls().len(), before, "no request was made");
        assert_eq!(pass.fetched, 0);
        assert_eq!(pass.present.len(), 1);
        assert_eq!(pass.missing.len(), 1);
    }

    #[test]
    fn loading_parses_the_page_its_macros_and_the_attachments_asked_for() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        let target = normalize_entry("turing-machine", "live").unwrap();
        let all: BTreeSet<ContentKey> = ALL_CONTENT_KEYS.into_iter().collect();
        let mut logged: Vec<String> = Vec::new();
        let err = load_entry(
            &mut Source::offline(&library),
            &target,
            LoadOptions {
                content: &all,
                log: &mut |m| logged.push(m.to_owned()),
            },
        )
        .unwrap_err();
        assert!(matches!(err, LoadError::NotInLibrary { .. }));
        assert!(err.to_string().contains("typesep fetch turing-machine"));

        let unit = temp.path().join("entries/turing-machine");
        std::fs::create_dir_all(&unit).unwrap();
        std::fs::write(
            unit.join("index.html"),
            "<h1>Turing Machines</h1><div id=\"pubinfo\">First published Mon Sep 24, 2018; substantive revision Wed May 21, 2025</div>\
             <div id=\"main-text\"><p>See<sup><a href=\"notes.html#note-1\">[1]</a></sup> and <a href=\"figdesc.html\">figures</a> \\(\\opL\\).</p></div>",
        )
        .unwrap();
        std::fs::write(
            unit.join("local.js"),
            "MathJax.Hub.Config({ TeX: { Macros: { opL: \"\\\\mathbin{L}\" } } });",
        )
        .unwrap();
        std::fs::write(
            unit.join("notes.html"),
            "<div id=\"aueditable\"><h1>Notes to Turing Machines</h1><div id=\"note-1\"><p>[1] A note.</p></div></div>",
        )
        .unwrap();
        std::fs::write(
            unit.join("meta.json"),
            r#"{"url":"https://plato.stanford.edu/entries/turing-machine/","files":{"index.html":{"fetchedAt":"2026-09-03T07:21:46.210Z"},"figdesc.html":{"fetchedAt":"2026-09-04T00:00:00.000Z","missing":true}}}"#,
        )
        .unwrap();

        let doc = load_entry(
            &mut Source::offline(&library),
            &target,
            LoadOptions {
                content: &all,
                log: &mut |m| logged.push(m.to_owned()),
            },
        )
        .unwrap()
        .doc;
        assert_eq!(doc.meta.slug, "turing-machine");
        assert_eq!(doc.meta.title, "Turing Machines");
        assert_eq!(doc.meta.revision, "Wed May 21, 2025");
        assert_eq!(doc.meta.source_url, target.url);
        assert_eq!(doc.meta.edition, "");
        assert_eq!(doc.meta.fetched_at, "2026-09-03");
        assert_eq!(doc.meta.macros.len(), 1);
        assert_eq!(doc.meta.macros["opL"].body, "\\mathbin{L}");
        assert_eq!(doc.body.len(), 1);
        assert_eq!(
            doc.attachments.len(),
            1,
            "the notes page is there, the supplement is not"
        );
        assert_eq!(doc.attachments[0].kind, AttachmentKind::Notes);
        assert_eq!(doc.attachments[0].title, "Notes to Turing Machines");
        assert!(
            doc.attachments[0]
                .notes
                .as_ref()
                .unwrap()
                .contains_key("note-1")
        );
        assert_eq!(
            logged,
            [
                "skipping https://plato.stanford.edu/entries/turing-machine/figdesc.html: HTTP 404 for https://plato.stanford.edu/entries/turing-machine/figdesc.html (remembered on 2026-09-04; --refresh asks the server again)"
            ]
        );

        let none = BTreeSet::new();
        let doc = load_entry(
            &mut Source::offline(&library),
            &target,
            LoadOptions {
                content: &none,
                log: &mut |_| panic!("nothing to skip"),
            },
        )
        .unwrap()
        .doc;
        assert!(
            doc.attachments.is_empty(),
            "--sections none loads no attachment"
        );
        assert_eq!(doc.links.notes.len(), 1, "the links are still discovered");

        let archived = normalize_entry("turing-machine", "sum2026").unwrap();
        let unit = temp.path().join("archives/sum2026/entries/turing-machine");
        std::fs::create_dir_all(&unit).unwrap();
        std::fs::write(unit.join("index.html"), "<html/>").unwrap();
        let doc = load_entry(
            &mut Source::offline(&library),
            &archived,
            LoadOptions {
                content: &all,
                log: &mut |_| {},
            },
        )
        .unwrap()
        .doc;
        assert_eq!(doc.meta.edition, "sum2026");
        assert_eq!(doc.meta.edition_label, "Summer 2026 Edition");
        assert_eq!(doc.meta.fetched_at, "", "no meta.json, no date");
        assert!(doc.body.is_empty());
    }
}
