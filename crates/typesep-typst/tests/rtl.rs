use typesep_model::{Block, Dir, Doc, Inline, List, Meta};
use typesep_style::{ResolveOptions, resolve_style};
use typesep_typst::{EmitOptions, Fonts, TypesetWorld, emit_document};
use typst::layout::{Frame, FrameItem};
use typst_layout::PagedDocument;

/// Text made up for these tests, in Arabic: "this is a sentence of text written for the test".
const ARABIC: &str = "هذه جملة من نص كتب للاختبار";

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

fn arabic_doc(body: Vec<Block>) -> Doc {
    Doc {
        meta: Meta {
            id: "rtl".into(),
            title: "RTL".into(),
            lang: "ar".into(),
            dir: Dir::Rtl,
            ..Meta::default()
        },
        body,
        ..Doc::default()
    }
}

/// The source `doc` is emitted as under `sets`, its warnings, and the document it compiles to.
fn compile(doc: &Doc, sets: &[&str]) -> (String, Vec<String>, PagedDocument) {
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
        &[doc],
        &style,
        EmitOptions {
            fonts: Some(&fonts),
            ..Default::default()
        },
    );
    let world = TypesetWorld::new(rendered.source.clone(), &rendered.figures, &fonts);
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .unwrap_or_else(|errors| panic!("{errors:?}\n{}", rendered.source));
    (rendered.source, rendered.warnings, document)
}

/// Every text run of `frame` with the x it starts and ends at on the page, its text and its
/// font's family.
fn runs(frame: &Frame, x: f64, out: &mut Vec<(f64, f64, String, String)>) {
    for (pos, item) in frame.items() {
        match item {
            FrameItem::Group(group) => runs(
                &group.frame,
                x + pos.x.to_pt() + group.transform.tx.to_pt(),
                out,
            ),
            FrameItem::Text(run) => {
                let start = x + pos.x.to_pt();
                out.push((
                    start,
                    start + run.width().to_pt(),
                    run.text.to_string(),
                    run.font.info().family.clone(),
                ));
            }
            _ => {}
        }
    }
}

fn page_runs(document: &PagedDocument, page: usize) -> Vec<(f64, f64, String, String)> {
    let mut out = Vec::new();
    runs(&document.pages()[page].frame, 0.0, &mut out);
    out
}

fn find<'a>(runs: &'a [(f64, f64, String, String)], word: &str) -> &'a (f64, f64, String, String) {
    runs.iter()
        .find(|(_, _, text, _)| text.contains(word))
        .unwrap_or_else(|| panic!("{word} is set: {runs:?}"))
}

/// Where the Arabic of a page ends on the right, and the family it is set in: Typst sets
/// each joined letter of it as a run of its own.
fn arabic_end(runs: &[(f64, f64, String, String)]) -> (f64, &str) {
    runs.iter()
        .filter(|(_, _, text, _)| text.chars().any(|c| ('\u{600}'..='\u{6ff}').contains(&c)))
        .map(|(_, end, _, family)| (*end, family.as_str()))
        .fold(
            (0.0, ""),
            |best, run| if run.0 > best.0 { run } else { best },
        )
}

#[test]
fn a_document_written_right_to_left_is_set_so_in_the_font_that_has_its_letters() {
    let doc = arabic_doc(vec![para(vec![text(ARABIC)])]);
    let (source, warnings, document) = compile(&doc, &[]);
    assert!(
        source.contains("#set text(lang: \"ar\", dir: rtl, hyphenate: false)"),
        "{source}"
    );
    assert!(
        source.contains("#let fs-main = (\"TeX Gyre Termes\", \"Noto Naskh Arabic\")"),
        "{source}"
    );
    assert_eq!(
        warnings,
        [
            "Typst has no hyphenation patterns for ar, the document's language, so it is not hyphenated"
        ]
    );
    let runs = page_runs(&document, 0);
    let (end, family) = arabic_end(&runs);
    assert_eq!(family, "Noto Naskh Arabic");
    assert!(
        (end - (396.0 - 38.88)).abs() < 1.0,
        "the line starts at the right margin: ends at {end}"
    );
    let footer = runs.last().unwrap();
    assert_eq!(footer.2, "1", "{runs:?}");
    assert!(
        (footer.1 - (396.0 - 38.88)).abs() < 1.0,
        "the page number is in the right slot of the footer, on the right: {footer:?}"
    );
}

