use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use typesep_epub::{EpubOptions, build_epub};
use typesep_model::{
    Align, Block, Decoration, Doc, Edges, Figure, Inline, Meta, NoteRef, Origin, Rules, Table,
    TableCell, TocItem, VerseLine,
};
use typesep_style::{ResolveOptions, Style, resolve_style};
use typesep_typst::images::Image;
use typesep_typst::{EmitOptions, Fonts, TypesetWorld, compile_pdf, emit_document};

/// A document made up for these tests that uses every construct the model has for sources
/// other than the SEP at least once, typeset to PDF (as is, on mirrored pages, and in two
/// columns on mirrored pages) and to EPUB. With `TYPESEP_FIXTURE_OUT` naming a directory, the
/// outputs are left there for a look: `fixture.typ`, `fixture.pdf`, `fixture.epub`,
/// `fixture-twoside.typ`, `fixture-twoside.pdf`, `fixture-2col.typ` and `fixture-2col.pdf`.
const OUT: &str = "TYPESEP_FIXTURE_OUT";

/// The mirrored variant: one column on pages whose outer margin, where margin notes go, is
/// wider than the inner one.
const TWO_SIDED: [&str; 2] = ["page.twoside=true", "page.margins.outer=0.9in"];

/// The two-column variant: mirrored pages whose outer margin is wider than the inner one,
/// so a margin note shows which side of the page it reads.
const TWO_COLUMNS: [&str; 3] = [
    "page.columns=2",
    "page.twoside=true",
    "page.margins.outer=0.9in",
];

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

fn plain(value: &str) -> Block {
    para(vec![text(value)])
}

fn heading(level: u8, id: &str, value: &str, outline_only: bool) -> Block {
    Block::Heading {
        level,
        id: Some(id.to_owned()),
        children: vec![text(value)],
        outline_only,
    }
}

fn remark(number: &str, children: Vec<Block>) -> Block {
    Block::Remark {
        number: number.to_owned(),
        id: Some(number.to_owned()),
        children,
    }
}

fn note(id: &str, label: &str) -> Inline {
    Inline::NoteRef(NoteRef {
        id: id.to_owned(),
        label: label.to_owned(),
        href: format!("#{id}"),
        ref_id: Some(format!("ref-{id}")),
    })
}

fn margin(value: &str) -> Inline {
    Inline::MarginNote {
        children: vec![text(value)],
    }
}

fn link_to(id: &str, value: &str) -> Inline {
    Inline::Link {
        href: id.to_owned(),
        internal: true,
        children: vec![text(value)],
    }
}

fn cell(value: &str, header: bool, edges: Option<Edges>) -> TableCell {
    TableCell {
        header,
        colspan: 1,
        rowspan: 1,
        children: vec![text(value)],
        edges,
    }
}

/// A truth table whose rules come from its cells: a rule under the header row and one
/// between the arguments and the value.
fn truth_table(values: [&str; 4]) -> Block {
    let edges = |bottom: bool, right: bool| {
        Some(Edges {
            top: false,
            right,
            bottom,
            left: false,
        })
    };
    let mut rows = vec![vec![
        cell("p", true, edges(true, false)),
        cell("q", true, edges(true, true)),
        cell("f", true, edges(true, false)),
    ]];
    for (row, value) in [("T", "T"), ("F", "T"), ("T", "F"), ("F", "F")]
        .into_iter()
        .zip(values)
    {
        rows.push(vec![
            cell(row.0, false, None),
            cell(row.1, false, edges(false, true)),
            cell(value, false, None),
        ]);
    }
    Block::Table(Table {
        rows,
        columns: 3,
        caption: None,
        rules: Some(Rules::Cells),
    })
}

const FILLER: &str = "The keeper writes down what the lamp shows and what it hides, and the \
     record grows by a line each night; nothing in it is remarkable except that it is kept, \
     and that every entry answers to the one before it.";

