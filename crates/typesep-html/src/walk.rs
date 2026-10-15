//! A reader of standard HTML into the document model: paragraphs, headings, lists, tables,
//! quotations, figures and rules, the inline markup of running text, and the inline styles
//! a browser would show (alignment, indents, letter spacing, small capitals, lines through
//! or under text). A source extends it with `Rules`: what it leaves out, what an element of
//! its own stands for, and what it does with the whole once read.

use scraper::{ElementRef, Node};
use typesep_model::{
    Align, Block, Decoration, Edges, Figure, Inline, List, ListNumbering, Rules as TableRules,
    Table, TableCell,
};
use url::Url;

use crate::css;
use crate::dom::{self, Ref};
use crate::js;

/// What a source adds to the standard reading. Each element is offered to `skip`, then to
/// `block` or `inline` as the place it is read in asks for blocks or for running text; what
/// they leave alone is read as standard HTML. A rule that reads what an element holds does
/// it through the walker, `walker.blocks(self, el.children())`, so nested elements meet the
/// rules too.
pub trait Rules {
    /// Whether `el` is left out, with all it holds.
    fn skip(&mut self, _el: ElementRef<'_>) -> bool {
        false
    }

    /// The blocks `el` stands for, when it is a construct of the source's own.
    fn block(&mut self, _walker: &mut Walker, _el: ElementRef<'_>) -> Option<Vec<Block>> {
        None
    }

    /// The running text `el` stands for, when it is a construct of the source's own.
    fn inline(&mut self, _walker: &mut Walker, _el: ElementRef<'_>) -> Option<Vec<Inline>> {
        None
    }

    /// The blocks of the whole once read, as the source puts them in order.
    fn finish(&mut self, _walker: &mut Walker, blocks: Vec<Block>) -> Vec<Block> {
        blocks
    }
}

/// The rules of standard HTML and nothing else.
pub struct Standard;

impl Rules for Standard {}

/// The elements a browser lays out as blocks. Any other element is part of running text.
const BLOCK_LEVEL: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "caption",
    "center",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
];

/// Elements of running text that stand for nothing of their own.
const TRANSPARENT_INLINE: &[&str] = &[
    "abbr", "acronym", "bdi", "bdo", "big", "data", "font", "label", "mark", "nobr", "small",
    "span", "time",
];

/// What is never content.
const NEVER: &[&str] = &[
    "script", "style", "noscript", "template", "head", "meta", "link",
];

/// Style properties that only lay text out, which the model leaves to the style.
const LAYOUT: &[&str] = &[
    "background",
    "background-color",
    "border",
    "border-bottom",
    "border-collapse",
    "border-left",
    "border-right",
    "border-top",
    "box-sizing",
    "clear",
    "color",
    "display",
    "float",
    "font-family",
    "font-size",
    "height",
    "line-height",
    "margin",
    "margin-bottom",
    "margin-right",
    "margin-top",
    "max-width",
    "min-width",
    "opacity",
    "padding",
    "padding-bottom",
    "padding-left",
    "padding-right",
    "padding-top",
    "text-indent",
    "vertical-align",
    "white-space",
    "width",
];

fn is_block_level(el: &ElementRef) -> bool {
    BLOCK_LEVEL.iter().any(|name| dom::is(el, name))
}

pub fn text(value: impl Into<String>) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

pub fn para(children: Vec<Inline>) -> Block {
    Block::Para {
        children,
        align: None,
        indent_em: 0,
    }
}

/// Whether running text shows nothing: only white space and line breaks.
pub fn is_blank(nodes: &[Inline]) -> bool {
    nodes.iter().all(|node| match node {
        Inline::Text { value } => js::is_blank(value),
        Inline::Break => true,
        _ => false,
    })
}

/// Running text without the white space at either end.
pub fn trim_inline(mut out: Vec<Inline>) -> Vec<Inline> {
    let blank = |n: &Inline| matches!(n, Inline::Text { value } if js::is_blank(value));
    while out.first().is_some_and(blank) {
        out.remove(0);
    }
    while out.last().is_some_and(blank) {
        out.pop();
    }
    if let Some(Inline::Text { value }) = out.first_mut() {
        *value = js::trim_start(value).to_owned();
    }
    if let Some(Inline::Text { value }) = out.last_mut() {
        *value = js::trim_end(value).to_owned();
    }
    out
}

