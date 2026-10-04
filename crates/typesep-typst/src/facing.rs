//! The facing pages of a bilingual edition: the first text on the left-hand pages, the second
//! on the right-hand ones, each spread holding the rows of remarks that both its pages have
//! room for, so that it starts at the same remark on either side.
//!
//! Typst lays out one flow of pages and cannot run two flows side by side, so the spreads are
//! made here, in three steps, with the compiler the PDF is made with:
//!
//! 1. Measure. Each side of each row is laid out on a page of its own, with a mark at its top,
//!    between its pieces (its blocks, and the blocks of a remark) and after the last; each
//!    footnote marks where it is set and where it is called from. Typst says where each mark
//!    landed, which gives each side's height, the height of its notes, and whether it fits on
//!    a page at all.
//! 2. Plan. Rows go on a spread as long as both of its pages have room for them, counting the
//!    space between rows and the footnote area as Typst sets them. A row longer than a page on
//!    either side is set on spreads of its own, each side cut where its pieces fill a page and
//!    continued on the same side of the next spread.
//! 3. Check. The spreads are typeset, a page break before each side, and every side whose end
//!    is not on the page it starts on, or whose footnote is not on its page, has its last row
//!    (or piece) moved on to the next spread, until none does. A piece longer than a page on
//!    its own cannot be moved and runs on to the next page, which is said.
//!
//! Every step is a compile of the same kind as the PDF's, so the result is as deterministic as
//! Typst is.

use std::collections::{BTreeMap, BTreeSet};

use typesep_model::{Block, Doc};
use typesep_style::{Style, View};
use typst::foundations::{Dict, Label, Selector, Value};
use typst::introspection::Introspector;
use typst::utils::PicoStr;
use typst_layout::PagedDocument;

use crate::emit::{
    EmitOptions, FOOTNOTE_AT, Layout, MEASURE_NOTES, MEASURE_PIECE, MEASURE_TOP, Rendered,
    SPREAD_END, SPREAD_START, emit_with, side_of,
};
use crate::fonts::Fonts;
use crate::style::TEX_PT_PER_TYPST_PT;
use crate::world::TypesetWorld;

/// How many times the spreads are typeset and checked before what is left is said.
const ROUNDS: usize = 8;

/// The spreads of a bilingual edition: on each side, the parts of rows it holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub spreads: Vec<Spread>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spread {
    pub sides: [Vec<Part>; 2],
    /// What the spread was made from, which a check that fails changes.
    origin: Origin,
}

/// Pieces `from..to` of a side of row `row`.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub row: usize,
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Origin {
    /// Whole rows that fit on both pages.
    Rows(Vec<usize>),
    /// The `cut`th page of a row longer than a page.
    Long { row: usize, cut: usize },
}

/// Where a mark landed: its page, from 1, and how far down it, in Typst points.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
struct At {
    page: usize,
    y: f64,
}

/// What measuring found of a side of a row.
#[derive(Debug, Clone, Default)]
struct Measure {
    /// Whether all of it and its notes are on the page it starts on.
    fits: bool,
    /// Its height from the top of its page to the end of its last piece.
    height: f64,
    /// The height its footnotes take, each with the gap above it.
    notes: f64,
    /// The height of each piece with the space after it, the first with what is above it,
    /// and of the notes called from it.
    pieces: Vec<(f64, f64)>,
}

/// The spacing of the footnote area: the clearance and separator above the first note, and
/// the gap above each.
#[derive(Debug, Clone, Copy)]
struct NoteArea {
    above: f64,
    gap: f64,
}

/// What the rest of the plan needs of the page and the style.
struct Page {
    /// The height of the text area.
    height: f64,
    /// The space between two rows on one page: a remark's.
    between: f64,
    notes: NoteArea,
}

/// What a check of the spreads asks the next plan to do differently.
#[derive(Debug, Default)]
struct Constraints {
    /// Rows that start a spread.
    breaks: BTreeSet<usize>,
    /// Rows set on spreads of their own, cut into pages.
    long: BTreeSet<usize>,
    /// Pieces a side of a long row is cut before, beyond those its measure says.
    cuts: BTreeMap<(usize, usize), BTreeSet<usize>>,
    /// Pages of long rows whose single piece is longer than a page, which are left to run on.
    over: BTreeSet<(usize, usize, usize)>,
}

