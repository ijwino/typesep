use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use sep_core::model::{
    Attachment, AttachmentKind, Block, Doc, Figure, Inline, List, MathMode, NoteRef, NoteTable,
    Table,
};
use sep_core::sections::{ContentKey, selected_sections};
use sep_typst::images::Image;
use sep_typst::mathml::Formula;
use sep_typst::style::View;

pub fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            '\u{0}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}' => {}
            c => out.push(c),
        }
    }
    out
}

pub fn xml_id(id: &str) -> String {
    let clean: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    match clean.chars().next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => clean,
        _ => format!("x{clean}"),
    }
}

fn list_type(numbering: sep_core::model::ListNumbering) -> &'static str {
    use sep_core::model::ListNumbering::*;
    match numbering {
        LowerAlpha => "a",
        UpperAlpha => "A",
        LowerRoman => "i",
        UpperRoman => "I",
        Decimal => "1",
    }
}

pub struct NoteBody {
    pub id: String,
    pub label: String,
    pub ref_id: Option<String>,
    pub blocks: Vec<String>,
}

pub enum MathPass {
    Collect(Vec<Formula>),
    Fill {
        items: Vec<Result<String, String>>,
        next: usize,
    },
}

pub struct Ctx<'a> {
    pub view: &'a View<'a>,
    pub images: &'a BTreeMap<String, Image>,
    pub assets: Vec<(PathBuf, String)>,
    pub notes: NoteTable,
    pub attached: BTreeMap<String, usize>,
    pub attachment_index: Option<usize>,
    pub self_urls: BTreeSet<String>,
    pub note_bodies: Vec<NoteBody>,
    pub has_math: bool,
    pub in_note: bool,
    pub math: MathPass,
    pub warnings: Vec<String>,
    pub ids: BTreeSet<String>,
    written: BTreeSet<String>,
}

