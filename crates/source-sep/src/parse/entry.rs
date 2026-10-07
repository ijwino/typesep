use std::collections::BTreeSet;

use ego_tree::NodeId;
use scraper::{CaseSensitivity, ElementRef, Html, Node, Selector};
use url::Url;

use super::metadata::{MetadataSource, parse_metadata};
use crate::sections::{ContentKey, SECTION_LABELS};
use typesep_html::dom::{self, Ref};
use typesep_html::js::{self, DOT, WORD, WS};
use typesep_html::{lazy_regex, lazy_selector};
use typesep_model::{
    Align, Attachment, AttachmentKind, Block, Doc, EntryLinks, Figure, Inline, List, ListNumbering,
    MathMode, NoteRef, NoteTable, Section, Table, TableCell, TocItem,
};

lazy_regex!(MATH_RE = r"\\\(([\s\S]*?)\\\)|\\\[([\s\S]*?)\\\]");
lazy_regex!(
    PDF_EXCLUDE_RE = format!(
        r"(?i)<!--{WS}*pdf{WS}+exclude{WS}+begin{WS}*-->[\s\S]*?<!--{WS}*pdf{WS}+exclude{WS}+end{WS}*-->"
    )
);
lazy_regex!(ICON_DIR_RE = r"(?i-u:/symbols/)");
lazy_regex!(ICON_FILE_RE = r"^(?i-u:sep-?man|sepman|inpho|pp\.gif|philpapers|iep)");
lazy_regex!(HTML_FILE_RE = r"^[^/]+\.(?i-u:html?)$");
lazy_regex!(INDEX_FILE_RE = r"^(?i-u:index\.html?)$");
lazy_regex!(NOTES_FILE_RE = r"^(?i-u:notes\.html?)$");
lazy_regex!(IMAGE_TYPE_RE = r"^(?i-u:image/)");
lazy_regex!(TITLE_SUFFIX_RE = format!(r"{WS}*\((?i-u:stanford encyclopedia of philosophy){DOT}*$"));
lazy_regex!(LABEL_LINK_RE = format!(r"^{WS}*\[?{WORD}{{1,4}}\.?\]?{WS}*$"));
lazy_regex!(LABEL_TEXT_RE = format!(r"^{WS}*(\[{WORD}{{1,4}}\]|{WORD}{{1,4}}\.){WS}+"));

lazy_selector!(PREAMBLE = "#preamble");
lazy_selector!(TOC_LISTS = "#toc > ul");
lazy_selector!(MAIN_TEXT = "#main-text");
lazy_selector!(BIBLIOGRAPHY = "#bibliography");
lazy_selector!(ACADEMIC_TOOLS = "#academic-tools");
lazy_selector!(OTHER_INTERNET_RESOURCES = "#other-internet-resources");
lazy_selector!(RELATED_ENTRIES = "#related-entries");
lazy_selector!(AUEDITABLE = "#aueditable");
lazy_selector!(BODY = "body");
lazy_selector!(TITLE = "title");
lazy_selector!(H1_OR_H2 = "h1, h2");
lazy_selector!(ATTACHMENT_STRIP = "#pubinfo, #article-copyright, script, style");
lazy_selector!(A_HREF = "a[href]");

const TRANSPARENT_INLINE: &[&str] = &[
    "span", "abbr", "small", "big", "u", "s", "del", "ins", "mark", "dfn", "font", "label", "time",
    "nobr", "acronym", "bdi", "bdo",
];
const TRANSPARENT_BLOCK: &[&str] = &[
    "div", "section", "article", "center", "main", "aside", "header", "footer", "nav", "details",
    "summary", "figure",
];
const INLINE_TAGS: &[&str] = &[
    "em", "i", "cite", "var", "strong", "b", "sub", "sup", "code", "tt", "kbd", "samp", "a", "q",
    "wbr",
];
const BLOCKISH: &[&str] = &[
    "p",
    "ul",
    "ol",
    "li",
    "dl",
    "dt",
    "dd",
    "table",
    "tr",
    "td",
    "th",
    "blockquote",
    "pre",
];
const ANCHOR_BLOCKS: &[&str] = &["div", "section", "li", "p", "blockquote", "dd"];
const ITEM_BLOCKS: &[&str] = &[
    "p",
    "ul",
    "ol",
    "dl",
    "blockquote",
    "table",
    "div",
    "pre",
    "section",
];
const IMAGES: &[&str] = &["img", "object"];

struct Ctx {
    base: String,
    entry_dir: String,
    self_urls: BTreeSet<String>,
    unknown: Vec<(String, usize)>,
}

impl Ctx {
    fn new(base: &str) -> Self {
        let entry_dir = if base.is_empty() {
            String::new()
        } else {
            dir_of(strip_fragment(base))
        };
        let mut self_urls = BTreeSet::new();
        if !base.is_empty() {
            self_urls.insert(strip_fragment(base).to_owned());
            self_urls.insert(entry_dir.clone());
            self_urls.insert(format!("{entry_dir}index.html"));
        }
        Ctx {
            base: base.to_owned(),
            entry_dir,
            self_urls,
            unknown: Vec::new(),
        }
    }

    fn note_unknown(&mut self, tag: &str) {
        match self.unknown.iter_mut().find(|(t, _)| t == tag) {
            Some((_, n)) => *n += 1,
            None => self.unknown.push((tag.to_owned(), 1)),
        }
    }

    fn warnings(&self) -> Vec<String> {
        self.unknown
            .iter()
            .map(|(tag, n)| format!("unhandled <{tag}> ({n}×) treated as plain content"))
            .collect()
    }
}

fn strip_fragment(url: &str) -> &str {
    url.split('#').next().unwrap_or(url)
}

fn dir_of(url: &str) -> String {
    match url.rfind('/') {
        Some(i) => url[..=i].to_owned(),
        None => String::new(),
    }
}

fn resolve(href: &str, base: &str) -> Option<String> {
    Url::parse(base)
        .ok()?
        .join(href)
        .ok()
        .map(|url| url.to_string())
}

fn text(value: impl Into<String>) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn para(children: Vec<Inline>) -> Block {
    Block::Para {
        children,
        align: None,
        indent_em: 0,
    }
}

#[derive(Default)]
pub struct ParseOptions<'a> {
    pub source_url: &'a str,
    pub slug: &'a str,
    pub edition: &'a str,
    pub fetched_at: &'a str,
}

pub fn parse_entry(html: &str, opts: &ParseOptions) -> Doc {
    let document = Html::parse_document(&PDF_EXCLUDE_RE.replace_all(html, ""));
    let mut ctx = Ctx::new(opts.source_url);

    let meta = parse_metadata(
        &document,
        &MetadataSource {
            source_url: opts.source_url,
            slug: opts.slug,
            edition: opts.edition,
            fetched_at: opts.fetched_at,
        },
    );
    let front = blocks_of_all(&mut ctx, dom::select(&document, &PREAMBLE));
    let toc = parse_toc(&mut ctx, dom::select(&document, &TOC_LISTS));
    let body = blocks_of_all(&mut ctx, dom::select(&document, &MAIN_TEXT));
    let links = collect_entry_links(&document, opts.source_url);

    let mut back = Vec::new();
    let mut absent = BTreeSet::new();
    for (key, label) in SECTION_LABELS {
        let selector: &Selector = match key {
            ContentKey::Bib => &BIBLIOGRAPHY,
            ContentKey::Academic => &ACADEMIC_TOOLS,
            ContentKey::Oir => &OTHER_INTERNET_RESOURCES,
            ContentKey::Related => &RELATED_ENTRIES,
            ContentKey::Notes | ContentKey::Supplements => continue,
        };
        let matched: Vec<ElementRef> = dom::select(&document, selector).collect();
        if !matched.is_empty() {
            back.push(Section {
                key: key.name().to_owned(),
                label: label.to_owned(),
                blocks: blocks_of_all(&mut ctx, matched),
            });
        } else {
            absent.insert(label.to_lowercase());
        }
    }

    Doc {
        meta,
        front,
        toc: without_items_named(toc, &absent),
        body,
        back,
        notes: NoteTable::new(),
        links,
        attachments: Vec::new(),
        warnings: ctx.warnings(),
    }
}

