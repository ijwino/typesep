use std::collections::BTreeMap;

use typesep_model::{Block, Doc, Figure, Inline, Meta};
use typesep_style::{ResolveOptions, resolve_style};
use typesep_typst::images::Image;
use typesep_typst::{EmitOptions, Fonts, TypesetWorld, emit_document};
use typst::layout::{Frame, FrameItem};
use typst_layout::PagedDocument;

/// A one-pixel PNG.
const PNG: [u8; 70] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0x60, 0xf8, 0x5f,
    0x0f, 0x00, 0x02, 0x87, 0x01, 0x80, 0xeb, 0x47, 0xba, 0x92, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn figure(width_px: Option<u32>, inline: bool) -> Figure {
    Figure {
        src: "https://example.org/dot.png".into(),
        alt: String::new(),
        ebook_src: None,
        width_px,
        caption: None,
        id: None,
        inline: inline.then_some(true),
    }
}

/// The widths, in points and in the order they are laid out, of the images on the pages.
fn image_widths(frame: &Frame, out: &mut Vec<f64>) {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Group(group) => image_widths(&group.frame, out),
            FrameItem::Image(_, size, _) => out.push(size.x.to_pt()),
            _ => {}
        }
    }
}

/// The source `body` is emitted as, and the widths its images come out at.
fn typeset(body: Vec<Block>) -> (String, Vec<f64>) {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("dot.png");
    std::fs::write(&file, PNG).unwrap();
    let images = BTreeMap::from([(
        "https://example.org/dot.png".to_owned(),
        Image {
            file,
            format: "png",
        },
    )]);
    let doc = Doc {
        meta: Meta {
            id: "figures".into(),
            title: "Figures".into(),
            ..Meta::default()
        },
        body,
        ..Doc::default()
    };
    let style = resolve_style(ResolveOptions {
        preset: Some("halfletter"),
        file: None,
        sets: vec!["cover.style=none".into()],
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
            images: &images,
            ..Default::default()
        },
    );
    let world = TypesetWorld::new(rendered.source.clone(), &rendered.figures, &fonts);
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    let mut widths = Vec::new();
    for page in document.pages() {
        image_widths(&page.frame, &mut widths);
    }
    (rendered.source, widths)
}

#[test]
fn a_figure_is_set_at_the_width_the_source_shows_it_at_and_no_wider_than_the_text() {
    let (source, widths) = typeset(vec![
        Block::Figure(figure(Some(120), false)),
        Block::Figure(figure(Some(4000), false)),
        Block::Para {
            children: vec![
                Inline::Text {
                    value: "a sign ".into(),
                },
                Inline::Figure(figure(Some(30), true)),
                Inline::Text {
                    value: " in a line".into(),
                },
            ],
            align: None,
            indent_em: 0,
        },
    ]);
    assert!(
        source.contains("#fs-figure-at(\"/figures/1.png\", 90pt)"),
        "{source}"
    );
    assert!(source.contains("#fs-figure-inline-at(\"/figures/1.png\", 22.5pt)"));
    // Half-letter is 396pt wide with 38.88pt margins.
    let text_width = 396.0 - 2.0 * 38.88;
    assert_eq!(widths.len(), 3, "{widths:?}");
    assert!((widths[0] - 90.0).abs() < 0.01, "120px is 90pt: {widths:?}");
    assert!(
        (widths[1] - text_width).abs() < 0.01,
        "4000px is held to the text width: {widths:?}"
    );
    assert!(
        (widths[2] - 22.5).abs() < 0.01,
        "30px in a line is 22.5pt: {widths:?}"
    );
}

#[test]
fn a_figure_without_a_width_and_a_document_without_one_are_as_they_were() {
    let (source, _) = typeset(vec![
        Block::Figure(figure(None, false)),
        Block::Para {
            children: vec![Inline::Figure(figure(None, true))],
            align: None,
            indent_em: 0,
        },
    ]);
    assert!(source.contains("#fs-figure(\"/figures/1.png\")"));
    assert!(source.contains("#fs-figure-inline(\"/figures/1.png\")"));
    assert!(
        !source.contains("fs-figure-at"),
        "the sized helpers are defined only where they are used"
    );
}