impl<'a> Ctx<'a> {
    pub fn new(view: &'a View<'a>, images: &'a BTreeMap<String, Image>, math: MathPass) -> Ctx<'a> {
        Ctx {
            view,
            images,
            assets: Vec::new(),
            notes: NoteTable::new(),
            attached: BTreeMap::new(),
            attachment_index: None,
            self_urls: BTreeSet::new(),
            note_bodies: Vec::new(),
            has_math: false,
            in_note: false,
            math,
            warnings: Vec::new(),
            ids: BTreeSet::new(),
            written: BTreeSet::new(),
        }
    }

    fn collecting(&self) -> bool {
        matches!(self.math, MathPass::Collect(_))
    }

    fn id_attr(&mut self, id: String) -> String {
        if self.collecting() {
            self.ids.insert(id.clone());
        }
        if self.written.insert(id.clone()) {
            format!(" id=\"{id}\"")
        } else {
            String::new()
        }
    }

    fn has_id(&self, id: &str) -> bool {
        self.collecting() || self.ids.contains(id)
    }

    pub fn collected(&mut self) -> Vec<Formula> {
        match &mut self.math {
            MathPass::Collect(formulas) => std::mem::take(formulas),
            MathPass::Fill { .. } => Vec::new(),
        }
    }

    fn id_ref(&self, id: &str) -> String {
        self.id_ref_in(id, self.attachment_index)
    }

    fn id_ref_in(&self, id: &str, attachment: Option<usize>) -> String {
        match attachment {
            None => xml_id(id),
            Some(n) => xml_id(&format!("a{n}--{id}")),
        }
    }

    fn resolve_target(&self, href: &str) -> Option<String> {
        let (page, fragment) = match href.split_once('#') {
            Some((page, fragment)) => (page, fragment),
            None => (href, ""),
        };
        if let Some(attachment) = self.attached.get(page) {
            let id = if fragment.is_empty() {
                self.id_ref_in("attachment", Some(*attachment))
            } else {
                self.id_ref_in(fragment, Some(*attachment))
            };
            return self.has_id(&id).then_some(id);
        }
        if self.self_urls.contains(page) && !fragment.is_empty() {
            let id = self.id_ref_in(fragment, None);
            return self.has_id(&id).then_some(id);
        }
        None
    }

    fn links_are_dropped(&self) -> bool {
        self.view.str("links.mode") == "none"
    }
}

pub fn inline_html(nodes: &[Inline], ctx: &mut Ctx) -> String {
    nodes.iter().map(|n| inline_node(n, ctx)).collect()
}

fn inline_node(node: &Inline, ctx: &mut Ctx) -> String {
    match node {
        Inline::Text { value } => esc(value),
        Inline::Emph { children } => format!("<em>{}</em>", inline_html(children, ctx)),
        Inline::Strong { children } => format!("<strong>{}</strong>", inline_html(children, ctx)),
        Inline::Sub { children } => format!("<sub>{}</sub>", inline_html(children, ctx)),
        Inline::Sup { children } => format!("<sup>{}</sup>", inline_html(children, ctx)),
        Inline::Code { children } => format!("<code>{}</code>", inline_html(children, ctx)),
        Inline::Break => "<br/>".to_owned(),
        Inline::Anchor { id } => {
            let id = ctx.id_ref(id);
            format!("<span{}></span>", ctx.id_attr(id))
        }
        Inline::Figure(figure) => image_html(figure, ctx),
        Inline::NoteRef(note) => note_ref(note, ctx),
        Inline::Math { mode, tex } => math_html(tex, *mode == MathMode::Display, ctx),
        Inline::Link {
            href,
            internal,
            children,
        } => {
            let text = inline_html(children, ctx);
            if *internal {
                let id = ctx.id_ref(href);
                return match ctx.has_id(&id) {
                    true => format!("<a href=\"#{id}\">{text}</a>"),
                    false => text,
                };
            }
            if let Some(target) = ctx.resolve_target(href) {
                return format!("<a href=\"#{target}\">{text}</a>");
            }
            if ctx.links_are_dropped() {
                return text;
            }
            format!("<a href=\"{}\">{text}</a>", esc(href))
        }
    }
}

fn note_ref(node: &NoteRef, ctx: &mut Ctx) -> String {
    let id_attr = match &node.ref_id {
        Some(ref_id) => {
            let id = xml_id(ref_id);
            ctx.id_attr(id)
        }
        None => String::new(),
    };
    let label = esc(&node.label);
    let content = ctx.notes.get(&node.id).cloned();
    if let Some(content) = content
        && ctx.view.str("notes.style") != "appendix"
        && !ctx.in_note
    {
        if !ctx.note_bodies.iter().any(|n| n.id == node.id) {
            ctx.in_note = true;
            let blocks = blocks_html(&content, ctx);
            ctx.in_note = false;
            ctx.note_bodies.push(NoteBody {
                id: node.id.clone(),
                label: node.label.clone(),
                ref_id: node.ref_id.clone(),
                blocks,
            });
        }
        return format!(
            "<sup><a class=\"noteref\" epub:type=\"noteref\" href=\"#{}\"{id_attr}>{label}</a></sup>",
            xml_id(&node.id)
        );
    }
    if let Some(target) = ctx.resolve_target(&node.href) {
        return format!("<sup>[<a href=\"#{target}\"{id_attr}>{label}</a>]</sup>");
    }
    if ctx.links_are_dropped() {
        return format!("<sup{id_attr}>[{label}]</sup>");
    }
    format!(
        "<sup>[<a href=\"{}\"{id_attr}>{label}</a>]</sup>",
        esc(&node.href)
    )
}

fn math_html(tex: &str, display: bool, ctx: &mut Ctx) -> String {
    match &mut ctx.math {
        MathPass::Collect(formulas) => {
            formulas.push(Formula {
                tex: tex.to_owned(),
                display,
            });
            String::new()
        }
        MathPass::Fill { items, next } => {
            let item = items.get(*next);
            *next += 1;
            match item {
                Some(Ok(math)) => {
                    let math = math.clone();
                    ctx.has_math = true;
                    math
                }
                Some(Err(reason)) => {
                    let reason = reason.clone();
                    ctx.warnings.push(format!(
                        "maths left as LaTeX, mitex could not convert it: {reason}"
                    ));
                    format!("<code>{}</code>", esc(tex))
                }
                None => format!("<code>{}</code>", esc(tex)),
            }
        }
    }
}

fn image_html(node: &Figure, ctx: &mut Ctx) -> String {
    let Some(image) = ctx.images.get(&node.src) else {
        return String::new();
    };
    let name = match ctx.assets.iter().find(|(file, _)| *file == image.file) {
        Some((_, name)) => name.clone(),
        None => {
            let name = format!("img-{}.{}", ctx.assets.len() + 1, image.format);
            ctx.assets.push((image.file.clone(), name.clone()));
            name
        }
    };
    format!("<img src=\"{}\" alt=\"{}\"/>", esc(&name), esc(&node.alt))
}

pub fn blocks_html(nodes: &[Block], ctx: &mut Ctx) -> Vec<String> {
    nodes
        .iter()
        .map(|b| block_html(b, ctx))
        .filter(|html| !html.is_empty())
        .collect()
}

fn list_html(node: &List, ctx: &mut Ctx) -> String {
    if let Some(labels) = &node.labels {
        let items: String = node
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let label = match labels.get(i) {
                    Some(label) if !label.is_empty() => {
                        format!("<span class=\"tag\">{}</span> ", inline_html(label, ctx))
                    }
                    _ => String::new(),
                };
                if let [Block::Para { children, .. }] = item.as_slice() {
                    return format!("<li>{label}{}</li>", inline_html(children, ctx));
                }
                format!("<li>{label}{}</li>", blocks_html(item, ctx).join(""))
            })
            .collect();
        return format!("<ul class=\"tags\">{items}</ul>");
    }
    let tag = if node.ordered { "ol" } else { "ul" };
    let mut attrs = String::new();
    if node.hanging {
        attrs.push_str(" class=\"hanging\"");
    }
    if node.ordered
        && let Some(numbering) = node.numbering
    {
        attrs.push_str(&format!(" type=\"{}\"", list_type(numbering)));
    }
    if node.ordered
        && let Some(start) = node.start
    {
        attrs.push_str(&format!(" start=\"{start}\""));
    }
    let items: String = node
        .items
        .iter()
        .map(|item| format!("<li>{}</li>", blocks_html(item, ctx).join("")))
        .collect();
    format!("<{tag}{attrs}>{items}</{tag}>")
}