pub struct AttachmentOptions<'a> {
    pub source_url: &'a str,
    pub kind: AttachmentKind,
}

pub fn parse_attachment(html: &str, opts: &AttachmentOptions) -> Attachment {
    let mut document = Html::parse_document(&PDF_EXCLUDE_RE.replace_all(html, ""));
    let mut ctx = Ctx::new(opts.source_url);

    let (root_ids, title, remove) = {
        let mut root_ids: Vec<NodeId> = dom::select(&document, &AUEDITABLE)
            .map(|el| el.id())
            .collect();
        if root_ids.is_empty() {
            root_ids = dom::select(&document, &BODY).map(|el| el.id()).collect();
        }
        let roots: Vec<ElementRef> = root_ids
            .iter()
            .filter_map(|id| ElementRef::wrap(document.tree.get(*id)?))
            .collect();

        let heading = roots.iter().flat_map(|root| root.select(&H1_OR_H2)).next();
        let mut title = heading
            .map(|h| js::trim(&js::collapse_spaces(&dom::text_of(*h))).to_owned())
            .unwrap_or_default();
        if title.is_empty() {
            let page_title: String = dom::select(&document, &TITLE)
                .map(|el| dom::text_of(*el))
                .collect();
            title = js::trim(&TITLE_SUFFIX_RE.replace(&page_title, "")).to_owned();
        }
        if title.is_empty() {
            title = match opts.kind {
                AttachmentKind::Notes => "Notes",
                AttachmentKind::Supplement => "Supplement",
            }
            .to_owned();
        }

        let mut remove: Vec<NodeId> = heading.map(|h| h.id()).into_iter().collect();
        remove.extend(
            roots
                .iter()
                .flat_map(|root| root.select(&ATTACHMENT_STRIP))
                .map(|el| el.id()),
        );
        (root_ids, title, remove)
    };
    for id in remove {
        if let Some(mut node) = document.tree.get_mut(id) {
            node.detach();
        }
    }

    let roots: Vec<ElementRef> = root_ids
        .iter()
        .filter_map(|id| ElementRef::wrap(document.tree.get(*id)?))
        .collect();
    let blocks = blocks_of_all(&mut ctx, roots.iter().copied());
    let notes = match opts.kind {
        AttachmentKind::Notes => Some(collect_notes(&mut ctx, &roots)),
        AttachmentKind::Supplement => None,
    };
    Attachment {
        kind: opts.kind,
        title,
        blocks,
        source_url: opts.source_url.to_owned(),
        notes,
    }
}

fn is_anchor_block(el: &ElementRef) -> bool {
    ANCHOR_BLOCKS.iter().any(|name| dom::is(el, name))
}

fn anchor_block_content(ctx: &mut Ctx, el: ElementRef) -> Vec<Block> {
    if dom::is(&el, "p") {
        vec![para(inline(ctx, *el))]
    } else {
        blocks(ctx, el.children())
    }
}

fn collect_notes(ctx: &mut Ctx, roots: &[ElementRef]) -> NoteTable {
    let mut notes = NoteTable::new();
    for root in roots {
        for el in dom::descendants(**root).filter(dom::has_id_or_name) {
            let Some(id) = dom::truthy(el.attr("id").or_else(|| el.attr("name"))) else {
                continue;
            };
            if notes.contains_key(id) {
                continue;
            }

            let content = if is_anchor_block(&el) {
                anchor_block_content(ctx, el)
            } else {
                let holder = el
                    .ancestors()
                    .take_while(|a| a.id() != root.id())
                    .filter_map(ElementRef::wrap)
                    .filter(is_anchor_block)
                    .last();
                let Some(holder) = holder else {
                    continue;
                };
                let mut content = anchor_block_content(ctx, holder);
                let mut next = dom::next_element(*holder);
                while let Some(sibling) = next {
                    if dom::has_id_or_name(&sibling)
                        || dom::descendants(*sibling).any(|d| dom::has_id_or_name(&d))
                    {
                        break;
                    }
                    content.extend(block_for(ctx, *sibling));
                    next = dom::next_element(*sibling);
                }
                content
            };
            notes.insert(id.to_owned(), strip_note_label(content));
        }
    }
    notes
}

fn strip_note_label(mut content: Vec<Block>) -> Vec<Block> {
    if let Some(Block::Para { children: kids, .. }) =
        content.iter_mut().find(|b| matches!(b, Block::Para { .. }))
    {
        while let Some(head) = kids.first() {
            let drop = match head {
                Inline::Text { value } => js::is_blank(value),
                Inline::Anchor { .. } => true,
                Inline::Link { children, .. } => LABEL_LINK_RE.is_match(&plain(children)),
                _ => false,
            };
            if !drop {
                break;
            }
            kids.remove(0);
        }
        if let Some(Inline::Text { value }) = kids.first_mut() {
            let stripped = LABEL_TEXT_RE.replace(value, "");
            let stripped = js::trim_start(&stripped).to_owned();
            if stripped.is_empty() {
                kids.remove(0);
            } else {
                *value = stripped;
            }
        }
    }
    content.retain(|b| !matches!(b, Block::Para { children, .. } if children.is_empty()));
    content
}

fn trim_inline(mut out: Vec<Inline>) -> Vec<Inline> {
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

fn plain(nodes: &[Inline]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Inline::Text { value } => out.push_str(value),
            Inline::Emph { children }
            | Inline::Strong { children }
            | Inline::Sub { children }
            | Inline::Sup { children }
            | Inline::Code { children }
            | Inline::Link { children, .. } => out.push_str(&plain(children)),
            _ => {}
        }
    }
    out
}

fn collect_entry_links(document: &Html, base: &str) -> EntryLinks {
    let mut out = EntryLinks::default();
    if base.is_empty() {
        return out;
    }
    let entry_dir = dir_of(base);

    for a in dom::select(document, &A_HREF) {
        let Some(href) = dom::truthy(a.attr("href")) else {
            continue;
        };
        let Some(abs) = resolve(href, base) else {
            continue;
        };
        let abs = strip_fragment(&abs).to_owned();
        let Some(file) = abs.strip_prefix(entry_dir.as_str()) else {
            continue;
        };
        if !HTML_FILE_RE.is_match(file) || INDEX_FILE_RE.is_match(file) {
            continue;
        }
        let list = if NOTES_FILE_RE.is_match(file) {
            &mut out.notes
        } else {
            &mut out.supplements
        };
        if !list.contains(&abs) {
            list.push(abs);
        }
    }
    out
}

