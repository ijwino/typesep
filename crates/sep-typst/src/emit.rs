use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use sep_core::model::{
    Attachment, AttachmentKind, Block, EntryDoc, Figure, Inline, List, ListNumbering, NoteRef,
    NoteTable, Table, TableCell, TocItem,
};
use sep_core::sections::{ContentKey, SECTION_LABELS, selected_sections};
use sep_core::style::Style;

use crate::escape::{escape_code, escape_string, escape_text, guard_line_start, safe_id};
use crate::fonts::Fonts;
use crate::math::Converter;
use crate::preamble::{PreambleInput, build_preamble};
use crate::style::{View, typst_pt};
use crate::text::{
    MERGED_RIGHTS, PERSONAL_USE_NOTICE, SEP_NAME, SUPPORT_LABEL, SUPPORT_NOTICE, SUPPORT_URL,
    copyright_line, dateline, retrieval_dates, volume_meta,
};
use crate::world::FIGURES_PREFIX;

#[derive(Clone, Copy)]
pub struct EmitOptions<'a> {
    pub sections: &'a BTreeSet<ContentKey>,
    pub images: &'a BTreeMap<String, crate::images::Image>,
    pub cover_template: Option<&'a str>,
    pub volume_title: Option<&'a str>,
    pub fonts: Option<&'a Fonts>,
    pub raw_math: &'a BTreeSet<usize>,
}

impl Default for EmitOptions<'_> {
    fn default() -> Self {
        static NONE: BTreeSet<ContentKey> = BTreeSet::new();
        static NO_IMAGES: BTreeMap<String, crate::images::Image> = BTreeMap::new();
        static NO_RAW: BTreeSet<usize> = BTreeSet::new();
        EmitOptions {
            sections: &NONE,
            images: &NO_IMAGES,
            cover_template: None,
            volume_title: None,
            fonts: None,
            raw_math: &NO_RAW,
        }
    }
}

pub struct Rendered {
    pub source: String,
    pub figures: Vec<(String, PathBuf)>,
    pub warnings: Vec<String>,
    pub formulas: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NoteKind {
    Footnote,
    Endnote,
}

struct Ctx<'a> {
    view: &'a View<'a>,
    images: &'a BTreeMap<String, crate::images::Image>,
    sections: &'a BTreeSet<ContentKey>,
    merged: bool,
    slug: String,
    region: &'static str,
    notes: NoteTable,
    attached: HashMap<String, usize>,
    attachment_index: Option<usize>,
    self_urls: HashSet<String>,
    in_note: bool,
    pre: bool,
    known: HashSet<String>,
    seen: HashSet<String>,
    figure_paths: HashMap<String, String>,
    figures: Vec<(String, PathBuf)>,
    note_running: i64,
    endnotes: Vec<(i64, String)>,
    endnote_next: i64,
    math: Converter,
    raw_math: &'a BTreeSet<usize>,
    formulas: Vec<String>,
    warnings: Vec<String>,
    warned: HashSet<String>,
    links_mode: String,
    links_sections: Vec<String>,
    notes_style: String,
    entry_index: usize,
    long_notes: bool,
}

impl<'a> Ctx<'a> {
    fn new(view: &'a View<'a>, opts: &EmitOptions<'a>, merged: bool) -> Self {
        let notes_style = view.str("notes.style");
        Ctx {
            view,
            images: opts.images,
            sections: opts.sections,
            merged,
            slug: String::new(),
            region: "body",
            notes: NoteTable::new(),
            attached: HashMap::new(),
            attachment_index: None,
            self_urls: HashSet::new(),
            in_note: false,
            pre: false,
            known: HashSet::new(),
            seen: HashSet::new(),
            figure_paths: HashMap::new(),
            figures: Vec::new(),
            note_running: 0,
            endnotes: Vec::new(),
            endnote_next: 1,
            math: Converter::new(&Default::default()),
            raw_math: opts.raw_math,
            formulas: Vec::new(),
            warnings: Vec::new(),
            warned: HashSet::new(),
            links_mode: view.str("links.mode"),
            links_sections: view.list("links.sections"),
            entry_index: 0,
            long_notes: notes_style == "footnote"
                && crate::preamble::note_max_height_pt(view) > 0.0,
            notes_style,
        }
    }

    fn warn(&mut self, message: impl Into<String>) {
        let message = message.into();
        if self.warned.insert(message.clone()) {
            self.warnings.push(message);
        }
    }

    fn label_for(&self, id: &str, attachment: Option<usize>) -> String {
        let scoped = match attachment {
            Some(n) => format!("a{n}--{id}"),
            None => id.to_owned(),
        };
        let full = if self.merged && !self.slug.is_empty() {
            format!("{}--{scoped}", self.slug)
        } else {
            scoped
        };
        safe_id(&full)
    }

    fn label_here(&self, id: &str) -> String {
        self.label_for(id, self.attachment_index)
    }

    fn anchor(&mut self, id: &str) -> String {
        let label = self.label_here(id);
        if self.seen.insert(label.clone()) {
            format!("#metadata(none)<{label}>")
        } else {
            String::new()
        }
    }

    fn claim(&mut self, id: &str) -> Option<String> {
        let label = self.label_here(id);
        if self.seen.insert(label.clone()) {
            Some(label)
        } else {
            None
        }
    }

    fn resolves(&self, label: &str) -> bool {
        self.known.contains(label)
    }

