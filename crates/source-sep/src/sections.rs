use typesep_net::source::SectionSpec;

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

/// What `--sections` can name in an entry: the four sections at the end of its page, and the
/// notes and supplements pages it links to.
pub const SECTIONS: [SectionSpec; 6] = [
    SectionSpec {
        key: "bib",
        label: "Bibliography",
        aliases: &["bib", "bibliography"],
        default: true,
        linked: false,
    },
    SectionSpec {
        key: "academic",
        label: "Academic Tools",
        aliases: &["academic", "academic-tools"],
        default: false,
        linked: false,
    },
    SectionSpec {
        key: "oir",
        label: "Other Internet Resources",
        aliases: &["oir", "other-internet-resources"],
        default: true,
        linked: true,
    },
    SectionSpec {
        key: "related",
        label: "Related Entries",
        aliases: &["related", "related-entries"],
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use typesep_net::source::{default_sections, parse_sections};

    use super::*;

    #[test]
    fn section_lists_parse_every_spelling() {
        use ContentKey::*;
        let set = |keys: &[ContentKey]| {
            keys.iter()
                .map(|key| key.name().to_owned())
                .collect::<BTreeSet<_>>()
        };
        let parse = |spec: &str| parse_sections(&SECTIONS, spec);
        assert_eq!(parse("all").unwrap(), set(&ALL_CONTENT_KEYS));
        assert_eq!(parse(" ALL ").unwrap(), set(&ALL_CONTENT_KEYS));
        assert_eq!(parse("none").unwrap(), set(&[]));
        assert_eq!(parse("").unwrap(), set(&[]));
        assert_eq!(
            parse(&default_sections(&SECTIONS)).unwrap(),
            set(&[Bib, Oir, Related, Notes])
        );
        assert_eq!(
            parse("Bibliography, supplement,,academic-tools").unwrap(),
            set(&[Bib, Supplements, Academic])
        );
        let err = parse("bib,figures").unwrap_err();
        assert_eq!(err.token, "figures");
        assert_eq!(
            err.to_string(),
            "unknown section \"figures\"\n  known: bib, bibliography, academic, academic-tools, oir, other-internet-resources, related, related-entries, notes, supplements, supplement"
        );
        assert_eq!(default_sections(&SECTIONS), "bib,oir,related,notes");
    }

    #[test]
    fn the_sections_are_the_content_keys_with_the_labels_the_parser_reads() {
        let keys: Vec<&str> = SECTIONS.iter().map(|spec| spec.key).collect();
        let names: Vec<&str> = ALL_CONTENT_KEYS.iter().map(|key| key.name()).collect();
        assert_eq!(keys, names);
        for (key, label) in SECTION_LABELS {
            let spec = SECTIONS.iter().find(|spec| spec.key == key.name()).unwrap();
            assert_eq!(spec.label, label);
        }
    }
}