fn label(name: &str) -> Selector {
    Selector::Label(Label::new(PicoStr::intern(name)).expect("a label the emitter names"))
}

fn int(dict: &Dict, key: &str) -> Option<usize> {
    match dict.get(key) {
        Ok(Value::Int(n)) => usize::try_from(*n).ok(),
        _ => None,
    }
}

fn length(dict: &Dict, key: &str) -> Option<f64> {
    match dict.get(key) {
        Ok(Value::Length(length)) => Some(length.abs.to_pt()),
        _ => None,
    }
}

/// The marks labelled `name`: each one's value and where it landed.
fn marks(document: &PagedDocument, name: &str) -> Vec<(Value, At)> {
    let introspector = document.introspector();
    introspector
        .query(&label(name))
        .iter()
        .filter_map(|mark| {
            let value = mark.get_by_name("value").ok()?;
            let position = introspector.position(mark.location()?)?;
            Some((
                value,
                At {
                    page: position.page.get(),
                    y: position.point.y.to_pt(),
                },
            ))
        })
        .collect()
}

/// Where each footnote is set, with where it is called from.
fn footnotes(document: &PagedDocument) -> Vec<(At, At)> {
    marks(document, FOOTNOTE_AT)
        .into_iter()
        .filter_map(|(value, at)| {
            let Value::Dict(call) = value else {
                return None;
            };
            let call = At {
                page: int(&call, "page")?,
                y: length(&call, "y")?,
            };
            Some((at, call))
        })
        .collect()
}

fn compile(rendered: &Rendered, fonts: &Fonts) -> Option<PagedDocument> {
    let world = TypesetWorld::new(rendered.source.clone(), &rendered.figures, fonts);
    typst::compile::<PagedDocument>(&world).output.ok()
}

/// The facing pages of `doc`, a bilingual edition, as Typst source: measured, planned and
/// checked as the module says. Should the measuring document not compile, the rows are set a
/// spread each, unmeasured, and whatever stopped it stops the PDF too, to be said there.
pub(crate) fn emit(doc: &Doc, style: &Style, opts: EmitOptions<'_>, fonts: &Fonts) -> Rendered {
    let rows = &doc.body;
    let view = View::new(style);
    let measured = emit_with(&[doc], style, opts, &Layout::Measure);
    let Some(document) = compile(&measured, fonts) else {
        let plan = Plan {
            spreads: (0..rows.len())
                .map(|row| whole_rows(rows, &[row]))
                .collect(),
        };
        return emit_with(&[doc], style, opts, &Layout::Facing(plan));
    };
    let page = page_of(&view, &document);
    let measures = measure(&document, rows, &page);

    let mut constraints = Constraints::default();
    let mut rendered = None;
    let mut left = Vec::new();
    for _ in 0..ROUNDS {
        let plan = plan(rows, &measures, &constraints, &page);
        let attempt = emit_with(&[doc], style, opts, &Layout::Facing(plan.clone()));
        let Some(document) = compile(&attempt, fonts) else {
            rendered = Some(attempt);
            break;
        };
        let bad = check(&document, &plan);
        let mut changed = false;
        for &spread in &bad {
            changed |= constrain(&plan.spreads[spread], &mut constraints);
        }
        // What runs on because a single piece is longer than a page is said on its own.
        left = bad
            .into_iter()
            .filter(|&spread| match plan.spreads[spread].origin {
                Origin::Long { row, cut } => {
                    !(0..2).any(|side| constraints.over.contains(&(row, side, cut)))
                }
                Origin::Rows(_) => true,
            })
            .collect();
        rendered = Some(attempt);
        if !changed {
            break;
        }
    }
    let mut rendered = rendered.expect("at least one round");
    rendered
        .warnings
        .extend(said(rows, &measures, &constraints, &left));
    rendered
}

/// The height of the text area, the space between rows, and the footnote area's spacing, as
/// the measuring document says it or the style.
fn page_of(view: &View<'_>, document: &PagedDocument) -> Page {
    let typst = |tex: f64| tex / TEX_PT_PER_TYPST_PT;
    let leading = view.size_pt("typography.leading") - view.body();
    let between =
        typst(leading + view.pt("typography.parSkip") + view.pt("blocks.remark.spaceBefore"));
    let notes = marks(document, MEASURE_NOTES)
        .into_iter()
        .find_map(|(value, _)| {
            let Value::Dict(dict) = value else {
                return None;
            };
            Some(NoteArea {
                above: length(&dict, "clearance")? + length(&dict, "separator")?,
                gap: length(&dict, "gap")?,
            })
        })
        .unwrap_or(NoteArea {
            above: typst(view.body() * 1.1),
            gap: typst(view.body() * 0.5),
        });
    Page {
        height: typst(view.text_height_pt()),
        between,
        notes,
    }
}

