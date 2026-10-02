use std::collections::BTreeMap;

use typesep_model::{
    Attachment, AttachmentKind, Block, Doc, Inline, Meta, NoteRef, NoteTable, Origin,
};
use typesep_style::{ResolveOptions, Style, resolve_style};
use typesep_typst::{Compiled, EmitOptions, Fonts, SepWorld, compile_pdf, emit_document};

fn style(sets: &[&str]) -> Style {
    resolve_style(ResolveOptions {
        preset: Some("halfletter"),
        file: None,
        sets: sets.iter().map(|s| (*s).into()).collect(),
        schema: None,
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

fn filler(words: usize) -> String {
    ["pleasure", "and", "pain", "are", "felt", "as", "such"]
        .iter()
        .cycle()
        .take(words)
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

fn entry(slug: &str, notes: &[(i64, usize)]) -> Doc {
    let mut table: NoteTable = BTreeMap::new();
    let mut body = Vec::new();
    for (number, words) in notes {
        let id = format!("note-{number}");
        table.insert(id.clone(), vec![para(&filler(*words))]);
        body.push(Block::Para {
            children: vec![
                text("A sentence that carries a note"),
                Inline::NoteRef(NoteRef {
                    id: id.clone(),
                    label: number.to_string(),
                    href: format!("notes.html#{id}"),
                    ref_id: None,
                }),
                text(", and then some words after it."),
            ],
            align: None,
            indent_em: 0,
        });
    }
    Doc {
        meta: Meta {
            id: slug.to_owned(),
            title: slug.to_owned(),
            origin: Origin {
                url: format!("https://plato.stanford.edu/entries/{slug}/"),
                ..Origin::default()
            },
            ..Meta::default()
        },
        body,
        attachments: vec![Attachment {
            kind: AttachmentKind::Notes,
            title: "Notes".to_owned(),
            blocks: Vec::new(),
            source_url: format!("https://plato.stanford.edu/entries/{slug}/notes.html"),
            notes: Some(table),
        }],
        ..Doc::default()
    }
}

fn render(docs: &[&Doc], style: &Style) -> (String, Compiled) {
    let fonts = Fonts::embedded();
    let volume = Meta {
        title: "A volume".to_owned(),
        ..Meta::default()
    };
    let rendered = emit_document(
        docs,
        style,
        EmitOptions {
            fonts: Some(&fonts),
            volume: (docs.len() > 1).then_some(&volume),
            ..Default::default()
        },
    );
    let world = SepWorld::new(rendered.source.clone(), &rendered.figures, &fonts);
    let compiled = compile_pdf(&world, "typesep tests").expect("the document compiles");
    (rendered.source, compiled)
}

#[test]
fn a_note_taller_than_the_threshold_is_set_at_the_end_of_the_entry() {
    let doc = entry("pleasure", &[(1, 100), (2, 1500)]);
    let (source, compiled) = render(&[&doc], &style(&[]));

    assert!(source.contains("#fs-note(0, 1)"), "{source}");
    assert!(source.contains("#fs-note(0, 2)"), "{source}");
    assert!(
        source.contains("#fs-moved-notes(0, title: [Notes])"),
        "{source}"
    );
    let moved = &compiled.moved_notes;
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(moved[0].number, 2);
    assert_eq!(moved[0].entry, 0);
    assert!(moved[0].heights > 1.0, "{moved:?}");
}

#[test]
fn a_lower_threshold_moves_the_shorter_note_too_and_zero_turns_the_policy_off() {
    let doc = entry("pleasure", &[(1, 100), (2, 1500)]);

    let (_, low) = render(&[&doc], &style(&["notes.maxHeight=0.05"]));
    let numbers: Vec<i64> = low.moved_notes.iter().map(|n| n.number).collect();
    assert_eq!(numbers, vec![1, 2], "{:?}", low.moved_notes);

    let (source, off) = render(&[&doc], &style(&["notes.maxHeight=0"]));
    assert!(off.moved_notes.is_empty(), "{:?}", off.moved_notes);
    assert!(
        !source.contains("#fs-note("),
        "no measurement at all: {source}"
    );
    assert!(!source.contains("#fs-moved-notes("), "{source}");
    assert!(source.contains("#footnote["), "{source}");
}

#[test]
fn each_entry_of_a_volume_keeps_its_own_moved_notes() {
    let first = entry("pleasure", &[(1, 1500)]);
    let second = entry("hedonism", &[(1, 40), (2, 1500)]);
    let (source, compiled) = render(&[&first, &second], &style(&[]));

    assert!(
        source.contains("#fs-moved-notes(0, title: [Notes])"),
        "{source}"
    );
    assert!(
        source.contains("#fs-moved-notes(1, title: [Notes])"),
        "{source}"
    );
    let moved: Vec<(usize, i64)> = compiled
        .moved_notes
        .iter()
        .map(|n| (n.entry, n.number))
        .collect();
    assert_eq!(moved, vec![(0, 1), (1, 2)], "{:?}", compiled.moved_notes);
}

#[test]
fn moved_notes_take_a_heading_of_their_own_when_the_endnotes_have_taken_notes() {
    let mut doc = entry("pleasure", &[(1, 1500)]);
    doc.body.push(Block::Para {
        children: vec![
            text("See "),
            Inline::Link {
                href: "https://plato.stanford.edu/entries/hedonism/".to_owned(),
                internal: false,
                children: vec![text("hedonism")],
            },
            text("."),
        ],
        align: None,
        indent_em: 0,
    });
    let (source, compiled) = render(&[&doc], &style(&["links.mode=endnote"]));
    assert!(source.contains("#heading(level: 2)[Notes]"), "{source}");
    assert!(
        source.contains("#fs-moved-notes(0, title: [Long notes])"),
        "{source}"
    );
    assert_eq!(compiled.moved_notes.len(), 1);
}

#[test]
fn notes_set_as_endnotes_never_reach_the_policy() {
    let doc = entry("pleasure", &[(1, 1500)]);
    let (source, compiled) = render(&[&doc], &style(&["notes.style=endnote"]));
    assert!(!source.contains("#fs-note("), "{source}");
    assert!(!source.contains("#fs-moved-notes("), "{source}");
    assert!(compiled.moved_notes.is_empty());
}

/// Text that is markup to Typst, which a note the document holds must keep as text.
const HOSTILE: &str = "a *b* #c $d$ <e> @f [g] ] [ // h /* i */ C:\\dir\\ \\\nj = k";

/// An entry that holds its notes itself, as a wiki page does: two the text refers to (the
/// tenth before the second, which their ids would sort the other way round) and one it
/// never refers to.
fn holding(notes: &[(&str, &str)]) -> Doc {
    let refer = |id: &str, label: &str| {
        Inline::NoteRef(NoteRef {
            id: id.to_owned(),
            label: label.to_owned(),
            href: format!("#{id}"),
            ref_id: Some(format!("cite_ref-{label}")),
        })
    };
    let mut doc = entry("wiki", &[]);
    doc.attachments.clear();
    doc.body = vec![Block::Para {
        children: vec![
            text("First"),
            refer("cite_note-10", "1"),
            text(" and second"),
            refer("cite_note-2", "2"),
            text("."),
        ],
        align: None,
        indent_em: 0,
    }];
    doc.notes = notes
        .iter()
        .map(|(id, value)| ((*id).to_owned(), vec![para(value)]))
        .collect();
    doc
}

fn own_notes() -> Doc {
    holding(&[
        ("cite_note-10", HOSTILE),
        ("cite_note-2", "The second note."),
        ("cite_note-9", "A note nothing refers to."),
    ])
}

/// The Typst source and the text the document typesets to, with `sections` selected.
fn typeset(doc: &Doc, style: &Style, sections: &[&str]) -> (String, String) {
    let fonts = Fonts::embedded();
    let sections = sections.iter().map(|s| (*s).to_owned()).collect();
    let rendered = emit_document(
        &[doc],
        style,
        EmitOptions {
            fonts: Some(&fonts),
            sections: &sections,
            ..Default::default()
        },
    );
    let world = SepWorld::new(rendered.source.clone(), &rendered.figures, &fonts);
    let document = typst::compile::<typst_layout::PagedDocument>(&world)
        .output
        .unwrap_or_else(|errors| panic!("{errors:?}\n{}", rendered.source));
    fn walk(frame: &typst::layout::Frame, out: &mut String) {
        for (_, item) in frame.items() {
            match item {
                typst::layout::FrameItem::Group(group) => walk(&group.frame, out),
                typst::layout::FrameItem::Text(run) => out.push_str(run.text.as_str()),
                _ => {}
            }
        }
    }
    let mut out = String::new();
    for page in document.pages() {
        walk(&page.frame, &mut out);
    }
    (rendered.source, out)
}

fn squeeze(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{ad}')
        .collect()
}

#[test]
fn notes_the_document_holds_are_set_as_footnotes_as_attached_ones_are() {
    let (source, text) = typeset(&own_notes(), &style(&[]), &["notes"]);
    assert!(source.contains("#fs-note(0, 1)"), "{source}");
    assert!(source.contains("#fs-note(0, 2)"), "{source}");
    assert!(
        source.contains("#fs-moved-notes(0, title: [Notes])"),
        "{source}"
    );
    assert!(squeeze(&text).contains(&squeeze(HOSTILE)), "{text}");
    assert!(!text.contains("A note nothing refers to"), "{text}");

    let (source, _) = typeset(&own_notes(), &style(&["notes.maxHeight=0"]), &["notes"]);
    assert!(source.contains("#footnote["), "{source}");
}

#[test]
fn notes_the_document_holds_are_set_as_endnotes_as_attached_ones_are() {
    let (source, text) = typeset(&own_notes(), &style(&["notes.style=endnote"]), &["notes"]);
    assert!(source.contains("#link(<endnote-1>)[#super[1]]"), "{source}");
    assert!(source.contains("#fs-endnote([1], ["), "{source}");
    assert!(
        source.contains("#fs-endnote([2], [The second note.])"),
        "{source}"
    );
    assert!(squeeze(&text).contains(&squeeze(HOSTILE)), "{text}");
}

#[test]
fn notes_the_document_holds_are_an_appendix_in_the_order_the_text_refers_to_them() {
    let (source, text) = typeset(&own_notes(), &style(&["notes.style=appendix"]), &["notes"]);
    assert!(source.contains("#heading(level: 2)[Notes]"), "{source}");
    assert!(
        source.contains("#super[\\[#link(<cite_note-10>)[1]\\]]"),
        "{source}"
    );
    assert!(
        source.contains("#super[\\[#link(<cite_note-2>)[2]\\]]"),
        "{source}"
    );
    let at = |needle: &str| {
        source
            .find(needle)
            .unwrap_or_else(|| panic!("{needle}: {source}"))
    };
    let tenth = at("#metadata(none)<cite_note-10>\n#fs-endnote([1], [");
    let second = at("#metadata(none)<cite_note-2>\n#fs-endnote([2], [The second note.])");
    let never = at("#metadata(none)<cite_note-9>\n#fs-endnote([], [A note nothing refers to.])");
    assert!(tenth < second && second < never, "{source}");
    assert!(squeeze(&text).contains(&squeeze(HOSTILE)), "{text}");

    let (source, _) = typeset(&own_notes(), &style(&["notes.style=appendix"]), &[]);
    assert!(!source.contains("[Notes]"), "{source}");
    assert!(
        source.contains("#super[\\[1\\]]"),
        "with the notes left out, a reference is its label: {source}"
    );
}

#[test]
fn an_attached_note_is_the_one_a_reference_names_when_both_have_its_id() {
    let mut doc = entry("pleasure", &[(1, 3)]);
    doc.notes.insert(
        "note-1".to_owned(),
        vec![para("The document's own, never used.")],
    );
    let (source, _) = typeset(&doc, &style(&[]), &["notes"]);
    assert!(source.contains("pleasure and pain"), "{source}");
    assert!(!source.contains("never used"), "{source}");
}