fn fixture() -> Doc {
    let mut body = vec![
        heading(2, "first", "First Part: Remarks", false),
        remark(
            "1",
            vec![para(vec![
                text("A lighthouse is kept by whoever climbs its stairs at dusk."),
                note("cite_note-1", "1"),
            ])],
        ),
        remark(
            "1.1",
            vec![
                plain("A remark may run over several paragraphs. This is its first one."),
                plain("And this is its second, which carries no number of its own."),
            ],
        ),
        remark(
            "1.11",
            vec![para(vec![
                text("Some words are "),
                Inline::Spaced {
                    children: vec![text("spaced out")],
                },
                text(", some are in "),
                Inline::SmallCaps {
                    children: vec![text("Small Capitals")],
                },
                text(", and this one points back to "),
                link_to("1.1", "remark 1.1"),
                text(" and on to "),
                link_to("2.0121", "remark 2.0121"),
                text("."),
                note("cite_note-2", "2"),
            ])],
        ),
        para(vec![
            text("Decorations: "),
            Inline::Decorated {
                line: Decoration::Under,
                dashed: false,
                children: vec![text("underlined")],
            },
            text(", "),
            Inline::Decorated {
                line: Decoration::Under,
                dashed: true,
                children: vec![text("dashed underline")],
            },
            text(", "),
            Inline::Decorated {
                line: Decoration::Over,
                dashed: false,
                children: vec![text("overlined")],
            },
            text(", "),
            Inline::Decorated {
                line: Decoration::Strike,
                dashed: false,
                children: vec![text("struck out")],
            },
            text(" and "),
            Inline::Decorated {
                line: Decoration::Strike,
                dashed: true,
                children: vec![text("dashed strike")],
            },
            text("."),
        ]),
        para(vec![
            margin("(Ms-101,7)"),
            text(
                " A paragraph that opens with a siglum in the margin, which is set in the line \
                  instead when the margin is too narrow for it. ",
            ),
            text(FILLER),
            text(" Halfway through, a second mark "),
            margin("*"),
            text(" stands beside its own line. "),
            text(FILLER),
        ]),
        Block::Para {
            children: vec![text("1931")],
            align: Some(Align::Right),
            indent_em: 0,
        },
        Block::Para {
            children: vec![text(
                "An indented paragraph: every one of its lines is set in by three ems, the \
                 first as well as the rest, as a letter's dateline or a quoted remark is.",
            )],
            align: None,
            indent_em: 3,
        },
        Block::Rule,
        Block::Verse {
            lines: vec![
                VerseLine {
                    indent_em: 0,
                    children: vec![text("The tide comes in across the stones,")],
                },
                VerseLine {
                    indent_em: 2,
                    children: vec![text("and every stone is counted;")],
                },
                VerseLine {
                    indent_em: 2,
                    children: vec![text(
                        "the keeper keeps a ledger of the tide, and a line too long for \
                         its measure runs over and hangs beneath itself;",
                    )],
                },
                VerseLine {
                    indent_em: 0,
                    children: vec![],
                },
                VerseLine {
                    indent_em: 4,
                    children: vec![text("and he closes it at dawn.")],
                },
            ],
        },
        Block::Aside {
            children: vec![
                plain(
                    "A boxed insert, set apart from the text around it by a thin grey \
                     rule. It is read on its own, or passed over.",
                ),
                plain("It may hold more than one paragraph."),
            ],
        },
        heading(2, "hidden", "A Heading Only the Outline Shows", true),
        remark(
            "2",
            vec![
                plain("Side by side, two tables whose rules come from their cells:"),
                Block::Columns {
                    columns: vec![
                        vec![truth_table(["T", "T", "T", "F"])],
                        vec![truth_table(["T", "F", "F", "F"])],
                    ],
                },
            ],
        ),
        Block::Remark {
            number: "2.0121".to_owned(),
            id: Some("2.0121".to_owned()),
            children: vec![
                Block::Table(Table {
                    rows: vec![
                        vec![
                            cell("A layout table", false, None),
                            cell("draws no rules", false, None),
                        ],
                        vec![
                            cell("whatever", false, None),
                            cell("the style says", false, None),
                        ],
                    ],
                    columns: 2,
                    caption: None,
                    rules: Some(Rules::None),
                }),
                plain("This remark opens with a table, so its number stands on its own line."),
            ],
        },
        Block::Columns {
            columns: vec![
                vec![
                    plain("The left column of two."),
                    plain("It has a second paragraph."),
                ],
                vec![plain(
                    "The right column, which is longer and so wraps over more lines than the \
                     left one does.",
                )],
            ],
        },
        Block::Figure(Figure {
            src: "fixture/grey.svg".to_owned(),
            alt: "a square".to_owned(),
            ebook_src: Some("fixture/colour.svg".to_owned()),
            width_px: None,
            caption: Some(vec![text(
                "Grey in the PDF, coloured in the EPUB, which reads ebookSrc.",
            )]),
            id: None,
            inline: None,
        }),
    ];
    for n in 1..=16 {
        body.push(remark(
            &format!("3.{n}"),
            vec![para(vec![
                margin(&format!("(Ts-{n})")),
                text(FILLER),
                text(" "),
                text(FILLER),
            ])],
        ));
    }
    body.push(heading(2, "after", "Second Part", false));
    body.push(plain("The end of the fixture."));

    Doc {
        meta: Meta {
            id: "fixture".to_owned(),
            title: "A Synthetic Treatise".to_owned(),
            byline: "Nobody in Particular".to_owned(),
            lang: "en".to_owned(),
            origin: Origin {
                source: "fixture".to_owned(),
                publisher: "typesep test fixtures".to_owned(),
                url: "https://example.org/fixture".to_owned(),
                ..Origin::default()
            },
            ..Meta::default()
        },
        toc: vec![
            TocItem {
                text: "First Part: Remarks".to_owned(),
                href: "#first".to_owned(),
                children: vec![],
            },
            TocItem {
                text: "A Heading Only the Outline Shows".to_owned(),
                href: "#hidden".to_owned(),
                children: vec![],
            },
            TocItem {
                text: "Second Part".to_owned(),
                href: "#after".to_owned(),
                children: vec![],
            },
        ],
        body,
        notes: BTreeMap::from([
            (
                "cite_note-1".to_owned(),
                vec![plain("A note the document holds itself.")],
            ),
            (
                "cite_note-2".to_owned(),
                vec![
                    plain("A second note, of two paragraphs."),
                    plain("This is its second."),
                ],
            ),
        ]),
        ..Doc::default()
    }
}