#[test]
fn a_remark_number_and_a_list_marker_stand_at_the_right_of_text_written_right_to_left() {
    let doc = arabic_doc(vec![
        Block::Remark {
            number: "1.1".into(),
            id: Some("1.1".into()),
            children: vec![para(vec![text(ARABIC)])],
        },
        Block::List(List {
            ordered: true,
            hanging: false,
            items: vec![vec![para(vec![text("كلمة")])]],
            numbering: None,
            start: None,
            labels: None,
        }),
    ]);
    let (_, _, document) = compile(&doc, &[]);
    let runs = page_runs(&document, 0);
    let &(number_x, _, _, _) = find(&runs, "1.1");
    let (text_end, _) = arabic_end(&runs);
    assert!(
        number_x > text_end,
        "the number is right of its text: {number_x} after {text_end}"
    );
    let &(marker_x, _, _, _) = runs
        .iter()
        .find(|(x, _, text, _)| text == "1" && *x > 300.0)
        .unwrap_or_else(|| panic!("a list marker: {runs:?}"));
    let &(_, item_end, _, _) = find(&runs, "كلمة");
    assert!(
        marker_x > item_end,
        "the marker is right of its item: {marker_x} after {item_end}"
    );
}

/// The x a margin note starts at on each page it is on, the document long enough to fill a
/// few pages with a note to each paragraph.
fn margin_notes(sets: &[&str]) -> Vec<(usize, f64)> {
    let filler: String = std::iter::repeat_n(ARABIC, 30)
        .collect::<Vec<_>>()
        .join(" ");
    let body = (1..=8)
        .map(|k| {
            para(vec![
                Inline::MarginNote {
                    children: vec![text(&format!("n{k}"))],
                },
                text(&format!(" {filler}")),
            ])
        })
        .collect();
    let (_, _, document) = compile(&arabic_doc(body), sets);
    let mut out = Vec::new();
    for page in 0..document.pages().len() {
        for (x, _, text, _) in page_runs(&document, page) {
            if text.starts_with('n') && text.len() == 2 {
                out.push((page + 1, x));
            }
        }
    }
    assert_eq!(out.len(), 8, "every note is set: {out:?}");
    out
}

#[test]
fn a_margin_note_is_in_the_margin_lines_start_from_in_text_written_right_to_left() {
    for (page, x) in margin_notes(&[]) {
        assert!(x > 396.0 - 38.88, "page {page}: the right margin, at {x}");
    }
}

#[test]
fn a_margin_note_is_in_the_outer_margin_of_a_book_bound_on_the_right() {
    let placed = margin_notes(&["page.twoside=true"]);
    for &(page, x) in &placed {
        if page % 2 == 1 {
            assert!(x < 38.88, "recto {page}, the outer margin on the left: {x}");
        } else {
            assert!(x > 396.0 - 38.88, "verso {page}, on the right: {x}");
        }
    }
    assert!(placed.iter().any(|(page, _)| page % 2 == 0), "{placed:?}");
}

#[test]
fn a_language_scope_sets_its_blocks_in_its_language_and_direction() {
    let doc = Doc {
        meta: Meta {
            id: "scopes".into(),
            title: "Scopes".into(),
            lang: "en".into(),
            ..Meta::default()
        },
        body: vec![
            Block::Lang {
                lang: "de-AT".into(),
                dir: None,
                children: vec![para(vec![text("Ein Satz.")])],
            },
            Block::Lang {
                lang: "ar".into(),
                dir: Some(Dir::Rtl),
                children: vec![para(vec![text(ARABIC)])],
            },
            Block::Lang {
                lang: "hi".into(),
                dir: None,
                children: vec![],
            },
        ],
        ..Doc::default()
    };
    let (source, warnings, document) = compile(&doc, &[]);
    assert!(
        source.contains("#[\n#set text(lang: \"de\", region: \"AT\")\n\nEin Satz.\n]"),
        "{source}"
    );
    assert!(
        source.contains("#[\n#set text(lang: \"ar\", hyphenate: false, dir: rtl)\n\n"),
        "{source}"
    );
    assert!(
        !source.contains("\"hi\""),
        "an empty scope is no scope: {source}"
    );
    assert!(
        source.contains("#set text(lang: \"en\", hyphenate: true)"),
        "{source}"
    );
    assert_eq!(
        warnings,
        ["Typst has no hyphenation patterns for ar, so text in it is not hyphenated"]
    );
    let runs = page_runs(&document, 0);
    let &(german, _, _, _) = find(&runs, "Ein");
    let (end, _) = arabic_end(&runs);
    assert!(
        (german - 38.88).abs() < 1.0,
        "German starts on the left: {german}"
    );
    assert!(
        (end - (396.0 - 38.88 - 11.955)).abs() < 1.0,
        "Arabic starts on the right, indented there as a paragraph after another: {end}"
    );
}
