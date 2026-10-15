use typesep_model::{Block, Doc, Inline, Meta};
use typesep_style::{ResolveOptions, resolve_style};
use typesep_typst::{EmitOptions, Fonts, TypesetWorld, emit_document};
use typst::layout::{Frame, FrameItem, Point};
use typst_layout::PagedDocument;

fn para(value: &str) -> Block {
    Block::Para {
        children: vec![Inline::Text {
            value: value.into(),
        }],
        align: None,
        indent_em: 0,
    }
}

fn remark(number: &str, paras: &[&str]) -> Block {
    Block::Remark {
        number: number.into(),
        id: Some(number.into()),
        children: paras.iter().map(|p| para(p)).collect(),
    }
}

/// Where each run of text that holds `word` sits, from the top of its page.
fn baselines(frame: &Frame, at: Point, word: &str, out: &mut Vec<f64>) {
    for (pos, item) in frame.items() {
        let here = at + *pos;
        match item {
            FrameItem::Group(group) => baselines(&group.frame, here, word, out),
            FrameItem::Text(text) if text.text.contains(word) => out.push(here.y.to_pt()),
            _ => {}
        }
    }
}

/// The Typst source of three remarks, the first of two paragraphs, and where the first lines
/// of its second paragraph and of the next remark sit below the first one's.
fn gaps(sets: &[&str]) -> (String, f64, f64) {
    let doc = Doc {
        meta: Meta {
            id: "remarks".into(),
            title: "Remarks".into(),
            ..Meta::default()
        },
        body: vec![remark("1", &["Alpha.", "Beta."]), remark("2", &["Gamma."])],
        ..Doc::default()
    };
    let mut all = vec!["cover.style=none"];
    all.extend(sets);
    let style = resolve_style(ResolveOptions {
        preset: Some("halfletter"),
        file: None,
        sets: all.iter().map(|s| (*s).into()).collect(),
        schema: None,
        default_preset: None,
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
    let world = TypesetWorld::new(rendered.source.clone(), &rendered.figures, &fonts);
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    let y = |word: &str| {
        let mut out = Vec::new();
        baselines(&document.pages()[0].frame, Point::zero(), word, &mut out);
        assert_eq!(out.len(), 1, "{word}: {out:?}");
        out[0]
    };
    let (alpha, beta, gamma) = (y("Alpha"), y("Beta"), y("Gamma"));
    (rendered.source, beta - alpha, gamma - beta)
}

#[test]
fn a_remark_is_set_apart_by_the_space_the_style_asks_for_beyond_a_paragraphs() {
    // The entry, between its heading and the colophon page.
    let body = |source: &str| {
        let entry = source.split("#heading(level: 1)").nth(1).unwrap_or("");
        entry.split("#page(").next().unwrap_or("").to_owned()
    };
    let (source, within, between) = gaps(&[]);
    assert!(
        !body(&source).contains("#v("),
        "no space asked for, none set: {source}"
    );
    assert!(
        (within - between).abs() < 0.01,
        "by default a remark is as far from the last as a paragraph: {within} {between}"
    );

    let (source, within, between) = gaps(&["blocks.remark.spaceBefore=12pt"]);
    assert!(
        body(&source).contains("#v(0.5em + 11.955pt, weak: true)"),
        "{source}"
    );
    // 12 TeX points are 11.955 of Typst's.
    assert!(
        (between - within - 11.955).abs() < 0.05,
        "the space comes on top of a paragraph's: {within} {between}"
    );

    let (_, within, between) = gaps(&["blocks.remark.spaceBefore=0.6em", "typography.parSkip=3pt"]);
    assert!(
        (between - within - 0.6 * 9.963).abs() < 0.05,
        "and on top of the paragraph skip, in the body's ems: {within} {between}"
    );
}
