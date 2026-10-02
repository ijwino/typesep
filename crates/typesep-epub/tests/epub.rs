use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

use typesep_epub::css;
use typesep_epub::nav::nav_items;
use typesep_epub::xhtml::{Ctx, MathPass, entry_html, esc, xml_id};
use typesep_epub::{EpubError, EpubOptions, build_epub};
use typesep_model::{
    Align, Attachment, AttachmentKind, Block, Decoration, Dir, Doc, Edges, Figure, Inline, List,
    Meta, NoteRef, NoteTable, Origin, Rules, Table, TableCell, TocItem, VerseLine,
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
        schema: None,
        default_preset: None,
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
        width_px: None,
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

/// The members of `doc`'s EPUB that are text, by name, built with `sets`.
fn epub_text(
    doc: &Doc,
    sets: &[&str],
    images: &BTreeMap<String, Image>,
) -> BTreeMap<String, String> {
    let style = resolve_style(ResolveOptions {
        preset: Some("halfletter"),
        file: None,
        sets: sets.iter().map(|s| (*s).into()).collect(),
        schema: None,
        default_preset: None,
    })
    .unwrap();
    let sections = every_section();
    let fonts = Fonts::embedded();
    let epub = build_epub(
        &[doc],
        &style,
        EpubOptions {
            sections: &sections,
            images,
            volume: None,
            fonts: &fonts,
        },
    )
    .unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(epub.bytes)).unwrap();
    let mut out = BTreeMap::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).unwrap();
        let mut text = String::new();
        if member.read_to_string(&mut text).is_ok() {
            out.insert(member.name().to_owned(), text);
        }
    }
    out
}

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
    let inline = |wrap: fn(Vec<Inline>) -> Inline| Block::Para {
        children: vec![wrap(vec![text(value)])],
        align: None,
        indent_em: 0,
    };
    vec![
        Block::Heading {
            level: 2,
            id: Some("hidden".into()),
            children: vec![text(value)],
            outline_only: true,
        },
        Block::Remark {
            number: value.to_owned(),
            id: Some(value.to_owned()),
            children: vec![para(value), para(value)],
        },
        Block::Remark {
            number: value.to_owned(),
            id: None,
            children: vec![Block::Rule],
        },
        inline(|children| Inline::MarginNote { children }),
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
            indent_em: 2,
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
            children: vec![para(value)],
        },
        Block::Columns {
            columns: vec![vec![para(value)], vec![para(value)]],
        },
        Block::Table(Table {
            rows: vec![vec![
                cell(
                    value,
                    Some(Edges {
                        top: true,
                        right: false,
                        bottom: false,
                        left: true,
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
        Block::Para {
            children: vec![
                text(value),
                Inline::NoteRef(NoteRef {
                    id: "cite_note-1".into(),
                    label: "1".into(),
                    href: "#cite_note-1".into(),
                    ref_id: Some("cite_ref-1".into()),
                }),
            ],
            align: None,
            indent_em: 0,
        },
    ]
}

fn with_every_construct(value: &str) -> Doc {
    let mut entry = doc(every_construct(value));
    entry.toc = vec![TocItem {
        text: value.to_owned(),
        href: "#hidden".into(),
        children: vec![],
    }];
    entry.notes = NoteTable::from([("cite_note-1".to_owned(), vec![para(value)])]);
    entry
}

#[test]
fn every_construct_of_another_source_becomes_its_xhtml() {
    let html = html(&with_every_construct("words"));
    for expected in [
        r#"<h2 id="hidden" class="outline-only">words</h2>"#,
        r#"<div class="remark" id="words"><p><strong class="remark-number">words</strong> words</p><p>words</p></div>"#,
        r#"<div class="remark"><p class="flush"><strong class="remark-number">words</strong></p><hr/></div>"#,
        r#"<p><span class="margin-note">words</span></p>"#,
        r#"<p><span class="spaced">words</span></p>"#,
        r#"<p><span class="smallcaps">words</span></p>"#,
        r#"<p><u class="dashed">words</u></p>"#,
        r#"<p><span class="overline">words</span></p>"#,
        r#"<p><s>words</s></p>"#,
        r#"<p class="right indent-2">words</p>"#,
        "<div class=\"verse\"><p>words</p><p>\u{a0}</p><p class=\"indent-2\">words</p></div>",
        r#"<div class="aside"><p>words</p></div>"#,
        r#"<div class="columns"><div class="column"><p>words</p></div><div class="column"><p>words</p></div></div>"#,
        r#"<table><tr><td class="edge-top edge-left">words</td><td>words</td></tr></table>"#,
        r#"<table class="layout"><tr><td>words</td></tr></table>"#,
        r##"<sup><a class="noteref" epub:type="noteref" href="#cite_note-1" id="cite_ref-1">1</a></sup>"##,
        r##"<aside class="note" epub:type="footnote" id="cite_note-1"><p class="flush"><a href="#cite_ref-1">1.</a> words</p></aside>"##,
    ] {
        assert!(html.contains(expected), "{expected}\n{html}");
    }
}

/// Text that is markup to XML, to come out as written wherever the constructs of other
/// sources put text.
const HOSTILE: &[&str] = &[
    "a < b & c > d",
    "</p></div><p>",
    "<![CDATA[x]]> and ]]>",
    "<!-- not a comment -->",
    "\"double\" and 'single' quotes",
    "&amp; &#60; &lt;",
    "a backslash then a newline \\\nand on",
    "#strong[x] $y$ <label> @label",
];

fn text_of(xml: &str) -> String {
    let document = roxmltree::Document::parse(xml).unwrap_or_else(|error| panic!("{error}\n{xml}"));
    document
        .descendants()
        .filter(|node| node.is_text())
        .filter_map(|node| node.text())
        .collect()
}

#[test]
fn the_xhtml_of_every_construct_parses_as_xml_and_keeps_its_text_as_written() {
    for value in std::iter::once("words").chain(HOSTILE.iter().copied()) {
        let members = epub_text(&with_every_construct(value), &[], &BTreeMap::new());
        for name in ["OEBPS/entry-1.xhtml", "OEBPS/nav.xhtml"] {
            let page = &members[name];
            let text = text_of(page);
            let wanted = if name.ends_with("nav.xhtml") { 1 } else { 20 };
            assert!(
                text.matches(value).count() >= wanted,
                "{value:?} {} times in {name}: {text}",
                text.matches(value).count()
            );
        }
    }
    for notes in ["endnote", "appendix"] {
        let set = format!("notes.style={notes}");
        let members = epub_text(&with_every_construct(HOSTILE[1]), &[&set], &BTreeMap::new());
        let text = text_of(&members["OEBPS/entry-1.xhtml"]);
        assert!(text.contains(HOSTILE[1]), "{notes}: {text}");
    }
}

#[test]
fn a_heading_for_the_outline_only_is_in_the_nav_and_hidden_on_the_page() {
    let members = epub_text(
        &with_every_construct("A Hidden Part"),
        &[],
        &BTreeMap::new(),
    );
    assert!(
        members["OEBPS/nav.xhtml"]
            .contains(r#"<li><a href="entry-1.xhtml#hidden">A Hidden Part</a></li>"#),
        "{}",
        members["OEBPS/nav.xhtml"]
    );
    assert!(
        members["OEBPS/entry-1.xhtml"]
            .contains(r#"<h2 id="hidden" class="outline-only">A Hidden Part</h2>"#)
    );
    assert!(
        members["OEBPS/style.css"].contains(".outline-only{display:none;}"),
        "{}",
        members["OEBPS/style.css"]
    );
}

#[test]
fn a_remark_id_is_where_a_link_to_it_lands() {
    let html = html(&doc(vec![
        Block::Remark {
            number: "2.0121".into(),
            id: Some("2.0121".into()),
            children: vec![para("The remark.")],
        },
        Block::Para {
            children: vec![Inline::Link {
                href: "2.0121".into(),
                internal: true,
                children: vec![text("see 2.0121")],
            }],
            align: None,
            indent_em: 0,
        },
    ]));
    assert!(
        html.contains(r#"<div class="remark" id="x2.0121">"#),
        "{html}"
    );
    assert!(
        html.contains(r##"<a href="#x2.0121">see 2.0121</a>"##),
        "{html}"
    );
}

fn svg(name: &str, fill: &str) -> Image {
    let file = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(
        &file,
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"9\" height=\"9\" fill=\"{fill}\"/></svg>"),
    )
    .unwrap();
    Image {
        file,
        format: "svg",
    }
}

#[test]
fn a_figure_with_an_ebook_image_shows_that_image() {
    let figure = Figure {
        src: "grey.svg".into(),
        alt: "a square".into(),
        ebook_src: Some("colour.svg".into()),
        width_px: None,
        caption: None,
        id: None,
        inline: None,
    };
    let entry = doc(vec![Block::Figure(figure)]);
    let both = BTreeMap::from([
        ("grey.svg".to_owned(), svg("ebook-grey.svg", "#888888")),
        ("colour.svg".to_owned(), svg("ebook-colour.svg", "#2a7ab0")),
    ]);
    let members = epub_text(&entry, &[], &both);
    assert!(
        members["OEBPS/img-1.svg"].contains("#2a7ab0"),
        "{members:?}"
    );
    assert!(
        !members.contains_key("OEBPS/img-2.svg"),
        "the grey one is not carried"
    );

    let print_only = BTreeMap::from([("grey.svg".to_owned(), svg("ebook-grey.svg", "#888888"))]);
    let members = epub_text(&entry, &[], &print_only);
    assert!(
        members["OEBPS/img-1.svg"].contains("#888888"),
        "without its ebook image, a figure shows the one it has"
    );
}

#[test]
fn a_figure_is_as_wide_as_the_source_shows_it_and_the_page_allows() {
    let sized = |width_px: Option<u32>, inline: bool| Figure {
        src: "grey.svg".into(),
        alt: "a square".into(),
        ebook_src: None,
        width_px,
        caption: None,
        id: None,
        inline: inline.then_some(true),
    };
    let images = BTreeMap::from([("grey.svg".to_owned(), svg("sized-grey.svg", "#888888"))]);
    let entry = doc(vec![
        Block::Figure(sized(Some(250), false)),
        Block::Para {
            children: vec![text("a sign "), Inline::Figure(sized(Some(30), true))],
            align: None,
            indent_em: 0,
        },
        Block::Figure(sized(None, false)),
    ]);
    let members = epub_text(&entry, &[], &images);
    let xhtml = &members["OEBPS/entry-1.xhtml"];
    assert!(
        xhtml.contains("<img src=\"img-1.svg\" alt=\"a square\" width=\"250\"/>"),
        "{xhtml}"
    );
    assert!(xhtml.contains("<img src=\"img-1.svg\" alt=\"a square\" width=\"30\"/>"));
    assert!(xhtml.contains("<img src=\"img-1.svg\" alt=\"a square\"/>"));
    roxmltree::Document::parse(xhtml).expect("well-formed XHTML");
    assert!(
        members["OEBPS/style.css"].contains("img{max-width:100%;height:auto;}"),
        "the style sheet keeps a wide one to the page"
    );
}

#[test]
fn the_stylesheet_has_rules_only_for_the_constructs_a_document_uses() {
    let style = style();
    let view = View::new(&style);
    let base = css::stylesheet(&view);
    let members = epub_text(&doc(vec![para("Plain.")]), &[], &BTreeMap::new());
    assert_eq!(members["OEBPS/style.css"], base);

    let spaced = doc(vec![Block::Para {
        children: vec![Inline::Spaced {
            children: vec![text("gesperrt")],
        }],
        align: None,
        indent_em: 0,
    }]);
    let members = epub_text(&spaced, &[], &BTreeMap::new());
    assert_eq!(
        members["OEBPS/style.css"],
        format!("{base}\nspan.spaced{{letter-spacing:0.2em;margin-right:-0.2em;}}")
    );

    let members = epub_text(&with_every_construct("words"), &[], &BTreeMap::new());
    for rule in [
        "p.right{",
        "p.indent-2{margin-left:2em;",
        "span.margin-note{",
        "span.smallcaps{font-variant:small-caps;}",
        "span.overline{",
        ".dashed{text-decoration-style:dashed;}",
        "div.aside{",
        "div.remark{margin-top:0pt;}",
        "div.columns{display:flex;",
        "div.verse>p{",
        "td.edge-top,th.edge-top{border-top:1px solid;}",
    ] {
        assert!(members["OEBPS/style.css"].contains(rule), "{rule}");
    }
}

#[test]
fn notes_the_document_holds_are_an_appendix_in_the_order_the_text_refers_to_them() {
    let refer = |id: &str, label: &str| {
        Inline::NoteRef(NoteRef {
            id: id.to_owned(),
            label: label.to_owned(),
            href: format!("#{id}"),
            ref_id: Some(format!("cite_ref-{label}")),
        })
    };
    let mut entry = doc(vec![Block::Para {
        children: vec![
            text("One"),
            refer("cite_note-10", "1"),
            refer("cite_note-2", "2"),
        ],
        align: None,
        indent_em: 0,
    }]);
    entry.notes = NoteTable::from([
        ("cite_note-10".to_owned(), vec![para("Tenth.")]),
        ("cite_note-2".to_owned(), vec![para("Second.")]),
        ("cite_note-9".to_owned(), vec![para("Never referred to.")]),
    ]);
    let members = epub_text(&entry, &["notes.style=appendix"], &BTreeMap::new());
    let page = &members["OEBPS/entry-1.xhtml"];
    text_of(page);
    assert!(
        page.contains(r##"<sup>[<a href="#cite_note-10" id="cite_ref-1">1</a>]</sup>"##),
        "{page}"
    );
    let tenth = page
        .find(r##"<aside class="note" epub:type="footnote" id="cite_note-10"><p class="flush"><a href="#cite_ref-1">1.</a> Tenth.</p></aside>"##)
        .unwrap_or_else(|| panic!("{page}"));
    let second = page.find(r#"id="cite_note-2""#).unwrap();
    let never = page
        .find(r#"id="cite_note-9"><p>Never referred to.</p></aside>"#)
        .unwrap_or_else(|| panic!("{page}"));
    assert!(tenth < second && second < never, "{page}");

    let members = epub_text(&entry, &["notes.style=footnote"], &BTreeMap::new());
    let page = &members["OEBPS/entry-1.xhtml"];
    assert!(
        page.contains(
            r##"<a class="noteref" epub:type="noteref" href="#cite_note-10" id="cite_ref-1">1</a>"##
        ),
        "{page}"
    );
    assert!(!page.contains("Never referred to."), "{page}");
}

#[test]
fn the_notes_the_document_holds_are_in_the_nav_when_they_are_an_appendix() {
    let mut entry = with_note(vec![para("An attached note.")]);
    entry.attachments.push(Attachment {
        kind: AttachmentKind::Supplement,
        title: "A Supplement".into(),
        blocks: vec![para("Supplementary.")],
        source_url: "https://plato.stanford.edu/entries/x/supplement.html".into(),
        notes: None,
    });
    entry.body.push(Block::Para {
        children: vec![
            text("Own"),
            Inline::NoteRef(NoteRef {
                id: "cite_note-1".into(),
                label: "2".into(),
                href: "#cite_note-1".into(),
                ref_id: None,
            }),
        ],
        align: None,
        indent_em: 0,
    });
    entry.notes = NoteTable::from([("cite_note-1".to_owned(), vec![para("Its own.")])]);

    let members = epub_text(&entry, &["notes.style=appendix"], &BTreeMap::new());
    let page = &members["OEBPS/entry-1.xhtml"];
    assert!(
        page.contains(
            r#"<section class="notes" epub:type="endnotes" id="notes-appendix"><h2>Notes</h2>"#
        ),
        "{page}"
    );
    let nav = &members["OEBPS/nav.xhtml"];
    assert!(
        nav.contains(concat!(
            r#"<li><a href="entry-1.xhtml#a1--attachment">Notes</a></li>"#,
            r#"<li><a href="entry-1.xhtml#notes-appendix">Notes</a></li>"#,
            r#"<li><a href="entry-1.xhtml#a2--attachment">A Supplement</a></li>"#
        )),
        "after the attached notes, before the supplement: {nav}"
    );

    let members = epub_text(&entry, &["notes.style=footnote"], &BTreeMap::new());
    assert!(
        !members["OEBPS/nav.xhtml"].contains("notes-appendix"),
        "as footnotes they are no section of their own"
    );

    let style = resolve_style(ResolveOptions {
        preset: Some("halfletter"),
        file: None,
        sets: vec!["notes.style=appendix".into()],
        schema: None,
        default_preset: None,
    })
    .unwrap();
    let view = View::new(&style);
    let images = BTreeMap::new();
    let sections: BTreeSet<String> = ["supplements".to_owned()].into();
    let mut ctx = Ctx::new(&view, &images, MathPass::Collect(Vec::new()));
    let html = entry_html(&entry, &mut ctx, &sections);
    assert!(!html.contains("notes-appendix"), "{html}");
    let nav = nav_items(&entry, "entry-1.xhtml", &view, &sections, &ctx.ids);
    assert!(
        !nav.contains("notes-appendix"),
        "without the notes, no entry for them: {nav}"
    );
}

#[test]
fn a_document_written_right_to_left_says_so_and_turns_its_pages_that_way() {
    let mut entry = doc(vec![para("نص")]);
    entry.meta.lang = "ar".into();
    entry.meta.dir = Dir::Rtl;
    let members = epub_text(&entry, &[], &BTreeMap::new());
    for name in ["OEBPS/entry-1.xhtml", "OEBPS/nav.xhtml"] {
        assert!(
            members[name].contains(r#" lang="ar" xml:lang="ar" dir="rtl">"#),
            "{name}: {}",
            members[name]
        );
    }
    let opf = &members["OEBPS/content.opf"];
    assert!(
        opf.contains(r#"<spine page-progression-direction="rtl"><itemref idref="c0"/></spine>"#),
        "{opf}"
    );

    let left = epub_text(&doc(vec![para("Text.")]), &[], &BTreeMap::new());
    assert!(!left["OEBPS/entry-1.xhtml"].contains("dir="));
    assert!(left["OEBPS/content.opf"].contains("<spine><itemref"));
}

#[test]
fn a_language_scope_is_a_division_that_names_its_language_and_direction() {
    let html = html(&doc(vec![
        Block::Lang {
            lang: "de-AT".into(),
            dir: None,
            children: vec![para("Ein Satz.")],
        },
        Block::Lang {
            lang: "ar".into(),
            dir: Some(Dir::Rtl),
            children: vec![para("نص")],
        },
        Block::Lang {
            lang: "en".into(),
            dir: Some(Dir::Ltr),
            children: vec![],
        },
    ]));
    assert!(
        html.contains(r#"<div lang="de-AT" xml:lang="de-AT"><p>Ein Satz.</p></div>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<div lang="ar" xml:lang="ar" dir="rtl"><p>نص</p></div>"#),
        "{html}"
    );
    assert!(
        !html.contains(r#"lang="en""#),
        "an empty scope is no scope: {html}"
    );
}

#[test]
fn a_language_tag_that_is_markup_is_left_out_and_the_text_kept() {
    for tag in HOSTILE
        .iter()
        .copied()
        .chain(["de\" onload=\"x", "ar><script>", "en&amp;"])
    {
        let entry = doc(vec![Block::Lang {
            lang: tag.into(),
            dir: Some(Dir::Rtl),
            children: vec![para(tag)],
        }]);
        let page = &epub_text(&entry, &[], &BTreeMap::new())["OEBPS/entry-1.xhtml"];
        assert!(page.contains(r#"<div dir="rtl"><p>"#), "{tag:?}: {page}");
        assert!(text_of(page).contains(tag), "{tag:?}: {page}");
    }
}
