use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

use typesep_epub::nav::nav_items;
use typesep_epub::xhtml::{Ctx, MathPass, entry_html, esc, xml_id};
use typesep_epub::{EpubError, EpubOptions, build_epub};
use typesep_model::{
    Attachment, AttachmentKind, Block, Doc, Figure, Inline, List, Meta, NoteRef, NoteTable, Origin,
    TocItem,
};
use typesep_style::View;
use typesep_style::{ResolveOptions, Style, resolve_style};
use typesep_typst::fonts::Fonts;
use typesep_typst::images::Image;

fn style() -> Style {
    resolve_style(ResolveOptions {
        preset: Some("halfletter"),
        file: None,
        sets: vec![],
    })
    .unwrap()
}

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.to_owned(),
    }
}

fn para(value: &str) -> Block {
    Block::Para {
        children: vec![text(value)],
        align: None,
        indent_em: 0,
    }
}

fn doc(body: Vec<Block>) -> Doc {
    Doc {
        meta: Meta {
            id: "x".into(),
            title: "An Entry".into(),
            origin: Origin {
                url: "https://plato.stanford.edu/entries/x/".into(),
                ..Default::default()
            },
            ..Default::default()
        },
        body,
        ..Default::default()
    }
}

fn every_section() -> BTreeSet<String> {
    ["bib", "academic", "oir", "related", "notes", "supplements"]
        .map(str::to_owned)
        .into()
}

fn html(doc: &Doc) -> String {
    let style = style();
    let view = View::new(&style);
    let images = BTreeMap::new();
    let sections = every_section();
    let mut first = Ctx::new(&view, &images, MathPass::Collect(Vec::new()));
    entry_html(doc, &mut first, &sections);
    let mut ctx = Ctx::new(
        &view,
        &images,
        MathPass::Fill {
            items: Vec::new(),
            next: 0,
        },
    );
    ctx.ids = std::mem::take(&mut first.ids);
    entry_html(doc, &mut ctx, &sections)
}

#[test]
fn an_id_is_one_an_xml_document_can_carry() {
    assert_eq!(xml_id("Cla"), "Cla");
    assert_eq!(xml_id("a b/c"), "a-b-c");
    assert_eq!(xml_id("1.2"), "x1.2");
    assert_eq!(xml_id("_x"), "_x");
}

