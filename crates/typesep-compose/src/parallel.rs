//! The bilingual edition: two texts whose remarks carry the same ids, as the Wittgenstein
//! Project's translations carry those of the German text, set beside each other remark by
//! remark.

use std::collections::{HashMap, HashSet, VecDeque};

use typesep_model::{
    Attachment, Block, Doc, EntryLinks, Inline, Meta, NoteTable, Part, TocItem, scoped_id,
};

/// What each text's ids are put under, the left one's first, so that the ids of the two
/// texts, which are mostly the same, stay apart in the one document.
pub const SIDES: [&str; 2] = ["left", "right"];

/// The document the two texts make, and what is to be said about how they were put together.
#[derive(Debug, Clone, PartialEq)]
pub struct Parallel {
    pub doc: Doc,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParallelError {
    #[error(
        "{title} has no numbered remarks, so there is nothing to set beside the other text: \
         --parallel puts two texts side by side by the ids of their remarks"
    )]
    NoRemarks { title: String },
}

/// The bilingual edition of `left` and `right` under `meta`, which the caller makes of the two
/// (the source's volume, each work's rights kept): one row to a remark of the left text, the
/// remark of the right text with the same id beside it, and each row's two columns in their
/// text's language and direction. What comes before the first remark of either text is a row
/// of its own, first; the blocks after a remark go with it; a remark only one text has is set
/// alone, where it stands in its text, which is said once for all of them. A text without
/// remarks cannot be aligned and is refused.
///
/// The document is made of the two texts, each with its ids under its side: its contents are
/// a line for each text that has any, the text's title, holding the text's own; and a link to
/// either text's address with a fragment goes to the fragment's id in that text, as it does
/// when the text is typeset alone.
pub fn parallel(left: &Doc, right: &Doc, mut meta: Meta) -> Result<Parallel, ParallelError> {
    let originals = [left, right];
    let mut cuts: Vec<Cut> = Vec::new();
    for (doc, side) in originals.into_iter().zip(SIDES) {
        let mut cut = cut(doc);
        if cut.units.is_empty() {
            return Err(ParallelError::NoRemarks {
                title: doc.meta.title.clone(),
            });
        }
        scope_blocks(&mut cut.front, side);
        for unit in &mut cut.units {
            scope_blocks(&mut unit.blocks, side);
        }
        cuts.push(cut);
    }

    let row = |blocks: [Vec<Block>; 2]| Block::Columns {
        columns: blocks
            .into_iter()
            .zip(originals)
            .map(|(blocks, doc)| in_language(doc, blocks))
            .collect(),
    };
    let mut body = Vec::new();
    let fronts = [
        std::mem::take(&mut cuts[0].front),
        std::mem::take(&mut cuts[1].front),
    ];
    if fronts.iter().any(|front| !front.is_empty()) {
        body.push(row(fronts));
    }
    let mut alone: [Vec<String>; 2] = [Vec::new(), Vec::new()];
    for (l, r) in pair(&cuts[0].units, &cuts[1].units) {
        let mut take = |side: usize, at: Option<usize>| {
            let unit = &mut cuts[side].units[at?];
            if [l, r][1 - side].is_none() {
                alone[side].push(shown_number(&unit.number));
            }
            Some(std::mem::take(&mut unit.blocks))
        };
        let blocks = [
            take(0, l).unwrap_or_default(),
            take(1, r).unwrap_or_default(),
        ];
        body.push(row(blocks));
    }

    let mut warnings = Vec::new();
    let count = alone[0].len() + alone[1].len();
    if count > 0 {
        let lists: Vec<String> = alone
            .iter()
            .zip(originals)
            .filter(|(numbers, _)| !numbers.is_empty())
            .map(|(numbers, doc)| format!("in {} {}", doc.meta.title, numbers.join(", ")))
            .collect();
        warnings.push(format!(
            "{count} remark{} {} in one text only and {} set with nothing beside {}: {}",
            if count == 1 { "" } else { "s" },
            if count == 1 { "is" } else { "are" },
            if count == 1 { "is" } else { "are" },
            if count == 1 { "it" } else { "them" },
            lists.join("; ")
        ));
    }

    meta.origin.parts = originals
        .iter()
        .zip(SIDES)
        .map(|(doc, side)| Part {
            url: doc.meta.origin.url.clone(),
            scope: side.to_owned(),
        })
        .collect();
    let [left, right] = [scoped_rest(left, SIDES[0]), scoped_rest(right, SIDES[1])];
    let toc = [&left, &right]
        .into_iter()
        .filter(|text| !text.toc.is_empty())
        .map(|text| TocItem {
            label: vec![Inline::Text {
                value: text.meta.title.clone(),
            }],
            href: String::new(),
            children: text.toc.clone(),
        })
        .collect();
    let mut notes = left.notes.clone();
    notes.extend(right.notes.clone());
    let doc = Doc {
        meta,
        front: Vec::new(),
        toc,
        body,
        back: [left.back.clone(), right.back.clone()].concat(),
        notes,
        links: EntryLinks {
            notes: [left.links.notes.clone(), right.links.notes.clone()].concat(),
            supplements: [
                left.links.supplements.clone(),
                right.links.supplements.clone(),
            ]
            .concat(),
        },
        attachments: [left.attachments.clone(), right.attachments.clone()].concat(),
        warnings: [
            left.warnings.clone(),
            right.warnings.clone(),
            warnings.clone(),
        ]
        .concat(),
    };
    Ok(Parallel { doc, warnings })
}

