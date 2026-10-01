use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContentKey {
    Bib,
    Academic,
    Oir,
    Related,
    Notes,
    Supplements,
}

impl ContentKey {
    pub fn name(self) -> &'static str {
        match self {
            ContentKey::Bib => "bib",
            ContentKey::Academic => "academic",
            ContentKey::Oir => "oir",
            ContentKey::Related => "related",
            ContentKey::Notes => "notes",
            ContentKey::Supplements => "supplements",
        }
    }
}

pub const ALL_CONTENT_KEYS: [ContentKey; 6] = [
    ContentKey::Bib,
    ContentKey::Academic,
    ContentKey::Oir,
    ContentKey::Related,
    ContentKey::Notes,
    ContentKey::Supplements,
];

pub const SECTION_LABELS: [(ContentKey, &str); 4] = [
    (ContentKey::Bib, "Bibliography"),
    (ContentKey::Academic, "Academic Tools"),
    (ContentKey::Oir, "Other Internet Resources"),
    (ContentKey::Related, "Related Entries"),
];

/// The keys as the renderers take them, which name the sections of `Doc::back` and the
/// notes and supplements.
pub fn key_names(keys: &BTreeSet<ContentKey>) -> BTreeSet<String> {
    keys.iter().map(|key| key.name().to_owned()).collect()
}

pub const DEFAULT_CONTENT_KEYS: [ContentKey; 4] = [
    ContentKey::Bib,
    ContentKey::Oir,
    ContentKey::Related,
    ContentKey::Notes,
];

const ALIASES: [(&str, ContentKey); 11] = [
    ("bib", ContentKey::Bib),
    ("bibliography", ContentKey::Bib),
    ("academic", ContentKey::Academic),
    ("academic-tools", ContentKey::Academic),
    ("oir", ContentKey::Oir),
    ("other-internet-resources", ContentKey::Oir),
    ("related", ContentKey::Related),
    ("related-entries", ContentKey::Related),
    ("notes", ContentKey::Notes),
    ("supplements", ContentKey::Supplements),
    ("supplement", ContentKey::Supplements),
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown section \"{token}\"\n  known: {known}")]
pub struct SectionError {
    pub token: String,
    known: String,
}

pub fn parse_content_keys(spec: &str) -> Result<BTreeSet<ContentKey>, SectionError> {
    let raw = spec.trim().to_ascii_lowercase();
    if raw == "all" {
        return Ok(ALL_CONTENT_KEYS.into_iter().collect());
    }
    if raw == "none" || raw.is_empty() {
        return Ok(BTreeSet::new());
    }
    let mut out = BTreeSet::new();
    for token in raw.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        match ALIASES.iter().find(|(alias, _)| *alias == token) {
            Some((_, key)) => {
                out.insert(*key);
            }
            None => {
                let known = ALIASES
                    .iter()
                    .map(|(alias, _)| *alias)
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

pub fn default_sections_text() -> String {
    DEFAULT_CONTENT_KEYS
        .iter()
        .map(|k| k.name())
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_lists_parse_every_spelling() {
        use ContentKey::*;
        let set = |keys: &[ContentKey]| keys.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(parse_content_keys("all").unwrap(), set(&ALL_CONTENT_KEYS));
        assert_eq!(parse_content_keys(" ALL ").unwrap(), set(&ALL_CONTENT_KEYS));
        assert_eq!(parse_content_keys("none").unwrap(), set(&[]));
        assert_eq!(parse_content_keys("").unwrap(), set(&[]));
        assert_eq!(
            parse_content_keys(&default_sections_text()).unwrap(),
            set(&DEFAULT_CONTENT_KEYS)
        );
        assert_eq!(
            parse_content_keys("Bibliography, supplement,,academic-tools").unwrap(),
            set(&[Bib, Supplements, Academic])
        );
        let err = parse_content_keys("bib,figures").unwrap_err();
        assert_eq!(err.token, "figures");
        assert_eq!(
            err.to_string(),
            "unknown section \"figures\"\n  known: bib, bibliography, academic, academic-tools, oir, other-internet-resources, related, related-entries, notes, supplements, supplement"
        );
        assert_eq!(default_sections_text(), "bib,oir,related,notes");
        assert_eq!(
            key_names(&set(&[Related, Bib, Notes])),
            ["bib", "notes", "related"].map(str::to_owned).into()
        );
        let regions: Vec<&str> = ["preamble", "body"]
            .into_iter()
            .chain(ALL_CONTENT_KEYS.iter().map(|k| k.name()))
            .collect();
        assert_eq!(typesep_style::regions::ALL_REGIONS.to_vec(), regions);
    }
}