fn table_html(node: &Table, ctx: &mut Ctx) -> String {
    let rows: String = node
        .rows
        .iter()
        .map(|cells| {
            let html: String = cells
                .iter()
                .map(|c| {
                    let tag = if c.header { "th" } else { "td" };
                    let mut span = String::new();
                    if c.colspan > 1 {
                        span.push_str(&format!(" colspan=\"{}\"", c.colspan));
                    }
                    if c.rowspan > 1 {
                        span.push_str(&format!(" rowspan=\"{}\"", c.rowspan));
                    }
                    format!("<{tag}{span}>{}</{tag}>", inline_html(&c.children, ctx))
                })
                .collect();
            format!("<tr>{html}</tr>")
        })
        .collect();
    let caption = match &node.caption {
        Some(caption) => format!("<caption>{}</caption>", inline_html(caption, ctx)),
        None => String::new(),
    };
    format!("<table>{caption}{rows}</table>")
}

fn block_html(node: &Block, ctx: &mut Ctx) -> String {
    match node {
        Block::Heading {
            level,
            id,
            children,
        } => {
            let id_attr = match id.as_deref().filter(|id| !id.is_empty()) {
                Some(id) => {
                    let id = ctx.id_ref(id);
                    ctx.id_attr(id)
                }
                None => String::new(),
            };
            format!(
                "<h{level}{id_attr}>{}</h{level}>",
                inline_html(children, ctx)
            )
        }
        Block::Para { children, align } => {
            let class = match align {
                Some(_) => " class=\"center\"".to_owned(),
                None => String::new(),
            };
            format!("<p{class}>{}</p>", inline_html(children, ctx))
        }
        Block::Pre { children } => format!("<pre>{}</pre>", inline_html(children, ctx)),
        Block::Blockquote { children } => {
            format!(
                "<blockquote>{}</blockquote>",
                blocks_html(children, ctx).join("")
            )
        }
        Block::List(list) => list_html(list, ctx),
        Block::Table(table) => table_html(table, ctx),
        Block::Figure(figure) => {
            let img = image_html(figure, ctx);
            let id = figure.id.clone().flatten().filter(|id| !id.is_empty());
            let id_attr = match &id {
                Some(id) => {
                    let id = ctx.id_ref(id);
                    ctx.id_attr(id)
                }
                None => String::new(),
            };
            if img.is_empty() {
                let label = match &figure.caption {
                    Some(caption) if !caption.is_empty() => {
                        format!("<p class=\"center\">{}</p>", inline_html(caption, ctx))
                    }
                    _ => String::new(),
                };
                return match id {
                    Some(_) => format!("<div{id_attr}></div>{label}"),
                    None => label,
                };
            }
            let caption = match &figure.caption {
                Some(caption) if !caption.is_empty() => {
                    format!("<figcaption>{}</figcaption>", inline_html(caption, ctx))
                }
                _ => String::new(),
            };
            format!("<figure{id_attr}>{img}{caption}</figure>")
        }
        Block::Rule => "<hr/>".to_owned(),
        Block::Anchor { id } => {
            let id = ctx.id_ref(id);
            format!("<div{}></div>", ctx.id_attr(id))
        }
    }
}