/// The text of running text, markup and all left out, as the page's text reads: a line break
/// as a newline and a formula as its TeX untrimmed, where
/// [`typesep_model::walk::plain_text`] makes a space of the one and trims the other.
pub fn plain(nodes: &[Inline]) -> String {
    typesep_model::walk::plain_text_with(nodes, &mut |node, out| match node {
        Inline::Break => out.push('\n'),
        Inline::Math { tex, .. } => out.push_str(tex),
        _ => {}
    })
}

// The limits browsers clamp spans to.
const MAX_COLSPAN: u32 = 1000;
const MAX_ROWSPAN: u32 = 65534;

fn span_of(attr: Option<&str>, max: u32) -> u32 {
    let n = match attr {
        None => 1.0,
        Some(value) => js::number(value).unwrap_or(f64::NAN),
    };
    let n = if n.is_nan() || n == 0.0 { 1.0 } else { n };
    (n.max(1.0) as u32).min(max)
}

/// Where the walk is: the address the page's links are relative to, and what it found that
/// it read as plain content, or left out, for the warnings.
pub struct Walker {
    base: Option<Url>,
    notes: Vec<(String, usize)>,
}

impl Walker {
    /// A walker for a page at `base`, against which its links and images resolve.
    pub fn new(base: Option<&str>) -> Self {
        Self {
            base: base.and_then(|base| Url::parse(base).ok()),
            notes: Vec::new(),
        }
    }

    /// Notes something the reader should hear of, once however often it happens.
    pub fn note(&mut self, what: impl Into<String>) {
        let what = what.into();
        match self.notes.iter_mut().find(|(seen, _)| *seen == what) {
            Some((_, n)) => *n += 1,
            None => self.notes.push((what, 1)),
        }
    }

    /// What was noted, each once, with how often it happened when more than once.
    pub fn warnings(&self) -> Vec<String> {
        self.notes
            .iter()
            .map(|(what, n)| {
                if *n > 1 {
                    format!("{what} ({n}×)")
                } else {
                    what.clone()
                }
            })
            .collect()
    }

    /// `href` made absolute against the page's address, or as it is when there is none.
    pub fn resolve(&self, href: &str) -> Option<String> {
        match &self.base {
            Some(base) => base.join(href).ok().map(|url| url.to_string()),
            None => Url::parse(href).ok().map(|url| url.to_string()),
        }
    }

    /// The blocks of what `root` holds, as the rules finish them.
    pub fn read(&mut self, rules: &mut dyn Rules, root: ElementRef<'_>) -> Vec<Block> {
        let blocks = self.blocks(rules, root.children());
        rules.finish(self, blocks)
    }

