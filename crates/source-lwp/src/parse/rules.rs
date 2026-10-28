//! What the Wittgenstein Project's pages hold beyond standard HTML, as rules for the walker:
//! the page furniture left out, the remarks and their numbers, the headings its template sets
//! as paragraphs, Sperrsatz, formulas, citations, margin notes, boxed inserts, columns, verse,
//! and figures in their full size.

use std::collections::BTreeSet;

use scraper::{ElementRef, Node};
use typesep_html::css;
use typesep_html::dom::{self, Ref};
use typesep_html::js;
use typesep_html::walk::{Rules, Walker, is_blank, plain, trim_inline};
use typesep_model::{Block, Figure, Inline, MathMode, NoteRef, Rules as TableRules, VerseLine};

use crate::address::full_size;

/// The classes of what a page holds for the site and not for a reader: menus, the title
/// banner, the contents bar, the print-only title page (its lines are the metadata), the
/// colophon (the rights), the references list (the notes), edit links, and the marks only a
/// screen shows.
const FURNITURE: [&str; 14] = [
    "navbar",
    "noprint",
    "header-lines",
    "header-title",
    "top-arrow",
    "custom-print-only",
    "custom-toc",
    "custom-desktop-only",
    "colophon",
    "mw-references-wrap",
    "mw-editsection",
    "mw-cite-backlink",
    "toc",
    "mw-empty-elt",
];

/// The classes of the spans that set a manuscript siglum in the margin.
const SIGLA: [&str; 2] = ["bbb-aside-par", "frazer-aside-par"];

/// The rules of a page, and what they need to know of it: the byline, so the title page's
/// lines in the text are known, and the heading ids given so far.
pub struct LwpRules {
    byline: String,
    heading_ids: BTreeSet<String>,
    ebook_only: usize,
}

impl LwpRules {
    pub fn new(byline: &str) -> Self {
        Self {
            byline: byline.to_owned(),
            heading_ids: BTreeSet::new(),
            ebook_only: 0,
        }
    }

