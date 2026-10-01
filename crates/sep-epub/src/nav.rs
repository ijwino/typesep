use std::collections::BTreeSet;

use sep_core::model::{AttachmentKind, Doc, TocItem};
use sep_core::sections::ContentKey;
use sep_typst::style::View;

use crate::xhtml::{esc, xml_id};

fn render(
    items: &[TocItem],
    depth: i64,
    max: i64,
    file: &str,
    dropped: &BTreeSet<String>,
    ids: &BTreeSet<String>,
) -> String {
    if depth > max {
        return String::new();
    }
    let kept: Vec<&TocItem> = items
        .iter()
        .filter(|item| !dropped.contains(&item.text.trim().to_lowercase()))
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    let lis: String = kept
        .iter()
        .filter_map(|item| {
            let children = render(&item.children, depth + 1, max, file, dropped, ids);
            let target = item
                .href
                .strip_prefix('#')
                .map(xml_id)
                .filter(|id| ids.contains(id));
            let label = esc(&item.text);
            match target {
                Some(id) => Some(format!(
                    "<li><a href=\"{}\">{label}</a>{children}</li>",
                    esc(&format!("{file}#{id}"))
                )),
                None if !children.is_empty() => {
                    Some(format!("<li><span>{label}</span>{children}</li>"))
                }
                None => None,
            }
        })
        .collect();
    if lis.is_empty() {
        return String::new();
    }
    format!("<ol>{lis}</ol>")
}

pub fn nav_items(
    doc: &Doc,
    file: &str,
    view: &View,
    sections: &BTreeSet<ContentKey>,
    ids: &BTreeSet<String>,
) -> String {
    let dropped: BTreeSet<String> = doc
        .back
        .iter()
        .filter(|section| !sections.iter().any(|key| key.name() == section.key))
        .map(|section| section.label.to_lowercase())
        .collect();
    let appendix = view.str("notes.style") == "appendix";
    let extras: Vec<String> = doc
        .attachments
        .iter()
        .enumerate()
        .filter(|(_, a)| match a.kind {
            AttachmentKind::Notes => appendix && sections.contains(&ContentKey::Notes),
            AttachmentKind::Supplement => sections.contains(&ContentKey::Supplements),
        })
        .filter_map(|(i, a)| {
            let id = xml_id(&format!("a{}--attachment", i + 1));
            ids.contains(&id)
                .then(|| format!("<li><a href=\"{file}#{id}\">{}</a></li>", esc(&a.title)))
        })
        .collect();
    let own = render(&doc.toc, 1, view.int("toc.depth"), file, &dropped, ids);
    if own.is_empty() && extras.is_empty() {
        return String::new();
    }
    if own.is_empty() {
        return format!("<ol>{}</ol>", extras.concat());
    }
    match own.strip_suffix("</ol>") {
        Some(head) => format!("{head}{}</ol>", extras.concat()),
        None => own,
    }
}
