//! Who made a text and what it may be read under, as its page says in its colophon: the
//! editors in the line that credits them, the public-domain or licence statement as the
//! terms, and every other paragraph as a notice with its licence link. Nothing of it is
//! written here; a page without a colophon has no rights to state.

use scraper::ElementRef;
use typesep_html::dom;
use typesep_html::js;
use typesep_model::{Notice, NoticeLink, Rights};

/// How a credits line names the editors, as it opens.
const EDITED_BY: [&str; 2] = ["herausgegeben von", "edited by"];

/// What a paragraph that states a text is in the public domain says, in the languages the
/// project's colophons are written in.
const PUBLIC_DOMAIN: [&str; 6] = [
    "gemeinfrei",
    "public domain",
    "pubblico dominio",
    "domaine public",
    "dominio público",
    "domínio público",
];

const LICENCES: &str = "creativecommons.org";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Colophon {
    pub editors: Vec<String>,
    pub rights: Rights,
}

fn words(el: ElementRef<'_>) -> String {
    js::collapse_spaces(js::trim(&dom::text_of(*el)))
}

/// The lines of a paragraph, between its line breaks, each with its spaces collapsed.
fn lines(p: ElementRef<'_>) -> Vec<String> {
    let mut out = vec![String::new()];
    for node in p.descendants() {
        if let Some(el) = ElementRef::wrap(node)
            && dom::is(&el, "br")
        {
            out.push(String::new());
        } else if let Some(text) = node.value().as_text() {
            out.last_mut().expect("a line").push_str(text);
        }
    }
    out.iter()
        .map(|line| js::collapse_spaces(js::trim(line)))
        .filter(|line| !line.is_empty())
        .collect()
}

/// The names a credits line gives after `Herausgegeben von` or `edited by`, when it opens so.
fn editors_in(line: &str) -> Option<Vec<String>> {
    let lower = line.to_lowercase();
    let marker = EDITED_BY.iter().find(|marker| lower.starts_with(*marker))?;
    let names = line.get(marker.len()..)?.trim().trim_end_matches('.');
    let mut out = Vec::new();
    for part in names.split(", ") {
        for name in part.split(" und ").flat_map(|n| n.split(" and ")) {
            let name = name.trim();
            if !name.is_empty() {
                out.push(name.to_owned());
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn licence_link(p: ElementRef<'_>) -> Option<NoticeLink> {
    let a = dom::descendants_named(*p, &["a"])
        .find(|a| a.attr("href").is_some_and(|href| href.contains(LICENCES)))?;
    let url = a.attr("href")?.to_owned();
    let label = url
        .split_once("://")
        .map_or(url.as_str(), |(_, rest)| rest)
        .trim_end_matches('/')
        .to_owned();
    Some(NoticeLink { url, label })
}

/// The colophon of a page, `div.colophon`: the credits paragraph, centred and in `plainlinks`,
/// then the paragraphs on the edition and its rights.
pub fn read(colophon: Option<ElementRef<'_>>) -> Colophon {
    let mut out = Colophon::default();
    let Some(colophon) = colophon else {
        return out;
    };
    let mut credits: Vec<String> = Vec::new();
    let mut terms: Vec<(String, Option<NoticeLink>)> = Vec::new();
    let mut others: Vec<(String, Option<NoticeLink>)> = Vec::new();
    for p in dom::children_named(*colophon, &["p"]) {
        if dom::has_class(&p, "plainlinks") {
            for line in lines(p) {
                match editors_in(&line) {
                    Some(names) => out.editors.extend(names),
                    None => credits.push(line),
                }
            }
            continue;
        }
        let text = words(p);
        if text.is_empty() {
            continue;
        }
        let lower = text.to_lowercase();
        let paragraph = (text, licence_link(p));
        if PUBLIC_DOMAIN.iter().any(|said| lower.contains(said)) {
            terms.push(paragraph);
        } else {
            others.push(paragraph);
        }
    }
    if terms.is_empty()
        && let Some(at) = others.iter().position(|(_, link)| link.is_some())
    {
        // No public domain: the terms are the licence, and its address goes with them.
        let (text, link) = others.remove(at);
        let url = link.map(|link| link.url).unwrap_or_default();
        terms.push((format!("{text} ({url})"), None));
    }
    out.rights = Rights {
        statement: String::new(),
        terms: terms
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>()
            .join(" "),
        notices: (!credits.is_empty())
            .then(|| Notice {
                text: credits.join("; "),
                link: None,
            })
            .into_iter()
            .chain(others.into_iter().map(|(text, link)| Notice { text, link }))
            .collect(),
    };
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::{Html, Selector};

    fn colophon(html: &str) -> Colophon {
        let document = Html::parse_fragment(html);
        let selector = Selector::parse("div.colophon").unwrap();
        read(document.select(&selector).next())
    }

    #[test]
    fn the_editors_are_read_from_the_credits_line_and_the_rest_is_kept() {
        let read = colophon(
            "<div class=\"colophon\"><p class=\"plainlinks\" style=\"text-align: center;\">A choice \
             of remarks<br/>\nHerausgegeben von <a href=\"https://example.org/a\">A. B. Cee</a>, D. Eff \
             und G. von Ha <br/>\nWith help from I. Jay</p><p>&#160;</p>\
             <p><br/>\nMade from an <i>old</i> edition. This work is in the public domain in its country.\n</p>\
             <p>Released under the <a href=\"https://creativecommons.org/licenses/by-sa/4.0/\">licence</a>, \
             see <a href=\"https://creativecommons.org/licenses/by/4.0/\">also</a>.</p>\
             <p>A note without a link.</p></div>",
        );
        assert_eq!(read.editors, ["A. B. Cee", "D. Eff", "G. von Ha"]);
        assert_eq!(read.rights.statement, "");
        assert_eq!(
            read.rights.terms,
            "Made from an old edition. This work is in the public domain in its country."
        );
        assert_eq!(
            read.rights.notices,
            [
                Notice {
                    text: "A choice of remarks; With help from I. Jay".into(),
                    link: None
                },
                Notice {
                    text: "Released under the licence, see also.".into(),
                    link: Some(NoticeLink {
                        url: "https://creativecommons.org/licenses/by-sa/4.0/".into(),
                        label: "creativecommons.org/licenses/by-sa/4.0".into()
                    })
                },
                Notice {
                    text: "A note without a link.".into(),
                    link: None
                }
            ]
        );
    }

    #[test]
    fn without_a_public_domain_the_licence_is_the_terms() {
        let read = colophon(
            "<div class=\"colophon\"><p>A translation.</p><p>It is free under the \
             <a href=\"https://creativecommons.org/licenses/by-nc/4.0/\">licence</a>.</p></div>",
        );
        assert!(read.editors.is_empty());
        assert_eq!(
            read.rights.terms,
            "It is free under the licence. (https://creativecommons.org/licenses/by-nc/4.0/)"
        );
        assert_eq!(read.rights.notices.len(), 1);
        assert_eq!(colophon("<p>no colophon</p>"), Colophon::default());
        assert_eq!(
            editors_in("edited by X. Wye and Z. Zed."),
            Some(vec!["X. Wye".to_owned(), "Z. Zed".to_owned()])
        );
        assert_eq!(editors_in("Translated by X and edited by Y"), None);
    }
}