/// How the warning names a remark: by its number.
fn shown_number(number: &str) -> String {
    if number.is_empty() {
        "(one without a number)".to_owned()
    } else {
        number.to_owned()
    }
}

/// A text cut into what is set beside the other's: what comes before its first remark, then
/// each remark with the blocks that follow it up to the next.
struct Cut {
    front: Vec<Block>,
    units: Vec<Unit>,
}

struct Unit {
    /// What the other text's remark is found by: the remark's id, or its number when it has
    /// none.
    key: Option<String>,
    number: String,
    blocks: Vec<Block>,
}

fn cut(doc: &Doc) -> Cut {
    let mut front = doc.front.clone();
    let mut units: Vec<Unit> = Vec::new();
    for block in &doc.body {
        match block {
            Block::Remark { number, id, .. } => {
                let key = id
                    .as_deref()
                    .filter(|id| !id.is_empty())
                    .or_else(|| (!number.is_empty()).then_some(number.as_str()))
                    .map(str::to_owned);
                units.push(Unit {
                    key,
                    number: number.clone(),
                    blocks: vec![block.clone()],
                });
            }
            other => match units.last_mut() {
                Some(unit) => unit.blocks.push(other.clone()),
                None => front.push(other.clone()),
            },
        }
    }
    Cut { front, units }
}

/// The rows, as the unit of each text they hold: the left text's units in its order, each
/// with the right text's unit of the same key, and the right text's units that no left one
/// has, each after the row of the unit before it in its text.
fn pair(left: &[Unit], right: &[Unit]) -> Vec<(Option<usize>, Option<usize>)> {
    let mut by_key: HashMap<&str, VecDeque<usize>> = HashMap::new();
    for (at, unit) in right.iter().enumerate() {
        if let Some(key) = &unit.key {
            by_key.entry(key).or_default().push_back(at);
        }
    }
    let partners: Vec<Option<usize>> = left
        .iter()
        .map(|unit| {
            unit.key
                .as_deref()
                .and_then(|key| by_key.get_mut(key)?.pop_front())
        })
        .collect();
    let matched: HashSet<usize> = partners.iter().flatten().copied().collect();
    let mut rows = Vec::new();
    let mut next = 0;
    for (l, partner) in partners.iter().enumerate() {
        let Some(r) = *partner else {
            rows.push((Some(l), None));
            continue;
        };
        while next < r {
            if !matched.contains(&next) {
                rows.push((None, Some(next)));
            }
            next += 1;
        }
        next = next.max(r + 1);
        rows.push((Some(l), Some(r)));
    }
    rows.extend(
        (next..right.len())
            .filter(|r| !matched.contains(r))
            .map(|r| (None, Some(r))),
    );
    rows
}

