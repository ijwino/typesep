pub mod canonical;
pub mod walk;

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

fn some_of<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MathMode {
    Inline,
    Display,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteRef {
    pub id: String,
    pub label: String,
    pub href: String,
    pub ref_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Figure {
    pub src: String,
    pub alt: String,
    /// The image an ebook shows in place of `src`, as a colour copy of a figure printed in
    /// black and white.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ebook_src: Option<String>,
    /// The width the source shows the figure at, in CSS pixels, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_px: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<Vec<Inline>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "some_of"
    )]
    pub id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Inline {
    Text {
        value: String,
    },
    Math {
        mode: MathMode,
        tex: String,
    },
    Emph {
        children: Vec<Inline>,
    },
    Strong {
        children: Vec<Inline>,
    },
    Sub {
        children: Vec<Inline>,
    },
    Sup {
        children: Vec<Inline>,
    },
    Code {
        children: Vec<Inline>,
    },
    Break,
    Anchor {
        id: String,
    },
    Link {
        href: String,
        internal: bool,
        children: Vec<Inline>,
    },
    #[serde(rename = "noteref")]
    NoteRef(NoteRef),
    Figure(Figure),
    /// Letter-spaced text, the German Sperrsatz.
    Spaced {
        children: Vec<Inline>,
    },
    SmallCaps {
        children: Vec<Inline>,
    },
    Decorated {
        line: Decoration,
        dashed: bool,
        children: Vec<Inline>,
    },
    /// Text set in the margin beside the line it belongs to, as a manuscript siglum.
    MarginNote {
        children: Vec<Inline>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decoration {
    Under,
    Over,
    Strike,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Center,
    Right,
}

/// The direction a script is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    #[default]
    Ltr,
    Rtl,
}

impl Dir {
    pub fn is_ltr(&self) -> bool {
        *self == Dir::Ltr
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ListNumbering {
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct List {
    pub ordered: bool,
    pub hanging: bool,
    pub items: Vec<Vec<Block>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numbering: Option<ListNumbering>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<Vec<Inline>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableCell {
    pub header: bool,
    pub colspan: u32,
    pub rowspan: u32,
    pub children: Vec<Inline>,
    /// The sides of the cell that are ruled, read when the table's `rules` are `Cells`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Edges>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Edges {
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
    pub left: bool,
}

/// Where a table's rules come from when the source says. Without it, the style decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rules {
    /// A table that only lays its cells out, with no rules at all.
    None,
    /// The rules the cells' `edges` name, and no others.
    Cells,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub rows: Vec<Vec<TableCell>>,
    pub columns: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<Vec<Inline>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Rules>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerseLine {
    pub indent_em: u8,
    pub children: Vec<Inline>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Block {
    Para {
        children: Vec<Inline>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<Align>,
        /// How far every line of the paragraph is set in from the left, in ems.
        #[serde(default, skip_serializing_if = "is_zero", rename = "indentEm")]
        indent_em: u8,
    },
    Pre {
        children: Vec<Inline>,
    },
    Heading {
        level: u8,
        id: Option<String>,
        children: Vec<Inline>,
        /// A heading that is in the outline (the PDF bookmarks, an ebook's contents) but not
        /// printed.
        #[serde(default, skip_serializing_if = "is_false", rename = "outlineOnly")]
        outline_only: bool,
    },
    List(List),
    Blockquote {
        children: Vec<Block>,
    },
    Table(Table),
    Figure(Figure),
    Rule,
    Anchor {
        id: String,
    },
    /// A numbered remark: its number is printed at the start of its first paragraph, and its
    /// id is where links to it land.
    Remark {
        number: String,
        id: Option<String>,
        children: Vec<Block>,
    },
    /// A boxed insert.
    Aside {
        children: Vec<Block>,
    },
    /// Blocks set side by side, one list of blocks to a column.
    Columns {
        columns: Vec<Vec<Block>>,
    },
    Verse {
        lines: Vec<VerseLine>,
    },
    /// Blocks in a language other than the document's, as one side of a bilingual edition
    /// is: `lang` an ISO 639 code that may carry a region (`de`, `en-GB`), and the direction
    /// its script is written in when it is not the document's.
    Lang {
        lang: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dir: Option<Dir>,
        children: Vec<Block>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroDef {
    pub body: String,
    pub args: u32,
}

pub type MacroTable = BTreeMap<String, MacroDef>;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Contributor {
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    pub source: String,
    pub publisher: String,
    pub url: String,
    pub edition: String,
    pub edition_label: String,
    pub revision: String,
    pub retrieved: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticeLink {
    pub url: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<NoticeLink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rights {
    pub statement: String,
    pub terms: String,
    pub notices: Vec<Notice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    pub title: String,
    pub contributors: Vec<Contributor>,
    pub byline: String,
    pub dateline: String,
    /// What a cover names in place of the byline when there is none, as a volume of several
    /// works has.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub credit: String,
    pub lang: String,
    /// The direction the document's script is written in, left to right unless it says.
    #[serde(default, skip_serializing_if = "Dir::is_ltr")]
    pub dir: Dir,
    pub origin: Origin,
    pub rights: Rights,
    pub macros: MacroTable,
}

/// A line of the contents. Its label is running text as a heading's is, without what only
/// running text holds: no note references, anchors or figures, and no links, since the line
/// is a link itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TocItem {
    pub label: Vec<Inline>,
    pub href: String,
    pub children: Vec<TocItem>,
}

impl TocItem {
    /// The line's text, its markup left out and a formula as its TeX: what the line is
    /// matched by against the names of the end matter a document leaves out.
    pub fn text(&self) -> String {
        fn text_of(nodes: &[Inline], out: &mut String) {
            for node in nodes {
                match node {
                    Inline::Text { value } => out.push_str(value),
                    Inline::Math { tex, .. } => out.push_str(tex.trim()),
                    Inline::Break => out.push(' '),
                    Inline::Anchor { .. } | Inline::Figure(_) | Inline::NoteRef(_) => {}
                    Inline::Emph { children }
                    | Inline::Strong { children }
                    | Inline::Sub { children }
                    | Inline::Sup { children }
                    | Inline::Code { children }
                    | Inline::Link { children, .. }
                    | Inline::Spaced { children }
                    | Inline::SmallCaps { children }
                    | Inline::Decorated { children, .. }
                    | Inline::MarginNote { children } => text_of(children, out),
                }
            }
        }
        let mut out = String::new();
        text_of(&self.label, &mut out);
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub key: String,
    pub label: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Notes,
    Supplement,
}

pub type NoteTable = BTreeMap<String, Vec<Block>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub kind: AttachmentKind,
    pub title: String,
    pub blocks: Vec<Block>,
    pub source_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<NoteTable>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EntryLinks {
    pub notes: Vec<String>,
    pub supplements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Doc {
    pub meta: Meta,
    pub front: Vec<Block>,
    pub toc: Vec<TocItem>,
    pub body: Vec<Block>,
    pub back: Vec<Section>,
    /// The notes the document holds itself, by the id its note references name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub notes: NoteTable,
    pub links: EntryLinks,
    pub attachments: Vec<Attachment>,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn text(value: &str) -> Inline {
        Inline::Text {
            value: value.to_owned(),
        }
    }

    #[test]
    fn nodes_serialise_to_their_json_value() {
        let doc = Doc {
            meta: Meta {
                id: "x".into(),
                contributors: vec![Contributor {
                    name: "A".into(),
                    role: "author".into(),
                }],
                rights: Rights {
                    notices: vec![
                        Notice {
                            text: "n".into(),
                            link: Some(NoticeLink {
                                url: "u".into(),
                                label: "l".into(),
                            }),
                        },
                        Notice {
                            text: "m".into(),
                            link: None,
                        },
                    ],
                    ..Default::default()
                },
                macros: BTreeMap::from([(
                    "opL".to_owned(),
                    MacroDef {
                        body: "\\mathbin{L}".into(),
                        args: 0,
                    },
                )]),
                ..Default::default()
            },
            body: vec![
                Block::Heading {
                    level: 2,
                    id: None,
                    children: vec![text("Heading")],
                    outline_only: false,
                },
                Block::Para {
                    align: Some(Align::Center),
                    indent_em: 0,
                    children: vec![
                        Inline::Emph {
                            children: vec![text("e")],
                        },
                        Inline::Break,
                        Inline::Math {
                            mode: MathMode::Display,
                            tex: "x^2".into(),
                        },
                        Inline::Link {
                            href: "#sec".into(),
                            internal: true,
                            children: vec![],
                        },
                        Inline::NoteRef(NoteRef {
                            id: "n1".into(),
                            label: "1".into(),
                            href: "notes.html#1".into(),
                            ref_id: None,
                        }),
                        Inline::Figure(Figure {
                            src: "https://plato.stanford.edu/entries/x/f.png".into(),
                            alt: "".into(),
                            ebook_src: None,
                            width_px: None,
                            caption: None,
                            id: None,
                            inline: Some(true),
                        }),
                    ],
                },
                Block::List(List {
                    ordered: true,
                    hanging: false,
                    items: vec![vec![Block::Rule]],
                    numbering: Some(ListNumbering::LowerAlpha),
                    start: Some(3),
                    labels: None,
                }),
                Block::Figure(Figure {
                    src: "s".into(),
                    alt: "a".into(),
                    ebook_src: None,
                    width_px: None,
                    caption: Some(vec![]),
                    id: Some(None),
                    inline: None,
                }),
                Block::Table(Table {
                    rows: vec![vec![TableCell {
                        header: true,
                        colspan: 1,
                        rowspan: 2,
                        children: vec![],
                        edges: None,
                    }]],
                    columns: 1,
                    caption: None,
                    rules: None,
                }),
                Block::Blockquote {
                    children: vec![
                        Block::Pre { children: vec![] },
                        Block::Anchor { id: "a".into() },
                    ],
                },
            ],
            toc: vec![TocItem {
                label: vec![
                    text("1. "),
                    Inline::Math {
                        mode: MathMode::Inline,
                        tex: "p".into(),
                    },
                ],
                href: "#s1".into(),
                children: vec![],
            }],
            back: vec![Section {
                key: "related".into(),
                label: "Related Entries".into(),
                blocks: vec![],
            }],
            attachments: vec![Attachment {
                kind: AttachmentKind::Notes,
                title: "Notes".into(),
                blocks: vec![],
                source_url: "u".into(),
                notes: Some(BTreeMap::new()),
            }],
            ..Default::default()
        };
        let value = serde_json::to_value(&doc).unwrap();
        let expected = json!({
            "meta": {
                "id": "x", "title": "", "contributors": [{ "name": "A", "role": "author" }], "byline": "", "dateline": "", "lang": "",
                "origin": { "source": "", "publisher": "", "url": "", "edition": "", "editionLabel": "", "revision": "", "retrieved": "" },
                "rights": { "statement": "", "terms": "", "notices": [{ "text": "n", "link": { "url": "u", "label": "l" } }, { "text": "m" }] },
                "macros": { "opL": { "body": "\\mathbin{L}", "args": 0 } }
            },
            "front": [],
            "toc": [{ "label": [{ "type": "text", "value": "1. " }, { "type": "math", "mode": "inline", "tex": "p" }], "href": "#s1", "children": [] }],
            "body": [
                { "type": "heading", "level": 2, "id": null, "children": [{ "type": "text", "value": "Heading" }] },
                { "type": "para", "align": "center", "children": [
                    { "type": "emph", "children": [{ "type": "text", "value": "e" }] },
                    { "type": "break" },
                    { "type": "math", "mode": "display", "tex": "x^2" },
                    { "type": "link", "href": "#sec", "internal": true, "children": [] },
                    { "type": "noteref", "id": "n1", "label": "1", "href": "notes.html#1", "refId": null },
                    { "type": "figure", "src": "https://plato.stanford.edu/entries/x/f.png", "alt": "", "inline": true }
                ] },
                { "type": "list", "ordered": true, "hanging": false, "items": [[{ "type": "rule" }]], "numbering": "lower-alpha", "start": 3 },
                { "type": "figure", "src": "s", "alt": "a", "caption": [], "id": null },
                { "type": "table", "rows": [[{ "header": true, "colspan": 1, "rowspan": 2, "children": [] }]], "columns": 1 },
                { "type": "blockquote", "children": [{ "type": "pre", "children": [] }, { "type": "anchor", "id": "a" }] }
            ],
            "back": [{ "key": "related", "label": "Related Entries", "blocks": [] }],
            "links": { "notes": [], "supplements": [] },
            "attachments": [{ "kind": "notes", "title": "Notes", "blocks": [], "sourceUrl": "u", "notes": {} }],
            "warnings": []
        });
        assert_eq!(value, expected);
        let read: Doc = serde_json::from_value(value).unwrap();
        assert_eq!(read, doc, "the value round-trips");

        assert!(
            serde_json::to_value(&doc.meta)
                .unwrap()
                .get("dir")
                .is_none(),
            "a document written left to right does not say so"
        );
        let rtl = Meta {
            dir: Dir::Rtl,
            ..Default::default()
        };
        let rtl_value = serde_json::to_value(&rtl).unwrap();
        assert_eq!(rtl_value["dir"], json!("rtl"));
        assert_eq!(serde_json::from_value::<Meta>(rtl_value).unwrap(), rtl);

        let volume = Meta {
            credit: "2 entries".into(),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&volume).unwrap()["credit"],
            json!("2 entries"),
            "a credit is written only where there is one"
        );
    }

    #[test]
    fn the_constructs_of_other_sources_serialise_to_their_json_value() {
        let doc = Doc {
            body: vec![
                Block::Heading {
                    level: 2,
                    id: Some("h".into()),
                    children: vec![text("Hidden")],
                    outline_only: true,
                },
                Block::Remark {
                    number: "2.0121".into(),
                    id: Some("2.0121".into()),
                    children: vec![Block::Para {
                        children: vec![
                            Inline::Spaced {
                                children: vec![text("s")],
                            },
                            Inline::SmallCaps {
                                children: vec![text("c")],
                            },
                            Inline::Decorated {
                                line: Decoration::Under,
                                dashed: true,
                                children: vec![text("u")],
                            },
                            Inline::Decorated {
                                line: Decoration::Over,
                                dashed: false,
                                children: vec![],
                            },
                            Inline::Decorated {
                                line: Decoration::Strike,
                                dashed: false,
                                children: vec![],
                            },
                            Inline::MarginNote {
                                children: vec![text("(Ms-110,177)")],
                            },
                            Inline::NoteRef(NoteRef {
                                id: "cite_note-1".into(),
                                label: "1".into(),
                                href: "#cite_note-1".into(),
                                ref_id: Some("cite_ref-1".into()),
                            }),
                        ],
                        align: Some(Align::Right),
                        indent_em: 3,
                    }],
                },
                Block::Remark {
                    number: "".into(),
                    id: None,
                    children: vec![],
                },
                Block::Aside {
                    children: vec![Block::Rule],
                },
                Block::Columns {
                    columns: vec![vec![Block::Rule], vec![]],
                },
                Block::Verse {
                    lines: vec![
                        VerseLine {
                            indent_em: 0,
                            children: vec![text("a")],
                        },
                        VerseLine {
                            indent_em: 2,
                            children: vec![],
                        },
                    ],
                },
                Block::Table(Table {
                    rows: vec![vec![TableCell {
                        header: false,
                        colspan: 1,
                        rowspan: 1,
                        children: vec![],
                        edges: Some(Edges {
                            top: true,
                            right: false,
                            bottom: true,
                            left: false,
                        }),
                    }]],
                    columns: 1,
                    caption: None,
                    rules: Some(Rules::Cells),
                }),
                Block::Table(Table {
                    rows: vec![],
                    columns: 0,
                    caption: None,
                    rules: Some(Rules::None),
                }),
                Block::Figure(Figure {
                    src: "bw.png".into(),
                    alt: "".into(),
                    ebook_src: Some("colour.png".into()),
                    width_px: Some(120),
                    caption: None,
                    id: None,
                    inline: None,
                }),
                Block::Lang {
                    lang: "ar".into(),
                    dir: Some(Dir::Rtl),
                    children: vec![Block::Rule],
                },
                Block::Lang {
                    lang: "de".into(),
                    dir: None,
                    children: vec![],
                },
            ],
            notes: BTreeMap::from([("cite_note-1".to_owned(), vec![Block::Rule])]),
            ..Default::default()
        };
        let value = serde_json::to_value(&doc).unwrap();
        assert_eq!(
            value["body"],
            json!([
                { "type": "heading", "level": 2, "id": "h", "children": [{ "type": "text", "value": "Hidden" }], "outlineOnly": true },
                { "type": "remark", "number": "2.0121", "id": "2.0121", "children": [
                    { "type": "para", "align": "right", "indentEm": 3, "children": [
                        { "type": "spaced", "children": [{ "type": "text", "value": "s" }] },
                        { "type": "smallcaps", "children": [{ "type": "text", "value": "c" }] },
                        { "type": "decorated", "line": "under", "dashed": true, "children": [{ "type": "text", "value": "u" }] },
                        { "type": "decorated", "line": "over", "dashed": false, "children": [] },
                        { "type": "decorated", "line": "strike", "dashed": false, "children": [] },
                        { "type": "marginnote", "children": [{ "type": "text", "value": "(Ms-110,177)" }] },
                        { "type": "noteref", "id": "cite_note-1", "label": "1", "href": "#cite_note-1", "refId": "cite_ref-1" }
                    ] }
                ] },
                { "type": "remark", "number": "", "id": null, "children": [] },
                { "type": "aside", "children": [{ "type": "rule" }] },
                { "type": "columns", "columns": [[{ "type": "rule" }], []] },
                { "type": "verse", "lines": [
                    { "indentEm": 0, "children": [{ "type": "text", "value": "a" }] },
                    { "indentEm": 2, "children": [] }
                ] },
                { "type": "table", "rules": "cells", "columns": 1, "rows": [[
                    { "header": false, "colspan": 1, "rowspan": 1, "children": [], "edges": { "top": true, "right": false, "bottom": true, "left": false } }
                ]] },
                { "type": "table", "rules": "none", "columns": 0, "rows": [] },
                { "type": "figure", "src": "bw.png", "alt": "", "ebookSrc": "colour.png", "widthPx": 120 },
                { "type": "lang", "lang": "ar", "dir": "rtl", "children": [{ "type": "rule" }] },
                { "type": "lang", "lang": "de", "children": [] }
            ])
        );
        assert_eq!(
            value["notes"],
            json!({ "cite_note-1": [{ "type": "rule" }] })
        );
        let read: Doc = serde_json::from_value(value).unwrap();
        assert_eq!(read, doc, "the value round-trips");

        let plain = Doc {
            body: vec![
                Block::Heading {
                    level: 1,
                    id: None,
                    children: vec![],
                    outline_only: false,
                },
                Block::Para {
                    children: vec![],
                    align: None,
                    indent_em: 0,
                },
            ],
            ..Default::default()
        };
        let value = serde_json::to_value(&plain).unwrap();
        assert_eq!(
            value["body"],
            json!([
                { "type": "heading", "level": 1, "id": null, "children": [] },
                { "type": "para", "children": [] }
            ]),
            "a field at its default is not written"
        );
        assert!(value.get("notes").is_none(), "{value}");
        let without: Doc = serde_json::from_value(value).unwrap();
        assert_eq!(without, plain, "and reads back as its default");
    }

    #[test]
    fn a_figure_id_is_absent_null_or_a_string() {
        let figure = |text: &str| serde_json::from_str::<Figure>(text).unwrap().id;
        assert_eq!(figure(r#"{"src":"s","alt":""}"#), None);
        assert_eq!(figure(r#"{"src":"s","alt":"","id":null}"#), Some(None));
        assert_eq!(
            figure(r#"{"src":"s","alt":"","id":"fig1"}"#),
            Some(Some("fig1".to_owned()))
        );
        let heading: Block = serde_json::from_value(
            json!({ "type": "heading", "level": 1, "id": null, "children": [] }),
        )
        .unwrap();
        assert!(matches!(heading, Block::Heading { id: None, .. }));
        let lenient: Block =
            serde_json::from_value(json!({ "type": "heading", "level": 1, "children": [] }))
                .unwrap();
        assert_eq!(
            serde_json::to_value(&lenient).unwrap()["id"],
            Value::Null,
            "read leniently, a heading is still written with its id"
        );
        assert_eq!(
            serde_json::to_value(Block::Rule).unwrap(),
            json!({ "type": "rule" })
        );
        assert!(serde_json::from_value::<Value>(json!(null)).is_ok());
    }

    #[test]
    fn a_contents_line_reads_as_its_text_markup_left_out() {
        let item = TocItem {
            label: vec![
                text("5. Common "),
                Inline::Math {
                    mode: MathMode::Inline,
                    tex: " p ".into(),
                },
                text("-Belief in "),
                Inline::Emph {
                    children: vec![text("Games")],
                },
            ],
            href: "#5".into(),
            children: vec![],
        };
        assert_eq!(item.text(), "5. Common p-Belief in Games");
    }
}
