//! A text of the Wittgenstein Project as the MediaWiki API renders it
//! (`action=parse`, `formatversion=2`), read into the document model.

pub mod colophon;
pub mod rules;

use scraper::{ElementRef, Html};
use serde_json::Value;
use typesep_html::dom;
use typesep_html::js;
use typesep_html::walk::{Walker, plain, trim_inline};
use typesep_model::{Block, Contributor, Doc, EntryLinks, Meta, NoteTable, Origin, TocItem};

use crate::address::{normalize_title, page_url};
use rules::{LwpRules, tidy_blocks};

pub const SOURCE: &str = "lwp";
pub const PUBLISHER: &str = "Ludwig Wittgenstein Project";
/// Whose every text is: the byline when a page does not print one.
pub const AUTHOR: &str = "Ludwig Wittgenstein";

pub struct ParseOptions<'a> {
    /// The title the text was asked for by, which the document is named after.
    pub id: &'a str,
    /// The address of its page.
    pub url: &'a str,
    /// When the library fetched it, an ISO 8601 date.
    pub fetched_at: &'a str,
}

fn string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    match value.get(key)? {
        Value::String(text) => Some(text),
        // formatversion=1 wraps strings as {"*": "…"}.
        Value::Object(map) => map.get("*").and_then(Value::as_str),
        _ => None,
    }
}

fn text_of_html(html: &str) -> String {
    let fragment = Html::parse_fragment(html);
    js::collapse_spaces(js::trim(&dom::text_of(*fragment.root_element())))
}

fn first_with_class<'a>(root: ElementRef<'a>, class: &str) -> Option<ElementRef<'a>> {
    dom::descendants(*root).find(|el| dom::has_class(el, class))
}

fn words(el: ElementRef<'_>) -> String {
    js::collapse_spaces(js::trim(&dom::text_of(*el)))
}

/// The page's own notes: the Cite list at its end, by the id each note's references name.
fn notes(walker: &mut Walker, rules: &mut LwpRules, root: ElementRef<'_>) -> NoteTable {
    let mut out = NoteTable::new();
    for list in dom::descendants(*root).filter(|el| dom::has_class(el, "mw-references-wrap")) {
        for li in dom::descendants_named(*list, &["li"]) {
            let Some(id) = dom::truthy(li.attr("id")) else {
                continue;
            };
            let children =
                match dom::descendants(*li).find(|el| dom::has_class(el, "reference-text")) {
                    Some(text) => walker.inlines(rules, text.children()),
                    None => walker.inlines(rules, li.children()),
                };
            let blocks = tidy_blocks(vec![typesep_html::walk::para(trim_inline(children))]);
            if !blocks.is_empty() {
                out.insert(id.to_owned(), blocks);
            }
        }
    }
    out
}

/// The contents: the headings of the text, those for the outline only among them, a level
/// two heading's level three ones under it.
fn contents(body: &[Block]) -> Vec<TocItem> {
    let mut out: Vec<TocItem> = Vec::new();
    for block in body {
        let Block::Heading {
            level,
            id: Some(id),
            children,
            ..
        } = block
        else {
            continue;
        };
        let item = TocItem {
            text: js::collapse_spaces(js::trim(&plain(children))),
            href: format!("#{id}"),
            children: Vec::new(),
        };
        match out.last_mut() {
            Some(parent) if *level > 2 => parent.children.push(item),
            _ => out.push(item),
        }
    }
    out
}