/// `blocks` of `doc` as one column: in its language and direction, in a scope of their own.
fn in_language(doc: &Doc, blocks: Vec<Block>) -> Vec<Block> {
    if blocks.is_empty() {
        return blocks;
    }
    vec![Block::Lang {
        lang: doc.meta.lang.clone(),
        dir: Some(doc.meta.dir),
        children: blocks,
    }]
}

fn scoped(side: &str, id: &str) -> String {
    scoped_id(side, id)
}

/// An address within the document, `#` and an id, put under `side`; any other is left as it
/// is.
fn scoped_href(side: &str, href: &str) -> String {
    match href.strip_prefix('#') {
        Some(id) if !id.is_empty() => format!("#{}", scoped(side, id)),
        _ => href.to_owned(),
    }
}

/// What `doc` has besides its text, with every id and every reference to one put under
/// `side`, as its text is: its notes and the notes of its attachments, its back matter and its
/// contents. Its front and body are left out, being set row by row.
fn scoped_rest(doc: &Doc, side: &str) -> Doc {
    let mut doc = Doc {
        front: Vec::new(),
        body: Vec::new(),
        ..doc.clone()
    };
    for section in &mut doc.back {
        scope_blocks(&mut section.blocks, side);
    }
    doc.notes = scope_notes(std::mem::take(&mut doc.notes), side);
    for attachment in &mut doc.attachments {
        scope_attachment(attachment, side);
    }
    scope_toc(&mut doc.toc, side);
    doc
}

fn scope_attachment(attachment: &mut Attachment, side: &str) {
    scope_blocks(&mut attachment.blocks, side);
    if let Some(notes) = attachment.notes.take() {
        attachment.notes = Some(scope_notes(notes, side));
    }
}

fn scope_notes(notes: NoteTable, side: &str) -> NoteTable {
    notes
        .into_iter()
        .map(|(id, mut blocks)| {
            scope_blocks(&mut blocks, side);
            (scoped(side, &id), blocks)
        })
        .collect()
}

fn scope_toc(items: &mut [TocItem], side: &str) {
    for item in items {
        item.href = scoped_href(side, &item.href);
        scope_toc(&mut item.children, side);
    }
}

fn scope_id(id: &mut Option<String>, side: &str) {
    if let Some(id) = id {
        *id = scoped(side, id);
    }
}

fn scope_blocks(blocks: &mut [Block], side: &str) {
    for block in blocks {
        match block {
            Block::Para { children, .. } | Block::Pre { children } => scope_inlines(children, side),
            Block::Heading { id, children, .. } => {
                scope_id(id, side);
                scope_inlines(children, side);
            }
            Block::List(list) => {
                for item in &mut list.items {
                    scope_blocks(item, side);
                }
                for label in list.labels.iter_mut().flatten() {
                    scope_inlines(label, side);
                }
            }
            Block::Blockquote { children }
            | Block::Aside { children }
            | Block::Lang { children, .. } => scope_blocks(children, side),
            Block::Remark { id, children, .. } => {
                scope_id(id, side);
                scope_blocks(children, side);
            }
            Block::Columns { columns } => {
                for column in columns {
                    scope_blocks(column, side);
                }
            }
            Block::Verse { lines } => {
                for line in lines {
                    scope_inlines(&mut line.children, side);
                }
            }
            Block::Table(table) => {
                for caption in table.caption.iter_mut() {
                    scope_inlines(caption, side);
                }
                for cell in table.rows.iter_mut().flatten() {
                    scope_inlines(&mut cell.children, side);
                }
            }
            Block::Figure(figure) => scope_figure(figure, side),
            Block::Anchor { id } => *id = scoped(side, id),
            Block::Rule => {}
        }
    }
}

fn scope_figure(figure: &mut typesep_model::Figure, side: &str) {
    if let Some(id) = &mut figure.id {
        scope_id(id, side);
    }
    for caption in figure.caption.iter_mut() {
        scope_inlines(caption, side);
    }
}