    fn resolve_target(&self, href: &str) -> Option<String> {
        let (page, fragment) = match href.split_once('#') {
            Some((page, fragment)) => (page, Some(fragment)),
            None => (href, None),
        };
        if let Some(&attachment) = self.attached.get(page) {
            let label = match fragment {
                Some(fragment) => self.label_for(fragment, Some(attachment)),
                None => self.label_for("attachment", Some(attachment)),
            };
            return self.resolves(&label).then_some(label);
        }
        if self.self_urls.contains(page) {
            let fragment = fragment?;
            let label = self.label_for(fragment, None);
            return self.resolves(&label).then_some(label);
        }
        None
    }

    fn figure_path(&mut self, url: &str) -> Option<String> {
        if let Some(path) = self.figure_paths.get(url) {
            return Some(path.clone());
        }
        let image = self.images.get(url)?;
        let path = format!(
            "{FIGURES_PREFIX}/{}.{}",
            self.figures.len() + 1,
            image.format
        );
        self.figure_paths.insert(url.to_owned(), path.clone());
        self.figures.push((path.clone(), image.file.clone()));
        Some(path)
    }
}

fn highest_note_number(doc: &EntryDoc) -> i64 {
    let mut max = 0;
    walk_doc(doc, &mut |node| {
        if let Inline::NoteRef(note) = node
            && let Ok(n) = note.label.parse::<i64>()
        {
            max = max.max(n);
        }
    });
    max
}

fn walk_doc(doc: &EntryDoc, visit: &mut impl FnMut(&Inline)) {
    let mut blocks: Vec<&Block> = Vec::new();
    blocks.extend(doc.preamble.iter());
    blocks.extend(doc.body.iter());
    for section in [
        &doc.sections.bibliography,
        &doc.sections.academic_tools,
        &doc.sections.other_internet_resources,
        &doc.sections.related_entries,
    ]
    .into_iter()
    .flatten()
    {
        blocks.extend(section.iter());
    }
    for attachment in &doc.attachments {
        blocks.extend(attachment.blocks.iter());
        for note in attachment.notes.iter().flat_map(|n| n.values()) {
            blocks.extend(note.iter());
        }
    }
    for block in blocks {
        walk_block(block, visit);
    }
}

fn walk_block(block: &Block, visit: &mut impl FnMut(&Inline)) {
    match block {
        Block::Para { children, .. }
        | Block::Pre { children }
        | Block::Heading { children, .. } => walk_inlines(children, visit),
        Block::List(list) => {
            for item in &list.items {
                for block in item {
                    walk_block(block, visit);
                }
            }
            for label in list.labels.iter().flatten() {
                walk_inlines(label, visit);
            }
        }
        Block::Blockquote { children } => {
            for block in children {
                walk_block(block, visit);
            }
        }
        Block::Table(table) => {
            if let Some(caption) = &table.caption {
                walk_inlines(caption, visit);
            }
            for row in &table.rows {
                for cell in row {
                    walk_inlines(&cell.children, visit);
                }
            }
        }
        Block::Figure(figure) => {
            if let Some(caption) = &figure.caption {
                walk_inlines(caption, visit);
            }
        }
        Block::Rule | Block::Anchor { .. } => {}
    }
}

fn walk_inlines(nodes: &[Inline], visit: &mut impl FnMut(&Inline)) {
    for node in nodes {
        visit(node);
        match node {
            Inline::Emph { children }
            | Inline::Strong { children }
            | Inline::Sub { children }
            | Inline::Sup { children }
            | Inline::Code { children }
            | Inline::Link { children, .. } => walk_inlines(children, visit),
            Inline::Figure(figure) => {
                if let Some(caption) = &figure.caption {
                    walk_inlines(caption, visit);
                }
            }
            _ => {}
        }
    }
}

fn plain_text(nodes: &[Inline]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Inline::Text { value } => out.push_str(value),
            Inline::Math { tex, .. } => out.push_str(tex.trim()),
            Inline::Break => out.push(' '),
            Inline::Anchor { .. } | Inline::Figure(_) | Inline::NoteRef(_) => {}
            Inline::Emph { children }
            | Inline::Strong { children }
            | Inline::Sub { children }
            | Inline::Sup { children }
            | Inline::Code { children }
            | Inline::Link { children, .. } => out.push_str(&plain_text(children)),
        }
    }
    out
}

fn hoist_anchors(nodes: &[Inline]) -> (Vec<String>, Vec<Inline>) {
    let mut ids = Vec::new();
    let children = hoist(nodes, &mut ids);
    (ids, children)
}

fn hoist(nodes: &[Inline], ids: &mut Vec<String>) -> Vec<Inline> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            Inline::Anchor { id } => ids.push(id.clone()),
            Inline::Emph { children } => out.push(Inline::Emph {
                children: hoist(children, ids),
            }),
            Inline::Strong { children } => out.push(Inline::Strong {
                children: hoist(children, ids),
            }),
            Inline::Sub { children } => out.push(Inline::Sub {
                children: hoist(children, ids),
            }),
            Inline::Sup { children } => out.push(Inline::Sup {
                children: hoist(children, ids),
            }),
            Inline::Code { children } => out.push(Inline::Code {
                children: hoist(children, ids),
            }),
            Inline::Link {
                href,
                internal,
                children,
            } => out.push(Inline::Link {
                href: href.clone(),
                internal: *internal,
                children: hoist(children, ids),
            }),
            other => out.push(other.clone()),
        }
    }
    out
}

