use std::collections::BTreeMap;

use scraper::Html;

use super::wording::{self, LANG, SEP_NAME, SOURCE};
use typesep_html::dom;
use typesep_html::js::{self, WS};
use typesep_html::{lazy_regex, lazy_selector};
use typesep_model::{Contributor, Dir, Meta, Origin};

lazy_selector!(AUEDITABLE_H1 = "#aueditable > h1");
lazy_selector!(H1 = "h1");
lazy_selector!(PUBINFO = "#pubinfo");
lazy_selector!(ARTICLE_COPYRIGHT = "#article-copyright");

lazy_regex!(FIRST_PUBLISHED = format!(r"(?i-u:first published){WS}+([^;]+)"));
lazy_regex!(REVISION = format!(r"(?i-u:substantive revision){WS}+([^;]+)"));
lazy_regex!(COPYRIGHT_YEAR = format!(r"(?i-u:copyright){WS}*(?:(?i-u:&copy;)|©){WS}*([0-9]{{4}})"));
lazy_regex!(MAILTO_ANCHOR = r"(?i-u:<a)[^>]*(?i-u:mailto)[^>]*>[\s\S]*?(?i-u:</a>)");
lazy_regex!(BR = format!(r"(?i-u:<br){WS}*/?>"));
lazy_regex!(COPYRIGHT_WORD = r"(?i-u:copyright)");
lazy_regex!(BY_ALONE = r"^(?i-u:by)$");
lazy_regex!(TRAILING_BY = format!(r"{WS}*(?-u:\b)(?i-u:by)(?-u:\b){WS}*$"));
lazy_regex!(EDITION = r"^(spr|sum|fall|win)([0-9]{4})$");

pub struct MetadataSource<'a> {
    pub source_url: &'a str,
    pub slug: &'a str,
    pub edition: &'a str,
    pub fetched_at: &'a str,
}

pub fn parse_metadata(document: &Html, src: &MetadataSource) -> Meta {
    let first_text = |selector| {
        dom::select(document, selector)
            .next()
            .map(|el| js::trim(&dom::text_of(*el)).to_owned())
            .unwrap_or_default()
    };
    let mut title = first_text(&AUEDITABLE_H1);
    if title.is_empty() {
        title = first_text(&H1);
    }

    let pubinfo: String = dom::select(document, &PUBINFO)
        .map(|el| dom::text_of(*el))
        .collect();
    let pubinfo = js::trim(&pubinfo);
    let capture = |re: &regex::Regex, text: &str| {
        re.captures(text)
            .map(|c| js::trim(&c[1]).to_owned())
            .unwrap_or_default()
    };
    let first_published = capture(&FIRST_PUBLISHED, pubinfo);
    let revision = capture(&REVISION, pubinfo);

    let copyright_html = dom::select(document, &ARTICLE_COPYRIGHT)
        .next()
        .map(|el| el.inner_html())
        .unwrap_or_default();
    let copyright_year = COPYRIGHT_YEAR
        .captures(&copyright_html)
        .map(|c| c[1].to_owned())
        .unwrap_or_default();

    let without_emails = MAILTO_ANCHOR.replace_all(&copyright_html, "");
    let without_emails = BR.replace_all(&without_emails, "\n");
    let fragment = Html::parse_fragment(&without_emails);
    let text = dom::text_of(*fragment.root_element());
    let authors: Vec<String> = text
        .split('\n')
        .map(|line| {
            let cleaned: String = line
                .chars()
                .map(|c| {
                    if matches!(c, '<' | '>' | '&' | ';') {
                        ' '
                    } else {
                        c
                    }
                })
                .collect();
            js::trim(&js::collapse_spaces(&cleaned)).to_owned()
        })
        .filter(|line| {
            !line.is_empty() && !COPYRIGHT_WORD.is_match(line) && !BY_ALONE.is_match(line)
        })
        .map(|line| js::trim(&TRAILING_BY.replace(&line, "")).to_owned())
        .filter(|line| !line.is_empty())
        .collect();

    let byline = format_authors(&authors);
    Meta {
        id: src.slug.to_owned(),
        title,
        contributors: authors
            .into_iter()
            .map(|name| Contributor {
                name,
                role: "author".to_owned(),
            })
            .collect(),
        dateline: wording::dateline(&first_published, &revision),
        rights: wording::rights(wording::copyright_line(&copyright_year, &byline)),
        byline,
        credit: String::new(),
        lang: LANG.to_owned(),
        dir: Dir::Ltr,
        origin: Origin {
            source: SOURCE.to_owned(),
            publisher: SEP_NAME.to_owned(),
            url: src.source_url.to_owned(),
            edition: src.edition.to_owned(),
            edition_label: edition_label(src.edition),
            revision,
            retrieved: src.fetched_at.to_owned(),
        },
        macros: BTreeMap::new(),
    }
}

fn format_authors(authors: &[String]) -> String {
    match authors {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [init @ .., last] => format!("{}, and {last}", init.join(", ")),
    }
}