    /// What the rules left out that a reader should hear of.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        match self.ebook_only {
            0 => {}
            1 => out.push("content meant for ebooks only left out".to_owned()),
            n => out.push(format!("content meant for ebooks only left out ({n}×)")),
        }
        out
    }

    /// An id for a heading of `children`, from its words, as no other heading has it.
    fn heading_id(&mut self, children: &[Inline]) -> String {
        let words = js::collapse_spaces(js::trim(&plain(children))).replace(' ', "_");
        let base = if words.is_empty() {
            "heading".to_owned()
        } else {
            words
        };
        let mut id = base.clone();
        let mut n = 1;
        while !self.heading_ids.insert(id.clone()) {
            n += 1;
            id = format!("{base}_{n}");
        }
        id
    }

    fn heading(&mut self, children: Vec<Inline>, outline_only: bool) -> Vec<Block> {
        if is_blank(&children) {
            return Vec::new();
        }
        vec![Block::Heading {
            level: 2,
            id: Some(self.heading_id(&children)),
            children,
            outline_only,
        }]
    }

    /// Whether `el` is the byline line of the title page in the text, which a `p.tl-title`
    /// follows.
    fn is_byline_line(&self, el: &ElementRef) -> bool {
        dom::is(el, "p")
            && !self.byline.is_empty()
            && js::collapse_spaces(js::trim(&dom::text_of(**el))) == self.byline
            && dom::next_element(**el).is_some_and(|next| dom::has_class(&next, "tl-title"))
    }

    /// Whether anything a reader sees comes before `el` among its siblings.
    fn after_content(&self, el: ElementRef<'_>) -> bool {
        el.prev_siblings()
            .filter_map(ElementRef::wrap)
            .any(|sibling| {
                !is_furniture(&sibling)
                    && !self.is_byline_line(&sibling)
                    && !js::is_blank(&dom::text_of(*sibling))
            })
    }

    /// A remark: a paragraph that opens with its number, `<b><span id="N">N</span></b>`, and
    /// the paragraph without it.
    fn remark(&mut self, walker: &mut Walker, el: ElementRef<'_>) -> Option<Vec<Block>> {
        let mut nodes = el.children();
        let opening = nodes.by_ref().find(|node| !is_spacing(*node))?;
        let bold = ElementRef::wrap(opening).filter(|b| dom::is(b, "b") || dom::is(b, "strong"))?;
        let mut inside = dom::child_elements(*bold);
        let span = inside.next().filter(|span| dom::is(span, "span"))?;
        let id = dom::truthy(span.attr("id"))?;
        let number = js::collapse_spaces(js::trim(&dom::text_of(*span)));
        let loose_text = bold
            .children()
            .filter_map(|c| c.value().as_text())
            .any(|t| !js::is_blank(t));
        if inside.next().is_some() || loose_text || number.is_empty() {
            return None;
        }
        let children = trim_inline(walker.inlines(self, nodes));
        let first = walker.paragraph_with(el, children);
        Some(vec![Block::Remark {
            number,
            id: Some(id.to_owned()),
            children: first.into_iter().collect(),
        }])
    }

    /// Verse: a paragraph whose lines, between line breaks, are set in by an inline block.
    fn verse(&mut self, walker: &mut Walker, el: ElementRef<'_>) -> Option<Vec<Block>> {
        if !el.children().any(|node| verse_indent(node).is_some()) {
            return None;
        }
        let mut lines: Vec<Vec<Ref>> = vec![Vec::new()];
        for node in el.children() {
            if ElementRef::wrap(node).is_some_and(|br| dom::is(&br, "br")) {
                lines.push(Vec::new());
            } else {
                lines.last_mut().expect("a line").push(node);
            }
        }
        let mut out: Vec<VerseLine> = Vec::new();
        for line in lines {
            let shown: Vec<Ref> = line
                .iter()
                .copied()
                .filter(|node| !node.value().as_text().is_some_and(|t| js::is_blank(t)))
                .collect();
            let (indent_em, children) = match shown.as_slice() {
                [only] if verse_indent(*only).is_some() => (
                    verse_indent(*only).unwrap_or(0),
                    walker.inlines(self, only.children()),
                ),
                _ => (0, walker.inlines(self, line)),
            };
            out.push(VerseLine {
                indent_em,
                children: trim_inline(children),
            });
        }
        while out.first().is_some_and(|line| is_blank(&line.children)) {
            out.remove(0);
        }
        while out.last().is_some_and(|line| is_blank(&line.children)) {
            out.pop();
        }
        Some(vec![Block::Verse { lines: out }])
    }

    /// Blocks set side by side, one column to each element the container holds.
    fn columns(&mut self, walker: &mut Walker, el: ElementRef<'_>) -> Vec<Block> {
        let mut columns: Vec<Vec<Block>> = Vec::new();
        for node in el.children() {
            let column = match node.value() {
                Node::Element(_) => walker.blocks(self, std::iter::once(node)),
                Node::Text(text) if !js::is_blank(text) => {
                    walker.blocks(self, std::iter::once(node))
                }
                _ => continue,
            };
            if !column.is_empty() {
                columns.push(column);
            }
        }
        match columns.len() {
            0 => Vec::new(),
            1 => columns.remove(0),
            _ => vec![Block::Columns { columns }],
        }
    }

    /// A figure printed in black and white, `div.noebook`, with the colour copy an ebook shows
    /// instead when an `div.ebookonly` follows it.
    fn print_figure(&mut self, walker: &mut Walker, el: ElementRef<'_>) -> Option<Vec<Block>> {
        let holder = dom::descendants_named(*el, &["figure"]).next()?;
        let mut figure = full_sized(walker.figure(self, holder)?);
        if let Some(colour) =
            dom::next_element(*el).filter(|next| dom::has_class(next, "ebookonly"))
            && let Some(img) = dom::descendants_named(*colour, &["img"]).next()
            && let Some(copy) = walker.image(img)
        {
            figure.ebook_src = Some(full_size(&copy.src));
        }
        Some(vec![Block::Figure(figure)])
    }

    /// Whether an `ebookonly` element is the colour copy of the figure before it, which
    /// `print_figure` has read.
    fn is_colour_copy(el: &ElementRef) -> bool {
        js::is_blank(&dom::text_of(**el))
            && dom::descendants_named(**el, &["img"]).next().is_some()
            && dom::prev_element(**el).is_some_and(|before| {
                dom::has_class(&before, "noebook")
                    && dom::descendants_named(*before, &["figure"])
                        .next()
                        .is_some()
            })
    }
}