/// What the measuring document says of each side of each row.
fn measure(document: &PagedDocument, rows: &[Block], page: &Page) -> Vec<[Measure; 2]> {
    let key = |value: &Value| match value {
        Value::Dict(dict) => Some((int(dict, "index")?, int(dict, "side")?, int(dict, "piece"))),
        _ => None,
    };
    let mut tops: BTreeMap<(usize, usize), At> = BTreeMap::new();
    for (value, at) in marks(document, MEASURE_TOP) {
        if let Some((row, side, _)) = key(&value) {
            tops.insert((row, side), at);
        }
    }
    let mut pieces: BTreeMap<(usize, usize), BTreeMap<usize, At>> = BTreeMap::new();
    for (value, at) in marks(document, MEASURE_PIECE) {
        if let Some((row, side, Some(piece))) = key(&value) {
            pieces.entry((row, side)).or_default().insert(piece, at);
        }
    }
    let notes = footnotes(document);
    // Each note's height, from where it is set to where the next one on its page is, or to
    // the bottom of the text area.
    let mut heights: Vec<(At, f64)> = Vec::new();
    let mut set: Vec<&(At, At)> = notes.iter().collect();
    set.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("positions are numbers"));
    for (index, (at, call)) in set.iter().enumerate() {
        let top = tops
            .values()
            .filter(|top| top.page == at.page)
            .map(|top| top.y)
            .next()
            .unwrap_or(0.0);
        let bottom = match set.get(index + 1) {
            Some((next, _)) if next.page == at.page => next.y - page.notes.gap,
            _ => top + page.height,
        };
        heights.push((*call, bottom - at.y + page.notes.gap));
    }

    (0..rows.len())
        .map(|row| {
            [0, 1].map(|side| {
                let Some(top) = tops.get(&(row, side)).copied() else {
                    return Measure::default();
                };
                let marks: Vec<At> = pieces
                    .get(&(row, side))
                    .map(|marks| marks.values().copied().collect())
                    .unwrap_or_default();
                let last = marks.last().copied().unwrap_or(top);
                let within = |call: &At| *call >= top && *call <= last;
                let own: Vec<f64> = heights
                    .iter()
                    .filter(|(call, _)| within(call))
                    .map(|(_, height)| *height)
                    .collect();
                let set_on_its_page = notes
                    .iter()
                    .filter(|(_, call)| within(call))
                    .all(|(at, _)| at.page == top.page);
                let span = |from: At, to: At| {
                    if from.page == to.page {
                        to.y - from.y
                    } else {
                        (top.y + page.height - from.y)
                            + (to.y - top.y)
                            + page.height * (to.page - from.page - 1) as f64
                    }
                };
                let pieces = marks
                    .windows(2)
                    .enumerate()
                    .map(|(k, pair)| {
                        let from = if k == 0 { top } else { pair[0] };
                        let notes = heights
                            .iter()
                            .filter(|(call, _)| *call >= pair[0] && *call < pair[1])
                            .map(|(_, height)| *height)
                            .sum();
                        (span(from, pair[1]), notes)
                    })
                    .collect();
                Measure {
                    fits: last.page == top.page && set_on_its_page,
                    height: span(top, last),
                    notes: own.iter().sum(),
                    pieces,
                }
            })
        })
        .collect()
}

/// A spread of whole rows.
fn whole_rows(rows: &[Block], held: &[usize]) -> Spread {
    let sides = [0, 1].map(|side| {
        held.iter()
            .map(|&row| Part {
                row,
                from: 0,
                to: side_of(&rows[row], side).pieces.len(),
            })
            .filter(|part| part.to > part.from)
            .collect()
    });
    Spread {
        sides,
        origin: Origin::Rows(held.to_vec()),
    }
}

/// The height a page's text and notes take.
fn taken(body: f64, notes: f64, page: &Page) -> f64 {
    if notes > 0.0 {
        body + page.notes.above + notes
    } else {
        body
    }
}

