use std::collections::BTreeSet;

use typesep_style::View;

/// A rule only some documents need, for a construct the base stylesheet does not style. A
/// document gets the rules for the constructs it uses and no others, so one without any of
/// them gets the stylesheet it always had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Construct {
    Right,
    Indent(u8),
    Spaced,
    SmallCaps,
    Overline,
    Dashed,
    MarginNote,
    OutlineOnly,
    Aside,
    Columns,
    Verse,
    Edges,
}

pub fn face_css(face: &str) -> &'static str {
    match face {
        "bold" => "font-weight:bold",
        "italic" => "font-style:italic",
        "bolditalic" => "font-weight:bold;font-style:italic",
        "smallcaps" => "font-variant:small-caps",
        _ => "font-weight:normal;font-style:normal",
    }
}

fn size(view: &View, path: &str) -> String {
    format!("{:.3}", view.size_pt(path) / view.body())
}

fn heading(view: &View, level: u8) -> String {
    let at = |knob: &str| format!("headings.h{level}.{knob}");
    let case = view.str(&at("case"));
    let transform = if case == "none" {
        String::new()
    } else {
        format!("text-transform:{case}case;")
    };
    format!(
        "h{level}{{font-size:{}em;{};text-align:{};{}margin-top:{};margin-bottom:{};color:{};}}",
        size(view, &at("size")),
        face_css(&view.str(&at("style"))),
        view.str(&at("align")),
        transform,
        view.str(&at("spaceBefore")),
        view.str(&at("spaceAfter")),
        view.str(&at("color")),
    )
}

pub fn stylesheet(view: &View) -> String {
    let family = {
        let main = view.font("main").family;
        if main.is_empty() {
            "serif".to_owned()
        } else {
            main
        }
    };
    let leading = view.str("typography.leading");
    let leading = match view.value("typography.leading") {
        Some(typesep_style::Value::Number(_)) => leading,
        _ => "1.4".to_owned(),
    };
    let mut lines = vec![
        format!("body{{font-family:\"{family}\",serif;"),
        format!("line-height:{leading};"),
        format!(
            "text-align:{};",
            if view.bool("typography.justify") {
                "justify"
            } else {
                "left"
            }
        ),
        format!(
            "hyphens:{};margin:0 1em;}}",
            if view.bool("typography.hyphenate") {
                "auto"
            } else {
                "manual"
            }
        ),
        format!(
            "p{{text-indent:{};margin:0 0 {};}}",
            view.str("typography.parIndent"),
            view.str("typography.parSkip")
        ),
        "p.flush,p.center,li>p:first-child,blockquote>p:first-child,td>p,th>p,aside p{text-indent:0;}".to_owned(),
        "p.center{text-align:center;}".to_owned(),
    ];
    lines.extend((1..=4).map(|level| heading(view, level)));
    lines.extend([
        format!(
            "blockquote{{margin-left:{};margin-right:{};font-size:{}em;}}",
            view.str("blocks.blockquote.leftIndent"),
            view.str("blocks.blockquote.rightIndent"),
            size(view, "blocks.blockquote.size")
        ),
        format!(
            "ul.hanging{{list-style:none;padding-left:{};}}",
            view.str("blocks.bibliography.hangingIndent")
        ),
        format!(
            "ul.hanging>li{{text-indent:-{};margin-bottom:{};font-size:{}em;}}",
            view.str("blocks.bibliography.hangingIndent"),
            view.str("blocks.bibliography.itemSep"),
            size(view, "blocks.bibliography.size")
        ),
        "ul.tags{list-style:none;padding-left:3.2em;}".to_owned(),
        "ul.tags>li{text-indent:-3.2em;}".to_owned(),
        "ul.tags>li>.tag{display:inline-block;width:2.6em;margin-right:0.6em;text-indent:0;}"
            .to_owned(),
        format!(
            "code,pre{{font-size:{}em;}}",
            size(view, "blocks.code.size")
        ),
        "pre{white-space:pre-wrap;margin:0.5em 0;}".to_owned(),
        "img{max-width:100%;height:auto;}".to_owned(),
        "figure{margin:1em 0;text-align:center;}".to_owned(),
        format!(
            "figcaption{{font-size:{}em;text-align:center;}}",
            size(view, "blocks.figure.captionSize")
        ),
        "table{border-collapse:collapse;margin:1em auto;}".to_owned(),
        format!(
            "td,th{{padding:0.2em 0.6em;vertical-align:top;font-size:{}em;}}",
            size(view, "blocks.table.size")
        ),
        "th{font-weight:bold;}".to_owned(),
        format!(
            "caption{{font-size:{}em;margin-bottom:0.4em;}}",
            size(view, "blocks.figure.captionSize")
        ),
        format!(
            "section.notes{{font-size:{}em;margin-top:2em;}}",
            size(view, "blocks.footnote.size")
        ),
        "aside.note{margin:0 0 0.6em;}".to_owned(),
        "a.noteref{text-decoration:none;}".to_owned(),
    ]);
    lines.join("\n")
}

pub fn construct_rules(view: &View, used: &BTreeSet<Construct>) -> Vec<String> {
    let mut lines = Vec::new();
    for construct in used {
        match construct {
            Construct::Right => lines.push("p.right{text-align:right;text-indent:0;}".to_owned()),
            Construct::Indent(em) => {
                lines.push(format!("p.indent-{em}{{margin-left:{em}em;text-indent:0;}}"))
            }
            Construct::Spaced => lines.push("span.spaced{letter-spacing:0.2em;margin-right:-0.2em;}".to_owned()),
            Construct::SmallCaps => {
                lines.push("span.smallcaps{font-variant:small-caps;}".to_owned())
            }
            Construct::Overline => {
                lines.push("span.overline{text-decoration-line:overline;}".to_owned())
            }
            Construct::Dashed => lines.push(".dashed{text-decoration-style:dashed;}".to_owned()),
            Construct::MarginNote => lines.push(format!(
                "span.margin-note{{font-size:{}em;}}",
                size(view, "blocks.footnote.size")
            )),
            Construct::OutlineOnly => lines.push(".outline-only{display:none;}".to_owned()),
            Construct::Aside => lines.extend([
                "div.aside{border:1px solid #999;padding:0.6em 1em;margin:1em 0;}".to_owned(),
                "div.aside>p:first-child{text-indent:0;}".to_owned(),
            ]),
            Construct::Columns => lines.extend([
                "div.columns{display:flex;flex-wrap:wrap;gap:0 1.5em;margin:1em 0;}".to_owned(),
                "div.column{flex:1 1 12em;min-width:0;}".to_owned(),
            ]),
            Construct::Verse => lines.extend([
                "div.verse{margin:1em 0;}".to_owned(),
                "div.verse>p{margin-top:0;margin-bottom:0;padding-left:1em;text-indent:-1em;text-align:left;}"
                    .to_owned(),
            ]),
            Construct::Edges => lines.extend(["top", "right", "bottom", "left"].map(|side| {
                format!("td.edge-{side},th.edge-{side}{{border-{side}:1px solid;}}")
            })),
        }
    }
    lines
}