fn style(sets: &[&str]) -> Style {
    resolve_style(ResolveOptions {
        preset: Some("halfletter"),
        file: None,
        sets: sets.iter().map(|s| (*s).into()).collect(),
        schema: None,
        default_preset: None,
    })
    .unwrap()
}

/// The two figures, as files: a grey square for print and a coloured one for the EPUB.
fn images(dir: &Path) -> BTreeMap<String, Image> {
    let square = |fill: &str| {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"120\" height=\"80\"><rect width=\"120\" height=\"80\" fill=\"{fill}\"/></svg>"
        )
    };
    let mut out = BTreeMap::new();
    for (name, fill) in [("grey", "#888888"), ("colour", "#2a7ab0")] {
        let file = dir.join(format!("{name}.svg"));
        std::fs::write(&file, square(fill)).unwrap();
        out.insert(
            format!("fixture/{name}.svg"),
            Image {
                file,
                format: "svg",
            },
        );
    }
    out
}

fn sections() -> BTreeSet<String> {
    ["notes"].map(str::to_owned).into()
}

fn out_dir() -> Option<PathBuf> {
    std::env::var_os(OUT)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

fn pdf(doc: &Doc, style: &Style, images: &BTreeMap<String, Image>, name: &str) -> usize {
    let fonts = Fonts::embedded();
    let sections = sections();
    let rendered = emit_document(
        &[doc],
        style,
        EmitOptions {
            sections: &sections,
            images,
            fonts: Some(&fonts),
            ..Default::default()
        },
    );
    assert!(rendered.warnings.is_empty(), "{:?}", rendered.warnings);
    let world = TypesetWorld::new(rendered.source.clone(), &rendered.figures, &fonts);
    let compiled = compile_pdf(&world, "typesep tests")
        .unwrap_or_else(|error| panic!("{name} does not compile: {error}"));
    assert!(compiled.warnings.is_empty(), "{:?}", compiled.warnings);
    if let Some(dir) = out_dir() {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.typ")), &rendered.source).unwrap();
        std::fs::write(dir.join(format!("{name}.pdf")), &compiled.pdf).unwrap();
    }
    compiled.pages
}

#[test]
fn the_fixture_typesets_to_pdf_in_one_column_on_plain_and_mirrored_pages_and_in_two() {
    let doc = fixture();
    let dir = tempfile::tempdir().unwrap();
    let images = images(dir.path());
    let one = pdf(&doc, &style(&[]), &images, "fixture");
    let mirrored = pdf(&doc, &style(&TWO_SIDED), &images, "fixture-twoside");
    let two = pdf(&doc, &style(&TWO_COLUMNS), &images, "fixture-2col");
    assert!(one > 2, "{one} pages");
    assert!(mirrored > 2, "{mirrored} pages");
    assert!(two > 1, "{two} pages");
}

#[test]
fn the_fixture_builds_as_an_epub() {
    let doc = fixture();
    let dir = tempfile::tempdir().unwrap();
    let images = images(dir.path());
    let fonts = Fonts::embedded();
    let sections = sections();
    let epub = build_epub(
        &[&doc],
        &style(&[]),
        EpubOptions {
            sections: &sections,
            images: &images,
            volume: None,
            fonts: &fonts,
        },
    )
    .unwrap();
    assert!(epub.warnings.is_empty(), "{:?}", epub.warnings);
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(epub.bytes.clone())).unwrap();
    let mut names: Vec<String> = archive.file_names().map(str::to_owned).collect();
    names.sort();
    assert!(names.contains(&"OEBPS/img-1.svg".to_owned()), "{names:?}");
    let mut image = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("OEBPS/img-1.svg").unwrap(), &mut image)
        .unwrap();
    assert!(
        image.contains("#2a7ab0"),
        "the EPUB has the coloured figure"
    );
    if let Some(dir) = out_dir() {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("fixture.epub"), &epub.bytes).unwrap();
    }
}