/// The spreads: rows as many as fit both pages, a row too long for a page on spreads of its
/// own, and wherever `constraints` say otherwise.
fn plan(rows: &[Block], measures: &[[Measure; 2]], constraints: &Constraints, page: &Page) -> Plan {
    let mut spreads: Vec<Spread> = Vec::new();
    let mut held: Vec<usize> = Vec::new();
    let mut used = [(0.0, 0.0); 2];
    for (row, measure) in measures.iter().enumerate() {
        let long = constraints.long.contains(&row) || measure.iter().any(|side| !side.fits);
        if long {
            if !held.is_empty() {
                spreads.push(whole_rows(rows, &held));
                held.clear();
            }
            spreads.extend(long_row(rows, row, measure, constraints, page));
            continue;
        }
        let added = [0, 1].map(|side| {
            let (body, notes) = used[side];
            let height = measure[side].height;
            let body = match (height > 0.0, body > 0.0) {
                (true, true) => body + page.between + height,
                (true, false) => height,
                (false, _) => body,
            };
            (body, notes + measure[side].notes)
        });
        let fits = added
            .iter()
            .all(|&(body, notes)| taken(body, notes, page) <= page.height);
        if !held.is_empty() && (constraints.breaks.contains(&row) || !fits) {
            spreads.push(whole_rows(rows, &held));
            held.clear();
            used = [0, 1].map(|side| (measure[side].height, measure[side].notes));
        } else {
            used = added;
        }
        held.push(row);
    }
    if !held.is_empty() {
        spreads.push(whole_rows(rows, &held));
    }
    Plan { spreads }
}

/// The spreads of a row longer than a page: each side cut where its pieces fill a page, its
/// `k`th page on the `k`th spread, a side with fewer pages than the other leaving the rest of
/// its pages empty.
fn long_row(
    rows: &[Block],
    row: usize,
    measure: &[Measure; 2],
    constraints: &Constraints,
    page: &Page,
) -> Vec<Spread> {
    let cuts = [0, 1].map(|side| {
        let extra = constraints.cuts.get(&(row, side));
        let mut ranges = Vec::new();
        let mut from = 0;
        let (mut body, mut notes) = (0.0, 0.0);
        for (k, &(height, own)) in measure[side].pieces.iter().enumerate() {
            let over = taken(body + height, notes + own, page) > page.height;
            if k > from && (over || extra.is_some_and(|cuts| cuts.contains(&k))) {
                ranges.push(from..k);
                from = k;
                (body, notes) = (0.0, 0.0);
            }
            body += height;
            notes += own;
        }
        let count = side_of(&rows[row], side).pieces.len();
        if count > from {
            ranges.push(from..count);
        }
        ranges
    });
    let pages = cuts[0].len().max(cuts[1].len()).max(1);
    (0..pages)
        .map(|cut| Spread {
            sides: [0, 1].map(|side| {
                cuts[side]
                    .get(cut)
                    .map(|range| {
                        vec![Part {
                            row,
                            from: range.start,
                            to: range.end,
                        }]
                    })
                    .unwrap_or_default()
            }),
            origin: Origin::Long { row, cut },
        })
        .collect()
}

/// The spreads of `plan` that the typeset `document` does not hold as planned: a side that
/// ends on another page than it starts on, or that has a footnote set on another page than
/// the one it is called from.
fn check(document: &PagedDocument, plan: &Plan) -> Vec<usize> {
    let key = |value: &Value| match value {
        Value::Dict(dict) => Some((int(dict, "index")?, int(dict, "side")?)),
        _ => None,
    };
    let starts: BTreeMap<(usize, usize), At> = marks(document, SPREAD_START)
        .into_iter()
        .filter_map(|(value, at)| Some((key(&value)?, at)))
        .collect();
    let ends: BTreeMap<(usize, usize), At> = marks(document, SPREAD_END)
        .into_iter()
        .filter_map(|(value, at)| Some((key(&value)?, at)))
        .collect();
    let mut bad: BTreeSet<usize> = BTreeSet::new();
    for (key, start) in &starts {
        if ends.get(key).is_none_or(|end| end.page != start.page) {
            bad.insert(key.0);
        }
    }
    let spread_on: BTreeMap<usize, usize> = starts
        .iter()
        .map(|((spread, _), start)| (start.page, *spread))
        .collect();
    for (at, call) in footnotes(document) {
        if at.page != call.page
            && let Some(spread) = spread_on.get(&call.page)
        {
            bad.insert(*spread);
        }
    }
    bad.into_iter()
        .filter(|&spread| spread < plan.spreads.len())
        .collect()
}