    /// The blocks of `nodes`. Running text between blocks is a paragraph of its own.
    pub fn blocks<'a>(
        &mut self,
        rules: &mut dyn Rules,
        nodes: impl IntoIterator<Item = Ref<'a>>,
    ) -> Vec<Block> {
        let mut out = Vec::new();
        let mut run: Vec<Ref<'a>> = Vec::new();
        for node in nodes {
            let Some(el) = ElementRef::wrap(node) else {
                if node.value().is_text() {
                    run.push(node);
                }
                continue;
            };
            if rules.skip(el) {
                continue;
            }
            if let Some(blocks) = rules.block(self, el) {
                self.flush(rules, &mut run, &mut out);
                out.extend(blocks);
                continue;
            }
            if !is_block_level(&el) {
                run.push(node);
                continue;
            }
            self.flush(rules, &mut run, &mut out);
            out.extend(self.element_blocks(rules, el));
        }
        self.flush(rules, &mut run, &mut out);
        out
    }

    fn flush<'a>(&mut self, rules: &mut dyn Rules, run: &mut Vec<Ref<'a>>, out: &mut Vec<Block>) {
        if run.is_empty() {
            return;
        }
        let children = trim_inline(self.inlines(rules, run.drain(..)));
        if !is_blank(&children) {
            out.push(para(children));
        }
    }

    /// Whether `el` is hidden from a reader, which leaves it out; that it held text is said.
    fn hidden(&mut self, el: ElementRef<'_>) -> bool {
        let hidden = css::get(&el, "display").is_some_and(|d| d.eq_ignore_ascii_case("none"));
        if hidden && !js::is_blank(&dom::text_of(*el)) {
            self.note("hidden text left out");
        }
        hidden
    }

    /// `el` read as standard HTML in a place that holds blocks, the rules asked only about
    /// what it holds.
    pub fn element_blocks(&mut self, rules: &mut dyn Rules, el: ElementRef<'_>) -> Vec<Block> {
        let tag = dom::tag(&el);
        if NEVER.contains(&tag.as_str()) || self.hidden(el) {
            return Vec::new();
        }
        let anchored = |content: Vec<Block>| -> Vec<Block> {
            match dom::truthy(el.attr("id")) {
                Some(id) if !content.is_empty() => {
                    let mut out = vec![Block::Anchor { id: id.to_owned() }];
                    out.extend(content);
                    out
                }
                _ => content,
            }
        };
        match tag.as_str() {
            "p" => anchored(self.paragraph(rules, el).into_iter().collect()),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.heading(rules, el).into_iter().collect()
            }
            "ul" | "ol" => anchored(vec![Block::List(self.list(rules, el))]),
            "blockquote" => anchored(vec![Block::Blockquote {
                children: self.blocks(rules, el.children()),
            }]),
            "table" => anchored(
                self.table(rules, el)
                    .map(Block::Table)
                    .into_iter()
                    .collect(),
            ),
            "figure" => self
                .figure(rules, el)
                .map(Block::Figure)
                .into_iter()
                .collect(),
            "hr" => vec![Block::Rule],
            "pre" => {
                let mut children = self.inlines(rules, el.children());
                if let Some(Inline::Text { value }) = children.first_mut()
                    && let Some(rest) = value.strip_prefix('\n')
                {
                    *value = rest.to_owned();
                }
                vec![Block::Pre { children }]
            }
            _ => anchored(self.blocks(rules, el.children())),
        }
    }

    /// A paragraph: its running text, set as its style says.
    pub fn paragraph(&mut self, rules: &mut dyn Rules, el: ElementRef<'_>) -> Option<Block> {
        let children = trim_inline(self.inlines(rules, el.children()));
        self.paragraph_with(el, children)
    }

    /// A paragraph of `children`, set as the style of `el`, the element they were read from,
    /// says; none when they show nothing.
    pub fn paragraph_with(&mut self, el: ElementRef<'_>, children: Vec<Inline>) -> Option<Block> {
        if is_blank(&children) {
            return None;
        }
        let style = css::style_of(&el);
        let mut align = None;
        let mut indent_em = 0;
        for (property, value) in &style {
            match property.as_str() {
                "text-align" => {
                    align = match value.to_ascii_lowercase().as_str() {
                        "center" => Some(Align::Center),
                        "right" => Some(Align::Right),
                        _ => None,
                    }
                }
                "margin-left" => {
                    indent_em = css::ems(value).map_or(0, |n| n.round().clamp(0.0, 255.0) as u8)
                }
                _ => {}
            }
        }
        Some(Block::Para {
            children: self.styled(&style, children),
            align,
            indent_em,
        })
    }

    fn heading(&mut self, rules: &mut dyn Rules, el: ElementRef<'_>) -> Option<Block> {
        let level = dom::tag(&el)[1..].parse().unwrap_or(2);
        let id = dom::truthy(el.attr("id"))
            .or_else(|| dom::descendants(*el).find_map(|d| dom::truthy(d.attr("id"))))
            .map(str::to_owned);
        let mut children = trim_inline(self.inlines(rules, el.children()));
        // The id the heading takes from a span in it is the heading's, not a place of its own.
        children.retain(
            |node| !matches!(node, Inline::Anchor { id: inner } if Some(inner) == id.as_ref()),
        );
        let children = trim_inline(children);
        (!is_blank(&children)).then_some(Block::Heading {
            level,
            id,
            children,
            outline_only: false,
        })
    }

    fn list(&mut self, rules: &mut dyn Rules, el: ElementRef<'_>) -> List {
        let ordered = dom::is(&el, "ol");
        let numbering = match el.attr("type") {
            Some("a") if ordered => Some(ListNumbering::LowerAlpha),
            Some("A") if ordered => Some(ListNumbering::UpperAlpha),
            Some("i") if ordered => Some(ListNumbering::LowerRoman),
            Some("I") if ordered => Some(ListNumbering::UpperRoman),
            _ => None,
        };
        let start = el
            .attr("start")
            .and_then(js::number)
            .filter(|n| ordered && js::is_integer(*n) && *n != 1.0)
            .map(|n| n as i64);
        let mut items = Vec::new();
        for li in dom::children_named(*el, &["li"]) {
            if rules.skip(li) {
                continue;
            }
            let has_blocks = dom::child_elements(*li).any(|c| is_block_level(&c));
            items.push(if has_blocks {
                self.blocks(rules, li.children())
            } else {
                let children = trim_inline(self.inlines(rules, li.children()));
                vec![para(children)]
            });
        }
        List {
            ordered,
            hanging: false,
            items,
            numbering,
            start,
            labels: None,
        }
    }

    /// A table, with the rules its cells' styles draw, if any of them draws one.
    pub fn table(&mut self, rules: &mut dyn Rules, el: ElementRef<'_>) -> Option<Table> {
        let mut trs: Vec<ElementRef> = dom::children_named(*el, &["tr"]).collect();
        for section in dom::children_named(*el, &["thead", "tbody", "tfoot"]) {
            trs.extend(dom::children_named(*section, &["tr"]));
        }
        let mut ruled = false;
        let mut rows: Vec<Vec<TableCell>> = Vec::new();
        for tr in trs {
            let mut cells = Vec::new();
            for cell in dom::children_named(*tr, &["td", "th"]) {
                let edges = cell_edges(&cell);
                ruled |= edges.is_some();
                cells.push(TableCell {
                    header: dom::is(&cell, "th"),
                    colspan: span_of(cell.attr("colspan"), MAX_COLSPAN),
                    rowspan: span_of(cell.attr("rowspan"), MAX_ROWSPAN),
                    children: trim_inline(self.inlines(rules, cell.children())),
                    edges,
                });
            }
            if !cells.is_empty() {
                rows.push(cells);
            }
        }
        if rows.is_empty() {
            return None;
        }
        if ruled {
            for cell in rows.iter_mut().flatten() {
                cell.edges.get_or_insert_with(Edges::default);
            }
        }
        let columns = rows
            .iter()
            .map(|row| {
                row.iter()
                    .fold(0u32, |sum, c| sum.saturating_add(c.colspan))
            })
            .max()
            .unwrap_or(0);
        let caption = dom::children_named(*el, &["caption"])
            .next()
            .map(|c| trim_inline(self.inlines(rules, c.children())))
            .filter(|c| !is_blank(c));
        Some(Table {
            rows,
            columns,
            caption,
            rules: ruled.then_some(TableRules::Cells),
        })
    }

    /// A figure: its first image, with the caption it has.
    pub fn figure(&mut self, rules: &mut dyn Rules, el: ElementRef<'_>) -> Option<Figure> {
        let image = dom::descendants_named(*el, &["img"]).next()?;
        let mut figure = self.image(image)?;
        figure.caption = dom::children_named(*el, &["figcaption"])
            .next()
            .map(|c| trim_inline(self.inlines(rules, c.children())))
            .filter(|c| !is_blank(c));
        figure.id = dom::truthy(el.attr("id")).map(|id| Some(id.to_owned()));
        Some(figure)
    }

    /// The image `img` shows, at its address made absolute, and at the width its `width`
    /// attribute gives it, in CSS pixels.
    pub fn image(&self, img: ElementRef<'_>) -> Option<Figure> {
        let src = self.resolve(dom::truthy(img.attr("src"))?)?;
        Some(Figure {
            src,
            alt: js::trim(img.attr("alt").unwrap_or("")).to_owned(),
            ebook_src: None,
            width_px: img
                .attr("width")
                .and_then(|width| js::trim(width).parse::<u32>().ok())
                .filter(|width| *width > 0),
            caption: None,
            id: None,
            inline: None,
        })
    }

    /// The running text of `nodes`.
    pub fn inlines<'a>(
        &mut self,
        rules: &mut dyn Rules,
        nodes: impl IntoIterator<Item = Ref<'a>>,
    ) -> Vec<Inline> {
        let mut out = Vec::new();
        for node in nodes {
            match node.value() {
                Node::Text(data) => {
                    if !data.is_empty() {
                        out.push(text(&**data));
                    }
                }
                Node::Element(_) => {
                    let el = ElementRef::wrap(node).expect("an element");
                    if rules.skip(el) {
                        continue;
                    }
                    if let Some(found) = rules.inline(self, el) {
                        out.extend(found);
                        continue;
                    }
                    if is_block_level(&el) && !is_blank(&out) {
                        out.push(Inline::Break);
                    }
                    out.extend(self.element_inlines(rules, el));
                }
                _ => {}
            }
        }
        out
    }

    /// `el` read as standard HTML in running text, the rules asked only about what it holds.
    pub fn element_inlines(&mut self, rules: &mut dyn Rules, el: ElementRef<'_>) -> Vec<Inline> {
        let tag = dom::tag(&el);
        if NEVER.contains(&tag.as_str()) || self.hidden(el) {
            return Vec::new();
        }
        let wrap = |children| match tag.as_str() {
            "em" | "i" | "cite" | "var" | "dfn" => Inline::Emph { children },
            "strong" | "b" => Inline::Strong { children },
            "sub" => Inline::Sub { children },
            "sup" => Inline::Sup { children },
            "code" | "tt" | "kbd" | "samp" => Inline::Code { children },
            "u" | "ins" => Inline::Decorated {
                line: Decoration::Under,
                dashed: false,
                children,
            },
            _ => Inline::Decorated {
                line: Decoration::Strike,
                dashed: false,
                children,
            },
        };
        let content = match tag.as_str() {
            "br" => return vec![Inline::Break],
            "wbr" => return Vec::new(),
            "img" => {
                return self
                    .image(el)
                    .map(|figure| {
                        Inline::Figure(Figure {
                            inline: Some(true),
                            ..figure
                        })
                    })
                    .into_iter()
                    .collect();
            }
            "a" => self.link(rules, el),
            "q" => {
                let mut out = vec![text("\u{201C}")];
                out.extend(self.inlines(rules, el.children()));
                out.push(text("\u{201D}"));
                out
            }
            "em" | "i" | "cite" | "var" | "dfn" | "strong" | "b" | "sub" | "sup" | "code"
            | "tt" | "kbd" | "samp" | "u" | "ins" | "s" | "strike" | "del" => {
                vec![wrap(self.inlines(rules, el.children()))]
            }
            _ => {
                if !TRANSPARENT_INLINE.contains(&tag.as_str()) && !is_block_level(&el) {
                    self.note(format!("unhandled <{tag}> treated as plain content"));
                }
                self.inlines(rules, el.children())
            }
        };
        let content = self.styled(&css::style_of(&el), content);
        let id = el
            .attr("id")
            .or_else(|| if tag == "a" { el.attr("name") } else { None });
        match dom::truthy(id) {
            Some(id) if !is_block_level(&el) => {
                let mut out = vec![Inline::Anchor { id: id.to_owned() }];
                out.extend(content);
                out
            }
            _ => content,
        }
    }

    fn link(&mut self, rules: &mut dyn Rules, el: ElementRef<'_>) -> Vec<Inline> {
        let children = self.inlines(rules, el.children());
        let Some(href) = dom::truthy(el.attr("href")) else {
            return children;
        };
        if let Some(fragment) = href.strip_prefix('#') {
            return vec![Inline::Link {
                href: fragment.to_owned(),
                internal: true,
                children,
            }];
        }
        match self.resolve(href) {
            Some(href) => vec![Inline::Link {
                href,
                internal: false,
                children,
            }],
            None => children,
        }
    }

    /// Running text as an element's style sets it: letter-spaced, in small capitals, with a
    /// line under, over or through it, in italics or bold. What only lays text out is left to
    /// the style; anything else is said.
    fn styled(&mut self, style: &[(String, String)], mut children: Vec<Inline>) -> Vec<Inline> {
        if children.is_empty() {
            return children;
        }
        let mut decoration: Option<Decoration> = None;
        let mut dashed = false;
        for (property, value) in style {
            let value = value.to_ascii_lowercase();
            match property.as_str() {
                "letter-spacing" => {
                    let spaced = css::ems(&value)
                        .or_else(|| {
                            value
                                .trim_end_matches(|c: char| c.is_alphabetic())
                                .parse()
                                .ok()
                        })
                        .is_some_and(|n| n > 0.0);
                    if spaced {
                        children = vec![Inline::Spaced { children }];
                    }
                }
                "font-variant" | "font-variant-caps" if value.contains("small-caps") => {
                    children = vec![Inline::SmallCaps { children }];
                }
                "font-style" if value == "italic" || value == "oblique" => {
                    children = vec![Inline::Emph { children }];
                }
                "font-weight"
                    if value == "bold"
                        || value == "bolder"
                        || value.parse::<u32>().is_ok_and(|w| w >= 600) =>
                {
                    children = vec![Inline::Strong { children }];
                }
                "text-decoration" | "text-decoration-line" | "text-decoration-style" => {
                    for word in value.split_whitespace() {
                        match word {
                            "underline" => decoration = Some(Decoration::Under),
                            "overline" => decoration = Some(Decoration::Over),
                            "line-through" => decoration = Some(Decoration::Strike),
                            "dashed" | "dotted" => dashed = true,
                            _ => {}
                        }
                    }
                }
                "text-align" | "margin-left" | "font-variant" | "font-variant-caps"
                | "font-style" | "font-weight" => {}
                other if LAYOUT.contains(&other) => {}
                other => self.note(format!("style \"{other}\" left to the style")),
            }
        }
        if let Some(line) = decoration {
            children = vec![Inline::Decorated {
                line,
                dashed,
                children,
            }];
        }
        children
    }
}