pub fn document_xhtml(title: &str, body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="en" xml:lang="en">
<head><meta charset="utf-8"/><title>{}</title>
<link rel="stylesheet" type="text/css" href="style.css"/></head>
<body>{body}</body></html>"#,
        esc(title)
    )
}

fn attachment_html(extra: &Attachment, ctx: &mut Ctx) -> String {
    let page = extra
        .source_url
        .split_once('#')
        .map(|(page, _)| page)
        .unwrap_or(&extra.source_url);
    ctx.attachment_index = ctx.attached.get(page).copied();
    let id = ctx.id_ref("attachment");
    let mut parts = vec![format!("<h2{}>{}</h2>", ctx.id_attr(id), esc(&extra.title))];
    parts.extend(blocks_html(&extra.blocks, ctx));
    ctx.attachment_index = None;
    parts.join("\n")
}

fn notes_section(ctx: &mut Ctx) -> String {
    if ctx.note_bodies.is_empty() {
        return String::new();
    }
    let notes = std::mem::take(&mut ctx.note_bodies);
    let asides: Vec<String> = notes
        .iter()
        .map(|note| {
            let back = match &note.ref_id {
                Some(ref_id) => {
                    format!("<a href=\"#{}\">{}.</a>", xml_id(ref_id), esc(&note.label))
                }
                None => format!("{}.", esc(&note.label)),
            };
            let id = xml_id(&note.id);
            let id_attr = ctx.id_attr(id);
            let mut blocks = note.blocks.iter();
            let first = blocks.next().map(String::as_str).unwrap_or("");
            let (lead, rest): (String, String) = match first
                .strip_prefix("<p>")
                .and_then(|inner| inner.strip_suffix("</p>"))
            {
                Some(inner) => (
                    format!("<p class=\"flush\">{back} {inner}</p>"),
                    blocks.cloned().collect(),
                ),
                None => (
                    format!("<p class=\"flush\">{back}</p>"),
                    note.blocks.concat(),
                ),
            };
            format!("<aside class=\"note\" epub:type=\"footnote\"{id_attr}>{lead}{rest}</aside>")
        })
        .collect();
    ctx.note_bodies = notes;
    format!(
        "<section class=\"notes\" epub:type=\"endnotes\"><h2>Notes</h2>{}</section>",
        asides.join("\n")
    )
}

pub fn entry_html(doc: &Doc, ctx: &mut Ctx, sections: &BTreeSet<ContentKey>) -> String {
    ctx.notes = doc
        .attachments
        .iter()
        .filter(|a| a.kind == AttachmentKind::Notes)
        .flat_map(|a| a.notes.iter().flatten())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    ctx.attached = doc
        .attachments
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let page = a
                .source_url
                .split_once('#')
                .map(|(page, _)| page)
                .unwrap_or(&a.source_url);
            (page.to_owned(), i + 1)
        })
        .collect();
    ctx.attachment_index = None;
    ctx.self_urls = BTreeSet::new();
    if !doc.meta.origin.url.is_empty() {
        let url = &doc.meta.origin.url;
        let directory = match url.rfind('/') {
            Some(at) => &url[..=at],
            None => "",
        };
        ctx.self_urls.insert(url.clone());
        ctx.self_urls.insert(format!("{directory}index.html"));
    }
    ctx.note_bodies = Vec::new();
    ctx.written = BTreeSet::new();

    let mut parts = vec![format!("<h1>{}</h1>", esc(&doc.meta.title))];
    if !doc.meta.dateline.is_empty() {
        parts.push(format!(
            "<p class=\"flush\"><em>{}</em></p>",
            esc(&doc.meta.dateline)
        ));
    }
    parts.extend(blocks_html(&doc.front, ctx));
    parts.extend(blocks_html(&doc.body, ctx));

    if ctx.view.str("notes.style") == "appendix" && sections.contains(&ContentKey::Notes) {
        for extra in doc
            .attachments
            .iter()
            .filter(|a| a.kind == AttachmentKind::Notes)
        {
            parts.push(attachment_html(extra, ctx));
        }
    }
    for section in selected_sections(doc, sections) {
        parts.extend(blocks_html(&section.blocks, ctx));
    }
    if sections.contains(&ContentKey::Supplements) {
        for extra in doc
            .attachments
            .iter()
            .filter(|a| a.kind == AttachmentKind::Supplement)
        {
            parts.push(attachment_html(extra, ctx));
        }
    }
    parts.push(notes_section(ctx));
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