/// The document a page's API answer holds, or why there is none: the API's error, or an
/// answer that is not one.
pub fn parse_page(answer: &str, opts: &ParseOptions) -> Result<Doc, String> {
    let value: Value =
        serde_json::from_str(answer).map_err(|err| format!("the answer is not JSON ({err})"))?;
    if let Some(error) = value.get("error") {
        let code = string(error, "code").unwrap_or("error");
        let info = string(error, "info").unwrap_or("no reason given");
        return Err(format!("the Wittgenstein Project answered {code}: {info}"));
    }
    let parse = value
        .get("parse")
        .ok_or("the answer holds no parsed page")?;
    let html = string(parse, "text").ok_or("the answer holds no page text")?;
    let revision = parse
        .get("revid")
        .and_then(Value::as_u64)
        .map(|revid| revid.to_string())
        .unwrap_or_default();

    let document = Html::parse_fragment(html);
    let root = first_with_class(document.root_element(), "mw-parser-output")
        .unwrap_or_else(|| document.root_element());
    // The lines of the title page the site prints, among which is the logo.
    let print_only: Vec<ElementRef> = dom::descendants(*root)
        .filter(|el| dom::has_class(el, "custom-print-only"))
        .collect();
    let printed = |pick: &dyn Fn(&ElementRef) -> bool| {
        print_only
            .iter()
            .flat_map(|div| dom::descendants_named(**div, &["p"]))
            .find(|p| pick(p))
            .map(words)
            .filter(|text| !text.is_empty())
    };
    let byline = printed(&|p| {
        !dom::has_class(p, "tl-title")
            && typesep_html::css::get(p, "text-align").is_some_and(|a| a == "center")
    })
    .unwrap_or_else(|| AUTHOR.to_owned());
    let title = string(parse, "displaytitle")
        .map(text_of_html)
        .filter(|title| !title.is_empty())
        .or_else(|| printed(&|p| dom::has_class(p, "tl-title")))
        .or_else(|| string(parse, "title").map(str::to_owned))
        .unwrap_or_else(|| opts.id.replace('_', " "));
    let url = string(parse, "title")
        .and_then(|title| normalize_title(title).ok())
        .map(|title| page_url(&title))
        .unwrap_or_else(|| opts.url.to_owned());
    let colophon = colophon::read(first_with_class(root, "colophon"));

    let mut contributors = vec![Contributor {
        name: byline.clone(),
        role: "author".to_owned(),
    }];
    contributors.extend(colophon.editors.iter().map(|name| Contributor {
        name: name.clone(),
        role: "editor".to_owned(),
    }));

    let mut walker = Walker::new(Some(&url));
    let mut rules = LwpRules::new(&byline);
    let body = walker.read(&mut rules, root);
    let notes = notes(&mut walker, &mut rules, root);
    let mut warnings = walker.warnings();
    warnings.extend(rules.warnings());

    Ok(Doc {
        meta: Meta {
            id: opts.id.to_owned(),
            title,
            contributors,
            byline,
            dateline: String::new(),
            credit: String::new(),
            lang: root.attr("lang").unwrap_or_default().to_owned(),
            origin: Origin {
                source: SOURCE.to_owned(),
                publisher: PUBLISHER.to_owned(),
                url,
                edition: String::new(),
                edition_label: String::new(),
                revision,
                retrieved: opts.fetched_at.chars().take(10).collect(),
            },
            rights: colophon.rights,
            macros: Default::default(),
        },
        front: Vec::new(),
        toc: contents(&body),
        body,
        back: Vec::new(),
        notes,
        links: EntryLinks::default(),
        attachments: Vec::new(),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A page made up for the test, in the shape of the project's pages: its furniture, its
    /// print-only title page and colophon, and one of each of the constructs of its texts.
    const PAGE: &str = r##"<div class="mw-content-ltr mw-parser-output" lang="de" dir="ltr">
<div class="navbar noprint"><a href="/w/index.php/Main_Page">Home</a></div>
<div class="header-lines noprint"></div>
<div class="header-title noprint"><span>Ein Heft</span></div>
<div class="top-arrow noprint"><a href="#top"><img src="/w/images/thumb/e/e1/Pfeil.png/30px-Pfeil.png"/></a></div>
<p class="noprint"><a href="/w/index.php/Ein_Heft_(Baumstruktur)">Baum</a></p>
<p><br />
<span class="mw-default-size custom-print-only lwp-logo"><img src="/w/images/2/29/Logo.png"/></span></p>
<div class="custom-print-only"><p>&#160;</p><p style="text-align:center;">Anna Beispiel</p><p class="tl-title">Ein Heft</p></div>
<div class="colophon"><p class="plainlinks" style="text-align: center;">Herausgegeben von <a href="https://example.org/c">C. Dee</a>, E. Eff</p><p>&#160;</p>
<p>Erfunden für diesen Test. Dieses Werk ist gemeinfrei.</p></div>
<div class="custom-toc noprint"><span class="toc-item"><a href="#1">1.</a></span></div>
<p><br />
</p>
<p style="text-align:center;">Anna Beispiel</p>
<p class="tl-title">Ein Heft</p>
<p style="display: none; font-size: 100%; font-family: -apple-system,&#39;Arial&#39;,sans-serif; text-align: center;"><i>Widmung</i></p>
<p style="text-align: center; font-variant: small-caps;">Für niemanden</p>
<p style="display: block; font-size: 100%; font-family: -apple-system,&#39;Arial&#39;,sans-serif; text-align: center;"><i>Vorwort</i></p>
<p>Ein Vorwort mit <span style="letter-spacing: 0.2em; margin-right: -0.2em;">Sperrung</span>.
</p><p><br />
</p>
<div class="noebook">
<hr />
</div>
<div class="tlp-container">
<p><b><span class="tlp-aside-par" id="1">1</span></b> Erste Bemerkung<sup id="cite_ref-a_1-0" class="reference"><a href="#cite_note-a-1"><span class="cite-bracket">&#91;</span>1<span class="cite-bracket">&#93;</span></a></sup>.
</p><p>Fortsetzung mit <span style="opacity:.5" class="smj-container">[math]\displaystyle{ x^2 }[/math]</span>.
</p>
<figure class="mw-halign-center" typeof="mw:File"><span><img src="/w/images/thumb/a/ab/Bild.png/120px-Bild.png" width="120" /></span><figcaption></figcaption></figure>
<p><br />
<b><span id="1.1">1.1.</span></b> Zweite, mit Tabellen:
</p>
<div class="tlp-flex-container"><div class="tlp-column"><table class="wikitable"><tbody><tr><th>p</th></tr><tr><td>W</td></tr></tbody></table></div>
<div class="tlp-column"><table class="wikitable"><tbody><tr><th>q</th></tr><tr><td>F</td></tr></tbody></table></div></div>
<table style="margin: 0 auto 0 auto;"><tbody><tr><td>links</td><td>rechts</td></tr></tbody></table>
<div style="border: 1px solid silver; padding: 12px 20px; margin: 20px 0;"><p>Im Kasten.</p></div>
<p style="text-align: right;">1931</p>
</div>
<hr />
<p>Nach dem Strich, <span class="tl-check"><sup>1</sup></span>ohne Bemerkung.</p>
<p>Zeile eins<br />
<span class="ebook-ignore-style" style="display: inline-block; margin-left: 2em;">Zeile zwei</span><br />
<span class="ebook-ignore-style" style="display: inline-block; margin-left: 2em;">Zeile drei</span>
</p>
<p><span class="ebook-ignore-style" style="margin-left: -3em; float: left;">ʃ</span>
<span class="custom-desktop-only">|</span><span class="frazer-aside-par" id="Ms-1,2"><i>(Ms-1,2)</i></span> Mit Rand, <u>unter</u>, <s>durch</s>, <span style="text-decoration-line: underline; text-decoration-style: dashed;">gestrichelt</span>.
</p>
<div class="noebook"><figure><span><img src="/w/images/thumb/c/cd/Grau.png/220px-Grau.png" /></span></figure></div>
<div class="ebookonly" style="display: none;"><figure><span><img src="/w/images/thumb/e/ef/Bunt.png/220px-Bunt.png" /></span></figure></div>
<div class="mw-references-wrap"><ol class="references">
<li id="cite_note-a-1"><span class="mw-cite-backlink"><a href="#cite_ref-a_1-0">↑</a></span> <span class="reference-text">Eine <i>Fußnote</i>.</span>
</li>
</ol></div>
<!--
NewPP limit report
-->
</div>"##;

    fn answer(text: &str) -> String {
        json!({
            "parse": {
                "title": "Ein Heft",
                "pageid": 1,
                "revid": 42,
                "text": text,
                "displaytitle": "<span class=\"mw-page-title-main\">Ein Heft</span>",
                "properties": {}
            }
        })
        .to_string()
    }

    fn doc(text: &str) -> Doc {
        parse_page(
            &answer(text),
            &ParseOptions {
                id: "Ein_Heft",
                url: "https://www.wittgensteinproject.org/w/index.php/Ein_Heft",
                fetched_at: "2026-10-02T09:00:00.000Z",
            },
        )
        .unwrap()
    }

    fn t(value: &str) -> serde_json::Value {
        json!({ "type": "text", "value": value })
    }

    #[test]
    fn the_metadata_and_the_rights_come_from_the_page() {
        let doc = doc(PAGE);
        let meta = serde_json::to_value(&doc.meta).unwrap();
        assert_eq!(
            meta,
            json!({
                "id": "Ein_Heft", "title": "Ein Heft", "byline": "Anna Beispiel", "dateline": "", "lang": "de",
                "contributors": [
                    { "name": "Anna Beispiel", "role": "author" },
                    { "name": "C. Dee", "role": "editor" },
                    { "name": "E. Eff", "role": "editor" }
                ],
                "origin": {
                    "source": "lwp", "publisher": "Ludwig Wittgenstein Project",
                    "url": "https://www.wittgensteinproject.org/w/index.php/Ein_Heft",
                    "edition": "", "editionLabel": "", "revision": "42", "retrieved": "2026-10-02"
                },
                "rights": { "statement": "", "terms": "Erfunden für diesen Test. Dieses Werk ist gemeinfrei.", "notices": [] },
                "macros": {}
            })
        );
        assert_eq!(
            serde_json::to_value(&doc.toc).unwrap(),
            json!([
                { "text": "Widmung", "href": "#Widmung", "children": [] },
                { "text": "Vorwort", "href": "#Vorwort", "children": [] }
            ]),
            "the contents list the headings, those for the outline only too"
        );
        assert_eq!(
            doc.warnings,
            ["check marks (the project's notes on its own text) left out"]
        );
    }

    #[test]
    fn the_constructs_of_a_page_are_read_and_its_furniture_left_out() {
        let doc = doc(PAGE);
        let body = serde_json::to_value(&doc.body).unwrap();
        let image = |path: &str| format!("https://www.wittgensteinproject.org/w/images/{path}");
        let cell = |header: bool, value: &str| json!({ "header": header, "colspan": 1, "rowspan": 1, "children": [t(value)] });
        assert_eq!(
            body,
            json!([
                { "type": "heading", "level": 2, "id": "Widmung", "outlineOnly": true, "children": [t("Widmung")] },
                { "type": "para", "align": "center", "children": [{ "type": "smallcaps", "children": [t("Für niemanden")] }] },
                { "type": "heading", "level": 2, "id": "Vorwort", "children": [t("Vorwort")] },
                { "type": "para", "children": [t("Ein Vorwort mit "), { "type": "spaced", "children": [t("Sperrung")] }, t(".")] },
                { "type": "rule" },
                { "type": "remark", "number": "1", "id": "1", "children": [
                    { "type": "para", "children": [
                        t("Erste Bemerkung"),
                        { "type": "noteref", "id": "cite_note-a-1", "label": "1", "href": "#cite_note-a-1", "refId": "cite_ref-a_1-0" },
                        t(".")
                    ] },
                    { "type": "para", "children": [
                        t("Fortsetzung mit "), { "type": "math", "mode": "inline", "tex": "\\displaystyle{ x^2 }" }, t(".")
                    ] },
                    { "type": "figure", "src": image("a/ab/Bild.png"), "alt": "" }
                ] },
                { "type": "remark", "number": "1.1.", "id": "1.1", "children": [
                    { "type": "para", "children": [t("Zweite, mit Tabellen:")] },
                    { "type": "columns", "columns": [
                        [{ "type": "table", "columns": 1, "rows": [[cell(true, "p")], [cell(false, "W")]] }],
                        [{ "type": "table", "columns": 1, "rows": [[cell(true, "q")], [cell(false, "F")]] }]
                    ] },
                    { "type": "table", "rules": "none", "columns": 2, "rows": [[cell(false, "links"), cell(false, "rechts")]] },
                    { "type": "aside", "children": [{ "type": "para", "children": [t("Im Kasten.")] }] },
                    { "type": "para", "align": "right", "children": [t("1931")] }
                ] },
                { "type": "rule" },
                { "type": "para", "children": [t("Nach dem Strich, "), t("ohne Bemerkung.")] },
                { "type": "verse", "lines": [
                    { "indentEm": 0, "children": [t("Zeile eins")] },
                    { "indentEm": 2, "children": [t("Zeile zwei")] },
                    { "indentEm": 2, "children": [t("Zeile drei")] }
                ] },
                { "type": "para", "children": [
                    { "type": "anchor", "id": "Ms-1,2" },
                    { "type": "marginnote", "children": [t("ʃ"), t("\u{a0}"), { "type": "emph", "children": [t("(Ms-1,2)")] }] },
                    t(" Mit Rand, "),
                    { "type": "decorated", "line": "under", "dashed": false, "children": [t("unter")] },
                    t(", "),
                    { "type": "decorated", "line": "strike", "dashed": false, "children": [t("durch")] },
                    t(", "),
                    { "type": "decorated", "line": "under", "dashed": true, "children": [t("gestrichelt")] },
                    t(".")
                ] },
                { "type": "figure", "src": image("c/cd/Grau.png"), "ebookSrc": image("e/ef/Bunt.png"), "alt": "" }
            ])
        );
        assert_eq!(
            serde_json::to_value(&doc.notes).unwrap(),
            json!({ "cite_note-a-1": [{ "type": "para", "children": [t("Eine "), { "type": "emph", "children": [t("Fußnote")] }, t(".")] }] })
        );
    }

    #[test]
    fn a_title_after_a_preface_opens_the_text_and_one_before_it_is_the_cover() {
        let page = |before: &str| {
            format!(
                "<div class=\"mw-parser-output\" lang=\"de\"><div class=\"custom-print-only\">\
                 <p style=\"text-align:center;\">Anna Beispiel</p><p class=\"tl-title\">Ein Heft</p></div>\
                 {before}<p style=\"text-align:center;\">Anna Beispiel</p><p class=\"tl-title\">Ein Heft</p>\
                 <p>Der Text.</p></div>"
            )
        };
        let shown = |doc: &Doc| serde_json::to_value(&doc.body).unwrap();
        assert_eq!(
            shown(&doc(&page("<p><br /></p>"))),
            json!([{ "type": "para", "children": [t("Der Text.")] }])
        );
        assert_eq!(
            shown(&doc(&page("<p>Ein Vorwort.</p>"))),
            json!([
                { "type": "para", "children": [t("Ein Vorwort.")] },
                { "type": "heading", "level": 2, "id": "Ein_Heft", "children": [t("Ein Heft")] },
                { "type": "para", "children": [t("Der Text.")] }
            ])
        );
    }

    #[test]
    fn an_answer_that_is_no_page_is_said_to_be_none() {
        let opts = ParseOptions {
            id: "X",
            url: "u",
            fetched_at: "",
        };
        assert_eq!(
            parse_page(
                &json!({ "error": { "code": "missingtitle", "info": "The page you specified doesn't exist." } })
                    .to_string(),
                &opts
            )
            .unwrap_err(),
            "the Wittgenstein Project answered missingtitle: The page you specified doesn't exist."
        );
        assert!(
            parse_page("<html>", &opts)
                .unwrap_err()
                .starts_with("the answer is not JSON")
        );
        assert_eq!(
            parse_page("{}", &opts).unwrap_err(),
            "the answer holds no parsed page"
        );
        let bare = parse_page(&answer("<p>Nur Text.</p>"), &opts).unwrap();
        assert_eq!(
            bare.meta.byline, AUTHOR,
            "a page without a title page is still Wittgenstein's"
        );
        assert_eq!(
            bare.meta.rights,
            Default::default(),
            "and without a colophon states no rights"
        );
        assert_eq!(bare.meta.lang, "");
    }
}
