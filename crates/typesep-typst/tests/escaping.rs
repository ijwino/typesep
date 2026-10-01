use typesep_model::{
    Align, Block, Decoration, Doc, Edges, Inline, List, ListNumbering, Meta, Rules, Table,
    TableCell, VerseLine,
};
use typesep_style::{ResolveOptions, resolve_style};
use typesep_typst::{EmitOptions, Fonts, SepWorld, emit_document};
use typst::foundations::{Element, Selector, Value};
use typst::introspection::Introspector;
use typst::layout::{Frame, FrameItem};
use typst::model::HeadingElem;
use typst_layout::PagedDocument;

const START: &str = "QQSTART";
const END: &str = "QQEND";
const STRUCTURE: &str = "#show list: it => \"<list>\"\n#show enum: it => \"<enum>\"\n#show terms: it => \"<terms>\"\n#show heading: it => \"<heading>\"";

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.to_owned(),
    }
}

fn para(children: Vec<Inline>) -> Block {
    Block::Para {
        children,
        align: None,
        indent_em: 0,
    }
}

fn pre(value: &str) -> Block {
    Block::Pre {
        children: vec![text(value)],
    }
}

fn code(value: &str) -> Inline {
    Inline::Code {
        children: vec![text(value)],
    }
}

fn plain_inlines(nodes: &[Inline], out: &mut String) {
    for node in nodes {
        match node {
            Inline::Text { value } => out.push_str(value),
            Inline::Emph { children }
            | Inline::Strong { children }
            | Inline::Sub { children }
            | Inline::Sup { children }
            | Inline::Code { children }
            | Inline::Link { children, .. }
            | Inline::Spaced { children }
            | Inline::SmallCaps { children }
            | Inline::Decorated { children, .. }
            | Inline::MarginNote { children } => plain_inlines(children, out),
            _ => {}
        }
    }
}

fn plain(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Para { children, .. } | Block::Pre { children } => {
                plain_inlines(children, &mut out)
            }
            Block::Blockquote { children } | Block::Aside { children } => {
                out.push_str(&plain(children))
            }
            Block::Remark {
                number, children, ..
            } => {
                out.push_str(number);
                out.push_str(&plain(children));
            }
            Block::Columns { columns } => {
                for column in columns {
                    out.push_str(&plain(column));
                }
            }
            Block::Verse { lines } => {
                for line in lines {
                    plain_inlines(&line.children, &mut out);
                }
            }
            Block::Heading {
                outline_only: true, ..
            }
            | Block::Rule => {}
            Block::Table(table) => {
                for cell in table.rows.iter().flatten() {
                    plain_inlines(&cell.children, &mut out);
                }
            }
            _ => panic!("the harness reads paragraphs, code blocks, quotes and the new blocks"),
        }
    }
    out
}

fn walk(frame: &Frame, out: &mut String) {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Group(group) => walk(&group.frame, out),
            FrameItem::Text(run) => out.push_str(run.text.as_str()),
            _ => {}
        }
    }
}

/// The text without its spaces, and with what typesetting (not markup) does to it undone:
/// Typst sets straight quotes as typographic ones and leaves a soft hyphen where it broke a
/// word at the end of a line.
fn squeeze(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{ad}')
        .map(|c| match c {
            '“' | '”' => '"',
            '‘' | '’' => '\'',
            c => c,
        })
        .collect()
}

fn marked(blocks: &[Block]) -> Doc {
    let mut body = vec![para(vec![text(START)])];
    body.extend(blocks.iter().cloned());
    body.push(para(vec![text(END)]));
    Doc {
        meta: Meta {
            id: "escaping".into(),
            title: "Escaping".into(),
            ..Meta::default()
        },
        body,
        ..Doc::default()
    }
}

/// The Typst source `doc` is emitted as under `sets`, and the document it compiles to.
fn compile(doc: &Doc, sets: &[&str]) -> (String, Result<PagedDocument, String>) {
    let style = resolve_style(ResolveOptions {
        preset: Some("halfletter"),
        file: None,
        sets: sets.iter().map(|s| (*s).into()).collect(),
    })
    .unwrap();
    let fonts = Fonts::embedded();
    let sections = ["notes".to_owned()].into();
    let rendered = emit_document(
        &[doc],
        &style,
        EmitOptions {
            fonts: Some(&fonts),
            sections: &sections,
            ..Default::default()
        },
    );
    let source = rendered
        .source
        .replacen(START, &format!("{START}\n\n{STRUCTURE}\n\n"), 1);
    let world = SepWorld::new(source.clone(), &rendered.figures, &fonts);
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|errors| {
            errors
                .iter()
                .map(|d| d.message.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        });
    (source, document)
}