/// Changes `constraints` so that the next plan sets `spread`, which did not hold, otherwise:
/// its last row on the next spread, a single row on spreads of its own cut into pages, a page
/// of such a row cut before its last piece. Whether there was anything to change.
fn constrain(spread: &Spread, constraints: &mut Constraints) -> bool {
    match &spread.origin {
        Origin::Rows(rows) => match rows.as_slice() {
            [] => false,
            [row] => constraints.long.insert(*row),
            [.., last] => constraints.breaks.insert(*last),
        },
        Origin::Long { row, cut } => {
            let mut changed = false;
            for (side, parts) in spread.sides.iter().enumerate() {
                for part in parts {
                    if part.to - part.from > 1 {
                        changed |= constraints
                            .cuts
                            .entry((*row, side))
                            .or_default()
                            .insert(part.to - 1);
                    } else {
                        constraints.over.insert((*row, side, *cut));
                    }
                }
            }
            changed
        }
    }
}

/// What the reader is told: which rows are longer than a page and so continue on the next
/// spread, and which spreads could not be made to hold.
fn said(
    rows: &[Block],
    measures: &[[Measure; 2]],
    constraints: &Constraints,
    left: &[usize],
) -> Vec<String> {
    let name = |row: usize| {
        (0..2)
            .flat_map(|side| side_of(&rows[row], side).pieces)
            .find_map(|piece| piece.number().map(str::to_owned))
            .filter(|number| !number.is_empty())
            .map(|number| format!("remark {number}"))
            .unwrap_or_else(|| "the text before the first remark".to_owned())
    };
    let long: Vec<String> = (0..rows.len())
        .filter(|row| {
            constraints.long.contains(row) || measures[*row].iter().any(|side| !side.fits)
        })
        .map(name)
        .collect();
    let mut out = Vec::new();
    if !long.is_empty() {
        out.push(match long.as_slice() {
            [one] => format!(
                "{one} is longer than a page, so it starts a spread and runs on to the same side of the next one, the shorter side's page left empty below it"
            ),
            many => format!(
                "{} rows are longer than a page, so each starts a spread and runs on to the same side of the next one, the shorter side's page left empty below it: {}",
                many.len(),
                many.join(", ")
            ),
        });
    }
    let over: BTreeSet<usize> = constraints.over.iter().map(|(row, _, _)| *row).collect();
    if !over.is_empty() {
        out.push(format!(
            "a paragraph longer than a page runs on to the next page, the opposite text's, in {}",
            over.into_iter().map(name).collect::<Vec<_>>().join(", ")
        ));
    }
    if !left.is_empty() {
        out.push(format!(
            "{} of the facing pages could not be made to hold their rows, and their text runs on to the next page",
            left.len()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use typesep_model::Inline;

    fn row(pieces: [usize; 2]) -> Block {
        let side = |count: usize| {
            vec![Block::Lang {
                lang: "en".into(),
                dir: None,
                children: (0..count)
                    .map(|_| Block::Para {
                        children: vec![Inline::Text { value: "x".into() }],
                        align: None,
                        indent_em: 0,
                    })
                    .collect(),
            }]
        };
        Block::Columns {
            columns: vec![side(pieces[0]), side(pieces[1])],
        }
    }

    /// A side of `count` pieces of `height` each, fitting when they come to a page at most.
    fn side(count: usize, height: f64, notes: f64) -> Measure {
        let total = count as f64 * height;
        Measure {
            fits: total + if notes > 0.0 { 10.0 + notes } else { 0.0 } <= 100.0,
            height: total,
            notes,
            pieces: (0..count).map(|_| (height, 0.0)).collect(),
        }
    }

    const PAGE: Page = Page {
        height: 100.0,
        between: 5.0,
        notes: NoteArea {
            above: 10.0,
            gap: 2.0,
        },
    };

    /// A part as its row and pieces.
    type Held = Vec<(usize, usize, usize)>;

    fn held(plan: &Plan) -> Vec<(Held, Held)> {
        let parts = |parts: &[Part]| {
            parts
                .iter()
                .map(|part| (part.row, part.from, part.to))
                .collect()
        };
        plan.spreads
            .iter()
            .map(|spread| (parts(&spread.sides[0]), parts(&spread.sides[1])))
            .collect()
    }

    #[test]
    fn rows_go_on_a_spread_while_both_its_pages_have_room() {
        let rows: Vec<Block> = (0..4).map(|_| row([2, 2])).collect();
        let measures = vec![
            [side(2, 20.0, 0.0), side(2, 15.0, 0.0)],
            [side(2, 20.0, 0.0), side(2, 30.0, 12.0)],
            [side(2, 10.0, 0.0), side(2, 10.0, 0.0)],
            [side(2, 10.0, 0.0), side(2, 10.0, 0.0)],
        ];
        let plan = plan(&rows, &measures, &Constraints::default(), &PAGE);
        // Right: 30 + 5 + 60 + 10 + 12 = 117 > 100, so the second row starts a spread; then
        // 60 + 22 + 5 + 20 = 107 > 100 again, so the third does; the third and fourth fit.
        assert_eq!(
            held(&plan),
            [
                (vec![(0, 0, 2)], vec![(0, 0, 2)]),
                (vec![(1, 0, 2)], vec![(1, 0, 2)]),
                (vec![(2, 0, 2), (3, 0, 2)], vec![(2, 0, 2), (3, 0, 2)]),
            ]
        );
        let mut breaks = Constraints::default();
        breaks.breaks.insert(3);
        assert_eq!(held(&plan_of(&rows, &measures, &breaks)).len(), 4);
    }

    fn plan_of(rows: &[Block], measures: &[[Measure; 2]], constraints: &Constraints) -> Plan {
        plan(rows, measures, constraints, &PAGE)
    }

    #[test]
    fn a_row_longer_than_a_page_is_cut_into_pages_on_spreads_of_its_own() {
        let rows = vec![row([1, 1]), row([6, 2]), row([1, 0])];
        let measures = vec![
            [side(1, 20.0, 0.0), side(1, 20.0, 0.0)],
            [side(6, 40.0, 0.0), side(2, 30.0, 0.0)],
            [side(1, 20.0, 0.0), side(0, 0.0, 0.0)],
        ];
        let plan = plan_of(&rows, &measures, &Constraints::default());
        assert_eq!(
            held(&plan),
            [
                (vec![(0, 0, 1)], vec![(0, 0, 1)]),
                (vec![(1, 0, 2)], vec![(1, 0, 2)]),
                (vec![(1, 2, 4)], vec![]),
                (vec![(1, 4, 6)], vec![]),
                (vec![(2, 0, 1)], vec![]),
            ],
            "two pieces of 40 to a page on the left, the right's two on the first; a side \
             with nothing to set has no part"
        );
    }

    #[test]
    fn a_spread_that_did_not_hold_is_set_otherwise_next_time() {
        let rows = |held: Vec<usize>| Spread {
            sides: [Vec::new(), Vec::new()],
            origin: Origin::Rows(held),
        };
        let mut constraints = Constraints::default();
        assert!(constrain(&rows(vec![3, 4, 5]), &mut constraints));
        assert_eq!(
            constraints.breaks,
            BTreeSet::from([5]),
            "its last row moves on"
        );
        assert!(constrain(&rows(vec![7]), &mut constraints));
        assert_eq!(
            constraints.long,
            BTreeSet::from([7]),
            "a row alone is cut into pages"
        );
        assert!(
            !constrain(&rows(vec![7]), &mut constraints),
            "and only once"
        );

        let page = |from: usize, to: usize| Spread {
            sides: [
                vec![Part { row: 2, from, to }],
                vec![Part {
                    row: 2,
                    from: 0,
                    to: 1,
                }],
            ],
            origin: Origin::Long { row: 2, cut: 1 },
        };
        assert!(constrain(&page(3, 6), &mut constraints));
        assert_eq!(
            constraints.cuts[&(2, 0)],
            BTreeSet::from([5]),
            "a page of a long row is cut before its last piece"
        );
        assert_eq!(
            constraints.over,
            BTreeSet::from([(2, 1, 1)]),
            "a page of a single piece cannot be cut, and runs on"
        );
        assert!(!constrain(&page(5, 6), &mut constraints));
    }
}
