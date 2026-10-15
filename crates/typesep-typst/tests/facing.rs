use std::collections::BTreeMap;

use typesep_model::{Block, Dir, Doc, Inline, Meta, NoteRef};
use typesep_style::{ResolveOptions, resolve_style};
use typesep_typst::{EmitOptions, Fonts, Rendered, TypesetWorld, emit_document};
use typst::foundations::{Label, Selector, Value};
use typst::introspection::Introspector;
use typst::utils::PicoStr;
use typst_layout::PagedDocument;

/// Words made up for these tests, to make sentences of in each language.
const GERMAN: [&str; 8] = [
    "Der Wärter",
    "zählt",
    "die Steine",
    "am Ufer",
    "und das Licht",
    "zeigt",
    "den Weg",
    "der Schiffe",
];
const ENGLISH: [&str; 8] = [
    "The keeper",
    "counts",
    "the stones",
    "on the shore",
    "and the light",
    "shows",
    "the way",
    "of the ships",
];
const ARABIC: [&str; 8] = [
    "الحارس",
    "يعد",
    "الحجارة",
    "على الشاطئ",
    "والضوء",
    "يظهر",
    "طريق",
    "السفن",
];

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.to_owned(),
    }
}

fn sentence(words: &[&str; 8], k: usize, length: usize) -> String {
    let mut out: Vec<&str> = (0..length).map(|i| words[(k + i * 3) % 8]).collect();
    out.push(".");
    out.join(" ").replace(" .", ".")
}

fn para(value: String) -> Block {
    Block::Para {
        children: vec![text(&value)],
        align: None,
        indent_em: 0,
    }
}

/// A text of remarks `ids`, the `k`th of `sizes[k % len]` paragraphs, each of a few lines; a
/// note to every seventh remark; and a preface.
fn work(
    title: &str,
    lang: &str,
    dir: Dir,
    words: &[&str; 8],
    ids: &[String],
    sizes: &[usize],
) -> Doc {
    let mut body = vec![para(sentence(words, 0, 30)), para(sentence(words, 1, 24))];
    let mut notes = BTreeMap::new();
    for (k, id) in ids.iter().enumerate() {
        let paragraphs = sizes[k % sizes.len()];
        let mut children: Vec<Block> = (0..paragraphs)
            .map(|p| para(sentence(words, k + p, 12 + (k * 7 + p) % 30)))
            .collect();
        if k % 7 == 3 {
            let note = format!("cite_note-{k}");
            if let Some(Block::Para { children, .. }) = children.first_mut() {
                children.push(Inline::NoteRef(NoteRef {
                    id: note.clone(),
                    label: (notes.len() + 1).to_string(),
                    href: format!("#{note}"),
                    ref_id: Some(format!("cite_ref-{k}")),
                }));
            }
            notes.insert(note, vec![para(sentence(words, k, 20 + k % 25))]);
        }
        body.push(Block::Remark {
            number: id.clone(),
            id: Some(id.clone()),
            children,
        });
    }
    Doc {
        meta: Meta {
            id: title.into(),
            title: title.into(),
            lang: lang.into(),
            dir,
            ..Meta::default()
        },
        body,
        notes,
        ..Doc::default()
    }
}

fn ids(count: usize) -> Vec<String> {
    (0..count)
        .map(|k| format!("{}.{}", k / 10 + 1, k % 10))
        .collect()
}

/// The source of the bilingual edition of `left` and `right` under `sets`, and what it
/// compiles to.
fn edition(left: &Doc, right: &Doc, sets: &[&str]) -> (Rendered, PagedDocument) {
    let style = resolve_style(ResolveOptions {
        preset: Some("lwp"),
        file: None,
        sets: sets.iter().map(|s| (*s).into()).collect(),
        schema: None,
        default_preset: None,
    })
    .unwrap();
    let made = typesep_compose::parallel(
        left,
        right,
        Meta {
            id: "both".into(),
            title: "Both".into(),
            lang: left.meta.lang.clone(),
            ..Meta::default()
        },
    )
    .unwrap();
    let fonts = Fonts::embedded();
    let rendered = emit_document(
        &[&made.doc],
        &style,
        EmitOptions {
            fonts: Some(&fonts),
            parallel: true,
            ..Default::default()
        },
    );
    let world = TypesetWorld::new(rendered.source.clone(), &rendered.figures, &fonts);
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .unwrap_or_else(|errors| panic!("{errors:?}"));
    (rendered, document)
}

