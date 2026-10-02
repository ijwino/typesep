use std::collections::BTreeSet;

use typesep_model::{Doc, Meta, Notice, Rights};

use crate::library::{Place, PlaceError, Store};
use crate::net::{LoadError, Net};

/// A work a source can load, as `Source::resolve` names it from what the user typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkRef {
    /// The key of the source it belongs to.
    pub source: &'static str,
    /// What the source calls it, and what the output file is named after.
    pub id: String,
    /// The fixed edition asked for, or empty for the one the source serves now.
    pub edition: String,
    /// Where the source publishes it, and what the library holds it under.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ResolveError(pub String);

/// A part of a work that `--sections` can name: a section of its back matter, or what it links
/// to and the loader fetches with it, such as notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionSpec {
    /// The name the renderers and `links.sections` know it by.
    pub key: &'static str,
    /// The heading the source gives it, or empty when it is not a section of the page.
    pub label: &'static str,
    /// The spellings `--sections` accepts for it, the key first.
    pub aliases: &'static [&'static str],
    /// Whether `--sections` selects it when not given.
    pub default: bool,
    /// Whether `links.sections` names it when the style does not set it, so that
    /// `links.mode` sets its links as notes.
    pub linked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown section \"{token}\"\n  known: {known}")]
pub struct SectionError {
    pub token: String,
    known: String,
}

/// The keys a `--sections` value selects: `all`, `none` (or nothing), or a comma list of the
/// aliases of `specs`, in any case.
pub fn parse_sections(specs: &[SectionSpec], spec: &str) -> Result<BTreeSet<String>, SectionError> {
    let raw = spec.trim().to_ascii_lowercase();
    if raw == "all" {
        return Ok(specs.iter().map(|s| s.key.to_owned()).collect());
    }
    if raw == "none" || raw.is_empty() {
        return Ok(BTreeSet::new());
    }
    let mut out = BTreeSet::new();
    for token in raw.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        match specs.iter().find(|s| s.aliases.contains(&token)) {
            Some(found) => {
                out.insert(found.key.to_owned());
            }
            None => {
                let known = specs
                    .iter()
                    .flat_map(|s| s.aliases.iter().copied())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(SectionError {
                    token: token.to_owned(),
                    known,
                });
            }
        }
    }
    Ok(out)
}

/// The `--sections` value that selects what `specs` selects by default.
pub fn default_sections(specs: &[SectionSpec]) -> String {
    specs
        .iter()
        .filter(|s| s.default)
        .map(|s| s.key)
        .collect::<Vec<_>>()
        .join(",")
}

pub struct LoadOptions<'a> {
    /// The keys of the sections asked for, which decide what is fetched besides the page.
    pub sections: &'a BTreeSet<String>,
    pub log: &'a mut dyn FnMut(&str),
}

#[derive(Debug)]
pub struct Loaded {
    pub doc: Doc,
    pub from_cache: bool,
    /// The revision the library had recorded before this load, if any.
    pub previous_revision: Option<String>,
}

/// A work the library holds, as `list` and `update` show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub id: String,
    /// Empty for the edition the source serves now.
    pub edition: String,
    pub url: String,
    pub revision: String,
    pub title: String,
    pub fetched_at: Option<String>,
}

pub trait Source: Sync {
    /// The source's name, as the library's directory and `--source` know it.
    fn key(&self) -> &'static str;
    /// Whether `input` is an address on this source's site.
    fn claims(&self, input: &str) -> bool;
    /// The work `input` names, in `edition` when one is given.
    fn resolve(&self, input: &str, edition: Option<&str>) -> Result<WorkRef, ResolveError>;
    /// Where the library keeps what `url` names.
    fn place(&self, url: &str) -> Result<Place, PlaceError>;
    /// The work parsed, with what it needs fetched or read from the library.
    fn load(
        &self,
        work: &WorkRef,
        net: &mut Net<'_>,
        opts: LoadOptions<'_>,
    ) -> Result<Loaded, LoadError>;
    /// What `--sections` can name.
    fn sections(&self) -> &'static [SectionSpec];
    /// The works the library holds.
    fn held(&self, store: &Store<'_>) -> Vec<Held>;
    /// What `--follow-related` follows from `doc`: inputs for `resolve`, taken in the edition
    /// the command line names, as the works it was given are.
    fn related(&self, _doc: &Doc) -> Vec<String> {
        Vec::new()
    }
    /// The preset a work of this source is typeset with when none is named.
    fn preset(&self) -> &'static str {
        ""
    }
    /// The metadata of `docs`, works of this source, typeset as one volume titled `title`.
    fn volume(&self, docs: &[&Doc], title: &str) -> Meta {
        volume_meta(docs, title)
    }
}

