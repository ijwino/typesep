use sep_core::model::{Block, Doc, Inline, List, ListNumbering, Meta};
use sep_core::style::{ResolveOptions, list_presets, resolve_style};
use sep_typst::{EmitOptions, Fonts, SepWorld, compile_pdf, emit_document};

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.to_owned(),
    }
}

fn para(value: &str) -> Block {
    Block::Para {
        children: vec![text(value)],
        align: None,
    }
}

fn doc() -> Doc {
    Doc {
        meta: Meta {
            slug: "pleasure".to_owned(),
            title: "Pleasure".to_owned(),
            author_line: "A. Author".to_owned(),
            source_url: "https://plato.stanford.edu/entries/pleasure/".to_owned(),
            ..Meta::default()
        },
        body: vec![
            Block::Heading {
                level: 2,
                id: Some("one".to_owned()),
                children: vec![text("The inclusive use")],
            },
            para("Pleasure, in the inclusive usage, includes the affective positivity of all joy."),
            Block::List(List {
                ordered: true,
                hanging: false,
                items: vec![vec![para("first")], vec![para("second")]],
                numbering: Some(ListNumbering::Decimal),
                start: None,
                labels: None,
            }),
            Block::Blockquote {
                children: vec![para(
                    "It is often contrasted with the similarly inclusive pain.",
                )],
            },
        ],
        ..Doc::default()
    }
}

#[test]
fn every_bundled_preset_typesets() {
    let fonts = Fonts::embedded();
    let doc = doc();
    for preset in list_presets() {
        let style = resolve_style(ResolveOptions {
            preset: Some(preset),
            file: None,
            sets: Vec::new(),
        })
        .unwrap_or_else(|err| panic!("{preset}: {err:?}"));
        let rendered = emit_document(
            &[&doc],
            &style,
            EmitOptions {
                fonts: Some(&fonts),
                ..Default::default()
            },
        );
        let world = SepWorld::new(rendered.source.clone(), &rendered.figures, &fonts);
        if let Err(err) = compile_pdf(&world, "typesep tests") {
            panic!("{preset} does not compile: {err}");
        }
    }
}
