use crate::lazy_regex;
use sep_core::model::Meta;

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
        .map(|m| m.origin.retrieved.chars().take(10).collect::<String>())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fetched(date: &str) -> Meta {
        let mut meta = Meta::default();
        meta.origin.retrieved = date.to_owned();
        meta
    }

    #[test]
    fn dates_are_written_out_in_english() {
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
}