/// The metadata of `docs` typeset as one volume titled `title`, in words of no source in
/// particular: the first work's language and source; the byline, publisher and rights the
/// works share, and where they differ no byline or publisher and each work's rights under its
/// title; everyone's contributors and macros; no dateline or address of its own; and for a
/// cover with no byline the credit "N works".
pub fn volume_meta(docs: &[&Doc], title: &str) -> Meta {
    let mut meta = docs[0].meta.clone();
    meta.title = title.to_owned();
    let shared = |pick: fn(&Meta) -> &String| {
        let first = pick(&docs[0].meta);
        if docs.iter().all(|doc| pick(&doc.meta) == first) {
            first.clone()
        } else {
            String::new()
        }
    };
    meta.byline = shared(|meta| &meta.byline);
    meta.origin.publisher = shared(|meta| &meta.origin.publisher);
    meta.dateline = String::new();
    meta.origin.url = String::new();
    meta.rights = volume_rights(docs);
    meta.credit = if docs.len() > 1 {
        format!("{} works", docs.len())
    } else {
        String::new()
    };
    meta.contributors.clear();
    meta.macros.clear();
    for doc in docs {
        for contributor in &doc.meta.contributors {
            if !meta.contributors.contains(contributor) {
                meta.contributors.push(contributor.clone());
            }
        }
        meta.macros
            .extend(doc.meta.macros.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    meta
}

/// The rights of works typeset together: theirs, when they are all the same; else each work's
/// statement and terms under its title in the terms, and its notices under its title too, so
/// that no work is said to be under rights it is not under.
pub fn volume_rights(docs: &[&Doc]) -> Rights {
    let first = &docs[0].meta.rights;
    if docs.iter().all(|doc| doc.meta.rights == *first) {
        return first.clone();
    }
    let mut terms: Vec<String> = Vec::new();
    let mut notices: Vec<Notice> = Vec::new();
    for doc in docs {
        let rights = &doc.meta.rights;
        let own = [rights.statement.as_str(), rights.terms.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !own.is_empty() {
            terms.push(format!("{}: {own}", doc.meta.title));
        }
        notices.extend(rights.notices.iter().map(|notice| Notice {
            text: format!("{}: {}", doc.meta.title, notice.text),
            link: notice.link.clone(),
        }));
    }
    Rights {
        statement: String::new(),
        terms: terms.join(" "),
        notices,
    }
}

/// The sources typesep knows, the first being the one a name that is not an address goes to.
pub struct Registry {
    sources: Vec<&'static dyn Source>,
}

impl Registry {
    pub fn new(sources: Vec<&'static dyn Source>) -> Self {
        assert!(!sources.is_empty(), "a registry has a source");
        Self { sources }
    }

    pub fn iter(&self) -> impl Iterator<Item = &'static dyn Source> + '_ {
        self.sources.iter().copied()
    }

    pub fn get(&self, key: &str) -> Option<&'static dyn Source> {
        self.iter().find(|source| source.key() == key)
    }

    /// The source a bare name, or an address no source claims, goes to unless the user names
    /// another.
    pub fn default_source(&self) -> &'static dyn Source {
        self.sources[0]
    }

    /// The source whose site `input` is on, else `bare`, the one the user has a name that is
    /// not an address looked up in, which then says why it cannot read it.
    pub fn for_input(&self, input: &str, bare: &'static dyn Source) -> &'static dyn Source {
        self.iter()
            .find(|source| source.claims(input))
            .unwrap_or(bare)
    }

    /// The keys of what `--sections` can name in any source, each once, in the order the
    /// sources list them.
    pub fn section_keys(&self) -> Vec<&'static str> {
        self.keys_where(|_| true)
    }

    /// The keys of those that `links.sections` names when the style does not set it.
    pub fn linked_section_keys(&self) -> Vec<&'static str> {
        self.keys_where(|spec| spec.linked)
    }

    fn keys_where(&self, keep: impl Fn(&SectionSpec) -> bool) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for spec in self.iter().flat_map(|source| source.sections()) {
            if keep(spec) && !out.contains(&spec.key) {
                out.push(spec.key);
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    New,
    Revised { previous: String },
    Unchanged,
}

impl UpdateStatus {
    pub fn of(loaded: &Loaded) -> Self {
        match &loaded.previous_revision {
            None => UpdateStatus::New,
            Some(previous) if *previous != loaded.doc.meta.origin.revision => {
                UpdateStatus::Revised {
                    previous: previous.clone(),
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    const SPECS: [SectionSpec; 3] = [
        SectionSpec {
            key: "bib",
            label: "Bibliography",
            aliases: &["bib", "bibliography"],
            default: true,
            linked: false,
        },
        SectionSpec {
            key: "notes",
            label: "",
            aliases: &["notes"],
            default: true,
            linked: false,
        },
        SectionSpec {
            key: "supplements",
            label: "",
            aliases: &["supplements", "supplement"],
            default: false,
            linked: false,
        },
    ];

    fn work(id: &str, byline: &str, terms: &str, notice: &str) -> Doc {
        let mut doc = Doc::default();
        doc.meta.id = id.into();
        doc.meta.title = format!("Title {id}");
        doc.meta.byline = byline.into();
        doc.meta.lang = "de".into();
        doc.meta.origin.source = "x".into();
        doc.meta.origin.publisher = "A Press".into();
        doc.meta.origin.url = format!("https://example.org/{id}");
        doc.meta.dateline = "once".into();
        doc.meta.contributors = vec![typesep_model::Contributor {
            name: byline.into(),
            role: "author".into(),
        }];
        doc.meta.rights = Rights {
            statement: String::new(),
            terms: terms.into(),
            notices: vec![Notice {
                text: notice.into(),
                link: Some(typesep_model::NoticeLink {
                    url: "https://example.org/l".into(),
                    label: "example.org/l".into(),
                }),
            }],
        };
        doc
    }

    #[test]
    fn a_volume_keeps_what_its_works_share_and_each_works_rights_where_they_differ() {
        let (a, b) = (
            work("a", "A. Author", "Free to all.", "See the licence."),
            work("b", "A. Author", "Free to all.", "See the licence."),
        );
        let meta = volume_meta(&[&a, &b], "Both");
        assert_eq!(meta.title, "Both");
        assert_eq!(meta.byline, "A. Author", "the byline they share");
        assert_eq!(meta.origin.publisher, "A Press");
        assert_eq!(meta.credit, "2 works");
        assert_eq!((meta.dateline.as_str(), meta.origin.url.as_str()), ("", ""));
        assert_eq!(meta.lang, "de");
        assert_eq!(meta.contributors.len(), 1, "each contributor once");
        assert_eq!(
            meta.rights, a.meta.rights,
            "the same rights are kept as they are"
        );

        let mut c = work("c", "B. Other", "Under a licence.", "Ask first.");
        c.meta.origin.publisher = "B Press".into();
        let meta = volume_meta(&[&a, &c], "Mixed");
        assert_eq!(meta.byline, "");
        assert_eq!(meta.origin.publisher, "");
        assert_eq!(meta.contributors.len(), 2);
        assert_eq!(
            meta.rights.terms, "Title a: Free to all. Title c: Under a licence.",
            "each work's terms under its title"
        );
        assert_eq!(
            meta.rights
                .notices
                .iter()
                .map(|n| n.text.as_str())
                .collect::<Vec<_>>(),
            ["Title a: See the licence.", "Title c: Ask first."]
        );
        assert!(
            meta.rights.notices.iter().all(|n| n.link.is_some()),
            "links kept"
        );
        assert_eq!(meta.rights.statement, "");
        assert_eq!(
            volume_meta(&[&a], "One").credit,
            "",
            "one work needs no credit"
        );
    }

    fn keys(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_section_list_is_read_against_the_aliases_of_the_source() {
        assert_eq!(
            parse_sections(&SPECS, " ALL ").unwrap(),
            keys(&["bib", "notes", "supplements"])
        );
        assert_eq!(parse_sections(&SPECS, "none").unwrap(), keys(&[]));
        assert_eq!(parse_sections(&SPECS, "").unwrap(), keys(&[]));
        assert_eq!(
            parse_sections(&SPECS, "Bibliography,, supplement").unwrap(),
            keys(&["bib", "supplements"])
        );
        assert_eq!(default_sections(&SPECS), "bib,notes");
        let err = parse_sections(&SPECS, "bib,figures").unwrap_err();
        assert_eq!(err.token, "figures");
        assert_eq!(
            err.to_string(),
            "unknown section \"figures\"\n  known: bib, bibliography, notes, supplements, supplement"
        );
    }
}