fn parse_toc<'a>(ctx: &mut Ctx, lists: impl Iterator<Item = ElementRef<'a>>) -> Vec<TocItem> {
    let mut items = Vec::new();
    for ul in lists {
        for li in dom::children_named(*ul, &["li"]) {
            let a = dom::children_named(*li, &["a"]).next();
            let nested = dom::children_named(*li, &["ul"]).next();
            let mut label = a
                .map(|a| contents_label(inline(ctx, *a)))
                .unwrap_or_default();
            if label.is_empty() {
                let own = li
                    .children()
                    .filter(|c| !ElementRef::wrap(*c).is_some_and(|el| dom::is(&el, "ul")));
                label = contents_label(inline_of(ctx, own));
            }
            items.push(TocItem {
                label,
                href: a.and_then(|a| a.attr("href")).unwrap_or("").to_owned(),
                children: nested
                    .map(|ul| parse_toc(ctx, std::iter::once(ul)))
                    .unwrap_or_default(),
            });
        }
    }
    items
}

/// Running text as a line of the contents holds it, white space at either end left out:
/// without note references, anchors or figures, and with the text of a link in place of
/// the link, since the line is a link itself.
fn contents_label(nodes: Vec<Inline>) -> Vec<Inline> {
    fn strip(nodes: Vec<Inline>) -> Vec<Inline> {
        let mut out = Vec::new();
        for mut node in nodes {
            match &mut node {
                Inline::NoteRef(_) | Inline::Anchor { .. } | Inline::Figure(_) => {}
                Inline::Link { children, .. } => out.extend(strip(std::mem::take(children))),
                Inline::Emph { children }
                | Inline::Strong { children }
                | Inline::Sub { children }
                | Inline::Sup { children }
                | Inline::Code { children }
                | Inline::Spaced { children }
                | Inline::SmallCaps { children }
                | Inline::Decorated { children, .. }
                | Inline::MarginNote { children } => {
                    *children = strip(std::mem::take(children));
                    out.push(node);
                }
                Inline::Text { .. } | Inline::Math { .. } | Inline::Break => out.push(node),
            }
        }
        out
    }
    trim_inline(strip(nodes))
}

/// The contents without the lines that name end matter the page does not have, at any
/// depth, matched as the renderers match the sections left out of a document.
fn without_items_named(items: Vec<TocItem>, labels: &BTreeSet<String>) -> Vec<TocItem> {
    items
        .into_iter()
        .filter(|item| !labels.contains(&item.text().trim().to_lowercase()))
        .map(|item| TocItem {
            children: without_items_named(item.children, labels),
            ..item
        })
        .collect()
}

fn blocks_of_all<'a>(
    ctx: &mut Ctx,
    elements: impl IntoIterator<Item = ElementRef<'a>>,
) -> Vec<Block> {
    let mut out = Vec::new();
    for el in elements {
        out.extend(blocks(ctx, el.children()));
    }
    out
}

fn blocks<'a>(ctx: &mut Ctx, nodes: impl Iterator<Item = Ref<'a>>) -> Vec<Block> {
    let mut out = Vec::new();
    for node in nodes {
        out.extend(block_for(ctx, node));
    }
    out
}

fn block_for(ctx: &mut Ctx, node: Ref) -> Vec<Block> {
    match node.value() {
        Node::Text(data) => {
            let children = split_math(data);
            if !children.is_empty() && !js::is_blank(data) {
                vec![para(children)]
            } else {
                Vec::new()
            }
        }
        Node::Element(_) => element_block(ctx, ElementRef::wrap(node).expect("element")),
        _ => Vec::new(),
    }
}

fn has_image(el: ElementRef) -> bool {
    dom::descendants_named(*el, IMAGES).next().is_some()
}

fn heading_level(tag: &str) -> u8 {
    match tag {
        "h1" => 1,
        "h2" => 2,
        "h3" => 3,
        _ => 4,
    }
}

fn element_block(ctx: &mut Ctx, el: ElementRef) -> Vec<Block> {
    let tag = dom::tag(&el);
    let id = el.attr("id");
    let with_anchor = |content: Vec<Block>| -> Vec<Block> {
        match dom::truthy(id) {
            Some(id) => {
                let mut out = vec![Block::Anchor { id: id.to_owned() }];
                out.extend(content);
                out
            }
            None => content,
        }
    };

    match tag.as_str() {
        "script" | "style" | "noscript" | "br" => Vec::new(),

        "p" => {
            if js::is_blank(&dom::text_of(*el)) && !has_image(el) {
                return Vec::new();
            }
            let align = dom::has_class(&el, "center").then_some(Align::Center);
            vec![Block::Para {
                children: inline(ctx, *el),
                align,
                indent_em: 0,
            }]
        }

        "pre" => vec![Block::Pre {
            children: preformatted(ctx, el),
        }],

        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => vec![Block::Heading {
            level: heading_level(&tag),
            id: id.map(str::to_owned),
            children: inline(ctx, *el),
            outline_only: false,
        }],

        "ul" | "ol" => with_anchor(vec![Block::List(list(ctx, el))]),

        "dl" => with_anchor(vec![Block::List(List {
            ordered: false,
            hanging: true,
            items: definition_items(ctx, el),
            numbering: None,
            start: None,
            labels: None,
        })]),

        "blockquote" => with_anchor(vec![Block::Blockquote {
            children: blocks(ctx, el.children()),
        }]),

        "table" => with_anchor(parse_table(ctx, el).map(Block::Table).into_iter().collect()),

        "hr" => vec![Block::Rule],

        "img" | "object" => image_from(ctx, el).map(Block::Figure).into_iter().collect(),

        _ => {
            if TRANSPARENT_BLOCK.contains(&tag.as_str()) {
                if dom::has_class(&el, "figure") || tag == "figure" {
                    return figure_block(ctx, el);
                }
                return with_anchor(blocks(ctx, el.children()));
            }
            if !TRANSPARENT_INLINE.contains(&tag.as_str()) && !INLINE_TAGS.contains(&tag.as_str()) {
                ctx.note_unknown(&tag);
            }
            if !js::is_blank(&dom::text_of(*el)) || has_image(el) {
                with_anchor(vec![para(inline(ctx, *el))])
            } else {
                with_anchor(Vec::new())
            }
        }
    }
}

fn holds_only_images(el: ElementRef) -> bool {
    js::is_blank(&dom::text_of(*el)) && has_image(el)
}

fn figure_block(ctx: &mut Ctx, div: ElementRef) -> Vec<Block> {
    let id = div.attr("id");
    let caption_el = dom::children_named(*div, &["p", "figcaption"]).find(|p| {
        dom::is(p, "figcaption")
            || dom::descendants(**p).any(|d| {
                d.value()
                    .has_class("figlabel", CaseSensitivity::CaseSensitive)
            })
    });

    let mut figures: Vec<Figure> = Vec::new();
    let mut rest: Vec<Block> = Vec::new();
    for node in div.children() {
        if caption_el.is_some_and(|c| c.id() == node.id()) {
            continue;
        }
        if let Some(el) = ElementRef::wrap(node) {
            let is_image = IMAGES.iter().any(|name| dom::is(&el, name));
            if is_image || holds_only_images(el) {
                let candidates: Vec<ElementRef> = if is_image {
                    vec![el]
                } else {
                    dom::descendants_named(node, IMAGES).collect()
                };
                for image in candidates {
                    if dom::is(&image, "img")
                        && image
                            .ancestors()
                            .filter_map(ElementRef::wrap)
                            .any(|a| dom::is(&a, "object"))
                    {
                        continue;
                    }
                    if let Some(fig) = image_from(ctx, image) {
                        figures.push(fig);
                    }
                }
                continue;
            }
        }
        rest.extend(block_for(ctx, node));
    }

    if figures.is_empty() {
        let mut out = Vec::new();
        if let Some(id) = dom::truthy(id) {
            out.push(Block::Anchor { id: id.to_owned() });
        }
        if let Some(caption) = caption_el {
            out.push(Block::Para {
                children: trim_inline(inline(ctx, *caption)),
                align: Some(Align::Center),
                indent_em: 0,
            });
        }
        out.extend(rest);
        return out;
    }

    figures[0].id = Some(id.map(str::to_owned));
    if let Some(caption) = caption_el {
        figures[0].caption = Some(trim_inline(inline(ctx, *caption)));
    }
    figures.into_iter().map(Block::Figure).chain(rest).collect()
}