/// Whether `el` is the site's and not the text's, or for ebooks only.
fn is_furniture(el: &ElementRef) -> bool {
    dom::has_class(el, "ebookonly") || FURNITURE.iter().any(|class| dom::has_class(el, class))
}

fn full_sized(figure: Figure) -> Figure {
    Figure {
        src: full_size(&figure.src),
        ..figure
    }
}

/// Whether `node` only spaces a paragraph out: white space or a line break.
fn is_spacing(node: Ref<'_>) -> bool {
    match node.value() {
        Node::Text(text) => js::is_blank(text),
        Node::Element(_) => ElementRef::wrap(node).is_some_and(|el| dom::is(&el, "br")),
        _ => true,
    }
}

/// How far a verse line `node` sets its line in: a span shown as an inline block with a left
/// margin in ems.
fn verse_indent(node: Ref<'_>) -> Option<u8> {
    let el = ElementRef::wrap(node).filter(|el| dom::is(el, "span"))?;
    let inline_block =
        css::get(&el, "display").is_some_and(|d| d.eq_ignore_ascii_case("inline-block"));
    let indent = css::get(&el, "margin-left").and_then(|m| css::ems(&m))?;
    inline_block.then(|| indent.round().clamp(0.0, 255.0) as u8)
}

/// Whether `el` is one of the headings the project's template sets as a paragraph in its own
/// font, shown (`display: block`) or for the contents only (`display: none`).
fn is_template_heading(el: &ElementRef) -> bool {
    let style = css::style_of(el);
    let has = |name: &str| style.iter().any(|(property, _)| property == name);
    has("display") && has("font-family")
}

/// A heading's words without the italics the template sets the whole of it in.
fn without_whole_emphasis(children: Vec<Inline>) -> Vec<Inline> {
    match <[Inline; 1]>::try_from(children) {
        Ok([Inline::Emph { children }]) => children,
        Ok([other]) => vec![other],
        Err(children) => children,
    }
}

impl Rules for LwpRules {
    fn skip(&mut self, el: ElementRef<'_>) -> bool {
        if dom::has_class(&el, "ebookonly") {
            if !Self::is_colour_copy(&el)
                && (!js::is_blank(&dom::text_of(*el))
                    || dom::descendants_named(*el, &["img"]).next().is_some())
            {
                self.ebook_only += 1;
            }
            return true;
        }
        is_furniture(&el)
    }

