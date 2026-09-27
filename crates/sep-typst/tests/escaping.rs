use sep_core::model::{Block, EntryDoc, EntryMeta, Inline};
use sep_core::style::{ResolveOptions, resolve_style};
use sep_typst::{EmitOptions, Fonts, SepWorld, emit_document};
use typst::layout::{Frame, FrameItem};
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
            | Inline::Link { children, .. } => plain_inlines(children, out),
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
            Block::Blockquote { children } => out.push_str(&plain(children)),
            _ => panic!("the harness reads paragraphs, code blocks and quotes"),
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

fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn typeset(blocks: &[Block]) -> Result<String, String> {
    let mut body = vec![para(vec![text(START)])];
    body.extend(blocks.iter().cloned());
    body.push(para(vec![text(END)]));
    let doc = EntryDoc {
        meta: EntryMeta {
            slug: "escaping".into(),
            title: "Escaping".into(),
            ..EntryMeta::default()
        },
        body,
        ..EntryDoc::default()
    };
    let style = resolve_style(ResolveOptions {
        preset: Some("halfletter"),
        file: None,
        sets: vec![],
    })
    .unwrap();
    let fonts = Fonts::embedded();
    let rendered = emit_document(
        &[&doc],
        &style,
        EmitOptions {
            fonts: Some(&fonts),
            ..Default::default()
        },
    );
    let source = rendered
        .source
        .replacen(START, &format!("{START}\n\n{STRUCTURE}\n\n"), 1);
    let world = SepWorld::new(source, &rendered.figures, &fonts);
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|errors| {
            errors
                .iter()
                .map(|d| d.message.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        })?;
    let mut out = String::new();
    for page in document.pages() {
        walk(&page.frame, &mut out);
    }
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
