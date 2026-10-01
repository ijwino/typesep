use crate::lazy_regex;
use sep_core::model::{Doc, Meta};

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

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub fn dateline(meta: &Meta) -> String {
    let mut parts = Vec::new();
    if !meta.first_published.is_empty() {
        parts.push(format!("First published {}", meta.first_published));
    }
    if !meta.revision.is_empty() {
        parts.push(format!("substantive revision {}", meta.revision));
    }
    parts.join("; ")
}

lazy_regex!(ISO_DATE_RE = r"^(\d{4})-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])".to_owned());

fn ymd(iso: &str) -> Option<(i64, usize, i64)> {
    let m = ISO_DATE_RE.captures(iso)?;
    Some((
        m[1].parse().ok()?,
        m[2].parse::<usize>().ok()?,
        m[3].parse().ok()?,
    ))
}

pub fn format_date(iso: &str) -> String {
    match ymd(iso) {
        Some((y, m, d)) => format!("{} {d}, {y}", MONTHS[m - 1]),
        None => String::new(),
    }
}

pub fn fetch_date_span(metas: &[&Meta]) -> Option<(String, String)> {
    let mut dates: Vec<String> = metas
        .iter()
        .map(|m| m.fetched_at.chars().take(10).collect::<String>())
        .filter(|d| !format_date(d).is_empty())
        .collect();
    dates.sort();
    dates.dedup();
    let first = dates.first()?.clone();
    let last = dates.last()?.clone();
    Some((first, last))
}

pub fn retrieval_dates(metas: &[&Meta]) -> String {
    let Some((first, last)) = fetch_date_span(metas) else {
        return String::new();
    };
    if first == last {
        return format_date(&first);
    }
    let (Some((y1, m1, d1)), Some((y2, m2, d2))) = (ymd(&first), ymd(&last)) else {
        return String::new();
    };
    if y1 == y2 && m1 == m2 {
        return format!("{} {d1}–{d2}, {y1}", MONTHS[m1 - 1]);
    }
    if y1 == y2 {
        return format!("{} {d1} – {} {d2}, {y1}", MONTHS[m1 - 1], MONTHS[m2 - 1]);
    }
    format!("{} – {}", format_date(&first), format_date(&last))
}

pub fn copyright_line(meta: &Meta) -> String {
    if meta.copyright_year.is_empty() {
        return String::new();
    }
    let by = if meta.author_line.is_empty() {
        "the authors"
    } else {
        meta.author_line.as_str()
    };
    format!("Copyright © {} by {by}", meta.copyright_year)
}

pub fn volume_meta(docs: &[&Doc], volume_title: Option<&str>) -> Meta {
    let mut meta = docs[0].meta.clone();
    meta.macros.clear();
    for doc in docs {
        meta.macros
            .extend(doc.meta.macros.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    if let Some(title) = volume_title {
        meta.title = title.to_owned();
        meta.author_line = String::new();
        meta.source_url = String::new();
    }
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetched(date: &str) -> Meta {
        Meta {
            fetched_at: date.to_owned(),
            ..Meta::default()
        }
    }

    #[test]
    fn dates_read_as_sep_writes_them() {
        assert_eq!(format_date("2026-09-10"), "September 10, 2026");
        assert_eq!(format_date("2026-13-10"), "");
        assert_eq!(format_date(""), "");
        let a = fetched("2026-09-10T09:00:00Z");
        let b = fetched("2026-09-03");
        let c = fetched("2026-08-28");
        let d = fetched("2025-12-30");
        assert_eq!(retrieval_dates(&[&a, &a]), "September 10, 2026");
        assert_eq!(retrieval_dates(&[&a, &b]), "September 3–10, 2026");
        assert_eq!(retrieval_dates(&[&c, &a]), "August 28 – September 10, 2026");
        assert_eq!(
            retrieval_dates(&[&d, &fetched("2026-01-04")]),
            "December 30, 2025 – January 4, 2026"
        );
        assert_eq!(retrieval_dates(&[&fetched("")]), "");
    }

    #[test]
    fn datelines_and_copyright() {
        let meta = Meta {
            first_published: "Mon Sep 24, 2018".into(),
            revision: "Wed May 21, 2025".into(),
            copyright_year: "2025".into(),
            author_line: "A, B, and C".into(),
            ..Meta::default()
        };
        assert_eq!(
            dateline(&meta),
            "First published Mon Sep 24, 2018; substantive revision Wed May 21, 2025"
        );
        assert_eq!(copyright_line(&meta), "Copyright © 2025 by A, B, and C");
        assert_eq!(copyright_line(&Meta::default()), "");
    }
}
