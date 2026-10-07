use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use typesep_model::{
    Align, Attachment, AttachmentKind, Block, Decoration, Dir, Doc, Figure, Inline, List,
    ListNumbering, Meta, NoteRef, NoteTable, Rules, Section, Table, TableCell, TocItem, VerseLine,
};
use typesep_style::Style;

use crate::escape::{escape_code, escape_string, escape_text, guard_line_start, safe_id};
use crate::fonts::Fonts;
use crate::lang::TypstLang;
use crate::math::Converter;
use crate::preamble::{Helper, PreambleInput, build_preamble};
use crate::style::typst_pt;
use crate::text::retrieval_dates;
use crate::world::FIGURES_PREFIX;
use typesep_style::View;

#[derive(Clone, Copy)]
pub struct EmitOptions<'a> {
    pub sections: &'a BTreeSet<String>,
    pub images: &'a BTreeMap<String, crate::images::Image>,
    pub cover_template: Option<&'a str>,
    pub volume: Option<&'a Meta>,
    pub fonts: Option<&'a Fonts>,
    pub raw_math: &'a BTreeSet<usize>,
    /// The document is a bilingual edition, whose body is a row of two columns to each pair of
    /// remarks, as the parallel pass makes it; `parallel.layout` decides how its PDF sets them.
    pub parallel: bool,
}

