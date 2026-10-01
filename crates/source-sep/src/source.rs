use typesep_model::Doc;
use typesep_net::library::{HOST, Library, Place, PlaceError, place_for};
use typesep_net::net::{LoadError, Net};
use typesep_net::source::{Held, LoadOptions, Loaded, ResolveError, SectionSpec, Source, WorkRef};

use crate::entry::{EntryTarget, load_entry, normalize_entry, related_entry_urls};
use crate::sections::SECTIONS;

/// The Stanford Encyclopedia of Philosophy.
pub struct Sep;

pub const KEY: &str = "sep";

const LIVE: &str = "live";

impl Source for Sep {
    fn key(&self) -> &'static str {
        KEY
    }

    fn claims(&self, input: &str) -> bool {
        let input = input.trim();
        let lower = input.to_ascii_lowercase();
        if !lower.starts_with("http://") && !lower.starts_with("https://") {
            return false;
        }
        let host = HOST.split_once("://").map_or(HOST, |(_, host)| host);
        url::Url::parse(input).is_ok_and(|url| url.host_str() == Some(host))
    }

    fn resolve(&self, input: &str, edition: Option<&str>) -> Result<WorkRef, ResolveError> {
        let target = normalize_entry(input, edition.unwrap_or(LIVE))
            .map_err(|err| ResolveError(err.to_string()))?;
        Ok(WorkRef {
            source: KEY,
            id: target.slug,
            edition: if target.edition == LIVE {
                String::new()
            } else {
                target.edition
            },
            url: target.url,
        })
    }

    fn place(&self, url: &str) -> Result<Place, PlaceError> {
        place_for(url)
    }

    fn load(
        &self,
        work: &WorkRef,
        net: &mut Net<'_>,
        opts: LoadOptions<'_>,
    ) -> Result<Loaded, LoadError> {
        let target = EntryTarget {
            slug: work.id.clone(),
            edition: if work.edition.is_empty() {
                LIVE.to_owned()
            } else {
                work.edition.clone()
            },
            url: work.url.clone(),
        };
        load_entry(net, &target, opts)
    }

    fn sections(&self) -> &'static [SectionSpec] {
        &SECTIONS
    }

    fn held(&self, library: &Library) -> Vec<Held> {
        library
            .list_entries()
            .into_iter()
            .map(|entry| Held {
                id: entry.slug,
                edition: if entry.edition == LIVE {
                    String::new()
                } else {
                    entry.edition
                },
                url: entry.url,
                revision: entry.revision,
                title: entry.title,
                fetched_at: entry.fetched_at,
            })
            .collect()
    }

    fn related(&self, doc: &Doc) -> Vec<String> {
        related_entry_urls(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_on_the_site_is_the_seps_and_a_name_is_resolved_as_an_entry() {
        assert!(Sep.claims("https://plato.stanford.edu/entries/turing-machine/"));
        assert!(Sep.claims(" HTTP://plato.stanford.edu/contents.html"));
        assert!(Sep.claims("https://plato.stanford.edu:8443/entries/x/"));
        assert!(!Sep.claims("turing-machine"));
        assert!(!Sep.claims("https://www.wittgensteinproject.org/w/index.php"));
        assert!(!Sep.claims("https://plato.stanford.edu.example.com/entries/x/"));

        let live = Sep.resolve("turing-machine", Some("live")).unwrap();
        assert_eq!(
            live,
            WorkRef {
                source: "sep",
                id: "turing-machine".into(),
                edition: String::new(),
                url: "https://plato.stanford.edu/entries/turing-machine/".into(),
            }
        );
        assert_eq!(Sep.resolve("turing-machine", None).unwrap(), live);
        let archived = Sep
            .resolve(
                "https://plato.stanford.edu/archives/win2020/entries/turing-machine/",
                Some("live"),
            )
            .unwrap();
        assert_eq!(archived.edition, "win2020");
        assert_eq!(
            Sep.resolve("turing-machine", Some("summer"))
                .unwrap_err()
                .to_string(),
            "\"summer\" does not look like an SEP edition (live, or one such as sum2026)"
        );
        assert_eq!(
            Sep.resolve("https://example.com/entries/x/", Some("live"))
                .unwrap_err()
                .to_string(),
            "not an SEP entry URL: https://example.com/entries/x/"
        );
    }
}