/// The page each mark labelled `name` landed on, with its value.
fn pages_of(document: &PagedDocument, name: &str) -> Vec<(Value, usize)> {
    let introspector = document.introspector();
    let label = Selector::Label(Label::new(PicoStr::intern(name)).unwrap());
    introspector
        .query(&label)
        .iter()
        .map(|mark| {
            let page = introspector.page(mark.location().unwrap()).unwrap().get();
            (mark.get_by_name("value").unwrap_or(Value::None), page)
        })
        .collect()
}

fn key(value: &Value) -> (i64, i64) {
    let Value::Dict(dict) = value else {
        panic!("a mark of a spread: {value:?}")
    };
    let int = |name: &str| match dict.get(name) {
        Ok(Value::Int(n)) => *n,
        other => panic!("{name}: {other:?}"),
    };
    (int("index"), int("side"))
}

/// That every side of every spread is on one page, the left on an even page and the right on
/// the one after it; that every footnote is on the page it is called from; and that a remark
/// both texts have starts on the two pages of one spread. The number of spreads.
fn assert_spreads_hold(document: &PagedDocument, ids: &[String]) -> usize {
    let starts: BTreeMap<(i64, i64), usize> = pages_of(document, "fs-spread")
        .iter()
        .map(|(value, page)| (key(value), *page))
        .collect();
    let ends: BTreeMap<(i64, i64), usize> = pages_of(document, "fs-spread-end")
        .iter()
        .map(|(value, page)| (key(value), *page))
        .collect();
    assert_eq!(starts.len(), ends.len());
    for (&(spread, side), &page) in &starts {
        assert_eq!(
            ends[&(spread, side)],
            page,
            "spread {spread}, side {side} is on one page"
        );
        let left = starts[&(spread, 0)];
        assert_eq!(left % 2, 0, "spread {spread} starts on a left-hand page");
        assert_eq!(page, left + side as usize, "spread {spread}, side {side}");
    }
    for (value, page) in pages_of(document, "fs-footnote-at") {
        let Value::Dict(call) = value else { panic!() };
        let Ok(Value::Int(called)) = call.get("page") else {
            panic!()
        };
        assert_eq!(
            *called as usize, page,
            "a note is on the page it is called from"
        );
    }
    for id in ids {
        let on = |side: &str| {
            pages_of(document, &format!("{side}--{id}"))
                .first()
                .map(|(_, page)| *page)
        };
        if let (Some(left), Some(right)) = (on("left"), on("right")) {
            assert_eq!(right, left + 1, "remark {id} starts on one spread");
        }
    }
    starts.len() / 2
}

#[test]
fn the_texts_face_each_other_a_spread_starting_at_the_same_remark_on_both_pages() {
    let ids = ids(60);
    let german = work("Ein Heft", "de", Dir::Ltr, &GERMAN, &ids, &[1, 2, 1, 3]);
    let english = work("A Notebook", "en", Dir::Ltr, &ENGLISH, &ids, &[1, 1, 2, 3]);
    let (rendered, document) = edition(&german, &english, &[]);
    let spreads = assert_spreads_hold(&document, &ids);
    assert!(spreads > 3, "{spreads} spreads");
    assert!(
        rendered.warnings.is_empty(),
        "no row is longer than a page: {:?}",
        rendered.warnings
    );
    assert!(
        !rendered.source.contains("#fs-columns("),
        "facing pages set no rows of columns"
    );
    let (again, _) = edition(&german, &english, &[]);
    assert_eq!(
        again.source, rendered.source,
        "the plan is the same every time"
    );
}