/// The sides of a cell its style rules, when it says anything of its borders.
fn cell_edges(cell: &ElementRef) -> Option<Edges> {
    let mut edges: Option<Edges> = None;
    for (property, value) in css::style_of(cell) {
        let draws = css::draws_border(&value);
        let set: fn(&mut Edges, bool) = match property.as_str() {
            "border" => |e, on| {
                *e = Edges {
                    top: on,
                    right: on,
                    bottom: on,
                    left: on,
                }
            },
            "border-top" => |e, on| e.top = on,
            "border-right" => |e, on| e.right = on,
            "border-bottom" => |e, on| e.bottom = on,
            "border-left" => |e, on| e.left = on,
            _ => continue,
        };
        set(edges.get_or_insert_with(Edges::default), draws);
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Html;
    use serde_json::{Value, json};

    fn read_with(rules: &mut dyn Rules, html: &str) -> (Vec<Block>, Vec<String>) {
        let document = Html::parse_fragment(html);
        let mut walker = Walker::new(Some("https://example.org/w/page"));
        let blocks = walker.read(rules, document.root_element());
        (blocks, walker.warnings())
    }

    fn read(html: &str) -> (Value, Vec<String>) {
        let (blocks, warnings) = read_with(&mut Standard, html);
        (serde_json::to_value(blocks).unwrap(), warnings)
    }

    #[test]
    fn standard_blocks_become_their_constructs() {
        let (body, warnings) = read(
            "<h2 id=\"one\">One</h2><p>A <i>b</i> <b>c</b> H<sub>2</sub>O x<sup>2</sup>, <a href=\"#one\">up</a> \
             and <a href=\"other\">on</a>.<br/>Next.</p><ul><li>i</li><li><p>ii</p></li></ul>\
             <ol type=\"a\" start=\"3\"><li>c</li></ol><blockquote><p>Q</p></blockquote><hr/>\
             <pre>\n  kept</pre><h3><span id=\"inner\">Two</span></h3>",
        );
        assert_eq!(
            body,
            json!([
                { "type": "heading", "level": 2, "id": "one", "children": [{ "type": "text", "value": "One" }] },
                { "type": "para", "children": [
                    { "type": "text", "value": "A " },
                    { "type": "emph", "children": [{ "type": "text", "value": "b" }] },
                    { "type": "text", "value": " " },
                    { "type": "strong", "children": [{ "type": "text", "value": "c" }] },
                    { "type": "text", "value": " H" },
                    { "type": "sub", "children": [{ "type": "text", "value": "2" }] },
                    { "type": "text", "value": "O x" },
                    { "type": "sup", "children": [{ "type": "text", "value": "2" }] },
                    { "type": "text", "value": ", " },
                    { "type": "link", "href": "one", "internal": true, "children": [{ "type": "text", "value": "up" }] },
                    { "type": "text", "value": " and " },
                    { "type": "link", "href": "https://example.org/w/other", "internal": false, "children": [{ "type": "text", "value": "on" }] },
                    { "type": "text", "value": "." },
                    { "type": "break" },
                    { "type": "text", "value": "Next." }
                ] },
                { "type": "list", "ordered": false, "hanging": false, "items": [
                    [{ "type": "para", "children": [{ "type": "text", "value": "i" }] }],
                    [{ "type": "para", "children": [{ "type": "text", "value": "ii" }] }]
                ] },
                { "type": "list", "ordered": true, "hanging": false, "numbering": "lower-alpha", "start": 3, "items": [
                    [{ "type": "para", "children": [{ "type": "text", "value": "c" }] }]
                ] },
                { "type": "blockquote", "children": [{ "type": "para", "children": [{ "type": "text", "value": "Q" }] }] },
                { "type": "rule" },
                { "type": "pre", "children": [{ "type": "text", "value": "  kept" }] },
                { "type": "heading", "level": 3, "id": "inner", "children": [{ "type": "text", "value": "Two" }] }
            ])
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn running_text_among_blocks_is_a_paragraph_and_blank_paragraphs_are_none() {
        let (body, _) =
            read("<div>  <span>loose</span> text\n<p>para</p>tail<p><br/></p><p>&#160;</p></div>");
        assert_eq!(
            body,
            json!([
                { "type": "para", "children": [{ "type": "text", "value": "loose" }, { "type": "text", "value": " text" }] },
                { "type": "para", "children": [{ "type": "text", "value": "para" }] },
                { "type": "para", "children": [{ "type": "text", "value": "tail" }] }
            ])
        );
    }

    #[test]
    fn inline_styles_become_what_they_show() {
        let (body, warnings) = read(
            "<p style=\"text-align: right; margin-left: 3em\">\
             <span style=\"letter-spacing: 0.2em; margin-right: -0.2em;\">wide</span> \
             <span style=\"font-variant: small-caps\">caps</span> \
             <u>under</u> <s>struck</s> \
             <span style=\"text-decoration-line: underline; text-decoration-style: dashed;\">dashed</span> \
             <span style=\"text-decoration: overline; aria-label=\">over</span> \
             <span style=\"color: red; cursor: help\">plain</span></p>\
             <p style=\"text-align: center; font-variant: small-caps;\">Centred</p>",
        );
        let deco = |line: &str, dashed: bool, word: &str| json!({ "type": "decorated", "line": line, "dashed": dashed, "children": [{ "type": "text", "value": word }] });
        assert_eq!(
            body,
            json!([
                { "type": "para", "align": "right", "indentEm": 3, "children": [
                    { "type": "spaced", "children": [{ "type": "text", "value": "wide" }] },
                    { "type": "text", "value": " " },
                    { "type": "smallcaps", "children": [{ "type": "text", "value": "caps" }] },
                    { "type": "text", "value": " " },
                    deco("under", false, "under"),
                    { "type": "text", "value": " " },
                    deco("strike", false, "struck"),
                    { "type": "text", "value": " " },
                    deco("under", true, "dashed"),
                    { "type": "text", "value": " " },
                    deco("over", false, "over"),
                    { "type": "text", "value": " " },
                    { "type": "text", "value": "plain" }
                ] },
                { "type": "para", "align": "center", "children": [
                    { "type": "smallcaps", "children": [{ "type": "text", "value": "Centred" }] }
                ] }
            ])
        );
        assert_eq!(warnings, ["style \"cursor\" left to the style"]);
    }

    #[test]
    fn a_table_is_ruled_where_its_cells_say() {
        let (body, _) = read(
            "<table><caption>\n</caption><tbody><tr><th>p</th><th colspan=\"2\">q</th></tr>\
             <tr><td>W</td><td>F</td><td></td></tr></tbody></table>\
             <table><tr><td style=\"width: 5em; border-right: 1px solid black; border-bottom: 1px solid black;\">a</td>\
             <td style=\"border-bottom: 1px solid black\">b</td></tr><tr><td>c</td><td style=\"border: none\">d</td></tr></table>",
        );
        let cell = |header: bool, colspan: u32, value: &str| {
            let children = if value.is_empty() {
                json!([])
            } else {
                json!([{ "type": "text", "value": value }])
            };
            json!({ "header": header, "colspan": colspan, "rowspan": 1, "children": children })
        };
        assert_eq!(
            body[0],
            json!({ "type": "table", "columns": 3, "rows": [
                [cell(true, 1, "p"), cell(true, 2, "q")],
                [cell(false, 1, "W"), cell(false, 1, "F"), cell(false, 1, "")]
            ] }),
            "a table whose cells draw nothing is left to the style, a blank caption left out"
        );
        let edges = |top: bool, right: bool, bottom: bool, left: bool| json!({ "top": top, "right": right, "bottom": bottom, "left": left });
        assert_eq!(body[1]["rules"], json!("cells"));
        assert_eq!(
            body[1]["rows"],
            json!([
                [
                    { "header": false, "colspan": 1, "rowspan": 1, "children": [{ "type": "text", "value": "a" }], "edges": edges(false, true, true, false) },
                    { "header": false, "colspan": 1, "rowspan": 1, "children": [{ "type": "text", "value": "b" }], "edges": edges(false, false, true, false) }
                ],
                [
                    { "header": false, "colspan": 1, "rowspan": 1, "children": [{ "type": "text", "value": "c" }], "edges": edges(false, false, false, false) },
                    { "header": false, "colspan": 1, "rowspan": 1, "children": [{ "type": "text", "value": "d" }], "edges": edges(false, false, false, false) }
                ]
            ])
        );
    }

    #[test]
    fn figures_and_images_resolve_against_the_page() {
        let (body, _) = read(
            "<figure id=\"f1\"><a href=\"/w/File:x\"><img src=\"/w/images/a/ab/X.png\" alt=\" an X \" width=\"250\"/></a>\
             <figcaption>The <i>X</i></figcaption></figure><figure><img src=\"y.png\" width=\"50%\"/><figcaption></figcaption></figure>\
             <p>see <img src=\"z.png\" width=\"30\" height=\"14\"/></p>",
        );
        assert_eq!(
            body,
            json!([
                { "type": "figure", "src": "https://example.org/w/images/a/ab/X.png", "alt": "an X", "id": "f1", "widthPx": 250,
                  "caption": [{ "type": "text", "value": "The " }, { "type": "emph", "children": [{ "type": "text", "value": "X" }] }] },
                { "type": "figure", "src": "https://example.org/w/y.png", "alt": "" },
                { "type": "para", "children": [
                    { "type": "text", "value": "see " },
                    { "type": "figure", "src": "https://example.org/w/z.png", "alt": "", "widthPx": 30, "inline": true }
                ] }
            ])
        );
    }

    #[test]
    fn what_the_walker_does_not_know_is_kept_and_said_once() {
        let (body, warnings) = read(
            "<p>a <blink>b</blink> <blink>c</blink> <marquee>d</marquee></p>\
             <p style=\"display: none\">hidden</p><p style=\"display:none\"> </p>\
             <script>no()</script><!-- a comment --><p>e</p>",
        );
        assert_eq!(
            body,
            json!([
                { "type": "para", "children": [
                    { "type": "text", "value": "a " }, { "type": "text", "value": "b" },
                    { "type": "text", "value": " " }, { "type": "text", "value": "c" },
                    { "type": "text", "value": " " }, { "type": "text", "value": "d" }
                ] },
                { "type": "para", "children": [{ "type": "text", "value": "e" }] }
            ])
        );
        assert_eq!(
            warnings,
            [
                "unhandled <blink> treated as plain content (2×)",
                "unhandled <marquee> treated as plain content",
                "hidden text left out"
            ]
        );
    }

    /// Rules of a made-up source: `.gone` is left out, `.box` is an aside, `.mark` is a
    /// margin note, and the blocks are counted when the walk is done.
    #[derive(Default)]
    struct Boxes {
        finished: usize,
    }

    impl Rules for Boxes {
        fn skip(&mut self, el: ElementRef<'_>) -> bool {
            dom::has_class(&el, "gone")
        }

        fn block(&mut self, walker: &mut Walker, el: ElementRef<'_>) -> Option<Vec<Block>> {
            dom::has_class(&el, "box").then(|| {
                vec![Block::Aside {
                    children: walker.blocks(self, el.children()),
                }]
            })
        }

        fn inline(&mut self, walker: &mut Walker, el: ElementRef<'_>) -> Option<Vec<Inline>> {
            dom::has_class(&el, "mark").then(|| {
                vec![Inline::MarginNote {
                    children: walker.inlines(self, el.children()),
                }]
            })
        }

        fn finish(&mut self, _walker: &mut Walker, blocks: Vec<Block>) -> Vec<Block> {
            self.finished = blocks.len();
            blocks
        }
    }

    #[test]
    fn a_sources_rules_come_before_the_standard_reading_and_reach_nested_elements() {
        let mut rules = Boxes::default();
        let (blocks, _) = read_with(
            &mut rules,
            "<div class=\"box\"><p>in <span class=\"mark\">(m)</span><span class=\"gone\">x</span></p>\
             <div class=\"box\"><p>deeper</p></div></div><p class=\"gone\">gone</p><p>out</p>",
        );
        assert_eq!(
            serde_json::to_value(&blocks).unwrap(),
            json!([
                { "type": "aside", "children": [
                    { "type": "para", "children": [
                        { "type": "text", "value": "in " },
                        { "type": "marginnote", "children": [{ "type": "text", "value": "(m)" }] }
                    ] },
                    { "type": "aside", "children": [{ "type": "para", "children": [{ "type": "text", "value": "deeper" }] }] }
                ] },
                { "type": "para", "children": [{ "type": "text", "value": "out" }] }
            ])
        );
        assert_eq!(rules.finished, 2);
    }
}