fn scope_inlines(nodes: &mut [Inline], side: &str) {
    for node in nodes {
        match node {
            Inline::Anchor { id } => *id = scoped(side, id),
            Inline::Link {
                href,
                internal,
                children,
            } => {
                *href = if *internal {
                    scoped(side, href)
                } else {
                    scoped_href(side, href)
                };
                scope_inlines(children, side);
            }
            Inline::NoteRef(note) => {
                note.id = scoped(side, &note.id);
                note.href = scoped_href(side, &note.href);
                scope_id(&mut note.ref_id, side);
            }
            Inline::Figure(figure) => scope_figure(figure, side),
            Inline::Emph { children }
            | Inline::Strong { children }
            | Inline::Sub { children }
            | Inline::Sup { children }
            | Inline::Code { children }
            | Inline::Spaced { children }
            | Inline::SmallCaps { children }
            | Inline::Decorated { children, .. }
            | Inline::MarginNote { children } => scope_inlines(children, side),
            Inline::Text { .. } | Inline::Math { .. } | Inline::Break => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use typesep_model::{Dir, NoteRef};

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

    fn remark(number: &str, value: &str) -> Block {
        Block::Remark {
            number: number.to_owned(),
            id: Some(number.trim_end_matches('.').to_owned()),
            children: vec![para(value)],
        }
    }

    fn work(title: &str, lang: &str, body: Vec<Block>) -> Doc {
        Doc {
            meta: Meta {
                id: title.to_owned(),
                title: title.to_owned(),
                lang: lang.to_owned(),
                ..Meta::default()
            },
            body,
            ..Doc::default()
        }
    }

    /// Each row as the text of its two columns, each column's language first.
    fn rows(doc: &Doc) -> Vec<[String; 2]> {
        fn plain(blocks: &[Block]) -> String {
            let mut out = Vec::new();
            for block in blocks {
                match block {
                    Block::Lang { lang, children, .. } => {
                        out.push(format!("{lang}:"));
                        out.push(plain(children));
                    }
                    Block::Remark {
                        number, children, ..
                    } => {
                        out.push(number.clone());
                        out.push(plain(children));
                    }
                    Block::Para { children, .. } => {
                        for child in children {
                            if let Inline::Text { value } = child {
                                out.push(value.clone());
                            }
                        }
                    }
                    Block::Heading { .. } => out.push("(heading)".to_owned()),
                    Block::Rule => out.push("(rule)".to_owned()),
                    _ => out.push("(block)".to_owned()),
                }
            }
            out.join(" ")
        }
        doc.body
            .iter()
            .map(|row| match row {
                Block::Columns { columns } => {
                    assert_eq!(columns.len(), 2, "two columns to a row");
                    [plain(&columns[0]), plain(&columns[1])]
                }
                other => panic!("a row is columns: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn remarks_with_the_same_id_are_set_in_one_row_with_what_follows_them() {
        let german = work(
            "Abhandlung",
            "de",
            vec![
                Block::Heading {
                    level: 2,
                    id: Some("Vorwort".into()),
                    children: vec![text("Vorwort")],
                    outline_only: false,
                },
                para("Ein Vorwort."),
                remark("1", "Eins."),
                remark("1.1", "Eins eins."),
                Block::Rule,
                para("Dazwischen."),
                remark("2", "Zwei."),
            ],
        );
        let english = work(
            "Treatise",
            "en",
            vec![
                para("A preface."),
                remark("1", "One."),
                remark("1.1", "One one."),
                remark("2", "Two."),
            ],
        );
        let made = parallel(&german, &english, Meta::default()).unwrap();
        assert_eq!(made.warnings, Vec::<String>::new());
        assert_eq!(
            rows(&made.doc),
            [
                ["de: (heading) Ein Vorwort.", "en: A preface."],
                ["de: 1 Eins.", "en: 1 One."],
                ["de: 1.1 Eins eins. (rule) Dazwischen.", "en: 1.1 One one."],
                ["de: 2 Zwei.", "en: 2 Two."],
            ]
        );
        let first = serde_json::to_value(&made.doc.body[1]).unwrap();
        assert_eq!(
            first["columns"][0][0],
            json!({ "type": "lang", "lang": "de", "dir": "ltr", "children": [
                { "type": "remark", "number": "1", "id": "left--1", "children": [
                    { "type": "para", "children": [{ "type": "text", "value": "Eins." }] }
                ] }
            ] }),
            "each column is in its text's language, its ids under its side"
        );
        assert_eq!(
            first["columns"][1][0]["children"][0]["id"],
            json!("right--1")
        );
    }

    #[test]
    fn a_remark_one_text_has_alone_is_set_alone_where_it_stands_and_said() {
        let left = work(
            "Left",
            "de",
            vec![
                remark("1", "a"),
                remark("2", "b"),
                remark("3", "c"),
                remark("4", "d"),
            ],
        );
        let right = work(
            "Right",
            "en",
            vec![
                remark("1", "A"),
                remark("2.5", "B"),
                remark("3", "C"),
                remark("5", "E"),
            ],
        );
        let made = parallel(&left, &right, Meta::default()).unwrap();
        assert_eq!(
            rows(&made.doc),
            [
                ["de: 1 a", "en: 1 A"],
                ["de: 2 b", ""],
                ["", "en: 2.5 B"],
                ["de: 3 c", "en: 3 C"],
                ["de: 4 d", ""],
                ["", "en: 5 E"],
            ]
        );
        assert_eq!(
            made.warnings,
            [
                "4 remarks are in one text only and are set with nothing beside them: in Left 2, 4; in Right 2.5, 5"
            ]
        );
        assert_eq!(
            made.doc.warnings, made.warnings,
            "the document keeps them too"
        );
        let one = parallel(
            &work("L", "de", vec![remark("1", "a"), remark("2", "b")]),
            &work("R", "en", vec![remark("1", "A")]),
            Meta::default(),
        )
        .unwrap();
        assert_eq!(
            one.warnings,
            ["1 remark is in one text only and is set with nothing beside it: in L 2"]
        );
    }

    #[test]
    fn a_text_without_remarks_is_refused() {
        let plain = work("Prose", "en", vec![para("No numbers here.")]);
        let numbered = work("Numbered", "de", vec![remark("1", "a")]);
        assert_eq!(
            parallel(&numbered, &plain, Meta::default()).unwrap_err(),
            ParallelError::NoRemarks {
                title: "Prose".into()
            }
        );
        assert_eq!(
            parallel(&plain, &numbered, Meta::default())
                .unwrap_err()
                .to_string(),
            "Prose has no numbered remarks, so there is nothing to set beside the other text: \
             --parallel puts two texts side by side by the ids of their remarks"
        );
    }

    #[test]
    fn the_notes_of_both_texts_are_kept_apart_and_each_reference_finds_its_own() {
        let note = |label: &str| {
            Inline::NoteRef(NoteRef {
                id: "cite_note-1".into(),
                label: label.into(),
                href: "#cite_note-1".into(),
                ref_id: Some("cite_ref-1".into()),
            })
        };
        let with_note = |title: &str, lang: &str, words: &str| {
            let mut doc = work(
                title,
                lang,
                vec![Block::Remark {
                    number: "1".into(),
                    id: Some("1".into()),
                    children: vec![Block::Para {
                        children: vec![
                            text(words),
                            note("1"),
                            Inline::Link {
                                href: "1".into(),
                                internal: true,
                                children: vec![text("self")],
                            },
                        ],
                        align: None,
                        indent_em: 0,
                    }],
                }],
            );
            doc.notes.insert("cite_note-1".into(), vec![para(words)]);
            doc.toc.push(TocItem {
                label: vec![text(words)],
                href: "#1".into(),
                children: Vec::new(),
            });
            doc
        };
        let made = parallel(
            &with_note("L", "de", "Anmerkung"),
            &with_note("R", "ar", "ملاحظة"),
            Meta::default(),
        )
        .unwrap();
        let notes: Vec<&str> = made.doc.notes.keys().map(String::as_str).collect();
        assert_eq!(notes, ["left--cite_note-1", "right--cite_note-1"]);
        let row = serde_json::to_value(&made.doc.body[0]).unwrap();
        for (column, side) in [(0, "left"), (1, "right")] {
            let para = &row["columns"][column][0]["children"][0]["children"][0]["children"];
            assert_eq!(
                para[1],
                json!({ "type": "noteref", "id": format!("{side}--cite_note-1"), "label": "1",
                        "href": format!("#{side}--cite_note-1"), "refId": format!("{side}--cite_ref-1") })
            );
            assert_eq!(para[2]["href"], json!(format!("{side}--1")));
        }
        assert_eq!(
            made.doc
                .toc
                .iter()
                .flat_map(|text| &text.children)
                .map(|item| item.href.as_str())
                .collect::<Vec<_>>(),
            ["#left--1", "#right--1"],
            "each text's contents go to its own ids"
        );
        assert_eq!(row["columns"][1][0]["lang"], json!("ar"));
        assert_eq!(row["columns"][1][0]["dir"], Value::from("ltr"));

        let mut rtl = with_note("R", "ar", "ملاحظة");
        rtl.meta.dir = Dir::Rtl;
        let made = parallel(&with_note("L", "de", "Anmerkung"), &rtl, Meta::default()).unwrap();
        let row = serde_json::to_value(&made.doc.body[0]).unwrap();
        assert_eq!(row["columns"][1][0]["dir"], json!("rtl"));
    }

    #[test]
    fn the_contents_are_each_texts_under_its_title_and_each_text_keeps_its_address() {
        let heading = |id: &str, words: &str| Block::Heading {
            level: 2,
            id: Some(id.into()),
            children: vec![text(words)],
            outline_only: false,
        };
        let line = |href: &str, words: &str| TocItem {
            label: vec![text(words)],
            href: href.into(),
            children: Vec::new(),
        };
        let mut german = work(
            "Abhandlung",
            "de",
            vec![heading("Vorwort", "Vorwort"), remark("1", "Eins.")],
        );
        german.meta.origin.url = "https://example.org/w/index.php/Abhandlung".into();
        german.toc = vec![line("#Vorwort", "Vorwort")];
        let mut english = work(
            "Treatise",
            "en",
            vec![
                heading("Preface", "Preface"),
                remark("1", "One."),
                heading("Notes", "Notes"),
            ],
        );
        english.meta.origin.url = "https://example.org/w/index.php/Treatise".into();
        english.toc = vec![TocItem {
            children: vec![line("#Notes", "Notes")],
            ..line("#Preface", "Preface")
        }];

        let made = parallel(&german, &english, Meta::default()).unwrap();
        assert_eq!(
            serde_json::to_value(&made.doc.toc).unwrap(),
            json!([
                { "label": [{ "type": "text", "value": "Abhandlung" }], "href": "", "children": [
                    { "label": [{ "type": "text", "value": "Vorwort" }], "href": "#left--Vorwort", "children": [] }
                ] },
                { "label": [{ "type": "text", "value": "Treatise" }], "href": "", "children": [
                    { "label": [{ "type": "text", "value": "Preface" }], "href": "#right--Preface", "children": [
                        { "label": [{ "type": "text", "value": "Notes" }], "href": "#right--Notes", "children": [] }
                    ] }
                ] }
            ]),
            "a line for each text, its title, holding its own lines"
        );
        assert_eq!(
            made.doc.meta.origin.parts,
            [
                Part {
                    url: "https://example.org/w/index.php/Abhandlung".into(),
                    scope: "left".into(),
                },
                Part {
                    url: "https://example.org/w/index.php/Treatise".into(),
                    scope: "right".into(),
                },
            ]
        );
        assert_eq!(made.doc.contents_above_headings(), 1);

        german.toc.clear();
        let made = parallel(&german, &english, Meta::default()).unwrap();
        let titles: Vec<String> = made.doc.toc.iter().map(TocItem::text).collect();
        assert_eq!(titles, ["Treatise"], "a text with no contents has no line");
    }
}
