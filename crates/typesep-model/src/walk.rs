use crate::{Block, Doc, Figure, Inline};

pub fn figure_urls(doc: &Doc) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |figure: &Figure| {
        if !figure.src.is_empty() && seen.insert(figure.src.clone()) {
            out.push(figure.src.clone());
        }
    };
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

pub fn links_in_block(block: &Block, found: &mut impl FnMut(&str)) {
    match block {
        Block::Para { children, .. }
        | Block::Pre { children }
        | Block::Heading { children, .. } => links_in_inlines(children, found),
        Block::List(list) => {
            for item in &list.items {
                for block in item {
                    links_in_block(block, found);
                }
            }
            for label in list.labels.iter().flatten() {
                links_in_inlines(label, found);
            }
        }
        Block::Blockquote { children } => {
            for block in children {
                links_in_block(block, found);
            }
        }
        Block::Table(table) => {
            for caption in table.caption.iter() {
                links_in_inlines(caption, found);
            }
            for row in &table.rows {
                for cell in row {
                    links_in_inlines(&cell.children, found);
                }
            }
        }
        Block::Figure(figure) => {
            for caption in figure.caption.iter() {
                links_in_inlines(caption, found);
            }
        }
        Block::Rule | Block::Anchor { .. } => {}
    }
}

fn links_in_inlines(nodes: &[Inline], found: &mut impl FnMut(&str)) {
    for node in nodes {
        match node {
            Inline::Link {
                href,
                internal,
                children,
            } => {
                if !internal {
                    found(href);
                }
                links_in_inlines(children, found);
            }
            Inline::Emph { children }
            | Inline::Strong { children }
            | Inline::Sub { children }
            | Inline::Sup { children }
            | Inline::Code { children } => links_in_inlines(children, found),
            Inline::Figure(figure) => {
                for caption in figure.caption.iter() {
                    links_in_inlines(caption, found);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut doc = Doc::default();
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
