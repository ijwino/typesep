use std::collections::BTreeSet;

use typesep_model::{Block, Doc, Inline, MathMode, Meta, Section, TocItem};
use typesep_style::{ResolveOptions, resolve_style};
use typesep_typst::{EmitOptions, Fonts, TypesetWorld, emit_document};
use typst::layout::{Frame, FrameItem};
use typst_layout::PagedDocument;

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.to_owned(),
    }
}

/// "Common p-Belief in Games", the p a formula and the games emphasised, as a heading and a
/// contents line both read.
fn belief() -> Vec<Inline> {
    vec![
        text("5. Common "),
        Inline::Math {
            mode: MathMode::Inline,
            tex: "p".to_owned(),
        },
        text("-Belief in "),
        Inline::Emph {
            children: vec![text("Games")],
        },
    ]
}

fn line(label: Vec<Inline>, href: &str) -> TocItem {
    TocItem {
        label,
        href: href.to_owned(),
        children: Vec::new(),
    }
}

fn entry() -> Doc {
    Doc {
        meta: Meta {
            id: "belief".into(),
            title: "Belief".into(),
            ..Meta::default()
        },
        toc: vec![
            line(belief(), "#Coo"),
            line(
                vec![Inline::Emph {
                    children: vec![text("Bibliography")],
                }],
                "#Bib",
            ),
        ],
        body: vec![
            Block::Heading {
                level: 2,
                id: Some("Coo".to_owned()),
                children: belief(),
                outline_only: false,
            },
            Block::Para {
                children: vec![text("Some text.")],
                align: None,
                indent_em: 0,
            },
        ],
        back: vec![Section {
            key: "bib".to_owned(),
            label: "Bibliography".to_owned(),
            blocks: vec![
                Block::Heading {
                    level: 2,
                    id: Some("Bib".to_owned()),
                    children: vec![text("Bibliography")],
                    outline_only: false,
                },
                Block::Para {
                    children: vec![text("A reference.")],
                    align: None,
                    indent_em: 0,
                },
            ],
        }],
        ..Doc::default()
    }
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

/// The Typst source of the entry with `sections` selected, and the text it typesets to.
fn typeset(sections: &[&str]) -> (String, String) {
    let style = resolve_style(ResolveOptions {
        preset: Some("halfletter"),
        file: None,
        sets: vec!["cover.style=none".into()],
        schema: None,
        default_preset: None,
    })
    .unwrap();
    let fonts = Fonts::embedded();
    let sections: BTreeSet<String> = sections.iter().map(|s| (*s).to_owned()).collect();
    let rendered = emit_document(
        &[&entry()],
        &style,
        EmitOptions {
            fonts: Some(&fonts),
            sections: &sections,
            ..Default::default()
        },
    );
    let world = TypesetWorld::new(rendered.source.clone(), &rendered.figures, &fonts);
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .unwrap_or_else(|errors| panic!("{errors:?}\n{}", rendered.source));
    let mut out = String::new();
    for page in document.pages() {
        walk(&page.frame, &mut out);
    }
    (rendered.source, out)
}

#[test]
fn a_contents_line_sets_its_formula_and_its_emphasis_as_the_heading_does() {
    let (source, _) = typeset(&["bib"]);
    let heading = source
        .lines()
        .find(|l| l.starts_with("#heading(level: 2)") && l.contains("Common"))
        .unwrap_or_else(|| panic!("the heading: {source}"));
    let body = heading
        .strip_prefix("#heading(level: 2)[")
        .and_then(|rest| rest.split_once("] <"))
        .map(|(body, _)| body)
        .unwrap_or_else(|| panic!("the heading's body: {heading}"));
    assert!(body.contains("#fs-math("), "{heading}");
    assert!(body.contains("#emph[Games]"), "{heading}");
    assert!(
        source.contains(&format!("#link(<Coo>)[{body}]")),
        "the contents line is the heading's text, linked: {source}"
    );
    assert!(
        !source.contains("\\("),
        "no formula is left as TeX: {source}"
    );
}

#[test]
fn end_matter_left_out_is_left_out_of_the_contents_markup_and_all() {
    let (source, text) = typeset(&["bib"]);
    assert!(
        source.contains("#link(<Bib>)[#emph[Bibliography]]"),
        "{source}"
    );
    assert!(text.contains("A reference."), "{text}");

    let (source, text) = typeset(&[]);
    assert!(!source.contains("#emph[Bibliography]"), "{source}");
    assert!(!text.contains("Bibliography"), "{text}");
    assert!(!text.contains("A reference."), "{text}");
    assert!(source.contains("#link(<Coo>)"), "{source}");
}