#[test]
fn a_remark_longer_than_a_page_continues_on_the_same_side_of_the_next_spread() {
    let ids = ids(12);
    let german = work(
        "Ein Heft",
        "de",
        Dir::Ltr,
        &GERMAN,
        &ids,
        &[1, 2, 1, 2, 1, 30],
    );
    let english = work(
        "A Notebook",
        "en",
        Dir::Ltr,
        &ENGLISH,
        &ids,
        &[1, 1, 2, 1, 1, 9],
    );
    let (rendered, document) = edition(&german, &english, &[]);
    assert_spreads_hold(&document, &ids);
    assert_eq!(
        rendered.warnings,
        [
            "2 rows are longer than a page, so each starts a spread and runs on to the same side of the next one, the shorter side's page left empty below it: remark 1.5, remark 2.1"
        ]
    );
}

#[test]
fn a_text_written_right_to_left_faces_the_other_too() {
    let ids = ids(30);
    let german = work("Ein Heft", "de", Dir::Ltr, &GERMAN, &ids, &[1, 2, 1]);
    let arabic = work("Daftar", "ar", Dir::Rtl, &ARABIC, &ids, &[2, 1, 1]);
    let (rendered, document) = edition(&german, &arabic, &[]);
    assert_spreads_hold(&document, &ids);
    assert!(
        rendered
            .source
            .contains("#[\n#set text(lang: \"ar\", hyphenate: false, dir: rtl)"),
        "the right side is set right to left"
    );
}

#[test]
fn in_columns_the_texts_share_every_page_a_row_to_each_remark() {
    let ids = ids(20);
    let german = work("Ein Heft", "de", Dir::Ltr, &GERMAN, &ids, &[1, 2]);
    let english = work("A Notebook", "en", Dir::Ltr, &ENGLISH, &ids, &[2, 1]);
    let (rendered, _) = edition(&german, &english, &["parallel.layout=columns"]);
    assert_eq!(rendered.source.matches("\n#fs-columns(").count(), 21);
    assert!(!rendered.source.contains("pagebreak(to:"));
    assert!(!rendered.source.contains("fs-footnote-at"));
}

#[test]
fn a_paragraph_longer_than_a_page_runs_on_and_the_spreads_after_it_hold() {
    let ids = ids(20);
    let mut german = work("Ein Heft", "de", Dir::Ltr, &GERMAN, &ids, &[1, 2]);
    let english = work("A Notebook", "en", Dir::Ltr, &ENGLISH, &ids, &[2, 1]);
    // Remark 1.4, the sixth block of the body after the two of the preface, gets one
    // paragraph of some six hundred words.
    if let Block::Remark { children, .. } = &mut german.body[6] {
        children.push(para(sentence(&GERMAN, 0, 1200)));
    }
    let (rendered, document) = edition(&german, &english, &[]);
    assert_eq!(
        rendered.warnings,
        [
            "remark 1.4 is longer than a page, so it starts a spread and runs on to the same side of the next one, the shorter side's page left empty below it",
            "a paragraph longer than a page runs on to the next page, the opposite text's, in remark 1.4"
        ]
    );
    let starts: Vec<((i64, i64), usize)> = pages_of(&document, "fs-spread")
        .iter()
        .map(|(value, page)| (key(value), *page))
        .collect();
    let ends: Vec<usize> = pages_of(&document, "fs-spread-end")
        .iter()
        .map(|(_, page)| *page)
        .collect();
    let ran_on: Vec<i64> = starts
        .iter()
        .zip(&ends)
        .filter(|((_, start), end)| start != *end)
        .map(|(((spread, _), _), _)| *spread)
        .collect();
    assert_eq!(ran_on.len(), 1, "one side runs on: {starts:?} {ends:?}");
    for ((spread, side), page) in &starts {
        if *spread > ran_on[0] {
            assert_eq!(
                page % 2,
                *side as usize,
                "spread {spread} after it starts on a left-hand page again"
            );
        }
    }
}
