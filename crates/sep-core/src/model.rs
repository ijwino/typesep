use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

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
pub struct Figure {
    pub src: String,
    pub alt: String,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Center,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub rows: Vec<Vec<TableCell>>,
    pub columns: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<Vec<Inline>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Block {
    Para {
        children: Vec<Inline>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<Align>,
    },
    Pre {
        children: Vec<Inline>,
    },
    Heading {
        level: u8,
        id: Option<String>,
        children: Vec<Inline>,
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
    pub origin: Origin,
    pub rights: Rights,
    pub macros: MacroTable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TocItem {
    pub text: String,
    pub href: String,
    pub children: Vec<TocItem>,
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
                },
                Block::Para {
                    align: Some(Align::Center),
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
                    }]],
                    columns: 1,
                    caption: None,
                }),
                Block::Blockquote {
                    children: vec![
                        Block::Pre { children: vec![] },
                        Block::Anchor { id: "a".into() },
                    ],
                },
            ],
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
            "toc": [],
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
}
