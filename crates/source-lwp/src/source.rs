use typesep_model::{Doc, Meta};
use typesep_net::library::{Place, PlaceError, Store};
use typesep_net::net::{LoadError, Net};
use typesep_net::source::{
    Held, LIVE, LoadOptions, Loaded, ResolveError, SectionSpec, Source, WorkRef, volume_meta,
};

use crate::address::{api_url, is_site_url, page_url, place_for, shell_word, title_of};
use crate::library::list_texts;
use crate::parse::{PUBLISHER, ParseOptions, SOURCE, parse_page};

/// The Ludwig Wittgenstein Project.
pub struct Lwp;

pub const KEY: &str = SOURCE;

/// The preset its texts are typeset with when none is named.
pub const PRESET: &str = "lwp";

impl Source for Lwp {
    fn key(&self) -> &'static str {
        KEY
    }

    fn claims(&self, input: &str) -> bool {
        is_site_url(input)
    }

    fn resolve(&self, input: &str, edition: Option<&str>) -> Result<WorkRef, ResolveError> {
        if edition.is_some_and(|edition| edition != LIVE) {
            return Err(ResolveError(
                "lwp has no editions; it always uses the current revision".to_owned(),
            ));
        }
        let title = title_of(input).map_err(|err| ResolveError(err.to_string()))?;
        Ok(WorkRef {
            source: KEY,
            url: page_url(&title),
            id: title,
            edition: String::new(),
        })
    }

    fn place(&self, url: &str) -> Result<Place, PlaceError> {
        place_for(url)
    }

    /// One request to the API for the text, rendered; the library keeps its answer, and
    /// `update` asks again and compares the revision.
    fn load(
        &self,
        work: &WorkRef,
        net: &mut Net<'_>,
        _opts: LoadOptions<'_>,
    ) -> Result<Loaded, LoadError> {
        let previous_revision = net
            .store
            .work_record(&work.url)?
            .map(|record| record.revision);
        let api = api_url(&work.id);
        let refresh = net.refresh || net.check_updates;
        let page = net.page(&api, refresh).map_err(|err| match err {
            LoadError::NotHeld { .. } => LoadError::NotInLibrary {
                url: work.url.clone(),
                dir: net.store.library_dir().display().to_string(),
                fetch: format!("typesep fetch --source {KEY} {}", shell_word(&work.id)),
            },
            other => other,
        })?;
        let fetched_at = page
            .meta
            .as_ref()
            .and_then(|meta| meta.fetched_at.as_deref())
            .unwrap_or_default();
        let doc = parse_page(
            &page.text,
            &ParseOptions {
                id: &work.id,
                url: &work.url,
                fetched_at,
            },
        )
        .map_err(|reason| LoadError::Unusable {
            url: work.url.clone(),
            reason,
        })?;
        if net.writes() {
            net.store
                .record_work(&work.url, &doc.meta.title, &doc.meta.origin.revision)?;
        }
        Ok(Loaded {
            doc,
            from_cache: page.from_cache,
            previous_revision,
        })
    }

    fn sections(&self) -> &'static [SectionSpec] {
        &[]
    }

    fn held(&self, store: &Store<'_>) -> Vec<Held> {
        list_texts(store)
    }

    fn preset(&self) -> &'static str {
        PRESET
    }

    /// Texts of the project as one volume: the byline they share, the project as publisher,
    /// "N texts from the Ludwig Wittgenstein Project" for a cover without a byline, and each
    /// text's rights as its colophon states them when they are not all the same.
    fn volume(&self, docs: &[&Doc], title: &str) -> Meta {
        let mut meta = volume_meta(docs, title);
        meta.origin.publisher = PUBLISHER.to_owned();
        meta.credit = if docs.len() > 1 {
            format!("{} texts from the {PUBLISHER}", docs.len())
        } else {
            String::new()
        };
        meta
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use typesep_net::fetch::testing::{Harness, ok};
    use typesep_net::library::Library;
    use typesep_net::source::UpdateStatus;

    use super::*;

    const TEXT: &str = "Some_Remarks";

    fn answer(revid: u64, body: &str) -> String {
        serde_json::json!({
            "parse": {
                "title": "Some Remarks",
                "pageid": 7,
                "revid": revid,
                "text": format!(
                    "<div class=\"mw-content-ltr mw-parser-output\" lang=\"de\" dir=\"ltr\">\
                     <div class=\"navbar noprint\"><a href=\"/w/index.php/Main_Page\">Home</a></div>\
                     <div class=\"custom-print-only\"><p style=\"text-align:center;\">Ludwig Wittgenstein</p>\
                     <p class=\"tl-title\">Some Remarks</p></div>\
                     <div class=\"colophon\"><p>An invented text. Dieses Werk ist gemeinfrei.</p></div>{body}</div>"
                ),
                "displaytitle": "<span class=\"mw-page-title-main\">Some Remarks</span>",
                "properties": { "notitle": "" }
            }
        })
        .to_string()
    }

    fn load(net: &mut Net<'_>, work: &WorkRef) -> Result<Loaded, LoadError> {
        Lwp.load(
            work,
            net,
            LoadOptions {
                sections: &BTreeSet::new(),
                log: &mut |_| {},
            },
        )
    }

    #[test]
    fn a_title_or_a_page_address_names_a_text_and_an_edition_but_the_live_one_is_refused() {
        let work = Lwp.resolve("Some Remarks", None).unwrap();
        assert_eq!(
            work,
            WorkRef {
                source: "lwp",
                id: TEXT.into(),
                edition: String::new(),
                url: "https://www.wittgensteinproject.org/w/index.php/Some_Remarks".into(),
            }
        );
        assert_eq!(
            Lwp.resolve(
                "https://wittgensteinproject.org/w/index.php?title=Some_Remarks",
                None
            )
            .unwrap(),
            work
        );
        assert_eq!(
            Lwp.resolve("Some Remarks", Some("live")).unwrap(),
            work,
            "the live edition is the current revision"
        );
        assert_eq!(
            Lwp.resolve("Some Remarks", Some("sum2026"))
                .unwrap_err()
                .to_string(),
            "lwp has no editions; it always uses the current revision"
        );
        assert!(
            Lwp.resolve(
                "Logisch-philosophische Abhandlung (statische Darstellung in Baumstruktur)",
                None
            )
            .unwrap_err()
            .to_string()
            .contains("is a tree view")
        );
        assert!(Lwp.claims("https://www.wittgensteinproject.org/w/index.php/Some_Remarks"));
        assert!(!Lwp.claims("Some_Remarks"));
        assert!(!Lwp.claims("https://plato.stanford.edu/entries/x/"));
    }

    #[test]
    fn a_text_is_fetched_once_kept_and_checked_for_a_new_revision() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        library.init().unwrap();
        let work = Lwp.resolve(TEXT, None).unwrap();
        let body = "<p><b><span id=\"1\">1.</span></b> A remark.</p>";

        let mut harness = Harness::new(vec![Ok(ok(&api_url(TEXT), &answer(11, body)))]);
        let loaded = load(
            &mut Net::fetching(library.store(&Lwp), &mut harness.fetcher),
            &work,
        )
        .unwrap();
        assert_eq!(harness.urls(), [api_url(TEXT)], "one request for the text");
        assert_eq!(loaded.doc.meta.title, "Some Remarks");
        assert_eq!(loaded.doc.meta.origin.revision, "11");
        assert_eq!(UpdateStatus::of(&loaded), UpdateStatus::New);
        let file = temp.path().join("lwp/pages/Some_Remarks/parse.json");
        assert!(file.is_file(), "the answer is kept at the text's place");

        let store = library.store(&Lwp);
        let held = Lwp.held(&store);
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].id, TEXT);
        assert_eq!(held[0].revision, "11");
        assert_eq!(held[0].title, "Some Remarks");
        assert_eq!(held[0].url, work.url);

        let again = load(&mut Net::offline(library.store(&Lwp)), &work).unwrap();
        assert!(again.from_cache);
        assert_eq!(again.doc, loaded.doc, "read back from the library, offline");

        let mut harness = Harness::new(vec![Ok(ok(&api_url(TEXT), &answer(12, body)))]);
        let checked = load(
            &mut Net::fetching(library.store(&Lwp), &mut harness.fetcher).with_check_updates(true),
            &work,
        )
        .unwrap();
        assert_eq!(
            UpdateStatus::of(&checked),
            UpdateStatus::Revised {
                previous: "11".into()
            }
        );
        assert_eq!(Lwp.held(&store)[0].revision, "12");
    }

    #[test]
    fn texts_of_the_project_make_a_volume_of_its_wording_with_each_texts_rights() {
        let doc = |id: &str, terms: &str| {
            let answer = answer(1, "<p>Text.</p>")
                .replace("An invented text. Dieses Werk ist gemeinfrei.", terms);
            parse_page(
                &answer,
                &ParseOptions {
                    id,
                    url: "https://www.wittgensteinproject.org/w/index.php/X",
                    fetched_at: "",
                },
            )
            .unwrap()
        };
        let free = doc("Eins", "Erfunden. Dieses Werk ist gemeinfrei.");
        let same = doc("Zwei", "Erfunden. Dieses Werk ist gemeinfrei.");
        let other = doc("Drei", "Erfunden, und auch dieses ist gemeinfrei.");
        let meta = Lwp.volume(&[&free, &same], "Zwei Hefte");
        assert_eq!(meta.title, "Zwei Hefte");
        assert_eq!(meta.byline, "Ludwig Wittgenstein");
        assert_eq!(meta.origin.publisher, "Ludwig Wittgenstein Project");
        assert_eq!(meta.credit, "2 texts from the Ludwig Wittgenstein Project");
        assert_eq!(meta.rights, free.meta.rights, "the rights they share");
        let meta = Lwp.volume(&[&free, &other], "Andere");
        assert_eq!(
            meta.rights.terms,
            "Some Remarks: Erfunden. Dieses Werk ist gemeinfrei. Some Remarks: Erfunden, und auch dieses ist gemeinfrei.",
            "each text's own terms, under its title"
        );
    }

    #[test]
    fn what_the_library_lacks_or_the_api_refuses_is_said() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        let work = Lwp
            .resolve("Tractatus Logico-Philosophicus (English)", None)
            .unwrap();
        let err = load(&mut Net::offline(library.store(&Lwp)), &work).unwrap_err();
        assert!(matches!(err, LoadError::NotInLibrary { .. }));
        assert!(
            err.to_string()
                .ends_with("fetch it first: typesep fetch --source lwp 'Tractatus_Logico-Philosophicus_(English)'"),
            "{err}"
        );

        let missing = serde_json::json!({
            "error": { "code": "missingtitle", "info": "The page you specified doesn't exist." }
        })
        .to_string();
        let mut harness = Harness::new(vec![Ok(ok(&api_url(&work.id), &missing))]);
        let err = load(
            &mut Net::fetching(library.store(&Lwp), &mut harness.fetcher),
            &work,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "{}: the Wittgenstein Project answered missingtitle: The page you specified doesn't exist.",
                work.url
            )
        );
        assert!(
            Lwp.held(&library.store(&Lwp)).is_empty(),
            "an answer that holds no text is no text the library holds"
        );
    }
}