fn content(body: &str) -> String {
    if body.trim().is_empty() {
        "[]".to_owned()
    } else {
        format!("[{}]", guard_line_start(body))
    }
}

fn render_blocks(nodes: &[Block], ctx: &mut Ctx<'_>) -> Vec<String> {
    nodes.iter().filter_map(|node| block(node, ctx)).collect()
}

fn joined(nodes: &[Block], ctx: &mut Ctx<'_>) -> String {
    render_blocks(nodes, ctx).join("\n\n")
}

fn in_region<T>(ctx: &mut Ctx<'_>, region: &'static str, f: impl FnOnce(&mut Ctx<'_>) -> T) -> T {
    let previous = ctx.region;
    ctx.region = region;
    let out = f(ctx);
    ctx.region = previous;
    out
}

fn block(node: &Block, ctx: &mut Ctx<'_>) -> Option<String> {
    match node {
        Block::Heading {
            level,
            id,
            children,
        } => Some(heading(*level, id.as_deref(), children, ctx)),
        Block::Para { children, align } => {
            let text = inlines(children, ctx);
            if text.trim().is_empty() {
                return None;
            }
            Some(match align {
                Some(_) => format!("#align(center){}", content(&text)),
                None => guard_line_start(&text),
            })
        }
        Block::Pre { children } => Some(pre(children, ctx)),
        Block::List(list) => list_block(list, ctx),
        Block::Blockquote { children } => {
            let body = joined(children, ctx);
            Some(format!("#fs-quote[\n{}\n]", guard_line_start(&body)))
        }
        Block::Table(table) => table_block(table, ctx),
        Block::Figure(figure) => figure_block(figure, ctx),
        Block::Rule => Some("#line(length: 100%, stroke: 0.4pt)".to_owned()),
        Block::Anchor { id } => {
            let anchor = ctx.anchor(id);
            (!anchor.is_empty()).then_some(anchor)
        }
    }
}

fn heading(level: u8, id: Option<&str>, children: &[Inline], ctx: &mut Ctx<'_>) -> String {
    let (hoisted, children) = hoist_anchors(children);
    let mut ids: Vec<String> = Vec::new();
    if let Some(id) = id {
        ids.push(id.to_owned());
    }
    ids.extend(hoisted);
    let body = content(&inlines(&children, ctx));
    let mut own = None;
    let mut extra = String::new();
    for id in &ids {
        if own.is_none()
            && let Some(label) = ctx.claim(id)
        {
            own = Some(label);
            continue;
        }
        extra.push_str(&ctx.anchor(id));
    }
    let label = own.map(|l| format!(" <{l}>")).unwrap_or_default();
    format!(
        "#heading(level: {level}){body}{label}{}",
        if extra.is_empty() {
            String::new()
        } else {
            format!("\n{extra}")
        }
    )
}

fn pre(children: &[Inline], ctx: &mut Ctx<'_>) -> String {
    ctx.pre = true;
    let text = inlines(children, ctx);
    ctx.pre = false;
    format!("#fs-code-block{}", content(&text))
}

fn numbering_pattern(numbering: ListNumbering) -> &'static str {
    match numbering {
        ListNumbering::Decimal => "1.",
        ListNumbering::LowerAlpha => "a.",
        ListNumbering::UpperAlpha => "A.",
        ListNumbering::LowerRoman => "i.",
        ListNumbering::UpperRoman => "I.",
    }
}