fn list(ctx: &mut Ctx, el: ElementRef) -> List {
    let ordered = dom::is(&el, "ol");
    let mut out = List {
        ordered,
        hanging: dom::has_class(&el, "hanging"),
        items: Vec::new(),
        numbering: None,
        start: None,
        labels: None,
    };

    if ordered {
        out.numbering = match el.attr("type") {
            Some("a") => Some(ListNumbering::LowerAlpha),
            Some("A") => Some(ListNumbering::UpperAlpha),
            Some("i") => Some(ListNumbering::LowerRoman),
            Some("I") => Some(ListNumbering::UpperRoman),
            _ => None,
        };
        if let Some(start) = el.attr("start").and_then(js::number)
            && js::is_integer(start)
            && start != 1.0
        {
            out.start = Some(start as i64);
        }
    }

    let mut labels = Vec::new();
    let mut any_label = false;
    for li in dom::children_named(*el, &["li"]) {
        let (label, content) = list_item(ctx, li);
        out.items.push(content);
        any_label |= label.is_some();
        labels.push(label.unwrap_or_default());
    }
    if any_label {
        out.labels = Some(labels);
    }
    out
}

fn list_item(ctx: &mut Ctx, li: ElementRef) -> (Option<Vec<Inline>>, Vec<Block>) {
    let tagged = dom::child_elements(*li)
        .next()
        .filter(|first| dom::is(first, "span") && dom::has_class(first, "tag"));
    let label = tagged.map(|tag| trim_inline(inline(ctx, *tag)));
    let skip = tagged.map(|tag| tag.id());
    let source = || li.children().filter(move |c| Some(c.id()) != skip);
    let has_blocks = source()
        .filter_map(ElementRef::wrap)
        .any(|c| ITEM_BLOCKS.iter().any(|name| dom::is(&c, name)));
    let content = if has_blocks {
        blocks(ctx, source())
    } else {
        vec![para(inline_of(ctx, source()))]
    };
    (label, content)
}

fn definition_items(ctx: &mut Ctx, dl: ElementRef) -> Vec<Vec<Block>> {
    let mut items: Vec<Vec<Block>> = Vec::new();
    for child in dom::child_elements(*dl) {
        if dom::is(&child, "dt") {
            items.push(vec![para(inline(ctx, *child))]);
        } else if dom::is(&child, "dd")
            && let Some(last) = items.last_mut()
        {
            last.extend(blocks(ctx, child.children()));
        }
    }
    items
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

fn parse_table(ctx: &mut Ctx, table: ElementRef) -> Option<Table> {
    let mut trs: Vec<ElementRef> = dom::children_named(*table, &["tr"]).collect();
    for section in dom::children_named(*table, &["thead", "tbody", "tfoot"]) {
        trs.extend(dom::children_named(*section, &["tr"]));
    }
    let mut rows: Vec<Vec<TableCell>> = Vec::new();
    for tr in trs {
        let mut cells = Vec::new();
        for cell in dom::children_named(*tr, &["td", "th"]) {
            cells.push(TableCell {
                header: dom::is(&cell, "th"),
                colspan: span_of(cell.attr("colspan"), MAX_COLSPAN),
                rowspan: span_of(cell.attr("rowspan"), MAX_ROWSPAN),
                children: inline(ctx, *cell),
                edges: None,
            });
        }
        if !cells.is_empty() {
            rows.push(cells);
        }
    }
    if rows.is_empty() {
        return None;
    }
    let columns = rows
        .iter()
        .map(|row| {
            row.iter()
                .fold(0u32, |sum, c| sum.saturating_add(c.colspan))
        })
        .max()
        .unwrap_or(0);
    let caption = dom::children_named(*table, &["caption"])
        .next()
        .filter(|c| !js::is_blank(&dom::text_of(**c)))
        .map(|c| trim_inline(inline(ctx, *c)));
    Some(Table {
        rows,
        columns,
        caption,
        rules: None,
    })
}

fn is_site_icon(abs: &str) -> bool {
    let Ok(url) = Url::parse(abs) else {
        return false;
    };
    let path = url.path();
    let file = path.rsplit('/').next().unwrap_or(path);
    ICON_DIR_RE.is_match(path) || ICON_FILE_RE.is_match(file)
}

fn image_from(ctx: &Ctx, el: ElementRef) -> Option<Figure> {
    let figure = |src: String, alt: &str| Figure {
        src,
        alt: alt.to_owned(),
        caption: None,
        id: None,
        inline: None,
        ebook_src: None,
        width_px: None,
    };
    if dom::is(&el, "object") {
        if let Some(fallback) = dom::descendants_named(*el, &["img"]).next() {
            return image_from(ctx, fallback);
        }
        let data = dom::truthy(el.attr("data"))?;
        if !IMAGE_TYPE_RE.is_match(el.attr("type").unwrap_or("")) {
            return None;
        }
        let abs = resolve(data, &ctx.base)?;
        if is_site_icon(&abs) {
            return None;
        }
        return Some(figure(abs, js::trim(el.attr("aria-label").unwrap_or(""))));
    }
    let src = dom::truthy(el.attr("src"))?;
    let abs = resolve(src, &ctx.base)?;
    if is_site_icon(&abs) {
        return None;
    }
    Some(figure(abs, js::trim(el.attr("alt").unwrap_or(""))))
}

fn inline(ctx: &mut Ctx, node: Ref) -> Vec<Inline> {
    inline_of(ctx, node.children())
}

fn inline_of<'a>(ctx: &mut Ctx, nodes: impl Iterator<Item = Ref<'a>>) -> Vec<Inline> {
    let mut out = Vec::new();
    for node in nodes {
        out.extend(inline_for(ctx, node));
    }
    out
}

fn preformatted(ctx: &mut Ctx, el: ElementRef) -> Vec<Inline> {
    let mut out = inline(ctx, *el);
    if let Some(Inline::Text { value }) = out.first_mut()
        && let Some(rest) = value
            .strip_prefix("\r\n")
            .or_else(|| value.strip_prefix('\n'))
    {
        *value = rest.to_owned();
    }
    if let Some(Inline::Text { value }) = out.last_mut() {
        *value = js::trim_end(value).to_owned();
    }
    out.retain(|n| !matches!(n, Inline::Text { value } if value.is_empty()));
    out
}

fn note_ref(ctx: &Ctx, a: ElementRef) -> Option<Inline> {
    let href = dom::truthy(a.attr("href"))?;
    if ctx.entry_dir.is_empty() {
        return None;
    }
    let abs = resolve(href, &ctx.base)?;
    let (page, fragment) = {
        let mut parts = abs.split('#');
        let page = parts.next().unwrap_or("").to_owned();
        let fragment = dom::truthy(parts.next())?.to_owned();
        (page, fragment)
    };
    let file = page.strip_prefix(ctx.entry_dir.as_str())?;
    if !NOTES_FILE_RE.is_match(file) {
        return None;
    }
    let full_text = dom::text_of(*a);
    let label = js::trim(&full_text);
    let label = label.strip_prefix('[').unwrap_or(label);
    let label = label.strip_suffix(']').unwrap_or(label);
    Some(Inline::NoteRef(NoteRef {
        id: fragment,
        label: js::trim(label).to_owned(),
        href: abs,
        ref_id: a.attr("id").or_else(|| a.attr("name")).map(str::to_owned),
    }))
}