    fn block(&mut self, walker: &mut Walker, el: ElementRef<'_>) -> Option<Vec<Block>> {
        match dom::tag(&el).as_str() {
            "p" => {
                if is_template_heading(&el) {
                    let hidden =
                        css::get(&el, "display").is_some_and(|d| d.eq_ignore_ascii_case("none"));
                    let children = trim_inline(walker.inlines(self, el.children()));
                    return Some(self.heading(without_whole_emphasis(children), hidden));
                }
                if dom::has_class(&el, "tl-title") {
                    // The title page in the text: the cover has it, but a title further on,
                    // after a preface, opens the text itself.
                    if !self.after_content(el) {
                        return Some(Vec::new());
                    }
                    let children = trim_inline(walker.inlines(self, el.children()));
                    return Some(self.heading(children, false));
                }
                if self.is_byline_line(&el) {
                    return Some(Vec::new());
                }
                self.remark(walker, el).or_else(|| self.verse(walker, el))
            }
            "div" => {
                if css::style_of(&el)
                    .iter()
                    .any(|(property, value)| property == "border" && css::draws_border(value))
                {
                    return Some(vec![Block::Aside {
                        children: walker.blocks(self, el.children()),
                    }]);
                }
                let flex = css::get(&el, "display").is_some_and(|d| d.eq_ignore_ascii_case("flex"));
                if dom::has_class(&el, "tlp-flex-container") || flex {
                    return Some(self.columns(walker, el));
                }
                if dom::has_class(&el, "noebook") {
                    return self.print_figure(walker, el);
                }
                None
            }
            "table" => {
                let mut table = walker.table(self, el)?;
                if table.rules.is_none() && !dom::has_class(&el, "wikitable") {
                    table.rules = Some(TableRules::None);
                }
                Some(vec![Block::Table(table)])
            }
            "figure" => Some(
                walker
                    .figure(self, el)
                    .map(|figure| Block::Figure(full_sized(figure)))
                    .into_iter()
                    .collect(),
            ),
            _ => None,
        }
    }

    fn inline(&mut self, walker: &mut Walker, el: ElementRef<'_>) -> Option<Vec<Inline>> {
        match dom::tag(&el).as_str() {
            "span" if dom::has_class(&el, "smj-container") => {
                let source = dom::text_of(*el);
                let tex = source
                    .split_once("[math]")
                    .map_or(source.as_str(), |(_, rest)| rest);
                let tex = tex.split_once("[/math]").map_or(tex, |(tex, _)| tex);
                let tex = js::trim(tex);
                Some(if tex.is_empty() {
                    Vec::new()
                } else {
                    vec![Inline::Math {
                        mode: MathMode::Inline,
                        tex: tex.to_owned(),
                    }]
                })
            }
            "span" if dom::has_class(&el, "tl-check") => {
                walker.note("check marks (the project's notes on its own text) left out");
                Some(Vec::new())
            }
            "span" if SIGLA.iter().any(|class| dom::has_class(&el, class)) => {
                let mut out: Vec<Inline> = dom::truthy(el.attr("id"))
                    .map(|id| Inline::Anchor { id: id.to_owned() })
                    .into_iter()
                    .collect();
                out.push(Inline::MarginNote {
                    children: trim_inline(walker.inlines(self, el.children())),
                });
                Some(out)
            }
            "span" if css::get(&el, "float").is_some_and(|f| !f.eq_ignore_ascii_case("none")) => {
                Some(vec![Inline::MarginNote {
                    children: trim_inline(walker.inlines(self, el.children())),
                }])
            }
            "sup" if dom::has_class(&el, "reference") => {
                let a = dom::children_named(*el, &["a"]).next()?;
                let fragment = dom::truthy(a.attr("href"))?.strip_prefix('#')?;
                let label = js::trim(&dom::text_of(*a))
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .trim()
                    .to_owned();
                Some(vec![Inline::NoteRef(NoteRef {
                    id: fragment.to_owned(),
                    label,
                    href: format!("#{fragment}"),
                    ref_id: el.attr("id").map(str::to_owned),
                })])
            }
            "img" => Some(
                walker
                    .image(el)
                    .map(|figure| {
                        Inline::Figure(Figure {
                            inline: Some(true),
                            ..full_sized(figure)
                        })
                    })
                    .into_iter()
                    .collect(),
            ),
            _ => None,
        }
    }

    fn finish(&mut self, _walker: &mut Walker, blocks: Vec<Block>) -> Vec<Block> {
        fold_remarks(tidy_blocks(blocks))
    }
}

/// The blocks after the remark that opens them belong to it, up to the next remark, heading
/// or rule.
fn fold_remarks(blocks: Vec<Block>) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for block in blocks {
        match block {
            Block::Remark { .. } | Block::Heading { .. } | Block::Rule => out.push(block),
            other => match out.last_mut() {
                Some(Block::Remark { children, .. }) => children.push(other),
                _ => out.push(other),
            },
        }
    }
    out
}

