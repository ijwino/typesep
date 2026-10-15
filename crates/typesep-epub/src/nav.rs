use std::collections::BTreeSet;

use typesep_model::{AttachmentKind, Doc, TocItem};

use crate::xhtml::{Ctx, OWN_NOTES_ID, esc, inline_html, xml_id};

fn render(
    items: &[TocItem],
    depth: i64,
    max: i64,
    file: &str,
    dropped: &BTreeSet<String>,
    ctx: &mut Ctx,
) -> String {
    if depth > max {
        return String::new();
    }
    let kept: Vec<&TocItem> = items
        .iter()
        .filter(|item| !dropped.contains(&item.text().trim().to_lowercase()))
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    let lis: String = kept
        .iter()
        .filter_map(|item| {
            let children = render(&item.children, depth + 1, max, file, dropped, ctx);
            let target = item
                .href
                .strip_prefix('#')
                .map(xml_id)
                .filter(|id| ctx.ids.contains(id));
            if target.is_none() && children.is_empty() {
                return None;
            }
            let label = inline_html(&item.label, ctx);
            match target {
                Some(id) => Some(format!(
                    "<li><a href=\"{}\">{label}</a>{children}</li>",
                    esc(&format!("{file}#{id}"))
                )),
                None => Some(format!("<li><span>{label}</span>{children}</li>")),
            }
        })
        .collect();
    if lis.is_empty() {
        return String::new();
    }
    format!("<ol>{lis}</ol>")
}

/// The lines of the contents of `doc`, set in `file`, as the items of an ordered list. Their
/// labels are running text, whose formulas `ctx` collects or fills in as it does the
/// document's.
pub fn nav_items(doc: &Doc, file: &str, sections: &BTreeSet<String>, ctx: &mut Ctx) -> String {
    let dropped: BTreeSet<String> = doc
        .back
        .iter()
        .filter(|section| !sections.contains(&section.key))
        .map(|section| section.label.to_lowercase())
        .collect();
    let appendix = ctx.view.str("notes.style") == "appendix";
    let listed: Vec<(AttachmentKind, String)> = doc
        .attachments
        .iter()
        .enumerate()
        .filter(|(_, a)| match a.kind {
            AttachmentKind::Notes => appendix && sections.contains("notes"),
            AttachmentKind::Supplement => sections.contains("supplements"),
        })
        .filter_map(|(i, a)| {
            let id = xml_id(&format!("a{}--attachment", i + 1));
            ctx.ids.contains(&id).then(|| {
                (
                    a.kind,
                    format!("<li><a href=\"{file}#{id}\">{}</a></li>", esc(&a.title)),
                )
            })
        })
        .collect();
    let mut extras: Vec<String> = listed.iter().map(|(_, item)| item.clone()).collect();
    if appendix && sections.contains("notes") && ctx.ids.contains(OWN_NOTES_ID) {
        // The document's own notes are set after the attached notes pages, so they are
        // listed after the last of them.
        let at = listed
            .iter()
            .rposition(|(kind, _)| *kind == AttachmentKind::Notes)
            .map_or(0, |last| last + 1);
        extras.insert(
            at,
            format!("<li><a href=\"{file}#{OWN_NOTES_ID}\">Notes</a></li>"),
        );
    }
    let depth = ctx.view.int("toc.depth");
    let first = 1 - doc.contents_above_headings();
    let own = render(&doc.toc, first, depth, file, &dropped, ctx);
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