fn list_block(list: &List, ctx: &mut Ctx<'_>) -> Option<String> {
    if let Some(labels) = &list.labels {
        let items: Vec<String> = list
            .items
            .iter()
            .enumerate()
            .map(|(i, blocks)| {
                let label = labels
                    .get(i)
                    .filter(|l| !l.is_empty())
                    .map(|l| inlines(l, ctx))
                    .unwrap_or_default();
                let body = joined(blocks, ctx);
                format!("#fs-tag-item({}, {})", content(&label), content(&body))
            })
            .collect();
        return (!items.is_empty()).then(|| items.join("\n\n"));
    }

    let items: Vec<String> = list
        .items
        .iter()
        .map(|blocks| content(&joined(blocks, ctx)))
        .collect();
    if items.is_empty() {
        return None;
    }
    if list.hanging {
        return Some(
            items
                .iter()
                .map(|item| format!("#fs-bib{item}"))
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
    }
    if !list.ordered {
        return Some(format!("#list(tight: false, {})", items.join(", ")));
    }
    let mut args = vec!["tight: false".to_owned()];
    if let Some(numbering) = list.numbering {
        args.push(format!("numbering: \"{}\"", numbering_pattern(numbering)));
    }
    if let Some(start) = list.start {
        args.push(format!("start: {start}"));
    }
    args.extend(items);
    Some(format!("#enum({})", args.join(", ")))
}

#[derive(Clone, Copy)]
struct Slot {
    cell: usize,
    origin: bool,
}

fn table_grid(table: &Table) -> Vec<Vec<Option<Slot>>> {
    let columns = table.columns as usize;
    let mut grid: Vec<Vec<Option<Slot>>> = vec![vec![None; columns]; table.rows.len()];
    let mut index = 0;
    for (r, cells) in table.rows.iter().enumerate() {
        let mut c = 0;
        for cell in cells {
            while c < columns && grid[r][c].is_some() {
                c += 1;
            }
            for dr in 0..cell.rowspan as usize {
                if r + dr >= grid.len() {
                    break;
                }
                for dc in 0..cell.colspan as usize {
                    if c + dc >= columns {
                        break;
                    }
                    grid[r + dr][c + dc] = Some(Slot {
                        cell: index,
                        origin: dr == 0 && dc == 0,
                    });
                }
            }
            c += cell.colspan as usize;
            index += 1;
        }
    }
    grid
}

fn cells_of(table: &Table) -> Vec<&TableCell> {
    table.rows.iter().flatten().collect()
}

fn cell_is_empty(cell: &TableCell) -> bool {
    plain_text(&cell.children).trim().is_empty()
        && !cell
            .children
            .iter()
            .any(|n| matches!(n, Inline::Figure(_) | Inline::Math { .. }))
}

fn table_block(table: &Table, ctx: &mut Ctx<'_>) -> Option<String> {
    let grid = table_grid(table);
    let cells = cells_of(table);
    let columns = table.columns as usize;
    let kept: Vec<usize> = (0..columns)
        .filter(|&c| {
            grid.iter()
                .any(|row| row[c].is_some_and(|slot| !cell_is_empty(cells[slot.cell])))
        })
        .collect();
    if kept.is_empty() {
        return None;
    }

    let mut emitted: Vec<String> = Vec::new();
    for (r, row) in grid.iter().enumerate() {
        let mut done: HashSet<usize> = HashSet::new();
        for (i, &c) in kept.iter().enumerate() {
            let _ = i;
            let Some(slot) = row[c] else {
                emitted.push("[]".to_owned());
                continue;
            };
            if !done.insert(slot.cell) {
                continue;
            }
            if !slot.origin
                && grid[..r]
                    .iter()
                    .any(|above| above.iter().flatten().any(|s| s.cell == slot.cell))
            {
                continue;
            }
            let cell = cells[slot.cell];
            let span = kept
                .iter()
                .filter(|&&k| row[k].is_some_and(|s| s.cell == slot.cell))
                .count();
            let rowspan = (cell.rowspan as usize).min(grid.len() - r);
            let mut body = inlines(&cell.children, ctx);
            if cell.header {
                body = format!("#strong{}", content(&body));
            }
            let mut args = Vec::new();
            if span > 1 {
                args.push(format!("colspan: {span}"));
            }
            if rowspan > 1 {
                args.push(format!("rowspan: {rowspan}"));
            }
            if args.is_empty() {
                emitted.push(content(&body));
            } else {
                emitted.push(format!(
                    "table.cell({}, {})",
                    args.join(", "),
                    content(&body)
                ));
            }
        }
    }

    let header_rows = table
        .rows
        .iter()
        .take_while(|row| !row.is_empty() && row.iter().all(|c| c.header))
        .count();
    let caption = table
        .caption
        .as_ref()
        .filter(|c| !c.is_empty())
        .map(|c| format!("caption: {}, ", content(&inlines(c, ctx))))
        .unwrap_or_default();
    Some(format!(
        "#fs-table({caption}columns: {}, rows: {}, header-rows: {header_rows}, {})",
        kept.len(),
        grid.len(),
        emitted.join(", ")
    ))
}

fn figure_markup(figure: &Figure, ctx: &mut Ctx<'_>) -> Option<String> {
    let anchor = figure
        .id
        .as_ref()
        .and_then(|id| id.as_ref())
        .map(|id| ctx.anchor(id))
        .unwrap_or_default();
    let caption = figure
        .caption
        .as_ref()
        .filter(|c| !c.is_empty())
        .map(|c| inlines(c, ctx));
    let Some(path) = ctx.figure_path(&figure.src) else {
        return match caption {
            Some(caption) => Some(format!(
                "{anchor}#align(center)[#fs-caption{}]",
                content(&caption)
            )),
            None => (!anchor.is_empty()).then_some(anchor),
        };
    };
    if figure.inline == Some(true) {
        return Some(format!(
            "{anchor}#fs-figure-inline({})",
            escape_string(&path)
        ));
    }
    let caption = caption
        .map(|c| format!(", caption: {}", content(&c)))
        .unwrap_or_default();
    Some(format!(
        "{anchor}#fs-figure({}{caption})",
        escape_string(&path)
    ))
}

fn figure_block(figure: &Figure, ctx: &mut Ctx<'_>) -> Option<String> {
    figure_markup(figure, ctx)
}

fn ends_unescaped(text: &str, marks: &[char]) -> bool {
    text.strip_suffix(marks)
        .is_some_and(|rest| rest.chars().rev().take_while(|&c| c == '\\').count() % 2 == 0)
}

fn guard_after_call(before: &str, piece: &str) -> String {
    if !ends_unescaped(before, &[']', ')']) {
        return piece.to_owned();
    }
    let mut chars = piece.chars();
    let guard = match chars.next() {
        Some('(') | Some('[') => true,
        Some('.') => chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_'),
        _ => false,
    };
    if guard {
        format!("\\{piece}")
    } else {
        piece.to_owned()
    }
}

fn join_inline(pieces: impl Iterator<Item = String>) -> String {
    let mut out = String::new();
    for piece in pieces {
        if piece.starts_with(['/', '*']) && ends_unescaped(&out, &['/']) {
            out.push('\\');
        }
        out.push_str(&guard_after_call(&out, &piece));
    }
    out
}

fn inlines(nodes: &[Inline], ctx: &mut Ctx<'_>) -> String {
    let last = nodes.len().saturating_sub(1);
    let pieces: Vec<String> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| inline_node(node, ctx, i == last))
        .collect();
    join_inline(pieces.into_iter())
}