/// Blocks with the spacing the site puts in paragraphs taken out: the line breaks at either
/// end of a paragraph, and paragraphs that held nothing else. Margin notes side by side are
/// one.
pub fn tidy_blocks(blocks: Vec<Block>) -> Vec<Block> {
    blocks.into_iter().filter_map(tidy_block).collect()
}

fn tidy_block(block: Block) -> Option<Block> {
    Some(match block {
        Block::Para {
            children,
            align,
            indent_em,
        } => {
            let children = tidy_inlines(children);
            if is_blank(&children) {
                return None;
            }
            Block::Para {
                children,
                align,
                indent_em,
            }
        }
        Block::Heading {
            level,
            id,
            children,
            outline_only,
        } => Block::Heading {
            level,
            id,
            children: tidy_inlines(children),
            outline_only,
        },
        Block::Remark {
            number,
            id,
            children,
        } => Block::Remark {
            number,
            id,
            children: tidy_blocks(children),
        },
        Block::Blockquote { children } => Block::Blockquote {
            children: tidy_blocks(children),
        },
        Block::Aside { children } => Block::Aside {
            children: tidy_blocks(children),
        },
        Block::Columns { columns } => Block::Columns {
            columns: columns.into_iter().map(tidy_blocks).collect(),
        },
        Block::List(mut list) => {
            list.items = list.items.into_iter().map(tidy_blocks).collect();
            Block::List(list)
        }
        Block::Table(mut table) => {
            for cell in table.rows.iter_mut().flatten() {
                cell.children = tidy_inlines(std::mem::take(&mut cell.children));
            }
            Block::Table(table)
        }
        Block::Verse { lines } => Block::Verse {
            lines: lines
                .into_iter()
                .map(|line| VerseLine {
                    indent_em: line.indent_em,
                    children: merge_margin_notes(line.children),
                })
                .collect(),
        },
        other => other,
    })
}

fn tidy_inlines(children: Vec<Inline>) -> Vec<Inline> {
    let mut children = merge_margin_notes(children);
    loop {
        let before = children.len();
        children = trim_inline(children);
        while matches!(children.first(), Some(Inline::Break)) {
            children.remove(0);
        }
        while matches!(children.last(), Some(Inline::Break)) {
            children.pop();
        }
        if children.len() == before {
            return children;
        }
    }
}

/// Margin notes with nothing but white space or anchors between them, merged into one, a
/// no-break space apart, so the margin keeps them on one line; the anchors go before it.
fn merge_margin_notes(children: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    // Where the last margin note is in `out`, and what has come since it that may be passed
    // over: white space and anchors.
    let mut open: Option<usize> = None;
    let mut between: Vec<Inline> = Vec::new();
    for node in children {
        match node {
            Inline::MarginNote { children } => {
                if let Some(at) = open {
                    let anchors: Vec<Inline> = between
                        .drain(..)
                        .filter(|n| matches!(n, Inline::Anchor { .. }))
                        .collect();
                    if let Inline::MarginNote { children: kept } = &mut out[at] {
                        kept.push(typesep_html::walk::text("\u{a0}"));
                        kept.extend(children);
                    }
                    let count = anchors.len();
                    for (i, anchor) in anchors.into_iter().enumerate() {
                        out.insert(at + i, anchor);
                    }
                    open = Some(at + count);
                } else {
                    out.push(Inline::MarginNote { children });
                    open = Some(out.len() - 1);
                }
            }
            Inline::Text { ref value } if open.is_some() && js::is_blank(value) => {
                between.push(node)
            }
            Inline::Anchor { .. } if open.is_some() => between.push(node),
            other => {
                out.append(&mut between);
                open = None;
                out.push(other);
            }
        }
    }
    out.append(&mut between);
    out
}