impl Default for EmitOptions<'_> {
    fn default() -> Self {
        static NONE: BTreeSet<String> = BTreeSet::new();
        static NO_IMAGES: BTreeMap<String, crate::images::Image> = BTreeMap::new();
        static NO_RAW: BTreeSet<usize> = BTreeSet::new();
        EmitOptions {
            sections: &NONE,
            images: &NO_IMAGES,
            cover_template: None,
            volume: None,
            fonts: None,
            raw_math: &NO_RAW,
            parallel: false,
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
    sections: &'a BTreeSet<String>,
    merged: bool,
    doc_id: String,
    region: String,
    notes: NoteTable,
    own_notes: NoteTable,
    own_refs: Vec<(String, String)>,
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
    helpers: BTreeSet<Helper>,
    layout: Layout,
}

/// How the body is laid out: as it flows, or, for a bilingual edition whose texts face each
/// other, each side of each row on a page of its own with marks to measure it by, or the rows
/// as the spreads of a plan made from those measures.
#[derive(Debug, Clone, Default)]
pub(crate) enum Layout {
    #[default]
    Flow,
    Measure,
    Facing(crate::facing::Plan),
}

impl<'a> Ctx<'a> {
    fn new(view: &'a View<'a>, opts: &EmitOptions<'a>, merged: bool) -> Self {
        let notes_style = view.str("notes.style");
        Ctx {
            view,
            images: opts.images,
            sections: opts.sections,
            merged,
            doc_id: String::new(),
            region: "body".to_owned(),
            notes: NoteTable::new(),
            own_notes: NoteTable::new(),
            own_refs: Vec::new(),
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
            helpers: BTreeSet::new(),
            layout: Layout::Flow,
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
        let full = if self.merged && !self.doc_id.is_empty() {
            format!("{}--{scoped}", self.doc_id)
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

fn highest_note_number(doc: &Doc) -> i64 {
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

fn walk_doc(doc: &Doc, visit: &mut impl FnMut(&Inline)) {
    let mut blocks: Vec<&Block> = Vec::new();
    blocks.extend(doc.front.iter());
    blocks.extend(doc.body.iter());
    for section in &doc.back {
        blocks.extend(section.blocks.iter());
    }
    for attachment in &doc.attachments {
        blocks.extend(attachment.blocks.iter());
        for note in attachment.notes.iter().flat_map(|n| n.values()) {
            blocks.extend(note.iter());
        }
    }
    for note in doc.notes.values() {
        blocks.extend(note.iter());
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
        Block::Blockquote { children }
        | Block::Remark { children, .. }
        | Block::Aside { children }
        | Block::Lang { children, .. } => {
            for block in children {
                walk_block(block, visit);
            }
        }
        Block::Columns { columns } => {
            for block in columns.iter().flatten() {
                walk_block(block, visit);
            }
        }
        Block::Verse { lines } => {
            for line in lines {
                walk_inlines(&line.children, visit);
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
            | Inline::Link { children, .. }
            | Inline::Spaced { children }
            | Inline::SmallCaps { children }
            | Inline::Decorated { children, .. }
            | Inline::MarginNote { children } => walk_inlines(children, visit),
            Inline::Figure(figure) => {
                if let Some(caption) = &figure.caption {
                    walk_inlines(caption, visit);
                }
            }
            _ => {}
        }
    }
}

/// The characters a document may print: its text, its remark numbers and note labels, what
/// its title page and colophon say of it, the words of the style that are printed, and the
/// addresses of its links when they are printed as notes. The fallback families a font list
/// names are chosen for these.
fn printed_chars(docs: &[&Doc], volume: Option<&Meta>, view: &View<'_>) -> BTreeSet<char> {
    let mut chars = BTreeSet::new();
    let mut add = |text: &str| chars.extend(text.chars());
    let addresses = matches!(view.str("links.mode").as_str(), "footnote" | "endnote");
    let metas = docs.iter().map(|doc| &doc.meta).chain(volume);
    for meta in metas {
        for text in [
            &meta.title,
            &meta.byline,
            &meta.dateline,
            &meta.credit,
            &meta.origin.publisher,
            &meta.origin.edition_label,
            &meta.origin.url,
            &meta.rights.statement,
            &meta.rights.terms,
        ] {
            add(text);
        }
        for notice in &meta.rights.notices {
            add(&notice.text);
            if let Some(link) = &notice.link {
                add(&link.label);
            }
        }
    }
    add(&view.str("toc.title"));
    for area in ["header", "footer"] {
        for slot in ["left", "center", "right"] {
            add(&view.str(&format!("pageFurniture.{area}.{slot}")));
        }
    }
    for doc in docs {
        fn toc(items: &[TocItem], add: &mut impl FnMut(&str)) {
            for item in items {
                add(&item.text());
                toc(&item.children, add);
            }
        }
        toc(&doc.toc, &mut add);
        for attachment in &doc.attachments {
            add(&attachment.title);
        }
        walk_doc_blocks(doc, &mut |block| {
            if let Block::Remark { number, .. } = block {
                add(number);
            }
        });
        walk_doc(doc, &mut |node| match node {
            Inline::Text { value } => add(value),
            Inline::Math { tex, .. } => add(tex),
            Inline::NoteRef(note) => add(&note.label),
            Inline::Link {
                href,
                internal: false,
                ..
            } if addresses => add(href),
            _ => {}
        });
    }
    chars
}

/// Every block of `doc`, the blocks inside others too, wherever the document holds them.
fn walk_doc_blocks(doc: &Doc, visit: &mut impl FnMut(&Block)) {
    fn walk(blocks: &[Block], visit: &mut impl FnMut(&Block)) {
        for block in blocks {
            visit(block);
            match block {
                Block::List(list) => {
                    for item in &list.items {
                        walk(item, visit);
                    }
                }
                Block::Blockquote { children }
                | Block::Remark { children, .. }
                | Block::Aside { children }
                | Block::Lang { children, .. } => walk(children, visit),
                Block::Columns { columns } => {
                    for column in columns {
                        walk(column, visit);
                    }
                }
                _ => {}
            }
        }
    }
    walk(&doc.front, visit);
    walk(&doc.body, visit);
    for section in &doc.back {
        walk(&section.blocks, visit);
    }
    for attachment in &doc.attachments {
        walk(&attachment.blocks, visit);
        for note in attachment.notes.iter().flat_map(|n| n.values()) {
            walk(note, visit);
        }
    }
    for note in doc.notes.values() {
        walk(note, visit);
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
            | Inline::Link { children, .. }
            | Inline::Spaced { children }
            | Inline::SmallCaps { children }
            | Inline::Decorated { children, .. }
            | Inline::MarginNote { children } => out.push_str(&plain_text(children)),
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
            Inline::Spaced { children } => out.push(Inline::Spaced {
                children: hoist(children, ids),
            }),
            Inline::SmallCaps { children } => out.push(Inline::SmallCaps {
                children: hoist(children, ids),
            }),
            Inline::Decorated {
                line,
                dashed,
                children,
            } => out.push(Inline::Decorated {
                line: *line,
                dashed: *dashed,
                children: hoist(children, ids),
            }),
            Inline::MarginNote { children } => out.push(Inline::MarginNote {
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

fn in_region<T>(ctx: &mut Ctx<'_>, region: &str, f: impl FnOnce(&mut Ctx<'_>) -> T) -> T {
    let previous = std::mem::replace(&mut ctx.region, region.to_owned());
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
            outline_only,
        } => {
            let markup = heading(*level, id.as_deref(), children, ctx);
            if !outline_only {
                return Some(markup);
            }
            ctx.helpers.insert(Helper::OutlineOnly);
            Some(format!("#fs-outline-only[{markup}]"))
        }
        Block::Para {
            children,
            align,
            indent_em,
        } => para(children, *align, *indent_em, String::new(), ctx),
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
        Block::Remark {
            number,
            id,
            children,
        } => remark(number, id.as_deref(), children, ctx),
        Block::Aside { children } => {
            let body = joined(children, ctx);
            if body.trim().is_empty() {
                return None;
            }
            ctx.helpers.insert(Helper::Aside);
            Some(format!("#fs-aside[\n{}\n]", guard_line_start(&body)))
        }
        Block::Columns { columns } => {
            if columns.is_empty() {
                return None;
            }
            ctx.helpers.insert(Helper::Columns);
            let columns: Vec<String> = columns
                .iter()
                .map(|blocks| content(&joined(blocks, ctx)))
                .collect();
            Some(format!("#fs-columns({})", columns.join(", ")))
        }
        Block::Verse { lines } => verse(lines, ctx),
        Block::Lang {
            lang,
            dir,
            children,
        } => lang_scope(lang, *dir, children, ctx),
    }
}

/// Blocks in a language of their own: set in it, and in its direction when it says, with the
/// hyphenation of the language, or none, which is said once, when Typst has no patterns for
/// it. A tag that is not a language tag is said too, and its blocks keep the language around
/// them, as they do without the word when there is no tag at all; no tag reaches the source
/// unless it is one.
fn lang_scope(
    lang: &str,
    dir: Option<Dir>,
    children: &[Block],
    ctx: &mut Ctx<'_>,
) -> Option<String> {
    let body = joined(children, ctx);
    if body.trim().is_empty() {
        return None;
    }
    Some(in_scope(&lang_args(lang, dir, ctx), body))
}

/// What `text()` is set to in a language scope of `lang` and `dir`; nothing when they leave
/// the text as it is around them.
fn lang_args(lang: &str, dir: Option<Dir>, ctx: &mut Ctx<'_>) -> Vec<String> {
    let mut args = Vec::new();
    match crate::lang::typst_lang(lang) {
        Some(TypstLang { lang, region }) => {
            args.push(format!("lang: \"{lang}\""));
            if let Some(region) = region {
                args.push(format!("region: \"{region}\""));
            }
            if ctx.view.bool("typography.hyphenate") && !crate::lang::has_hyphenation(&lang) {
                ctx.warn(format!(
                    "Typst has no hyphenation patterns for {lang}, so text in it is not hyphenated"
                ));
                args.push("hyphenate: false".to_owned());
            }
        }
        None if lang.trim().is_empty() => {}
        None => ctx.warn(format!(
            "a part of the text names the language {}, which is no language tag; it is set in the language around it",
            escape_string(lang)
        )),
    }
    match dir {
        Some(Dir::Rtl) => args.push("dir: rtl".to_owned()),
        Some(Dir::Ltr) => args.push("dir: ltr".to_owned()),
        None => {}
    }
    args
}

/// `body` in a scope that sets `args` of `text()`, or as it is when there are none.
fn in_scope(args: &[String], body: String) -> String {
    if args.is_empty() {
        return body;
    }
    format!(
        "#[\n#set text({})\n\n{}\n]",
        args.join(", "),
        guard_line_start(&body)
    )
}

/// A paragraph, after `lead` (markup that opens it, as a remark's number) when there is one.
/// An indent sets every line in, the first as well.
fn para(
    children: &[Inline],
    align: Option<Align>,
    indent_em: u8,
    lead: String,
    ctx: &mut Ctx<'_>,
) -> Option<String> {
    let text = inlines(children, ctx);
    let text = match (lead.is_empty(), text.trim().is_empty()) {
        (true, _) => text,
        (false, true) => lead,
        (false, false) => format!("{lead} {text}"),
    };
    if text.trim().is_empty() {
        return None;
    }
    let body = if indent_em > 0 {
        format!(
            "#par(first-line-indent: (amount: {indent_em}em, all: true), hanging-indent: {indent_em}em){}",
            content(&text)
        )
    } else {
        text
    };
    Some(match align {
        Some(Align::Center) => format!("#align(center){}", content(&body)),
        Some(Align::Right) => format!("#align(right){}", content(&body)),
        None => guard_line_start(&body),
    })
}

/// A remark: its number in bold, then a space, at the start of its first paragraph, and the
/// place its id names right before the number. A remark that does not open with a paragraph
/// has its number on a line of its own.
fn remark(number: &str, id: Option<&str>, children: &[Block], ctx: &mut Ctx<'_>) -> Option<String> {
    let mut lead = id.map(|id| ctx.anchor(id)).unwrap_or_default();
    if !number.is_empty() {
        ctx.helpers.insert(Helper::RemarkNumber);
        lead.push_str(&format!(
            "#fs-remark-number{}",
            content(&escape_text(number))
        ));
    }
    let mut out = Vec::new();
    let mut rest = children;
    if let Some((
        Block::Para {
            children,
            align,
            indent_em,
        },
        after,
    )) = children.split_first()
    {
        out.extend(para(children, *align, *indent_em, lead, ctx));
        rest = after;
    } else if !lead.is_empty() {
        out.push(guard_line_start(&lead));
    }
    out.extend(render_blocks(rest, ctx));
    if out.is_empty() {
        return None;
    }
    // The space that sets it apart from what comes before: the paragraph spacing and as much
    // again as the style asks. Weak spacing takes the place of the paragraph spacing next to
    // it, so it is the two together, and a remark at the top of a page or column has none.
    let space = ctx.view.pt("blocks.remark.spaceBefore");
    if space > 0.0 {
        let skip = ctx.view.pt("typography.parSkip");
        out.insert(
            0,
            format!(
                "#v({}, weak: true)",
                crate::preamble::gap(ctx.view, skip + space)
            ),
        );
    }
    Some(out.join("\n\n"))
}

/// Verse, a line to a block, each set in by its indent; a line that runs over hangs.
fn verse(lines: &[VerseLine], ctx: &mut Ctx<'_>) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    ctx.helpers.insert(Helper::Verse);
    let lines: Vec<String> = lines
        .iter()
        .map(|line| {
            let text = inlines(&line.children, ctx);
            let text = if text.trim().is_empty() {
                "~".to_owned()
            } else {
                text
            };
            format!("({}, {})", line.indent_em, content(&text))
        })
        .collect();
    Some(format!("#fs-verse({})", lines.join(", ")))
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
        let least = match list.numbering {
            None | Some(ListNumbering::Decimal) => 0,
            Some(_) => 1,
        };
        args.push(format!("start: {}", start.max(least)));
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
            if table.rules == Some(Rules::Cells)
                && let Some(stroke) = cell_stroke(cell)
            {
                args.push(stroke);
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
    if table.rules.is_some() {
        ctx.helpers.insert(Helper::PlainTable);
        return Some(format!(
            "#fs-table-plain({caption}columns: {}, {})",
            kept.len(),
            emitted.join(", ")
        ));
    }
    Some(format!(
        "#fs-table({caption}columns: {}, rows: {}, header-rows: {header_rows}, {})",
        kept.len(),
        grid.len(),
        emitted.join(", ")
    ))
}

/// The rules a cell draws on its own sides, in a table whose rules come from its cells.
fn cell_stroke(cell: &TableCell) -> Option<String> {
    let edges = cell.edges?;
    let sides: Vec<String> = [
        ("top", edges.top),
        ("right", edges.right),
        ("bottom", edges.bottom),
        ("left", edges.left),
    ]
    .into_iter()
    .filter(|(_, ruled)| *ruled)
    .map(|(side, _)| format!("{side}: 0.4pt"))
    .collect();
    (!sides.is_empty()).then(|| format!("stroke: ({})", sides.join(", ")))
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
    // The width the source shows it at, a CSS pixel being 0.75 of Typst's points (not TeX's,
    // which `typst_pt` converts from); the helpers keep it to the width there is.
    let width = figure.width_px.map(|px| {
        ctx.helpers.insert(Helper::SizedFigure);
        format!("{}pt", typesep_style::units::fmt_pt(f64::from(px) * 0.75))
    });
    if figure.inline == Some(true) {
        return Some(match width {
            Some(width) => format!(
                "{anchor}#fs-figure-inline-at({}, {width})",
                escape_string(&path)
            ),
            None => format!("{anchor}#fs-figure-inline({})", escape_string(&path)),
        });
    }
    let caption = caption
        .map(|c| format!(", caption: {}", content(&c)))
        .unwrap_or_default();
    Some(match width {
        Some(width) => format!(
            "{anchor}#fs-figure-at({}, {width}{caption})",
            escape_string(&path)
        ),
        None => format!("{anchor}#fs-figure({}{caption})", escape_string(&path)),
    })
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
        .map(|(i, node)| match node {
            Inline::MarginNote { children } => {
                let space = match nodes.get(i + 1) {
                    Some(Inline::Text { value }) => value.starts_with(collapsible),
                    _ => false,
                };
                margin_note(children, space, ctx)
            }
            Inline::Text { value }
                if !ctx.pre && i > 0 && matches!(nodes[i - 1], Inline::MarginNote { .. }) =>
            {
                escape_text(value.trim_start_matches(collapsible))
            }
            _ => inline_node(node, ctx, i == last),
        })
        .collect();
    join_inline(pieces.into_iter())
}

/// Whitespace that the text collapses into one space, which a no-break space is not.
fn collapsible(c: char) -> bool {
    c.is_whitespace() && c != '\u{a0}' && c != '\u{202f}'
}

/// A note in the margin takes no room in its line, so the space that follows it in the text
/// is the note's to give back when the margin is too narrow and it is set in the line.
fn margin_note(children: &[Inline], space: bool, ctx: &mut Ctx<'_>) -> String {
    let text = inlines(children, ctx);
    if text.trim().is_empty() {
        return String::new();
    }
    ctx.helpers.insert(Helper::MarginNote);
    let space = if space { "(space: true)" } else { "" };
    format!("#fs-margin-note{space}{}", content(&text))
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
        Inline::Math { mode, tex } => math(tex, *mode == typesep_model::MathMode::Display, ctx),
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
        Inline::Spaced { children } => {
            ctx.helpers.insert(Helper::Spaced);
            format!("#fs-spaced{}", content(&inlines(children, ctx)))
        }
        Inline::SmallCaps { children } => {
            format!("#smallcaps{}", content(&inlines(children, ctx)))
        }
        Inline::Decorated {
            line,
            dashed,
            children,
        } => {
            let function = match line {
                Decoration::Under => "underline",
                Decoration::Over => "overline",
                Decoration::Strike => "strike",
            };
            let stroke = if *dashed {
                "(stroke: (dash: \"dashed\"))"
            } else {
                ""
            };
            format!("#{function}{stroke}{}", content(&inlines(children, ctx)))
        }
        Inline::MarginNote { children } => margin_note(children, false, ctx),
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

    if ctx.own_notes.contains_key(&node.id) {
        if !ctx.own_refs.iter().any(|(id, _)| *id == node.id) {
            ctx.own_refs.push((node.id.clone(), node.label.clone()));
        }
        let target = ctx.label_for(&node.id, None);
        if ctx.resolves(&target) {
            return format!("{anchor}#super[\\[#link(<{target}>){}\\]]", content(&label));
        }
        return format!("{anchor}#super[\\[{label}\\]]");
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
        && (!ctx.links_sections.contains(&ctx.region) || ctx.in_note)
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

/// The notes the document holds itself, set as a section of their own when notes are an
/// appendix: in the order the text first refers to them, each under the label it was
/// referred to by, and then any the text never refers to.
fn own_notes_markup(ctx: &mut Ctx<'_>) -> Option<String> {
    if ctx.own_notes.is_empty() {
        return None;
    }
    let mut lines = vec!["#heading(level: 2)[Notes]".to_owned()];
    let mut done: HashSet<String> = HashSet::new();
    let mut next = 0;
    loop {
        let (id, label) = match ctx.own_refs.get(next) {
            Some(found) => found.clone(),
            None => match ctx.own_notes.keys().find(|id| !done.contains(*id)) {
                Some(id) => (id.clone(), String::new()),
                None => break,
            },
        };
        next += 1;
        if !done.insert(id.clone()) {
            continue;
        }
        let blocks = ctx.own_notes[&id].clone();
        let body = joined(&blocks, ctx);
        let target = ctx.label_for(&id, None);
        let anchor = if ctx.seen.insert(target.clone()) {
            format!("#metadata(none)<{target}>\n")
        } else {
            String::new()
        };
        lines.push(format!(
            "{anchor}#fs-endnote({}, {})",
            content(&escape_text(&label)),
            content(&body)
        ));
    }
    Some(lines.join("\n\n"))
}

fn table_of_contents(toc: &[TocItem], back: &[Section], ctx: &mut Ctx<'_>) -> Option<String> {
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

    let dropped: HashSet<String> = back
        .iter()
        .filter(|section| !ctx.sections.contains(&section.key))
        .map(|section| section.label.to_lowercase())
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
        .filter(|item| !dropped.contains(&item.text().trim().to_lowercase()))
        .collect();
    if kept.is_empty() {
        return None;
    }
    let entries: Vec<String> = kept
        .iter()
        .map(|item| {
            let label = escape_text(&item.text());
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

/// Where the entry came from and who holds it: the publisher and edition, the date it was
/// retrieved and the rights statement. Both the title page and the colophon open with these.
fn provenance(meta: &Meta, retrieved: &str) -> Vec<String> {
    let source = [&meta.origin.publisher, &meta.origin.edition_label]
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| escape_text(part))
        .collect::<Vec<_>>()
        .join(", ");
    [
        source,
        if retrieved.is_empty() {
            String::new()
        } else {
            format!("Retrieved {}", escape_text(retrieved))
        },
        escape_text(&meta.rights.statement),
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
fn colophon(meta: &Meta, retrieved: &str) -> String {
    let mut parts = provenance(meta, retrieved);
    if !meta.rights.terms.is_empty() {
        parts.push(escape_text(&meta.rights.terms));
    }
    for notice in &meta.rights.notices {
        parts.push(match &notice.link {
            Some(link) => format!(
                "{} #link({})[{}]",
                escape_text(&notice.text),
                escape_string(&link.url),
                crate::escape::url_for_display(&link.label)
            ),
            None => escape_text(&notice.text),
        });
    }
    format!(
        "#page(header: none, footer: none, columns: 1, numbering: none)[\n  #v(1fr)\n  {}\n]",
        note_block(&parts)
    )
}

fn simple_cover(meta: &Meta, retrieved: &str, ctx: &mut Ctx<'_>) -> String {
    let mut lines: Vec<String> = vec!["  #set align(center)".to_owned(), "  #v(18%)".to_owned()];
    macro_rules! add {
        ($markup:expr) => {{
            lines.push(format!("  {}", $markup));
            lines.push("  #v(1.2em)".to_owned());
        }};
    }

    for field in ctx.view.list("cover.fields") {
        match field.as_str() {
            "title" => add!(format!(
                "#text(size: 20pt){}",
                content(&escape_text(&meta.title))
            )),
            "author" => {
                let credit = if meta.byline.is_empty() {
                    &meta.credit
                } else {
                    &meta.byline
                };
                if !credit.is_empty() {
                    add!(format!(
                        "#text(size: 1.2em){}",
                        content(&escape_text(credit))
                    ));
                }
            }
            "dates" => {
                if !meta.dateline.is_empty() {
                    add!(format!("#emph{}", content(&escape_text(&meta.dateline))));
                }
            }
            "sourceUrl" => {
                let url = &meta.origin.url;
                if !url.is_empty() {
                    add!(format!(
                        "#text(size: 0.9em)[#link({})[#fs-code{}]]",
                        escape_string(url),
                        content(&crate::escape::url_for_display(url))
                    ));
                }
            }
            "copyright" => {
                lines.push("  #v(1fr)".to_owned());
                lines.push(format!("  {}", note_block(&provenance(meta, retrieved))));
            }
            _ => {}
        }
    }
    format!(
        "#page(header: none, footer: none, columns: 1, numbering: none)[\n{}\n]",
        lines.join("\n")
    )
}

fn entry_body(doc: &Doc, ctx: &mut Ctx<'_>) -> Vec<String> {
    ctx.doc_id = doc.meta.id.clone();
    ctx.math = Converter::new(&doc.meta.macros);
    let attached_notes: NoteTable = doc
        .attachments
        .iter()
        .filter(|a| a.kind == AttachmentKind::Notes)
        .filter_map(|a| a.notes.as_ref())
        .flat_map(|notes| notes.iter().map(|(k, v)| (k.clone(), v.clone())))
        .collect();
    ctx.own_notes = doc
        .notes
        .iter()
        .filter(|(id, _)| !attached_notes.contains_key(*id))
        .map(|(id, blocks)| (id.clone(), blocks.clone()))
        .collect();
    ctx.own_refs.clear();
    ctx.notes = ctx.own_notes.clone();
    ctx.notes.extend(attached_notes);
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
    ctx.self_urls = if doc.meta.origin.url.is_empty() {
        HashSet::new()
    } else {
        let base = doc.meta.origin.url.clone();
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

    let mut out = Vec::new();
    match &ctx.layout {
        Layout::Flow => {}
        Layout::Measure => out.push(format!("#metadata({})<{MEASURE_TOP}>", side_key(0, 0))),
        Layout::Facing(_) => out.push(format!(
            "#pagebreak(to: \"even\", weak: true)\n#metadata({})<{SPREAD_START}>",
            side_key(0, 0)
        )),
    }
    out.push(format!(
        "#heading(level: 1){}",
        content(&escape_text(&doc.meta.title))
    ));
    if notes_inline && ctx.note_running > 0 {
        out.push(format!("#counter(footnote).update({})", ctx.note_running));
    }

    if !doc.meta.dateline.is_empty() {
        out.push(format!(
            "#block(below: 0.8em)[#emph{}]",
            content(&escape_text(&doc.meta.dateline))
        ));
    }

    out.extend(in_region(ctx, "preamble", |ctx| {
        render_blocks(&doc.front, ctx)
    }));

    if let Some(toc) = table_of_contents(&doc.toc, &doc.back, ctx) {
        out.push(toc);
    }

    out.extend(in_region(ctx, "body", |ctx| match ctx.layout {
        Layout::Flow => render_blocks(&doc.body, ctx),
        _ => facing_body(&doc.body, ctx),
    }));

    let notes_attached: Vec<&Attachment> = doc
        .attachments
        .iter()
        .filter(|a| a.kind == AttachmentKind::Notes)
        .collect();
    if ctx.notes_style == "appendix" && ctx.sections.contains("notes") {
        for extra in notes_attached {
            let extra = extra.clone();
            out.extend(in_region(ctx, "notes", |ctx| {
                attachment_markup(&extra, ctx)
            }));
        }
        out.extend(in_region(ctx, "notes", own_notes_markup));
    }

    let sections = ctx.sections;
    for section in doc.back.iter().filter(|s| sections.contains(&s.key)) {
        out.extend(in_region(ctx, &section.key, |ctx| {
            render_blocks(&section.blocks, ctx)
        }));
    }

    if ctx.sections.contains("supplements") {
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

fn render_body(docs: &[&Doc], ctx: &mut Ctx<'_>, opts: &EmitOptions<'_>) -> String {
    let view = ctx.view;
    let metas: Vec<&Meta> = docs.iter().map(|d| &d.meta).collect();
    let meta = opts.volume.unwrap_or(&docs[0].meta);
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
        out.push(simple_cover(meta, &retrieved, ctx));
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

    out.push(colophon(meta, &retrieved));
    out.join("\n\n")
}

pub fn emit_document(docs: &[&Doc], style: &Style, opts: EmitOptions<'_>) -> Rendered {
    assert!(!docs.is_empty(), "a group holds at least one entry");
    if let ([doc], Some(fonts)) = (docs, opts.fonts)
        && opts.parallel
        && View::new(style).str("parallel.layout") == "facing"
    {
        return crate::facing::emit(doc, style, opts, fonts);
    }
    emit_with(docs, style, opts, &Layout::Flow)
}

/// The Typst source of `docs` with their body laid out as `layout` says.
pub(crate) fn emit_with(
    docs: &[&Doc],
    style: &Style,
    opts: EmitOptions<'_>,
    layout: &Layout,
) -> Rendered {
    let view = View::new(style);
    let merged = docs.len() > 1;

    let mut first = Ctx::new(&view, &opts, merged);
    first.layout = layout.clone();
    let _ = render_body(docs, &mut first, &opts);
    let known = first.seen;

    let mut ctx = Ctx::new(&view, &opts, merged);
    ctx.known = known;
    ctx.layout = layout.clone();
    let body = render_body(docs, &mut ctx, &opts);

    let metas: Vec<&Meta> = docs.iter().map(|d| &d.meta).collect();
    let meta = opts.volume.unwrap_or(&docs[0].meta);
    let input = PreambleInput {
        title: meta.title.clone(),
        byline: meta.byline.clone(),
        edition: meta.origin.edition_label.clone(),
        date: retrieval_dates(&metas),
        subject: meta.origin.publisher.clone(),
        lang: meta.lang.clone(),
        dir: meta.dir,
        helpers: std::mem::take(&mut ctx.helpers),
        chars: printed_chars(docs, opts.volume, &view),
    };
    let (preamble, mut warnings) = build_preamble(&view, &input, opts.fonts);
    warnings.extend(ctx.warnings);

    let marks = match layout {
        Layout::Flow => String::new(),
        _ => FACING_MARKS.to_owned(),
    };
    let mut source = String::with_capacity(preamble.len() + marks.len() + body.len() + 64);
    let _ = write!(source, "{preamble}{marks}\n{body}\n");
    Rendered {
        source,
        figures: ctx.figures,
        warnings,
        formulas: ctx.formulas,
    }
}

/// The labels of the marks a bilingual edition's facing pages are measured and checked by:
/// the top of a side of a row on its page of its own, the place before each piece of it and
/// after the last; the start and end of a side of a spread; each footnote, where it is set and
/// where it is called; and the footnote area's spacing.
pub(crate) const MEASURE_TOP: &str = "fs-measure-top";
pub(crate) const MEASURE_PIECE: &str = "fs-measure-piece";
pub(crate) const MEASURE_NOTES: &str = "fs-measure-notes";
pub(crate) const SPREAD_START: &str = "fs-spread";
pub(crate) const SPREAD_END: &str = "fs-spread-end";
pub(crate) const FOOTNOTE_AT: &str = "fs-footnote-at";

/// What a document laid out as facing pages adds to the preamble: a mark at each footnote,
/// which says where the note is set and where it is called from, and the spacing of the
/// footnote area, which the plan of the spreads counts with.
const FACING_MARKS: &str = "// ---- facing pages ----\n\
#show footnote.entry: it => [#context [#metadata(it.note.location().position())<fs-footnote-at>]#it]\n\
#context [#metadata((clearance: footnote.entry.clearance.to-absolute(), gap: footnote.entry.gap.to-absolute(), separator: measure(footnote.entry.separator).height))<fs-measure-notes>]\n";

/// The key of a side of a row or of a spread in the marks: its index and the side's.
pub(crate) fn side_key(index: usize, side: usize) -> String {
    format!("(index: {index}, side: {side})")
}

/// What a side of a row of a bilingual edition is cut into where a page may end between them:
/// each of its blocks, and each block of a remark in it, the first set with the remark's
/// number.
#[derive(Debug)]
pub(crate) enum Piece<'b> {
    Block(&'b Block),
    Remark {
        number: &'b str,
        id: Option<&'b str>,
        first: &'b [Block],
    },
}

impl Piece<'_> {
    /// The number of the remark the piece opens, if it opens one.
    pub(crate) fn number(&self) -> Option<&str> {
        match self {
            Piece::Remark { number, .. } => Some(number),
            Piece::Block(_) => None,
        }
    }
}

/// A side of a row: the language scope its blocks are in, if any, and its pieces.
pub(crate) struct Side<'b> {
    pub(crate) scope: Option<(&'b str, Option<Dir>)>,
    pub(crate) pieces: Vec<Piece<'b>>,
}

/// Side `side` (0 the left, 1 the right) of `row`: a column of the row of two the parallel
/// pass makes, or, of any other block, the block on the left and nothing on the right.
pub(crate) fn side_of(row: &Block, side: usize) -> Side<'_> {
    let blocks: &[Block] = match row {
        Block::Columns { columns } if columns.len() == 2 => &columns[side],
        other if side == 0 => std::slice::from_ref(other),
        _ => &[],
    };
    let (scope, blocks) = match blocks {
        [
            Block::Lang {
                lang,
                dir,
                children,
            },
        ] => (Some((lang.as_str(), *dir)), children.as_slice()),
        blocks => (None, blocks),
    };
    let mut pieces = Vec::new();
    for block in blocks {
        match block {
            Block::Remark {
                number,
                id,
                children,
            } => {
                let split = children.len().min(1);
                pieces.push(Piece::Remark {
                    number,
                    id: id.as_deref(),
                    first: &children[..split],
                });
                pieces.extend(children[split..].iter().map(Piece::Block));
            }
            other => pieces.push(Piece::Block(other)),
        }
    }
    Side { scope, pieces }
}

/// The pieces `range` of `side` in its language scope, with `mark(k)` before the `k`th piece
/// and after the last one.
fn side_markup(
    side: &Side<'_>,
    range: std::ops::Range<usize>,
    ctx: &mut Ctx<'_>,
    mark: &dyn Fn(usize) -> Option<String>,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for k in range.clone() {
        parts.extend(mark(k));
        let markup = match &side.pieces[k] {
            Piece::Block(node) => block(node, ctx),
            Piece::Remark { number, id, first } => remark(number, *id, first, ctx),
        };
        parts.extend(markup);
    }
    parts.extend(mark(range.end));
    if parts.is_empty() {
        return None;
    }
    let args = match side.scope {
        Some((lang, dir)) => lang_args(lang, dir, ctx),
        None => Vec::new(),
    };
    Some(in_scope(&args, parts.join("\n\n")))
}

/// The rows of a bilingual edition laid out to be measured, each side of each on a page of its
/// own, or as the facing pages of the plan made from those measures: the left side of a
/// spread on a left-hand page, an even one, and its right side on the page opposite.
fn facing_body(rows: &[Block], ctx: &mut Ctx<'_>) -> Vec<String> {
    let mut out = Vec::new();
    match ctx.layout.clone() {
        Layout::Flow => {}
        Layout::Measure => {
            for (index, row) in rows.iter().enumerate() {
                for side in 0..2 {
                    if (index, side) != (0, 0) {
                        out.push(format!(
                            "#pagebreak(weak: true)\n#metadata({})<{MEASURE_TOP}>",
                            side_key(index, side)
                        ));
                    }
                    let mark = |piece: usize| {
                        Some(format!(
                            "#metadata((index: {index}, side: {side}, piece: {piece}))<{MEASURE_PIECE}>"
                        ))
                    };
                    let half = side_of(row, side);
                    let count = half.pieces.len();
                    out.extend(side_markup(&half, 0..count, ctx, &mark));
                }
            }
        }
        Layout::Facing(plan) => {
            for (index, spread) in plan.spreads.iter().enumerate() {
                for (side, parts) in spread.sides.iter().enumerate() {
                    if (index, side) != (0, 0) {
                        out.push(format!(
                            "#pagebreak(to: \"{}\")\n#metadata({})<{SPREAD_START}>",
                            if side == 0 { "even" } else { "odd" },
                            side_key(index, side)
                        ));
                    }
                    for part in parts {
                        let half = side_of(&rows[part.row], side);
                        out.extend(side_markup(&half, part.from..part.to, ctx, &|_| None));
                    }
                    out.push(format!(
                        "#metadata({})<{SPREAD_END}>",
                        side_key(index, side)
                    ));
                }
            }
            out.push("#pagebreak(weak: true)".to_owned());
        }
    }
    out
}

pub fn length_pt(view: &View<'_>, path: &str) -> String {
    typst_pt(view.pt(path))
}