fn code_inlines(nodes: &[Inline], ctx: &mut Ctx<'_>) -> String {
    let last = nodes.len().saturating_sub(1);
    let pieces: Vec<String> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| match node {
            Inline::Text { value } if !ctx.pre => escape_code(value),
            other => inline_node(other, ctx, i == last),
        })
        .collect();
    join_inline(pieces.into_iter())
}

fn pre_text(value: &str) -> String {
    value
        .split('\n')
        .map(|line| {
            guard_line_start(&escape_code(&line.replace('\t', "    ")).replace(' ', "\u{a0}"))
        })
        .collect::<Vec<_>>()
        .join("\\\n")
}

fn inline_node(node: &Inline, ctx: &mut Ctx<'_>, is_last: bool) -> String {
    match node {
        Inline::Text { value } => {
            if ctx.pre {
                pre_text(value)
            } else {
                escape_text(value)
            }
        }
        Inline::Math { mode, tex } => math(tex, *mode == sep_core::model::MathMode::Display, ctx),
        Inline::Emph { children } => format!("#emph{}", content(&inlines(children, ctx))),
        Inline::Strong { children } => format!("#strong{}", content(&inlines(children, ctx))),
        Inline::Sub { children } => format!("#sub{}", content(&inlines(children, ctx))),
        Inline::Sup { children } => format!("#super{}", content(&inlines(children, ctx))),
        Inline::Code { children } => format!("#fs-code{}", content(&code_inlines(children, ctx))),
        Inline::Break => {
            if ctx.pre {
                "\\\n".to_owned()
            } else if is_last {
                String::new()
            } else {
                "#linebreak()".to_owned()
            }
        }
        Inline::Anchor { id } => ctx.anchor(id),
        Inline::Figure(figure) => figure_markup(figure, ctx).unwrap_or_default(),
        Inline::NoteRef(note) => note_ref(note, ctx),
        Inline::Link {
            href,
            internal,
            children,
        } => link(href, *internal, children, ctx),
    }
}

fn math(tex: &str, display: bool, ctx: &mut Ctx<'_>) -> String {
    let raw = || format!("#fs-math-raw({})", escape_string(tex.trim()));
    let index = ctx.formulas.len();
    ctx.formulas.push(tex.to_owned());
    if ctx.raw_math.contains(&index) {
        return raw();
    }
    match ctx.math.convert(tex, display) {
        Ok(code) => format!(
            "#fs-math({}{})",
            escape_string(&code),
            if display { ", block: true" } else { "" }
        ),
        Err(message) => {
            ctx.warn(format!(
                "maths left as LaTeX, mitex could not convert it: {message}"
            ));
            raw()
        }
    }
}

fn note(kind: NoteKind, number: Option<i64>, body: &str, ctx: &mut Ctx<'_>) -> String {
    let body = content(body);
    match kind {
        NoteKind::Footnote => match number {
            Some(n) if ctx.long_notes => format!(
                "#counter(footnote).update({})#fs-note({}, {n}){body}#counter(footnote).update({})",
                n - 1,
                ctx.entry_index,
                ctx.note_running
            ),
            Some(n) => format!(
                "#counter(footnote).update({})#footnote{body}#counter(footnote).update({})",
                n - 1,
                ctx.note_running
            ),
            None => {
                ctx.note_running += 1;
                format!("#footnote{body}")
            }
        },
        NoteKind::Endnote => {
            let n = match number {
                Some(n) => n,
                None => {
                    let n = ctx.endnote_next;
                    ctx.endnote_next += 1;
                    n
                }
            };
            ctx.endnote_next = ctx.endnote_next.max(n + 1);
            ctx.endnotes.push((n, body.clone()));
            let label = ctx.label_for(&format!("endnote-{n}"), None);
            ctx.seen.insert(label.clone());
            if ctx.resolves(&label) {
                format!("#link(<{label}>)[#super[{n}]]")
            } else {
                format!("#super[{n}]")
            }
        }
    }
}

fn note_kind(style: &str) -> NoteKind {
    if style == "endnote" {
        NoteKind::Endnote
    } else {
        NoteKind::Footnote
    }
}

fn note_ref(node: &NoteRef, ctx: &mut Ctx<'_>) -> String {
    let anchor = node
        .ref_id
        .as_ref()
        .map(|id| ctx.anchor(id))
        .unwrap_or_default();
    let label = escape_text(&node.label);
    let note_blocks = ctx.notes.get(&node.id).cloned();

    if let Some(blocks) = note_blocks
        && ctx.notes_style != "appendix"
        && !ctx.in_note
    {
        ctx.in_note = true;
        let body = joined(&blocks, ctx);
        ctx.in_note = false;
        let number = node.label.parse::<i64>().ok();
        let kind = note_kind(&ctx.notes_style);
        return format!("{anchor}{}", note(kind, number, &body, ctx));
    }

    if let Some(target) = ctx.resolve_target(&node.href) {
        return format!("{anchor}#super[\\[#link(<{target}>){}\\]]", content(&label));
    }
    if ctx.links_mode == "none" {
        return format!("{anchor}#super[\\[{label}\\]]");
    }
    format!(
        "{anchor}#super[\\[#link({}){}\\]]",
        escape_string(&node.href),
        content(&label)
    )
}

