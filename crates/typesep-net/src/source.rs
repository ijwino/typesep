use std::collections::BTreeSet;

use typesep_model::Doc;

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

    /// The source a bare name, or an address no source claims, goes to.
    pub fn default_source(&self) -> &'static dyn Source {
        self.sources[0]
    }

    /// The source whose site `input` is on, else the default one, which then says why it
    /// cannot read it.
    pub fn for_input(&self, input: &str) -> &'static dyn Source {
        self.iter()
            .find(|source| source.claims(input))
            .unwrap_or_else(|| self.default_source())
    }

    /// The keys of what `--sections` can name in any source, each once, in the order the
    /// sources list them.
    pub fn section_keys(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for spec in self.iter().flat_map(|source| source.sections()) {
            if !out.contains(&spec.key) {
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
        },
        SectionSpec {
            key: "notes",
            label: "",
            aliases: &["notes"],
            default: true,
        },
        SectionSpec {
            key: "supplements",
            label: "",
            aliases: &["supplements", "supplement"],
            default: false,
        },
    ];

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