pub(crate) fn is_edition(text: &str) -> bool {
    EDITION.is_match(text)
}

pub fn edition_label(edition: &str) -> String {
    let Some(m) = EDITION.captures(edition) else {
        return String::new();
    };
    let season = match &m[1] {
        "spr" => "Spring",
        "sum" => "Summer",
        "fall" => "Fall",
        _ => "Winter",
    };
    format!("{season} {} Edition", &m[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(meta: &Meta) -> Vec<&str> {
        meta.contributors.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn metadata_comes_off_the_page() {
        let html = r#"<html><body><div id="aueditable">
          <h1>The Philosophy of Computer Science</h1>
          <div id="pubinfo"><em>First published Tue Aug 20, 2013; substantive revision Thu Jan 25, 2024</em></div>
          <div id="article-copyright">
            <p>
 <a href="../../info.html#c">Copyright &copy; 2024</a> by
<br>
<a href="https://example.org/a">Nicola Angius</a>
&lt;<a href="mailto:nicola%40example%2eorg"><em>nicola<abbr title=" at ">@</abbr>example.org</em></a>&gt;
<br>
<a href="https://example.org/b">Giuseppe Primiero</a>
&lt;<a href="mailto:g%40example%2eorg">g@example.org</a>&gt;<br>
Raymond Turner &lt;<a href="mailto:r%40x">r@x</a>&gt;
            </p>
          </div></div></body></html>"#;
        let document = Html::parse_document(html);
        let meta = parse_metadata(
            &document,
            &MetadataSource {
                source_url: "https://plato.stanford.edu/entries/computer-science/",
                slug: "computer-science",
                edition: "sum2026",
                fetched_at: "2026-09-01",
            },
        );
        assert_eq!(meta.id, "computer-science");
        assert_eq!(meta.title, "The Philosophy of Computer Science");
        assert_eq!(
            names(&meta),
            ["Nicola Angius", "Giuseppe Primiero", "Raymond Turner"]
        );
        assert!(meta.contributors.iter().all(|c| c.role == "author"));
        assert_eq!(
            meta.byline,
            "Nicola Angius, Giuseppe Primiero, and Raymond Turner"
        );
        assert_eq!(
            meta.dateline,
            "First published Tue Aug 20, 2013; substantive revision Thu Jan 25, 2024"
        );
        assert_eq!(
            meta.rights.statement,
            "Copyright © 2024 by Nicola Angius, Giuseppe Primiero, and Raymond Turner"
        );
        assert_eq!(meta.rights.terms, wording::PERSONAL_USE_NOTICE);
        assert_eq!(meta.rights.notices.len(), 1);
        assert_eq!(meta.lang, "en");
        assert_eq!(meta.credit, "");
        assert_eq!(meta.origin.source, "sep");
        assert_eq!(meta.origin.publisher, SEP_NAME);
        assert_eq!(
            meta.origin.url,
            "https://plato.stanford.edu/entries/computer-science/"
        );
        assert_eq!(meta.origin.edition, "sum2026");
        assert_eq!(meta.origin.revision, "Thu Jan 25, 2024");
        assert_eq!(meta.origin.edition_label, "Summer 2026 Edition");
        assert_eq!(meta.origin.retrieved, "2026-09-01");
        assert!(
            !serde_json::to_string(&meta).unwrap().contains('@'),
            "metadata holds no email addresses"
        );

        let bare = Html::parse_document(
            "<h1>Only a title</h1><div id=\"article-copyright\">Copyright © 2020 by<br>Ann Author</div>",
        );
        let meta = parse_metadata(
            &bare,
            &MetadataSource {
                source_url: "",
                slug: "",
                edition: "",
                fetched_at: "",
            },
        );
        assert_eq!(meta.title, "Only a title");
        assert_eq!(names(&meta), ["Ann Author"]);
        assert_eq!(meta.byline, "Ann Author");
        assert_eq!(meta.rights.statement, "Copyright © 2020 by Ann Author");
        assert_eq!(meta.dateline, "");
        assert_eq!(meta.origin.edition_label, "");
    }

    #[test]
    fn authors_join_and_editions_read() {
        let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(format_authors(&s(&[])), "");
        assert_eq!(format_authors(&s(&["A"])), "A");
        assert_eq!(format_authors(&s(&["A", "B"])), "A and B");
        assert_eq!(format_authors(&s(&["A", "B", "C"])), "A, B, and C");
        assert_eq!(edition_label("sum2026"), "Summer 2026 Edition");
        assert_eq!(edition_label("spr2001"), "Spring 2001 Edition");
        assert_eq!(edition_label("fall1997"), "Fall 1997 Edition");
        assert_eq!(edition_label("win2020"), "Winter 2020 Edition");
        assert_eq!(edition_label(""), "");
        assert_eq!(edition_label("live"), "");
        assert_eq!(edition_label("Sum2026"), "");
    }
}
