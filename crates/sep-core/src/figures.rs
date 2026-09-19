use crate::entry::Source;
use crate::fetch::{FetchOptions, FetchResult};
use crate::model::{Block, EntryDoc, Figure, Inline};

pub fn figure_urls(doc: &EntryDoc) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |figure: &Figure| {
        if !figure.src.is_empty() && seen.insert(figure.src.clone()) {
            out.push(figure.src.clone());
        }
    };
    let mut blocks: Vec<&Block> = Vec::new();
    blocks.extend(doc.preamble.iter());
    blocks.extend(doc.body.iter());
    for section in [
        &doc.sections.bibliography,
        &doc.sections.academic_tools,
        &doc.sections.other_internet_resources,
        &doc.sections.related_entries,
    ] {
        blocks.extend(section.iter().flat_map(|s| s.iter()));
    }
    for attachment in &doc.attachments {
        blocks.extend(attachment.blocks.iter());
        for note in attachment.notes.iter().flat_map(|n| n.values()) {
            blocks.extend(note.iter());
        }
    }
    for block in blocks {
        walk_block(block, &mut push);
    }
    out
}

fn walk_block(block: &Block, push: &mut impl FnMut(&Figure)) {
    match block {
        Block::Para { children, .. }
        | Block::Pre { children }
        | Block::Heading { children, .. } => walk_inlines(children, push),
        Block::List(list) => {
            for item in &list.items {
                for block in item {
                    walk_block(block, push);
                }
            }
            for label in list.labels.iter().flatten() {
                walk_inlines(label, push);
            }
        }
        Block::Blockquote { children } => {
            for block in children {
                walk_block(block, push);
            }
        }
        Block::Table(table) => {
            for caption in table.caption.iter() {
                walk_inlines(caption, push);
            }
            for row in &table.rows {
                for cell in row {
                    walk_inlines(&cell.children, push);
                }
            }
        }
        Block::Figure(figure) => {
            push(figure);
            for caption in figure.caption.iter() {
                walk_inlines(caption, push);
            }
        }
        Block::Rule | Block::Anchor { .. } => {}
    }
}

fn walk_inlines(nodes: &[Inline], push: &mut impl FnMut(&Figure)) {
    for node in nodes {
        match node {
            Inline::Figure(figure) => {
                push(figure);
                for caption in figure.caption.iter() {
                    walk_inlines(caption, push);
                }
            }
            Inline::Emph { children }
            | Inline::Strong { children }
            | Inline::Sub { children }
            | Inline::Sup { children }
            | Inline::Code { children }
            | Inline::Link { children, .. } => walk_inlines(children, push),
            _ => {}
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FigureFetch {
    pub present: Vec<String>,
    pub fetched: usize,
    pub missing: Vec<String>,
    pub failed: Vec<(String, String)>,
}

pub fn fetch_figures(
    doc: &EntryDoc,
    source: &mut Source<'_>,
    log: &mut dyn FnMut(&str),
) -> FigureFetch {
    let mut result = FigureFetch::default();
    for url in figure_urls(doc) {
        match source.library.has(&url) {
            Ok(true) => {
                result.present.push(url);
                continue;
            }
            Ok(false) => {}
            Err(err) => {
                log(&format!("skipping image {url}: {err}"));
                result.failed.push((url, err.to_string()));
                continue;
            }
        }
        if !source.refresh && source.library.is_missing(&url).unwrap_or(false) {
            log(&format!(
                "skipping image {url}: the server had no such file last time (--refresh asks again)"
            ));
            result.missing.push(url);
            continue;
        }
        let Some(fetcher) = source.fetcher.as_deref_mut() else {
            continue;
        };
        match fetcher.get(&url, FetchOptions::default()) {
            Ok(FetchResult::NotModified) => {}
            Ok(FetchResult::Fetched(res)) => {
                if let Err(err) = source.library.write(
                    &url,
                    &res.body,
                    res.etag.as_deref(),
                    res.last_modified.as_deref(),
                ) {
                    log(&format!("skipping image {url}: {err}"));
                    result.failed.push((url, err.to_string()));
                    continue;
                }
                result.present.push(url);
                result.fetched += 1;
            }
            Err(err) => {
                let reason = err.to_string();
                log(&format!("skipping image {url}: {reason}"));
                if err.is_gone() {
                    let _ = source.library.mark_missing(&url);
                    result.missing.push(url);
                } else {
                    result.failed.push((url, reason));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Figure, Inline};

    fn figure(src: &str) -> Figure {
        Figure {
            src: src.to_owned(),
            alt: String::new(),
            caption: None,
            id: None,
            inline: None,
        }
    }

    #[test]
    fn figures_are_listed_once_each_in_the_order_they_appear() {
        let mut doc = EntryDoc::default();
        doc.body.push(Block::Figure(figure("a.png")));
        doc.body.push(Block::Para {
            children: vec![
                Inline::Figure(figure("b.svg")),
                Inline::Figure(figure("a.png")),
            ],
            align: None,
        });
        assert_eq!(figure_urls(&doc), vec!["a.png", "b.svg"]);
    }
}