fn all_text(document: &PagedDocument) -> String {
    let mut out = String::new();
    for page in document.pages() {
        walk(&page.frame, &mut out);
    }
    out
}

fn typeset(blocks: &[Block]) -> Result<String, String> {
    let document = compile(&marked(blocks), &[]).1?;
    let out = all_text(&document);
    let from = out.find(START).ok_or("the start marker is missing")? + START.len();
    let to = out.rfind(END).ok_or("the end marker is missing")?;
    Ok(out[from..to].to_owned())
}

fn assert_literal(cases: Vec<Vec<Block>>) {
    let mut failures = Vec::new();
    for blocks in cases {
        let expected = squeeze(&plain(&blocks));
        match typeset(&blocks) {
            Ok(got) if squeeze(&got) == expected => {}
            Ok(got) => failures.push(format!("{blocks:?}\n  typeset as {got:?}")),
            Err(error) => failures.push(format!("{blocks:?}\n  failed: {error}")),
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

fn paras(texts: &[&str]) -> Vec<Vec<Block>> {
    texts.iter().map(|t| vec![para(vec![text(t)])]).collect()
}

#[test]
fn markup_in_text_stays_text() {
    assert_literal(paras(&[
        "- item",
        "+ plus",
        "= heading",
        "/ term: x",
        "12. twelve",
        "== second level",
        "=\u{a0}Heading?",
        "-\u{a0}x",
        "+\u{2003}y",
        "/\u{a0}term: z",
        "7.\u{a0}seven",
        "a *b* _c_ #d $e$ `f` <g> @h [i] ~j",
        "C:\\dir and back\\",
        "soft -? hyphen",
        "see https://x.test/a//b /* not a comment */",
    ]));
}

#[test]
fn a_comment_is_not_opened_across_text_nodes() {
    assert_literal(vec![
        vec![para(vec![text("see a /"), text("/ b and the rest")])],
        vec![para(vec![
            Inline::Emph {
                children: vec![text("x /"), text("/ y")],
            },
            text(" after"),
        ])],
        vec![para(vec![
            Inline::Anchor { id: "x".into() },
            text("a /"),
            Inline::Anchor { id: "x".into() },
            text("/ b"),
        ])],
        vec![para(vec![code("p /"), text("/ q")])],
        vec![para(vec![
            Inline::Code {
                children: vec![text("r /"), text("/ s")],
            },
            text(" t"),
        ])],
    ]);
}

#[test]
fn markup_in_code_stays_text() {
    assert_literal(vec![
        vec![para(vec![
            code("a*b_c#d$e"),
            text(" and "),
            code("\"q\" it's"),
        ])],
        vec![para(vec![code("x // y /* z */")])],
        vec![para(vec![code("a... b -? c -1 d ~ e -- f --- g")])],
        vec![para(vec![code("line\n- item\n= x\n\n+ y")])],
        vec![pre("a... b -? c -1 d ~ e -- f")],
        vec![pre("fn main() {\n    let a = [1, 2]; // #x\n}")],
        vec![pre("- first line")],
        vec![pre("= first line")],
        vec![pre("line one\n- item\n= head\n+ plus\n1. one\n/ term: x")],
        vec![Block::Pre {
            children: vec![text("a"), Inline::Break, text("  - b")],
        }],
    ]);
}

#[test]
fn a_call_is_not_continued_by_the_text_after_it() {
    assert_literal(vec![
        vec![para(vec![code("f"), text("(x) and [y]")])],
        vec![para(vec![code("\\"), text("(set difference)")])],
        vec![para(vec![code("a\\\\"), text("[b]")])],
        vec![para(vec![text("escaped \\"), text("(c)")])],
        vec![para(vec![
            Inline::Emph {
                children: vec![text("e")],
            },
            text(".field"),
        ])],
    ]);
}

#[test]
fn an_ordered_list_may_start_below_one() {
    for (start, numbering) in [
        (-3, None),
        (0, Some(ListNumbering::Decimal)),
        (-1, Some(ListNumbering::LowerAlpha)),
        (0, Some(ListNumbering::UpperRoman)),
    ] {
        let list = Block::List(List {
            ordered: true,
            hanging: false,
            items: vec![vec![para(vec![text("first")])]],
            numbering,
            start: Some(start),
            labels: None,
        });
        assert_eq!(
            typeset(&[list]).map(|t| squeeze(&t)),
            Ok("<enum>".to_owned()),
            "start {start}, {numbering:?}"
        );
    }
}

/// Text that is markup to Typst, to come out as written wherever the constructs of other
/// sources put text.
const HOSTILE: &[&str] = &[
    "a *b* _c_ #d $e$ `f` <g> @h [i] ~j",
    "close ] and open [",
    "#strong[x] and #let y = 1",
    "see https://x.test/a//b /* not a comment */",
    "C:\\dir and back\\",
    "a backslash then a newline \\\nand on",
    "\"quoted\" <label> @label",
    "= heading",
    "- item",
    "12. twelve",
    "/ term: x",
    "$ x^2 $",
];

fn cell(value: &str, edges: Option<Edges>) -> TableCell {
    TableCell {
        header: false,
        colspan: 1,
        rowspan: 1,
        children: vec![text(value)],
        edges,
    }
}

/// `value` in every place a construct of another source sets text.
fn every_construct(value: &str) -> Vec<Block> {
    let inline = |wrap: fn(Vec<Inline>) -> Inline| para(vec![text("x "), wrap(vec![text(value)])]);
    vec![
        Block::Remark {
            number: value.to_owned(),
            id: Some(value.to_owned()),
            children: vec![para(vec![text(value)]), para(vec![text(value)])],
        },
        Block::Remark {
            number: value.to_owned(),
            id: None,
            children: vec![Block::Rule],
        },
        para(vec![
            text("before "),
            Inline::MarginNote {
                children: vec![text(value)],
            },
            text(" after"),
        ]),
        para(vec![
            Inline::MarginNote {
                children: vec![text(value)],
            },
            text(value),
        ]),
        inline(|children| Inline::Spaced { children }),
        inline(|children| Inline::SmallCaps { children }),
        inline(|children| Inline::Decorated {
            line: Decoration::Under,
            dashed: true,
            children,
        }),
        inline(|children| Inline::Decorated {
            line: Decoration::Over,
            dashed: false,
            children,
        }),
        inline(|children| Inline::Decorated {
            line: Decoration::Strike,
            dashed: false,
            children,
        }),
        Block::Para {
            children: vec![text(value)],
            align: Some(Align::Right),
            indent_em: 0,
        },
        Block::Para {
            children: vec![text(value)],
            align: Some(Align::Right),
            indent_em: 2,
        },
        Block::Para {
            children: vec![text(value)],
            align: None,
            indent_em: 3,
        },
        Block::Verse {
            lines: vec![
                VerseLine {
                    indent_em: 0,
                    children: vec![text(value)],
                },
                VerseLine {
                    indent_em: 0,
                    children: vec![],
                },
                VerseLine {
                    indent_em: 2,
                    children: vec![text(value)],
                },
            ],
        },
        Block::Aside {
            children: vec![para(vec![text(value)]), para(vec![text(value)])],
        },
        Block::Columns {
            columns: vec![vec![para(vec![text(value)])], vec![para(vec![text(value)])]],
        },
        Block::Heading {
            level: 2,
            id: Some(value.to_owned()),
            children: vec![text(value)],
            outline_only: true,
        },
        Block::Table(Table {
            rows: vec![vec![
                cell(
                    value,
                    Some(Edges {
                        top: true,
                        right: false,
                        bottom: true,
                        left: false,
                    }),
                ),
                cell(value, None),
            ]],
            columns: 2,
            caption: None,
            rules: Some(Rules::Cells),
        }),
        Block::Table(Table {
            rows: vec![vec![cell(value, None)]],
            columns: 1,
            caption: None,
            rules: Some(Rules::None),
        }),
    ]
}

#[test]
fn every_construct_of_another_source_typesets_with_its_text() {
    assert_literal(vec![every_construct("plain words")]);
}

#[test]
fn markup_in_the_constructs_of_other_sources_stays_text() {
    assert_literal(HOSTILE.iter().map(|value| every_construct(value)).collect());
}

#[test]
fn the_constructs_typeset_in_two_columns_on_mirrored_pages_too() {
    let mut failures = Vec::new();
    for value in ["plain words", HOSTILE[0], HOSTILE[5], "- item", "]", "#"] {
        let doc = marked(&every_construct(value));
        let (_, document) = compile(&doc, &["page.columns=2", "page.twoside=true"]);
        match document {
            Ok(document) => {
                let out = all_text(&document);
                let expected = squeeze(&plain(&every_construct(value)));
                let from = out.find(START).unwrap() + START.len();
                let to = out.rfind(END).unwrap();
                if squeeze(&out[from..to]) != expected {
                    failures.push(format!("{value:?} typeset as {:?}", &out[from..to]));
                }
            }
            Err(error) => failures.push(format!("{value:?} failed: {error}")),
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

fn heading_bodies(document: &PagedDocument) -> Vec<String> {
    document
        .introspector()
        .query(&Selector::Elem(Element::of::<HeadingElem>(), None))
        .iter()
        .filter_map(|heading| match heading.get_by_name("body") {
            Ok(Value::Content(body)) => Some(body.plain_text().to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_heading_for_the_outline_only_is_in_the_outline_and_not_on_the_page() {
    for value in std::iter::once("A Hidden Part").chain(HOSTILE.iter().copied()) {
        let doc = marked(&[Block::Heading {
            level: 2,
            id: Some("hidden".into()),
            children: vec![text(value)],
            outline_only: true,
        }]);
        let (source, document) = compile(&doc, &[]);
        let document = document.unwrap_or_else(|error| panic!("{value:?}: {error}"));
        assert!(
            source.contains("#fs-outline-only[#heading(level: 2)["),
            "{source}"
        );
        let out = all_text(&document);
        let from = out.find(START).unwrap() + START.len();
        let to = out.rfind(END).unwrap();
        assert_eq!(squeeze(&out[from..to]), "", "{value:?} is not printed");
        let bodies: Vec<String> = heading_bodies(&document)
            .iter()
            .map(|body| squeeze(body))
            .collect();
        assert!(
            bodies.contains(&squeeze(value)),
            "{value:?} is in the outline: {bodies:?}"
        );
    }
}

#[test]
fn a_remark_number_opens_its_first_paragraph_and_its_id_is_a_label() {
    let doc = marked(&[
        Block::Remark {
            number: "2.0121".into(),
            id: Some("2.0121".into()),
            children: vec![para(vec![text("The remark.")])],
        },
        Block::Remark {
            number: "3-1".into(),
            id: Some("Ts-309,1".into()),
            children: vec![Block::Rule, para(vec![text("After a rule.")])],
        },
        para(vec![
            Inline::Link {
                href: "2.0121".into(),
                internal: true,
                children: vec![text("to 2.0121")],
            },
            Inline::Link {
                href: "Ts-309,1".into(),
                internal: true,
                children: vec![text("to Ts-309,1")],
            },
        ]),
    ]);
    let (source, document) = compile(&doc, &[]);
    document.unwrap();
    assert!(
        source.contains("#metadata(none)<2.0121>#fs-remark-number[2.0121] The remark."),
        "{source}"
    );
    assert!(
        source.contains("#metadata(none)<Ts-309-1>#fs-remark-number[3-1]\n\n#line("),
        "a remark that opens with another block has its number to itself: {source}"
    );
    assert!(source.contains("#link(<2.0121>)[to 2.0121]"), "{source}");
    assert!(
        source.contains("#link(<Ts-309-1>)[to Ts-309,1]"),
        "{source}"
    );
}

#[test]
fn a_table_with_rules_of_its_own_ignores_the_table_style() {
    let table = |rules| {
        Block::Table(Table {
            rows: vec![vec![
                cell(
                    "a",
                    Some(Edges {
                        top: false,
                        right: true,
                        bottom: true,
                        left: false,
                    }),
                ),
                cell("b", None),
            ]],
            columns: 2,
            caption: None,
            rules,
        })
    };
    for style in ["blocks.table.style=grid", "blocks.table.style=booktabs"] {
        let (source, document) = compile(
            &marked(&[
                table(Some(Rules::Cells)),
                table(Some(Rules::None)),
                table(None),
            ]),
            &[style],
        );
        document.unwrap();
        assert!(
            source.contains(
                "#fs-table-plain(columns: 2, table.cell(stroke: (right: 0.4pt, bottom: 0.4pt), [a]), [b])"
            ),
            "{source}"
        );
        assert!(
            source.contains("#fs-table-plain(columns: 2, [a], [b])"),
            "a layout table draws no rules: {source}"
        );
        assert!(
            source.contains("#fs-table(columns: 2, rows: 1, header-rows: 0, [a], [b])"),
            "without rules of its own, the style decides: {source}"
        );
    }
}

#[test]
fn a_helper_is_in_the_preamble_only_when_the_document_uses_its_construct() {
    let (plain_source, _) = compile(&marked(&[para(vec![text("Plain.")])]), &[]);
    assert!(
        !plain_source.contains("// ---- constructs ----"),
        "{plain_source}"
    );
    let (source, document) = compile(
        &marked(&[para(vec![Inline::Spaced {
            children: vec![text("gesperrt")],
        }])]),
        &[],
    );
    document.unwrap();
    assert!(
        source.contains(
            "// ---- constructs ----\n#let fs-spaced(body) = text(tracking: 0.2em, body)\n"
        ),
        "{source}"
    );
    for other in [
        "fs-margin-note",
        "fs-outline-only",
        "fs-remark-number",
        "fs-aside",
        "fs-columns",
        "fs-verse",
        "fs-table-plain",
    ] {
        assert!(!source.contains(other), "{other}: {source}");
    }
    assert_eq!(
        source
            .replace(
                "// ---- constructs ----\n#let fs-spaced(body) = text(tracking: 0.2em, body)\n",
                ""
            )
            .split("#heading(level: 1)")
            .next(),
        plain_source.split("#heading(level: 1)").next(),
        "the rest of the preamble is the same"
    );
}
