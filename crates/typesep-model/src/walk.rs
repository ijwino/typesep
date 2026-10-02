use crate::{Block, Doc, Figure, Inline};

pub fn figure_urls(doc: &Doc) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |figure: &Figure| {
        for src in std::iter::once(&figure.src).chain(figure.ebook_src.iter()) {
            if !src.is_empty() && seen.insert(src.clone()) {
                out.push(src.clone());
            }
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
    for note in doc.notes.values() {
        blocks.extend(note.iter());
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
        Block::Blockquote { children }
        | Block::Remark { children, .. }
        | Block::Aside { children } => {
            for block in children {
                walk_block(block, push);
            }
        }
        Block::Columns { columns } => {
            for block in columns.iter().flatten() {
                walk_block(block, push);
            }
        }
        Block::Verse { lines } => {
            for line in lines {
                walk_inlines(&line.children, push);
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
            | Inline::Link { children, .. }
            | Inline::Spaced { children }
            | Inline::SmallCaps { children }
            | Inline::Decorated { children, .. }
            | Inline::MarginNote { children } => walk_inlines(children, push),
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
        Block::Blockquote { children }
        | Block::Remark { children, .. }
        | Block::Aside { children } => {
            for block in children {
                links_in_block(block, found);
            }
        }
        Block::Columns { columns } => {
            for block in columns.iter().flatten() {
                links_in_block(block, found);
            }
        }
        Block::Verse { lines } => {
            for line in lines {
                links_in_inlines(&line.children, found);
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
            | Inline::Code { children }
            | Inline::Spaced { children }
            | Inline::SmallCaps { children }
            | Inline::Decorated { children, .. }
            | Inline::MarginNote { children } => links_in_inlines(children, found),
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
            ebook_src: None,
            width_px: None,
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
            indent_em: 0,
        });
        assert_eq!(figure_urls(&doc), vec!["a.png", "b.svg"]);
    }

    #[test]
    fn figures_inside_the_new_blocks_and_the_documents_own_notes_are_listed_too() {
        let mut doc = Doc::default();
        let mut coloured = figure("grey.png");
        coloured.ebook_src = Some("colour.png".into());
        doc.body.push(Block::Remark {
            number: "1".into(),
            id: None,
            children: vec![Block::Aside {
                children: vec![Block::Figure(coloured)],
            }],
        });
        doc.body.push(Block::Columns {
            columns: vec![vec![], vec![Block::Figure(figure("column.svg"))]],
        });
        doc.body.push(Block::Verse {
            lines: vec![crate::VerseLine {
                indent_em: 2,
                children: vec![Inline::MarginNote {
                    children: vec![Inline::Figure(figure("margin.png"))],
                }],
            }],
        });
        doc.notes
            .insert("n1".into(), vec![Block::Figure(figure("note.png"))]);
        assert_eq!(
            figure_urls(&doc),
            vec![
                "grey.png",
                "colour.png",
                "column.svg",
                "margin.png",
                "note.png"
            ]
        );
    }

    #[test]
    fn links_inside_the_new_nodes_are_found() {
        let link = |href: &str| Inline::Link {
            href: href.to_owned(),
            internal: false,
            children: vec![],
        };
        let block = Block::Remark {
            number: "2".into(),
            id: Some("2".into()),
            children: vec![
                Block::Columns {
                    columns: vec![vec![Block::Verse {
                        lines: vec![crate::VerseLine {
                            indent_em: 0,
                            children: vec![Inline::Spaced {
                                children: vec![link("a")],
                            }],
                        }],
                    }]],
                },
                Block::Para {
                    children: vec![Inline::Decorated {
                        line: crate::Decoration::Under,
                        dashed: true,
                        children: vec![Inline::SmallCaps {
                            children: vec![Inline::MarginNote {
                                children: vec![link("b")],
                            }],
                        }],
                    }],
                    align: None,
                    indent_em: 0,
                },
            ],
        };
        let mut found = Vec::new();
        links_in_block(&block, &mut |href| found.push(href.to_owned()));
        assert_eq!(found, vec!["a", "b"]);
    }
}