fn inline_for(ctx: &mut Ctx, node: Ref) -> Vec<Inline> {
    let el = match node.value() {
        Node::Text(data) => return split_math(data),
        Node::Element(_) => ElementRef::wrap(node).expect("element"),
        _ => return Vec::new(),
    };

    let tag = dom::tag(&el);
    let id = el
        .attr("id")
        .or_else(|| if tag == "a" { el.attr("name") } else { None });
    let anchored = |nodes: Vec<Inline>| -> Vec<Inline> {
        match dom::truthy(id) {
            Some(id) => {
                let mut out = vec![Inline::Anchor { id: id.to_owned() }];
                out.extend(nodes);
                out
            }
            None => nodes,
        }
    };

    match tag.as_str() {
        "script" | "style" => Vec::new(),
        "em" | "i" | "cite" | "var" => anchored(vec![Inline::Emph {
            children: inline(ctx, node),
        }]),
        "strong" | "b" => anchored(vec![Inline::Strong {
            children: inline(ctx, node),
        }]),
        "sub" => anchored(vec![Inline::Sub {
            children: inline(ctx, node),
        }]),
        "sup" => {
            let links: Vec<ElementRef> = dom::children_named(node, &["a"]).collect();
            let other_text = node
                .children()
                .filter_map(|c| c.value().as_text())
                .flat_map(|t| t.chars())
                .any(|c| !js::is_space(c) && c != '[' && c != ']');
            if links.len() == 1
                && !other_text
                && dom::child_elements(node).count() == 1
                && let Some(reference) = note_ref(ctx, links[0])
            {
                return anchored(vec![reference]);
            }
            anchored(vec![Inline::Sup {
                children: inline(ctx, node),
            }])
        }
        "code" | "tt" | "kbd" | "samp" => anchored(vec![Inline::Code {
            children: inline(ctx, node),
        }]),
        "q" => {
            let mut out = vec![text("\u{201C}")];
            out.extend(inline(ctx, node));
            out.push(text("\u{201D}"));
            anchored(out)
        }
        "br" => vec![Inline::Break],
        "wbr" => Vec::new(),
        "img" | "object" => image_from(ctx, el)
            .map(|fig| {
                Inline::Figure(Figure {
                    inline: Some(true),
                    ..fig
                })
            })
            .into_iter()
            .collect(),
        "a" => {
            let href = el.attr("href");
            let children = inline(ctx, node);
            let Some(href) = dom::truthy(href) else {
                return anchored(children);
            };
            if let Some(reference) = note_ref(ctx, el) {
                return vec![reference];
            }
            if let Some(fragment) = href.strip_prefix('#') {
                return anchored(vec![Inline::Link {
                    href: fragment.to_owned(),
                    internal: true,
                    children,
                }]);
            }
            let abs = resolve(href, &ctx.base).unwrap_or_else(|| href.to_owned());
            let mut parts = abs.split('#');
            let page = parts.next().unwrap_or("");
            let fragment = parts.next();
            if ctx.self_urls.contains(page) && fragment.is_none_or(str::is_empty) {
                return anchored(children);
            }
            anchored(vec![Inline::Link {
                href: abs,
                internal: false,
                children,
            }])
        }
        "span" => {
            if dom::has_class(&el, "figlabel") {
                anchored(vec![Inline::Strong {
                    children: inline(ctx, node),
                }])
            } else if dom::has_class(&el, "tag") {
                let mut out = inline(ctx, node);
                out.push(text(" "));
                anchored(out)
            } else {
                anchored(inline(ctx, node))
            }
        }
        _ => {
            let name = tag.as_str();
            if !TRANSPARENT_INLINE.contains(&name)
                && !TRANSPARENT_BLOCK.contains(&name)
                && !BLOCKISH.contains(&name)
            {
                ctx.note_unknown(&tag);
            }
            anchored(inline(ctx, node))
        }
    }
}

fn trim_math(tex: &str) -> &str {
    let tex = js::trim_start(tex);
    let kept = js::trim_end(tex);
    let run = &tex[kept.len()..];
    if run.is_empty() || !kept.ends_with('\\') {
        return kept;
    }
    let first = run.chars().next().map_or(0, char::len_utf8);
    &tex[..kept.len() + first]
}

