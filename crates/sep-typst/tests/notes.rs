use std::collections::BTreeMap;

use sep_core::model::{Attachment, AttachmentKind, Block, Doc, Inline, Meta, NoteRef, NoteTable};
use sep_core::style::{ResolveOptions, Style, resolve_style};
use sep_typst::{Compiled, EmitOptions, Fonts, SepWorld, compile_pdf, emit_document};

fn style(sets: &[&str]) -> Style {
    resolve_style(ResolveOptions {
        preset: Some("halfletter"),
        file: None,
        sets: sets.iter().map(|s| (*s).into()).collect(),
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
        });
    }
    Doc {
        meta: Meta {
            slug: slug.to_owned(),
            title: slug.to_owned(),
            source_url: format!("https://plato.stanford.edu/entries/{slug}/"),
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
    let rendered = emit_document(
        docs,
        style,
        EmitOptions {
            fonts: Some(&fonts),
            volume_title: (docs.len() > 1).then_some("A volume"),
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