#[test]
fn every_block_kind_becomes_its_xhtml() {
    let entry = doc(vec![
        Block::Heading {
            level: 2,
            id: Some("Sec".into()),
            children: vec![text("A Section")],
            outline_only: false,
        },
        para("Plain."),
        Block::Blockquote {
            children: vec![para("Quoted.")],
        },
        Block::List(List {
            ordered: true,
            hanging: false,
            items: vec![vec![para("One")]],
            numbering: Some(typesep_model::ListNumbering::LowerAlpha),
            start: Some(3),
            labels: None,
        }),
        Block::Rule,
    ]);
    let html = html(&entry);
    assert!(html.contains(r##"<h2 id="Sec">A Section</h2>"##), "{html}");
    assert!(html.contains("<p>Plain.</p>"));
    assert!(html.contains("<blockquote><p>Quoted.</p></blockquote>"));
    assert!(
        html.contains(r##"<ol type="a" start="3"><li><p>One</p></li></ol>"##),
        "{html}"
    );
    assert!(html.contains("<hr/>"));
    assert!(html.contains("<h1>An Entry</h1>"));
}

fn with_note(blocks: Vec<Block>) -> Doc {
    let mut entry = doc(vec![Block::Para {
        children: vec![
            text("Body"),
            Inline::NoteRef(NoteRef {
                id: "note-1".into(),
                label: "1".into(),
                href: "https://plato.stanford.edu/entries/x/notes.html#note-1".into(),
                ref_id: Some("ref-1".into()),
            }),
        ],
        align: None,
        indent_em: 0,
    }]);
    entry.attachments = vec![Attachment {
        kind: AttachmentKind::Notes,
        title: "Notes".into(),
        blocks: vec![],
        source_url: "https://plato.stanford.edu/entries/x/notes.html".into(),
        notes: Some(NoteTable::from([("note-1".to_owned(), blocks)])),
    }];
    entry
}

#[test]
fn a_note_of_one_paragraph_is_one_paragraph() {
    let html = html(&with_note(vec![para("The note.")]));
    assert!(
        html.contains(
            r##"<aside class="note" epub:type="footnote" id="note-1"><p class="flush"><a href="#ref-1">1.</a> The note.</p></aside>"##
        ),
        "{html}"
    );
    assert!(html.contains(
        r##"<sup><a class="noteref" epub:type="noteref" href="#note-1" id="ref-1">1</a></sup>"##
    ));
}

#[test]
fn a_note_of_several_blocks_closes_its_aside() {
    let html = html(&with_note(vec![
        para("Russell writes:"),
        Block::Blockquote {
            children: vec![para("Quoted.")],
        },
    ]));
    assert!(
        html.contains(
            r##"<p class="flush"><a href="#ref-1">1.</a> Russell writes:</p><blockquote><p>Quoted.</p></blockquote></aside>"##
        ),
        "{html}"
    );
    assert!(!html.contains("</blockquote></p>"), "{html}");
}

#[test]
fn a_link_to_an_id_the_document_does_not_carry_is_not_a_link() {
    let entry = doc(vec![
        Block::Anchor { id: "here".into() },
        Block::Para {
            children: vec![Inline::Link {
                href: "here".into(),
                internal: true,
                children: vec![text("There")],
            }],
            align: None,
            indent_em: 0,
        },
        Block::Para {
            children: vec![Inline::Link {
                href: "gone".into(),
                internal: true,
                children: vec![text("Fig. 3")],
            }],
            align: None,
            indent_em: 0,
        },
    ]);
    let html = html(&entry);
    assert!(html.contains(r##"<a href="#here">There</a>"##), "{html}");
    assert!(html.contains("<p>Fig. 3</p>"), "{html}");
}

#[test]
fn an_id_the_document_writes_twice_is_written_once() {
    let entry = doc(vec![
        Block::Anchor { id: "twice".into() },
        Block::Heading {
            level: 2,
            id: Some("twice".into()),
            children: vec![text("A Section")],
            outline_only: false,
        },
    ]);
    let html = html(&entry);
    assert_eq!(html.matches(r#"id="twice""#).count(), 1, "{html}");
    assert!(html.contains(r##"<div id="twice"></div>"##), "{html}");
}

#[test]
fn a_contents_entry_with_no_target_is_a_span_or_is_left_out() {
    let mut entry = doc(vec![Block::Heading {
        level: 2,
        id: Some("Cla".into()),
        children: vec![text("1. Clarifications")],
        outline_only: false,
    }]);
    entry.toc = vec![
        TocItem {
            text: "1. Clarifications".into(),
            href: "#Cla".into(),
            children: vec![],
        },
        TocItem {
            text: "2. Related Principles".into(),
            href: "#RelPri".into(),
            children: vec![TocItem {
                text: "2.1 One".into(),
                href: "#Cla".into(),
                children: vec![],
            }],
        },
        TocItem {
            text: "3. Alone".into(),
            href: "#Gone".into(),
            children: vec![],
        },
    ];
    let style = style();
    let view = View::new(&style);
    let images = BTreeMap::new();
    let sections = every_section();
    let mut ctx = Ctx::new(&view, &images, MathPass::Collect(Vec::new()));
    entry_html(&entry, &mut ctx, &sections);
    let nav = nav_items(&entry, "entry-1.xhtml", &view, &sections, &ctx.ids);
    assert!(
        nav.contains(r##"<a href="entry-1.xhtml#Cla">1. Clarifications</a>"##),
        "{nav}"
    );
    assert!(
        nav.contains("<span>2. Related Principles</span><ol>"),
        "{nav}"
    );
    assert!(!nav.contains("3. Alone"), "{nav}");
}

#[test]
fn an_epub_of_no_entry_is_an_error() {
    let style = style();
    let images = BTreeMap::new();
    let sections = every_section();
    let fonts = Fonts::embedded();
    let volume = Meta {
        title: "vol".into(),
        ..Default::default()
    };
    let built = build_epub(
        &[],
        &style,
        EpubOptions {
            sections: &sections,
            images: &images,
            volume: Some(&volume),
            fonts: &fonts,
        },
    );
    assert!(matches!(built, Err(EpubError::Empty)));
}

#[test]
fn an_svg_image_by_reference_does_not_mark_its_document_as_svg() {
    let file = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("figure.svg");
    std::fs::write(&file, "<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();
    let images = BTreeMap::from([(
        "figure.svg".to_owned(),
        Image {
            file,
            format: "svg",
        },
    )]);
    let entry = doc(vec![Block::Figure(Figure {
        src: "figure.svg".into(),
        alt: "a figure".into(),
        caption: None,
        id: None,
        inline: None,
        ebook_src: None,
    })]);
    let style = style();
    let sections = every_section();
    let fonts = Fonts::embedded();
    let epub = build_epub(
        &[&entry],
        &style,
        EpubOptions {
            sections: &sections,
            images: &images,
            volume: None,
            fonts: &fonts,
        },
    )
    .unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(epub.bytes)).unwrap();
    let mut opf = String::new();
    archive
        .by_name("OEBPS/content.opf")
        .unwrap()
        .read_to_string(&mut opf)
        .unwrap();
    assert!(opf.contains("media-type=\"image/svg+xml\""), "{opf}");
    assert!(!opf.contains("properties=\"svg\""), "{opf}");
}

#[test]
fn every_document_is_in_the_language_its_metadata_names() {
    let mut entry = doc(vec![para("Die Welt ist alles, was der Fall ist.")]);
    entry.meta.lang = "de".into();
    let style = style();
    let images = BTreeMap::new();
    let sections = every_section();
    let fonts = Fonts::embedded();
    let epub = build_epub(
        &[&entry],
        &style,
        EpubOptions {
            sections: &sections,
            images: &images,
            volume: None,
            fonts: &fonts,
        },
    )
    .unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(epub.bytes)).unwrap();
    let mut read = |name: &str| {
        let mut text = String::new();
        archive
            .by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    };
    let opf = read("OEBPS/content.opf");
    assert!(
        opf.contains(r#"version="3.0" unique-identifier="pub-id" xml:lang="de">"#),
        "{opf}"
    );
    assert!(opf.contains("<dc:language>de</dc:language>"), "{opf}");
    for name in ["OEBPS/entry-1.xhtml", "OEBPS/nav.xhtml"] {
        let page = read(name);
        assert!(
            page.contains(r#" lang="de" xml:lang="de">"#),
            "{name}: {page}"
        );
    }
}

#[test]
fn characters_xml_forbids_are_dropped() {
    assert_eq!(esc("a\u{1}b\u{8}c\u{1f}d"), "abcd");
    assert_eq!(esc("\u{fffe}x\u{ffff}"), "x");
    assert_eq!(esc("tab\tline\nreturn\r"), "tab\tline\nreturn\r");
    assert_eq!(esc("<&>"), "&lt;&amp;&gt;");
}