fn link(href: &str, internal: bool, children: &[Inline], ctx: &mut Ctx<'_>) -> String {
    let text = inlines(children, ctx);
    if internal {
        let label = ctx.label_for(href, ctx.attachment_index);
        return if ctx.resolves(&label) {
            format!("#link(<{label}>){}", content(&text))
        } else {
            text
        };
    }
    if let Some(target) = ctx.resolve_target(href) {
        return format!("#link(<{target}>){}", content(&text));
    }

    let mut mode = ctx.links_mode.clone();
    if (mode == "footnote" || mode == "endnote")
        && (!ctx.links_sections.iter().any(|s| s == ctx.region) || ctx.in_note)
    {
        mode = "inline".to_owned();
    }
    let url = escape_string(href);
    match mode.as_str() {
        "none" => text,
        "footnote" => {
            let body = format!("#fs-code{}", content(&escape_code(href)));
            format!(
                "#link({url}){}{}",
                content(&text),
                note(NoteKind::Footnote, None, &body, ctx)
            )
        }
        "endnote" => {
            let body = format!("#fs-code[{}]", escape_code(href));
            format!("{text}{}", note(NoteKind::Endnote, None, &body, ctx))
        }
        _ => format!("#link({url}){}", content(&text)),
    }
}

fn attachment_markup(extra: &Attachment, ctx: &mut Ctx<'_>) -> Vec<String> {
    let page = extra.source_url.split('#').next().unwrap_or("").to_owned();
    ctx.attachment_index = ctx.attached.get(&page).copied();
    let anchor = ctx.anchor("attachment");
    let title = escape_text(&extra.title);
    let mut out = vec![format!("{anchor}\n#heading(level: 2){}", content(&title))];
    out.extend(render_blocks(&extra.blocks, ctx));
    ctx.attachment_index = None;
    out
}

fn table_of_contents(toc: &[TocItem], ctx: &mut Ctx<'_>) -> Option<String> {
    let style = ctx.view.str("toc.style");
    let depth = ctx.view.int("toc.depth");
    if style == "none" || toc.is_empty() {
        return None;
    }
    let title = ctx.view.str("toc.title");
    let heading = if title.is_empty() {
        String::new()
    } else {
        format!("#heading(level: 2){}\n", content(&escape_text(&title)))
    };
    if style == "latex" {
        return Some(
            format!("#outline(depth: {}, title: none)", (depth + 1).max(1))
                .to_string()
                .pipe(|outline| format!("{heading}{outline}")),
        );
    }

    let dropped: HashSet<String> = SECTION_LABELS
        .iter()
        .filter(|(key, _)| !ctx.sections.contains(key))
        .map(|(_, label)| label.to_lowercase())
        .collect();
    let page_numbers = ctx.view.bool("toc.pageNumbers");
    let body = toc_items(toc, 1, depth, &dropped, page_numbers, ctx)?;
    Some(format!("{heading}{body}"))
}