fn split_math(data: &str) -> Vec<Inline> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut last = 0;
    for m in MATH_RE.captures_iter(data) {
        let whole = m.get(0).expect("match");
        if whole.start() > last {
            out.push(text(&data[last..whole.start()]));
        }
        let (mode, tex) = match m.get(1) {
            Some(inline) => (MathMode::Inline, inline.as_str()),
            None => (MathMode::Display, m.get(2).map_or("", |g| g.as_str())),
        };
        out.push(Inline::Math {
            mode,
            tex: trim_math(tex).to_owned(),
        });
        last = whole.end();
    }
    if last < data.len() {
        out.push(text(&data[last..]));
    }
    out.retain(|n| !matches!(n, Inline::Text { value } if value.is_empty()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const SOURCE: &str = "https://plato.stanford.edu/entries/computer-science/";

    fn entry(html: &str) -> Doc {
        parse_entry(
            html,
            &ParseOptions {
                source_url: SOURCE,
                ..Default::default()
            },
        )
    }

    fn value<T: serde::Serialize>(v: &T) -> Value {
        serde_json::to_value(v).unwrap()
    }

    fn main_text(inner: &str) -> String {
        format!("<div id=\"main-text\">{inner}</div>")
    }

    #[test]
    fn the_walker_reaches_nodes_nested_in_lists_and_tables() {
        let d = entry(&main_text(
            "\n    <ul><li><p>a <em>b</em></p></li></ul>\n    <table><tr><td><a href=\"#x\">c</a></td></tr></table>\n  ",
        ));
        assert_eq!(
            value(&d.body),
            json!([
                { "type": "list", "ordered": false, "hanging": false, "items": [[
                    { "type": "para", "children": [{ "type": "text", "value": "a " }, { "type": "emph", "children": [{ "type": "text", "value": "b" }] }] }
                ]] },
                { "type": "table", "columns": 1, "rows": [[
                    { "header": false, "colspan": 1, "rowspan": 1, "children": [{ "type": "link", "href": "x", "internal": true, "children": [{ "type": "text", "value": "c" }] }] }
                ]] }
            ])
        );
        assert!(d.warnings.is_empty());
        assert_eq!(value(&d.back), json!([]));
        assert_eq!(value(&d.links), json!({ "notes": [], "supplements": [] }));
    }

    #[test]
    fn only_site_icons_are_dropped_from_figures() {
        let d = entry(&main_text(
            "<p><img src=\"../../symbols/sep-man-red.png\" alt=\"SEP\"/><img src=\"archive-photo.png\" alt=\"an archive\"/></p>",
        ));
        assert_eq!(
            value(&d.body),
            json!([{ "type": "para", "children": [
                { "type": "figure", "src": format!("{SOURCE}archive-photo.png"), "alt": "an archive", "inline": true }
            ] }])
        );
        assert!(is_site_icon("https://plato.stanford.edu/inpho.png"));
        assert!(is_site_icon("https://plato.stanford.edu/Symbols/x.png"));
        assert!(!is_site_icon(
            "https://plato.stanford.edu/entries/x/sepia.png"
        ));
    }

    #[test]
    fn passages_inside_pdf_exclude_markers_are_dropped() {
        let d = entry(&main_text(
            "<p>kept</p><!-- pdf exclude begin --><p>dropped</p><!-- PDF Exclude End --><p>also kept</p>",
        ));
        assert_eq!(
            value(&d.body),
            json!([
                { "type": "para", "children": [{ "type": "text", "value": "kept" }] },
                { "type": "para", "children": [{ "type": "text", "value": "also kept" }] }
            ])
        );
    }

    #[test]
    fn note_references_are_recognised_brackets_and_all() {
        let d = entry(&main_text(
            "<p>text<sup><a id=\"ref-1\" href=\"notes.html#note-1\">[1]</a></sup> and <a href=\"notes.html#note-2\">2</a> \
             and <a href=\"notes.html\">the notes</a>, <a href=\"figdesc.html\">figures</a>, <a href=\"index.html#Sec\">there</a>, \
             <a href=\"https://plato.stanford.edu/entries/computer-science/\">self</a></p>",
        ));
        let Block::Para { children, .. } = &d.body[0] else {
            panic!("a paragraph")
        };
        assert_eq!(
            value(&children[1]),
            json!({ "type": "noteref", "id": "note-1", "label": "1", "href": format!("{SOURCE}notes.html#note-1"), "refId": "ref-1" })
        );
        assert_eq!(
            value(&children[3]),
            json!({ "type": "noteref", "id": "note-2", "label": "2", "href": format!("{SOURCE}notes.html#note-2"), "refId": null })
        );
        assert_eq!(
            value(&children[5]),
            json!({ "type": "link", "href": format!("{SOURCE}notes.html"), "internal": false, "children": [{ "type": "text", "value": "the notes" }] })
        );
        assert_eq!(
            value(&children[9]),
            json!({ "type": "link", "href": format!("{SOURCE}index.html#Sec"), "internal": false, "children": [{ "type": "text", "value": "there" }] })
        );
        assert_eq!(
            value(&children[11]),
            json!({ "type": "text", "value": "self" }),
            "a link to the entry itself with no fragment is plain text"
        );
        assert_eq!(
            value(&d.links),
            json!({ "notes": [format!("{SOURCE}notes.html")], "supplements": [format!("{SOURCE}figdesc.html")] })
        );
    }

    #[test]
    fn older_notes_pages_with_inline_anchors_are_understood() {
        let html = "<div id=\"aueditable\"><h1>Notes to X</h1><p><a name=\"1\" href=\"index.html#ref-1\">[1]</a> First note.</p><p>Continued.</p>\
                    <p><a name=\"2\" href=\"index.html#ref-2\">[2]</a> Second.</p></div>";
        let att = parse_attachment(
            html,
            &AttachmentOptions {
                source_url: &format!("{SOURCE}notes.html"),
                kind: AttachmentKind::Notes,
            },
        );
        assert_eq!(att.title, "Notes to X");
        let notes = att.notes.as_ref().unwrap();
        assert_eq!(notes.keys().collect::<Vec<_>>(), ["1", "2"]);
        assert_eq!(
            value(&notes["1"]),
            json!([
                { "type": "para", "children": [{ "type": "text", "value": "First note." }] },
                { "type": "para", "children": [{ "type": "text", "value": "Continued." }] }
            ])
        );
        assert_eq!(
            value(&notes["2"]),
            json!([{ "type": "para", "children": [{ "type": "text", "value": "Second." }] }])
        );
        assert!(
            !att.blocks
                .iter()
                .any(|b| matches!(b, Block::Heading { .. })),
            "the page title is not repeated in the body"
        );
        assert_eq!(
            value(&att.blocks[0]),
            json!({ "type": "para", "children": [
                { "type": "anchor", "id": "1" },
                { "type": "link", "href": format!("{SOURCE}index.html#ref-1"), "internal": false, "children": [{ "type": "text", "value": "[1]" }] },
                { "type": "text", "value": " First note." }
            ] })
        );
    }

    #[test]
    fn a_modern_notes_page_yields_its_title_and_one_entry_per_note() {
        let html = "<html><head><title>Notes to Turing Machines (Stanford Encyclopedia of Philosophy)</title></head><body>\
                    <div id=\"aueditable\"><div id=\"pubinfo\">x</div>\
                    <div id=\"note-1\"><p>\n <a href=\"index.html#ref-1\">1.</a>\n The new version <em>was</em> influenced.</p>\
                    <ol type=\"i\"><li>one</li></ol></div>\
                    <div id=\"note-2\"><p>[2] Second.</p></div></div></body></html>";
        let att = parse_attachment(
            html,
            &AttachmentOptions {
                source_url: &format!("{SOURCE}notes.html"),
                kind: AttachmentKind::Notes,
            },
        );
        assert_eq!(
            att.title, "Notes to Turing Machines",
            "from <title>, suffix removed"
        );
        let notes = att.notes.as_ref().unwrap();
        assert_eq!(
            value(&notes["note-1"]),
            json!([
                { "type": "para", "children": [
                    { "type": "text", "value": "The new version " },
                    { "type": "emph", "children": [{ "type": "text", "value": "was" }] },
                    { "type": "text", "value": " influenced." }
                ] },
                { "type": "list", "ordered": true, "hanging": false, "numbering": "lower-roman", "items": [[
                    { "type": "para", "children": [{ "type": "text", "value": "one" }] }
                ]] }
            ])
        );
        assert_eq!(
            value(&notes["note-2"]),
            json!([{ "type": "para", "children": [{ "type": "text", "value": "Second." }] }])
        );
        assert_eq!(
            value(&att.blocks[0]),
            json!({ "type": "anchor", "id": "note-1" })
        );
        let supplement = parse_attachment(
            "<body><h2>A supplement</h2><p>Body.</p></body>",
            &AttachmentOptions {
                source_url: &format!("{SOURCE}supplement.html"),
                kind: AttachmentKind::Supplement,
            },
        );
        assert_eq!(supplement.title, "A supplement");
        assert!(supplement.notes.is_none());
        assert_eq!(
            value(&supplement.blocks),
            json!([{ "type": "para", "children": [{ "type": "text", "value": "Body." }] }])
        );
        let untitled = parse_attachment(
            "<body><p>Body.</p></body>",
            &AttachmentOptions {
                source_url: "",
                kind: AttachmentKind::Supplement,
            },
        );
        assert_eq!(untitled.title, "Supplement");
    }

    #[test]
    fn preformatted_text_keeps_its_line_breaks_and_indentation() {
        let d = entry(&main_text("<pre>\nline 1\n   indented\n</pre>"));
        assert_eq!(
            value(&d.body),
            json!([{ "type": "pre", "children": [{ "type": "text", "value": "line 1\n   indented" }] }])
        );
    }

    #[test]
    fn ordered_lists_keep_numbering_and_start_and_tags_become_labels() {
        let d = entry(&main_text("<ol type=\"i\" start=\"3\"><li>a</li></ol>"));
        assert_eq!(
            value(&d.body),
            json!([{ "type": "list", "ordered": true, "hanging": false, "numbering": "lower-roman", "start": 3, "items": [[
                { "type": "para", "children": [{ "type": "text", "value": "a" }] }
            ]] }])
        );
        let d = entry(&main_text(
            "<ul class=\"hanging\" id=\"bib\"><li><span class=\"tag\">(1)</span> <span class=\"sen\">Text</span></li><li><p>Two</p><p>paras</p></li></ul>",
        ));
        assert_eq!(
            value(&d.body),
            json!([
                { "type": "anchor", "id": "bib" },
                { "type": "list", "ordered": false, "hanging": true,
                  "labels": [[{ "type": "text", "value": "(1)" }], []],
                  "items": [
                    [{ "type": "para", "children": [{ "type": "text", "value": " " }, { "type": "text", "value": "Text" }] }],
                    [{ "type": "para", "children": [{ "type": "text", "value": "Two" }] }, { "type": "para", "children": [{ "type": "text", "value": "paras" }] }]
                  ] }
            ])
        );
        let d = entry(&main_text(
            "<ol start=\"1\" type=\"1\"><li>a</li></ol><ol start=\"\"><li>b</li></ol>",
        ));
        assert_eq!(
            value(&d.body[0])["start"],
            Value::Null,
            "start 1 is the default"
        );
        assert_eq!(
            value(&d.body[0])["numbering"],
            Value::Null,
            "decimal is the default"
        );
        assert_eq!(
            value(&d.body[1])["start"],
            json!(0),
            "an empty start attribute is 0"
        );
    }

    #[test]
    fn a_tag_in_running_text_is_followed_by_a_space() {
        let d = entry(&main_text(
            "<p><span class=\"tag\">(1)</span><span class=\"sen\">Text</span></p>",
        ));
        assert_eq!(
            value(&d.body),
            json!([{ "type": "para", "children": [
                { "type": "text", "value": "(1)" }, { "type": "text", "value": " " }, { "type": "text", "value": "Text" }
            ] }])
        );
    }

    #[test]
    fn ids_on_containers_become_anchors_and_definition_lists_hang() {
        let d = entry(&main_text(
            "<div id=\"Sec\"><p>x</p></div><dl id=\"d\"><dt>Term</dt><dd><p>Def</p></dd><dd>More</dd></dl><h3 id=\"\">Untitled</h3><p id=\"dropped\">p</p>",
        ));
        assert_eq!(
            value(&d.body),
            json!([
                { "type": "anchor", "id": "Sec" },
                { "type": "para", "children": [{ "type": "text", "value": "x" }] },
                { "type": "anchor", "id": "d" },
                { "type": "list", "ordered": false, "hanging": true, "items": [[
                    { "type": "para", "children": [{ "type": "text", "value": "Term" }] },
                    { "type": "para", "children": [{ "type": "text", "value": "Def" }] },
                    { "type": "para", "children": [{ "type": "text", "value": "More" }] }
                ]] },
                { "type": "heading", "level": 3, "id": "", "children": [{ "type": "text", "value": "Untitled" }] },
                { "type": "para", "children": [{ "type": "text", "value": "p" }] }
            ])
        );
    }

    #[test]
    fn table_cells_carry_spans_and_captions() {
        let d = entry(&main_text(
            "<table><caption>Cap</caption><tr><th colspan=\"2\">H</th></tr><tr><td rowspan=\"2\">a</td><td>b</td></tr></table>",
        ));
        assert_eq!(
            value(&d.body),
            json!([{ "type": "table", "columns": 2, "caption": [{ "type": "text", "value": "Cap" }], "rows": [
                [{ "header": true, "colspan": 2, "rowspan": 1, "children": [{ "type": "text", "value": "H" }] }],
                [{ "header": false, "colspan": 1, "rowspan": 2, "children": [{ "type": "text", "value": "a" }] },
                 { "header": false, "colspan": 1, "rowspan": 1, "children": [{ "type": "text", "value": "b" }] }]
            ] }])
        );
        assert_eq!(span_of(None, MAX_COLSPAN), 1);
        assert_eq!(span_of(Some("0"), MAX_COLSPAN), 1);
        assert_eq!(span_of(Some("-3"), MAX_COLSPAN), 1);
        assert_eq!(span_of(Some("x"), MAX_COLSPAN), 1);
        assert_eq!(span_of(Some(" 4 "), MAX_COLSPAN), 4);
        assert_eq!(span_of(Some("4294967295"), MAX_COLSPAN), MAX_COLSPAN);
        assert_eq!(span_of(Some("1e10"), MAX_ROWSPAN), MAX_ROWSPAN);
        assert_eq!(span_of(Some("Infinity"), MAX_COLSPAN), MAX_COLSPAN);
        let d = entry(&main_text("<table id=\"t\"></table>"));
        assert_eq!(
            value(&d.body),
            json!([{ "type": "anchor", "id": "t" }]),
            "an empty table leaves its anchor"
        );
    }

    #[test]
    fn figures_take_their_id_and_caption_from_the_figure_div() {
        let d = entry(&main_text(
            "<div class=\"figure\" id=\"CC\"><object data=\"anim.svg\" type=\"image/svg+xml\"><img src=\"S1.svg\" alt=\"extended description below\"></object>\
             <p><span class=\"figlabel\">Figure 1:</span> A caption <a href=\"figdesc.html\">[description]</a> </p><p>Trailing.</p></div>\
             <div class=\"figure\"><p><span class=\"figlabel\">Figure 2:</span> no image</p></div>",
        ));
        assert_eq!(
            value(&d.body),
            json!([
                { "type": "figure", "src": format!("{SOURCE}S1.svg"), "alt": "extended description below", "id": "CC",
                  "caption": [
                    { "type": "strong", "children": [{ "type": "text", "value": "Figure 1:" }] },
                    { "type": "text", "value": " A caption " },
                    { "type": "link", "href": format!("{SOURCE}figdesc.html"), "internal": false, "children": [{ "type": "text", "value": "[description]" }] }
                  ] },
                { "type": "para", "children": [{ "type": "text", "value": "Trailing." }] },
                { "type": "para", "align": "center", "children": [
                    { "type": "strong", "children": [{ "type": "text", "value": "Figure 2:" }] },
                    { "type": "text", "value": " no image" }
                ] }
            ])
        );
        let d = entry(&main_text(
            "<div class=\"figure\"><img src=\"a.png\"></div>",
        ));
        assert_eq!(
            value(&d.body),
            json!([{ "type": "figure", "src": format!("{SOURCE}a.png"), "alt": "", "id": null }]),
            "a figure without an id keeps the null"
        );
    }

    #[test]
    fn mathjax_delimiters_become_math_nodes() {
        let nodes = split_math("Let \\(x\\) be \\[ y^2 \\] and \\(a\\ \\) too");
        assert_eq!(
            value(&nodes),
            json!([
                { "type": "text", "value": "Let " },
                { "type": "math", "mode": "inline", "tex": "x" },
                { "type": "text", "value": " be " },
                { "type": "math", "mode": "display", "tex": "y^2" },
                { "type": "text", "value": " and " },
                { "type": "math", "mode": "inline", "tex": "a\\ " },
                { "type": "text", "value": " too" }
            ])
        );
        assert_eq!(
            trim_math("  a \\  "),
            "a \\ ",
            "one space stays after a backslash"
        );
        assert_eq!(trim_math("a \\ "), "a \\ ");
        assert_eq!(trim_math("a\\"), "a\\");
        assert_eq!(trim_math(" a  "), "a");
        assert_eq!(
            trim_math("\\\\ "),
            "\\\\ ",
            "an escaped backslash still ends in a backslash"
        );
        let d = entry(&main_text("<p>x</p>\\[ d \\]<p>y</p>   "));
        assert_eq!(
            value(&d.body[1]),
            json!({ "type": "para", "children": [{ "type": "math", "mode": "display", "tex": "d" }] })
        );
        assert_eq!(d.body.len(), 3, "whitespace between blocks is dropped");
    }

    #[test]
    fn markup_the_parser_does_not_know_is_reported_once_per_tag() {
        let d = entry(&main_text(
            "<p>a <marquee>b</marquee> <marquee>c</marquee></p><ruby>x</ruby><p><q>quoted</q></p><p>a<br>b<wbr>c</p>",
        ));
        assert_eq!(
            d.warnings,
            [
                "unhandled <marquee> (2×) treated as plain content",
                "unhandled <ruby> (1×) treated as plain content"
            ]
        );
        assert_eq!(
            value(&d.body[1]),
            json!({ "type": "para", "children": [{ "type": "text", "value": "x" }] })
        );
        assert_eq!(
            value(&d.body[2]),
            json!({ "type": "para", "children": [{ "type": "text", "value": "\u{201C}" }, { "type": "text", "value": "quoted" }, { "type": "text", "value": "\u{201D}" }] })
        );
        assert_eq!(
            value(&d.body[3]),
            json!({ "type": "para", "children": [{ "type": "text", "value": "a" }, { "type": "break" }, { "type": "text", "value": "b" }, { "type": "text", "value": "c" }] })
        );
    }

    #[test]
    fn the_contents_list_nests() {
        let d = entry(
            "<div id=\"toc\"><ul><li><a href=\"#A\">1. A</a><ul><li><a href=\"#A1\"> 1.1 A1 </a></li></ul></li><li>Plain <ul><li>x</li></ul></li></ul></div>",
        );
        assert_eq!(
            value(&d.toc),
            json!([
                { "label": [{ "type": "text", "value": "1. A" }], "href": "#A", "children": [
                    { "label": [{ "type": "text", "value": "1.1 A1" }], "href": "#A1", "children": [] }
                ] },
                { "label": [{ "type": "text", "value": "Plain" }], "href": "", "children": [
                    { "label": [{ "type": "text", "value": "x" }], "href": "", "children": [] }
                ] }
            ])
        );
    }

    #[test]
    fn a_contents_line_is_running_text_as_a_heading_is() {
        let d = entry(
            "<div id=\"toc\"><ul><li><a href=\"#Coo\"> 5. Common \\(p\\)-Belief in <em>Games</em> </a></li>\
             <li><a href=\"#Sub\"><span id=\"x\">Subsystems</span> of <b>Z</b><sub>2</sub><img src=\"z.png\" alt=\"z\"/></a></li>\
             <li>Unlinked<sup><a href=\"notes.html#note-1\">[1]</a></sup><ul><li><a href=\"#U1\">U1</a></li></ul></li>\
             <li><a href=\"#Aca\"><em>Academic</em> Tools</a></li></ul></div>\
             <div id=\"main-text\"><h2 id=\"Coo\">5. Common \\(p\\)-Belief in <em>Games</em></h2></div>",
        );
        assert_eq!(
            value(&d.toc[0].label),
            json!([
                { "type": "text", "value": "5. Common " },
                { "type": "math", "mode": "inline", "tex": "p" },
                { "type": "text", "value": "-Belief in " },
                { "type": "emph", "children": [{ "type": "text", "value": "Games" }] }
            ]),
            "a formula is maths and emphasis stays"
        );
        let Block::Heading { children, .. } = &d.body[0] else {
            panic!("a heading: {:?}", d.body[0]);
        };
        assert_eq!(
            &d.toc[0].label, children,
            "as in the heading the line names"
        );
        assert_eq!(
            value(&d.toc[1].label),
            json!([
                { "type": "text", "value": "Subsystems" },
                { "type": "text", "value": " of " },
                { "type": "strong", "children": [{ "type": "text", "value": "Z" }] },
                { "type": "sub", "children": [{ "type": "text", "value": "2" }] }
            ]),
            "no anchor and no figure"
        );
        assert_eq!(
            value(&d.toc[2].label),
            json!([{ "type": "text", "value": "Unlinked" }]),
            "a line with no link is its own text, without a note reference"
        );
        assert_eq!(
            d.toc.len(),
            3,
            "the academic tools are matched by their text, markup and all, and left out"
        );
    }

    #[test]
    fn the_contents_leave_out_end_matter_the_page_does_not_have() {
        let d = entry(
            "<div id=\"toc\"><ul><li><a href=\"#S\">1. S</a><ul><li><a href=\"#T\"> ACADEMIC tools </a></li>\
             <li><a href=\"#U\">Academic Tools and More</a></li></ul></li><li><a href=\"#Bib\">Bibliography</a></li>\
             <li><a href=\"#Aca\">Academic Tools</a><ul><li><a href=\"#x\">x</a></li></ul></li>\
             <li><a href=\"#Oth\">Other Internet Resources</a></li><li><a href=\"#Rel\">Related Entries</a></li></ul></div>\
             <div id=\"main-text\"><h2 id=\"S\">S</h2></div><div id=\"bibliography\"><p>Ref</p></div>\
             <div id=\"other-internet-resources\"><p>Web</p></div><div id=\"related-entries\"><p>See</p></div>",
        );
        let texts = |items: &[TocItem]| items.iter().map(TocItem::text).collect::<Vec<_>>();
        assert_eq!(
            texts(&d.toc),
            [
                "1. S",
                "Bibliography",
                "Other Internet Resources",
                "Related Entries"
            ],
            "the academic tools are not on the page, so not in its contents"
        );
        assert_eq!(
            texts(&d.toc[0].children),
            ["Academic Tools and More"],
            "at any depth, by the whole label and nothing looser"
        );
    }

    #[test]
    fn the_preamble_and_the_end_matter_sections_are_read() {
        let d = entry(
            "<div id=\"preamble\"><p>Pre</p></div><div id=\"main-text\"><h2 id=\"S\">S</h2></div>\
             <div id=\"related-entries\"><p><a href=\"../logic/\">logic</a></p></div><div id=\"bibliography\"><ul class=\"hanging\"><li>Ref</li></ul></div>",
        );
        assert_eq!(
            value(&d.front),
            json!([{ "type": "para", "children": [{ "type": "text", "value": "Pre" }] }])
        );
        assert_eq!(
            value(&d.body),
            json!([{ "type": "heading", "level": 2, "id": "S", "children": [{ "type": "text", "value": "S" }] }])
        );
        assert_eq!(
            d.back
                .iter()
                .map(|s| (s.key.as_str(), s.label.as_str()))
                .collect::<Vec<_>>(),
            [("bib", "Bibliography"), ("related", "Related Entries")],
            "the sections there are, in the SEP's order rather than the page's"
        );
        assert_eq!(
            value(&d.back[1].blocks),
            json!([{ "type": "para", "children": [{ "type": "link", "href": "https://plato.stanford.edu/entries/logic/", "internal": false, "children": [{ "type": "text", "value": "logic" }] }] }])
        );
    }

    #[test]
    fn without_a_source_url_nothing_resolves() {
        let d = parse_entry(
            &main_text("<p><a href=\"x.html\">x</a><img src=\"a.png\"></p>"),
            &ParseOptions::default(),
        );
        assert_eq!(
            value(&d.body),
            json!([{ "type": "para", "children": [
                { "type": "link", "href": "x.html", "internal": false, "children": [{ "type": "text", "value": "x" }] }
            ] }]),
            "the raw href is kept and the image, which cannot be resolved, is dropped"
        );
        assert_eq!(value(&d.links), json!({ "notes": [], "supplements": [] }));
    }
}
