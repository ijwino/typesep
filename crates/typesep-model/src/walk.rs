use crate::{Block, Doc, Figure, Inline};

/// A node of the document a walk visits.
#[derive(Debug, Clone, Copy)]
pub enum Node<'a> {
    Block(&'a Block),
    Inline(&'a Inline),
}

/// Visits every node of `doc`, wherever the document holds it: its front matter, body and
/// end matter, its attachments and their notes, and its own notes, each node before the
/// nodes inside it.
pub fn walk_doc<'a>(doc: &'a Doc, visit: &mut impl FnMut(Node<'a>)) {
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

/// Visits `block` and every node inside it, each before the nodes inside it.
pub fn walk_block<'a>(block: &'a Block, visit: &mut impl FnMut(Node<'a>)) {
    visit(Node::Block(block));
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

/// Visits each of `nodes` and every node inside it, each before the nodes inside it.
pub fn walk_inlines<'a>(nodes: &'a [Inline], visit: &mut impl FnMut(Node<'a>)) {
    for node in nodes {
        visit(Node::Inline(node));
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
            Inline::Text { .. }
            | Inline::Math { .. }
            | Inline::Break
            | Inline::Anchor { .. }
            | Inline::NoteRef(_) => {}
        }
    }
}

/// The text of running text, its markup left out, a formula as its TeX and a line break as a
/// space; note references, anchors and figures say nothing.
pub fn plain_text(nodes: &[Inline]) -> String {
    plain_text_with(nodes, &mut |node, out| match node {
        Inline::Math { tex, .. } => out.push_str(tex.trim()),
        Inline::Break => out.push(' '),
        _ => {}
    })
}

/// The text of running text, its markup left out, with what `leaf` writes of each node that is
/// neither text nor holds any: a formula, a line break, a note reference, an anchor or a figure.
pub fn plain_text_with(nodes: &[Inline], leaf: &mut impl FnMut(&Inline, &mut String)) -> String {
    fn walk(nodes: &[Inline], leaf: &mut impl FnMut(&Inline, &mut String), out: &mut String) {
        for node in nodes {
            match node {
                Inline::Text { value } => out.push_str(value),
                Inline::Emph { children }
                | Inline::Strong { children }
                | Inline::Sub { children }
                | Inline::Sup { children }
                | Inline::Code { children }
                | Inline::Link { children, .. }
                | Inline::Spaced { children }
                | Inline::SmallCaps { children }
                | Inline::Decorated { children, .. }
                | Inline::MarginNote { children } => walk(children, leaf, out),
                Inline::Math { .. }
                | Inline::Break
                | Inline::Anchor { .. }
                | Inline::Figure(_)
                | Inline::NoteRef(_) => leaf(node, out),
            }
        }
    }
    let mut out = String::new();
    walk(nodes, leaf, &mut out);
    out
}

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
    walk_doc(doc, &mut |node| match node {
        Node::Block(Block::Figure(figure)) | Node::Inline(Inline::Figure(figure)) => push(figure),
        _ => {}
    });
    out
}

pub fn links_in_block(block: &Block, found: &mut impl FnMut(&str)) {
    walk_block(block, &mut |node| {
        if let Node::Inline(Inline::Link {
            href,
            internal: false,
            ..
        }) = node
        {
            found(href);
        }
    });
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
            columns: vec![
                vec![],
                vec![Block::Lang {
                    lang: "de".into(),
                    dir: None,
                    children: vec![Block::Figure(figure("column.svg"))],
                }],
            ],
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

    #[test]
    fn every_node_is_visited_before_the_nodes_inside_it() {
        let text = |value: &str| Inline::Text {
            value: value.to_owned(),
        };
        let mut doc = Doc::default();
        doc.body.push(Block::Remark {
            number: "1".into(),
            id: None,
            children: vec![Block::Para {
                children: vec![
                    Inline::Emph {
                        children: vec![text("a")],
                    },
                    Inline::Figure(Figure {
                        caption: Some(vec![text("b")]),
                        ..figure("f.png")
                    }),
                ],
                align: None,
                indent_em: 0,
            }],
        });
        doc.notes.insert(
            "n1".into(),
            vec![Block::Para {
                children: vec![text("c")],
                align: None,
                indent_em: 0,
            }],
        );
        let mut seen = Vec::new();
        walk_doc(&doc, &mut |node| {
            seen.push(match node {
                Node::Block(Block::Remark { .. }) => "remark".to_owned(),
                Node::Block(Block::Para { .. }) => "para".to_owned(),
                Node::Inline(Inline::Emph { .. }) => "emph".to_owned(),
                Node::Inline(Inline::Figure(_)) => "figure".to_owned(),
                Node::Inline(Inline::Text { value }) => value.clone(),
                other => format!("{other:?}"),
            })
        });
        assert_eq!(
            seen,
            ["remark", "para", "emph", "a", "figure", "b", "para", "c"]
        );
    }

    #[test]
    fn plain_text_is_the_text_a_formula_as_its_tex_and_a_line_break_a_space() {
        let nodes = vec![
            Inline::Strong {
                children: vec![Inline::Text {
                    value: "Satz".into(),
                }],
            },
            Inline::Break,
            Inline::Math {
                mode: crate::MathMode::Inline,
                tex: " p \\lor q ".into(),
            },
            Inline::NoteRef(crate::NoteRef {
                id: "n1".into(),
                label: "1".into(),
                href: "#n1".into(),
                ref_id: None,
            }),
        ];
        assert_eq!(plain_text(&nodes), "Satz p \\lor q");
        assert_eq!(
            plain_text_with(&nodes, &mut |node, out| if let Inline::Break = node {
                out.push('\n')
            }),
            "Satz\n"
        );
    }
}
