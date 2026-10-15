//! The SEP's own wording: its name, the terms its entries are read under, and how an entry
//! or a volume of entries is dated and credited. The parser writes it into the model; the
//! renderers print what they find there.

use typesep_model::{Doc, Meta, Notice, NoticeLink, Rights};

pub const SOURCE: &str = "sep";
pub const LANG: &str = "en";
pub const SEP_NAME: &str = "Stanford Encyclopedia of Philosophy";
pub const PERSONAL_USE_NOTICE: &str = concat!(
    "Generated for personal use. The SEP's Terms of Use allow electronic distribution only to ",
    "devices you own and control (2(a)) and, in private correspondence, to closed, ",
    "invitation-only groups of no more than 30 individuals (2(b)); no other electronic ",
    "distribution rights are granted (2(d)). A printed copy may be distributed only with the ",
    "prior written permission of the entry's author (3).",
);
pub const SUPPORT_NOTICE: &str = concat!(
    "The SEP is free to read because its readers pay for it: the Friends of the SEP Society, ",
    "and SEPIA for institutions.",
);
pub const SUPPORT_URL: &str = "https://plato.stanford.edu/support/";
pub const SUPPORT_LABEL: &str = "plato.stanford.edu/support";
pub const MERGED_RIGHTS: &str = "Each entry is copyright its own authors.";

pub fn dateline(first_published: &str, revision: &str) -> String {
    let mut parts = Vec::new();
    if !first_published.is_empty() {
        parts.push(format!("First published {first_published}"));
    }
    if !revision.is_empty() {
        parts.push(format!("substantive revision {revision}"));
    }
    parts.join("; ")
}

pub fn copyright_line(year: &str, byline: &str) -> String {
    if year.is_empty() {
        return String::new();
    }
    let by = if byline.is_empty() {
        "the authors"
    } else {
        byline
    };
    format!("Copyright © {year} by {by}")
}

pub fn rights(statement: String) -> Rights {
    Rights {
        statement,
        terms: PERSONAL_USE_NOTICE.to_owned(),
        notices: vec![Notice {
            text: SUPPORT_NOTICE.to_owned(),
            link: Some(NoticeLink {
                url: SUPPORT_URL.to_owned(),
                label: SUPPORT_LABEL.to_owned(),
            }),
        }],
    }
}

/// The metadata of entries typeset as one volume under `title`: the first entry's source,
/// everyone's contributors and macros, no byline, dateline or URL of its own, the merged
/// rights, and a credit for the cover when there is more than one entry.
pub fn volume_meta(docs: &[&Doc], title: &str) -> Meta {
    let mut meta = docs[0].meta.clone();
    meta.title = title.to_owned();
    meta.byline = String::new();
    meta.dateline = String::new();
    meta.origin.url = String::new();
    meta.rights.statement = MERGED_RIGHTS.to_owned();
    if docs.len() > 1 {
        meta.credit = format!("{} entries from the {SEP_NAME}", docs.len());
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use typesep_model::{Contributor, MacroDef};

    #[test]
    fn datelines_and_copyright() {
        assert_eq!(
            dateline("Mon Sep 24, 2018", "Wed May 21, 2025"),
            "First published Mon Sep 24, 2018; substantive revision Wed May 21, 2025"
        );
        assert_eq!(
            dateline("", "Wed May 21, 2025"),
            "substantive revision Wed May 21, 2025"
        );
        assert_eq!(dateline("", ""), "");
        assert_eq!(
            copyright_line("2025", "A, B, and C"),
            "Copyright © 2025 by A, B, and C"
        );
        assert_eq!(
            copyright_line("2025", ""),
            "Copyright © 2025 by the authors"
        );
        assert_eq!(copyright_line("", "A"), "");
    }

    #[test]
    fn a_volume_is_credited_to_the_sep_and_its_entries_to_their_authors() {
        let entry = |id: &str, author: &str, revision: &str| {
            let mut doc = Doc::default();
            doc.meta.id = id.into();
            doc.meta.title = id.to_uppercase();
            doc.meta.byline = author.into();
            doc.meta.dateline = dateline("Mon Sep 24, 2018", revision);
            doc.meta.contributors = vec![Contributor {
                name: author.into(),
                role: "author".into(),
            }];
            doc.meta.origin.url = format!("https://plato.stanford.edu/entries/{id}/");
            doc.meta.origin.publisher = SEP_NAME.into();
            doc.meta.origin.revision = revision.into();
            doc.meta.rights = rights(copyright_line("2025", author));
            doc.meta.macros.insert(
                format!("op{id}"),
                MacroDef {
                    body: id.into(),
                    args: 0,
                },
            );
            doc
        };
        let (a, b, c) = (
            entry("a", "Ann", "Wed May 21, 2025"),
            entry("b", "Bea", ""),
            entry("c", "Ann", ""),
        );
        let volume = volume_meta(&[&a, &b, &c], "Miscellany");
        assert_eq!(volume.title, "Miscellany");
        assert_eq!(volume.id, "a");
        assert_eq!((volume.byline.as_str(), volume.dateline.as_str()), ("", ""));
        assert_eq!(volume.origin.url, "");
        assert_eq!(volume.origin.revision, "Wed May 21, 2025");
        assert_eq!(volume.origin.publisher, SEP_NAME);
        assert_eq!(
            volume.credit,
            "3 entries from the Stanford Encyclopedia of Philosophy"
        );
        assert_eq!(volume.rights.statement, MERGED_RIGHTS);
        assert_eq!(volume.rights.terms, PERSONAL_USE_NOTICE);
        assert_eq!(volume.rights.notices, a.meta.rights.notices);
        let names: Vec<&str> = volume
            .contributors
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, ["Ann", "Bea"]);
        assert_eq!(
            volume.macros.keys().collect::<Vec<_>>(),
            ["opa", "opb", "opc"]
        );

        let alone = volume_meta(&[&a], "Alone");
        assert_eq!(alone.credit, "", "one entry is no collection to credit");
        assert_eq!(alone.rights.statement, MERGED_RIGHTS);
    }
}