fn toc_items(
    items: &[TocItem],
    level: i64,
    depth: i64,
    dropped: &HashSet<String>,
    page_numbers: bool,
    ctx: &mut Ctx<'_>,
) -> Option<String> {
    if level > depth {
        return None;
    }
    let kept: Vec<&TocItem> = items
        .iter()
        .filter(|item| !dropped.contains(&item.text.trim().to_lowercase()))
        .collect();
    if kept.is_empty() {
        return None;
    }
    let entries: Vec<String> = kept
        .iter()
        .map(|item| {
            let label = escape_text(&item.text);
            let target = item
                .href
                .strip_prefix('#')
                .map(|id| ctx.label_for(id, ctx.attachment_index))
                .filter(|label| ctx.resolves(label));
            let text = match &target {
                Some(target) => format!("#link(<{target}>){}", content(&label)),
                None => label,
            };
            let page = match (&target, page_numbers) {
                (Some(target), true) => format!(
                    " #box(width: 1fr, repeat[.]) #context counter(page).at(<{target}>).at(0)"
                ),
                _ => String::new(),
            };
            let children = toc_items(&item.children, level + 1, depth, dropped, page_numbers, ctx)
                .map(|c| format!("\n\n{c}"))
                .unwrap_or_default();
            content(&format!("{text}{page}{children}"))
        })
        .collect();
    Some(format!("#list(tight: true, {})", entries.join(", ")))
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

/// Where the entry came from and who holds it: the source, the date it was fetched and the
/// copyright line. Both the title page and the colophon open with these.
fn provenance(meta: &sep_core::model::EntryMeta, merged: bool, retrieved: &str) -> Vec<String> {
    let source = if meta.edition_label.is_empty() {
        SEP_NAME.to_owned()
    } else {
        format!("{SEP_NAME}, {}", escape_text(&meta.edition_label))
    };
    let rights = if merged {
        MERGED_RIGHTS.to_owned()
    } else {
        escape_text(&copyright_line(meta))
    };
    [
        source,
        if retrieved.is_empty() {
            String::new()
        } else {
            format!("Retrieved {}", escape_text(retrieved))
        },
        rights,
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect()
}

fn note_block(parts: &[String]) -> String {
    let parts: Vec<String> = parts
        .iter()
        .map(|part| format!("#par{}", content(part)))
        .collect();
    format!("#text(size: fs-note-size)[{}]", parts.join("\n    "))
}

/// The terms the entry is read under, as the last page. This is not a style option: no
/// setting removes it, because `cover.style` and `cover.fields` are the reader's to choose
/// and the notice is not. The EPUB carries the same words in `dc:rights`.
fn colophon(meta: &sep_core::model::EntryMeta, merged: bool, retrieved: &str) -> String {
    let mut parts = provenance(meta, merged, retrieved);
    parts.push(escape_text(PERSONAL_USE_NOTICE));
    parts.push(format!(
        "{} #link({})[{}]",
        escape_text(SUPPORT_NOTICE),
        escape_string(SUPPORT_URL),
        crate::escape::url_for_display(SUPPORT_LABEL)
    ));
    format!(
        "#page(header: none, footer: none, columns: 1, numbering: none)[\n  #v(1fr)\n  {}\n]",
        note_block(&parts)
    )
}

fn simple_cover(
    meta: &sep_core::model::EntryMeta,
    docs: &[&EntryDoc],
    merged: bool,
    retrieved: &str,
    ctx: &mut Ctx<'_>,
) -> String {
    let mut lines: Vec<String> = vec!["  #set align(center)".to_owned(), "  #v(18%)".to_owned()];
    macro_rules! add {
        ($markup:expr) => {{
            lines.push(format!("  {}", $markup));
            lines.push("  #v(1.2em)".to_owned());
        }};
    }

    let fields: Vec<String> = ctx
        .view
        .list("cover.fields")
        .into_iter()
        .filter(|f| !(merged && f == "dates"))
        .collect();

    for field in fields {
        match field.as_str() {
            "title" => add!(format!(
                "#text(size: 20pt){}",
                content(&escape_text(&meta.title))
            )),
            "author" => {
                if !meta.author_line.is_empty() {
                    add!(format!(
                        "#text(size: 1.2em){}",
                        content(&escape_text(&meta.author_line))
                    ));
                } else if docs.len() > 1 {
                    add!(format!(
                        "#text(size: 1.2em)[{} entries from the {SEP_NAME}]",
                        docs.len()
                    ));
                }
            }
            "dates" => {
                let dates = dateline(meta);
                if !dates.is_empty() {
                    add!(format!("#emph{}", content(&escape_text(&dates))));
                }
            }
            "sourceUrl" => {
                if !meta.source_url.is_empty() {
                    add!(format!(
                        "#text(size: 0.9em)[#link({})[#fs-code{}]]",
                        escape_string(&meta.source_url),
                        content(&crate::escape::url_for_display(&meta.source_url))
                    ));
                }
            }
            "copyright" => {
                lines.push("  #v(1fr)".to_owned());
                lines.push(format!(
                    "  {}",
                    note_block(&provenance(meta, merged, retrieved))
                ));
            }
            _ => {}
        }
    }
    format!(
        "#page(header: none, footer: none, columns: 1, numbering: none)[\n{}\n]",
        lines.join("\n")
    )
}

fn entry_body(doc: &EntryDoc, ctx: &mut Ctx<'_>) -> Vec<String> {
    ctx.slug = doc.meta.slug.clone();
    ctx.math = Converter::new(&doc.meta.macros);
    ctx.notes = doc
        .attachments
        .iter()
        .filter(|a| a.kind == AttachmentKind::Notes)
        .filter_map(|a| a.notes.as_ref())
        .flat_map(|notes| notes.iter().map(|(k, v)| (k.clone(), v.clone())))
        .collect();
    ctx.attached = doc
        .attachments
        .iter()
        .enumerate()
        .map(|(i, a)| {
            (
                a.source_url.split('#').next().unwrap_or("").to_owned(),
                i + 1,
            )
        })
        .collect();
    ctx.attachment_index = None;
    ctx.self_urls = if doc.meta.source_url.is_empty() {
        HashSet::new()
    } else {
        let base = doc.meta.source_url.clone();
        let directory = match base.rfind('/') {
            Some(at) => format!("{}index.html", &base[..=at]),
            None => format!("{base}index.html"),
        };
        HashSet::from([base, directory])
    };
    ctx.endnotes.clear();
    ctx.endnote_next = 1;

    let notes_inline = ctx.notes_style != "appendix" && !ctx.notes.is_empty();
    ctx.note_running = if notes_inline {
        highest_note_number(doc)
    } else {
        0
    };

    let mut out = vec![format!(
        "#heading(level: 1){}",
        content(&escape_text(&doc.meta.title))
    )];
    if notes_inline && ctx.note_running > 0 {
        out.push(format!("#counter(footnote).update({})", ctx.note_running));
    }

    let dates = dateline(&doc.meta);
    if !dates.is_empty() {
        out.push(format!(
            "#block(below: 0.8em)[#emph{}]",
            content(&escape_text(&dates))
        ));
    }

    out.extend(in_region(ctx, "preamble", |ctx| {
        render_blocks(&doc.preamble, ctx)
    }));

    if let Some(toc) = table_of_contents(&doc.toc, ctx) {
        out.push(toc);
    }

    out.extend(in_region(ctx, "body", |ctx| render_blocks(&doc.body, ctx)));

    let notes_attached: Vec<&Attachment> = doc
        .attachments
        .iter()
        .filter(|a| a.kind == AttachmentKind::Notes)
        .collect();
    if ctx.notes_style == "appendix" && ctx.sections.contains(&ContentKey::Notes) {
        for extra in notes_attached {
            let extra = extra.clone();
            out.extend(in_region(ctx, "notes", |ctx| {
                attachment_markup(&extra, ctx)
            }));
        }
    }

    for (key, _, blocks) in selected_sections(doc, ctx.sections) {
        let blocks = blocks.to_vec();
        let region = region_name(key);
        out.extend(in_region(ctx, region, |ctx| render_blocks(&blocks, ctx)));
    }

    if ctx.sections.contains(&ContentKey::Supplements) {
        let supplements: Vec<Attachment> = doc
            .attachments
            .iter()
            .filter(|a| a.kind == AttachmentKind::Supplement)
            .cloned()
            .collect();
        for extra in supplements {
            out.extend(in_region(ctx, "supplements", |ctx| {
                attachment_markup(&extra, ctx)
            }));
        }
    }

    let endnotes_here = !ctx.endnotes.is_empty();
    if endnotes_here {
        let mut notes = std::mem::take(&mut ctx.endnotes);
        notes.sort_by_key(|(n, _)| *n);
        let mut lines = vec!["#heading(level: 2)[Notes]".to_owned()];
        for (n, body) in notes {
            let label = ctx.label_for(&format!("endnote-{n}"), None);
            lines.push(format!(
                "#metadata(none)<{label}>\n#fs-endnote([{n}], {body})"
            ));
        }
        out.push(lines.join("\n\n"));
    }

    if ctx.long_notes && !ctx.notes.is_empty() {
        let title = if endnotes_here { "Long notes" } else { "Notes" };
        out.push(format!(
            "#fs-moved-notes({}, title: [{title}])",
            ctx.entry_index
        ));
    }
    out
}

fn region_name(key: ContentKey) -> &'static str {
    match key {
        ContentKey::Bib => "bib",
        ContentKey::Academic => "academic",
        ContentKey::Oir => "oir",
        ContentKey::Related => "related",
        ContentKey::Notes => "notes",
        ContentKey::Supplements => "supplements",
    }
}

fn render_body(docs: &[&EntryDoc], ctx: &mut Ctx<'_>, opts: &EmitOptions<'_>) -> String {
    let view = ctx.view;
    let metas: Vec<&sep_core::model::EntryMeta> = docs.iter().map(|d| &d.meta).collect();
    let meta = volume_meta(docs, opts.volume_title);
    let retrieved = retrieval_dates(&metas);

    let mut out: Vec<String> = Vec::new();
    let start_at = view.int("pageFurniture.numbering.startAt");
    if start_at != 1 {
        out.push(format!("#counter(page).update({start_at})"));
    }
    let cover_style = view.str("cover.style");
    if cover_style == "custom" {
        if let Some(template) = opts.cover_template {
            out.push(template.trim_end().to_owned());
        }
    } else if cover_style == "simple" {
        let merged = opts.volume_title.is_some();
        out.push(simple_cover(&meta, docs, merged, &retrieved, ctx));
    }

    let before = view.str("advanced.hooks.beforeBody");
    if !before.trim().is_empty() {
        out.push(before.trim().to_owned());
    }

    for (index, doc) in docs.iter().enumerate() {
        if index > 0 {
            out.push("#pagebreak(weak: true)".to_owned());
        }
        ctx.entry_index = index;
        out.extend(entry_body(doc, ctx));
    }

    let after = view.str("advanced.hooks.afterBody");
    if !after.trim().is_empty() {
        out.push(after.trim().to_owned());
    }

    out.push(colophon(&meta, opts.volume_title.is_some(), &retrieved));
    out.join("\n\n")
}

pub fn emit_document(docs: &[&EntryDoc], style: &Style, opts: EmitOptions<'_>) -> Rendered {
    assert!(!docs.is_empty(), "a group holds at least one entry");
    let view = View::new(style);
    let merged = docs.len() > 1;

    let mut first = Ctx::new(&view, &opts, merged);
    let _ = render_body(docs, &mut first, &opts);
    let known = first.seen;

    let mut ctx = Ctx::new(&view, &opts, merged);
    ctx.known = known;
    let body = render_body(docs, &mut ctx, &opts);

    let metas: Vec<&sep_core::model::EntryMeta> = docs.iter().map(|d| &d.meta).collect();
    let meta = volume_meta(docs, opts.volume_title);
    let input = PreambleInput {
        title: meta.title.clone(),
        author_line: meta.author_line.clone(),
        edition: meta.edition_label.clone(),
        date: retrieval_dates(&metas),
        subject: SEP_NAME.to_owned(),
    };
    let (preamble, mut warnings) = build_preamble(&view, &input, opts.fonts);
    warnings.extend(ctx.warnings);

    let mut source = String::with_capacity(preamble.len() + body.len() + 64);
    let _ = write!(source, "{preamble}\n{body}\n");
    Rendered {
        source,
        figures: ctx.figures,
        warnings,
        formulas: ctx.formulas,
    }
}

pub fn length_pt(view: &View<'_>, path: &str) -> String {
    typst_pt(view.pt(path))
}
