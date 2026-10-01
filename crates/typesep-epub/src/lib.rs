pub mod container;
pub mod css;
pub mod nav;
pub mod package;
pub mod xhtml;

use std::collections::{BTreeMap, BTreeSet};

use typesep_model::{Doc, Meta};
use typesep_style::View;
use typesep_typst::fonts::Fonts;
use typesep_typst::images::Image;
use typesep_typst::mathml;

use crate::package::Chapter;
use crate::xhtml::{Ctx, MathPass, document_xhtml, entry_html};

#[derive(Debug)]
pub enum EpubError {
    Asset { file: String, reason: String },
    Container(String),
    Empty,
}

impl std::fmt::Display for EpubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EpubError::Asset { file, reason } => write!(f, "cannot read {file}: {reason}"),
            EpubError::Container(reason) => write!(f, "cannot write the EPUB: {reason}"),
            EpubError::Empty => write!(f, "cannot write an EPUB with no entry in it"),
        }
    }
}

impl std::error::Error for EpubError {}

pub struct EpubOptions<'a> {
    pub sections: &'a BTreeSet<String>,
    pub images: &'a BTreeMap<String, Image>,
    pub volume: Option<&'a Meta>,
    pub fonts: &'a Fonts,
}

pub struct Epub {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

pub fn build_epub(
    docs: &[&Doc],
    style: &typesep_style::Style,
    options: EpubOptions,
) -> Result<Epub, EpubError> {
    if docs.is_empty() {
        return Err(EpubError::Empty);
    }
    let view = View::new(style);
    let mut warnings = Vec::new();

    let mut maths = Vec::with_capacity(docs.len());
    let mut ids = Vec::with_capacity(docs.len());
    for doc in docs {
        let mut ctx = Ctx::new(&view, options.images, MathPass::Collect(Vec::new()));
        entry_html(doc, &mut ctx, options.sections);
        let formulas = ctx.collected();
        ids.push(std::mem::take(&mut ctx.ids));
        let converted = mathml::convert(&formulas, &doc.meta.macros, options.fonts);
        for warning in &converted.warnings {
            if !warnings.contains(warning) {
                warnings.push(warning.clone());
            }
        }
        maths.push(converted);
    }
    let math_css = maths
        .iter()
        .map(|m| m.css.as_str())
        .find(|css| !css.is_empty())
        .unwrap_or_default()
        .to_owned();

    let mut ctx = Ctx::new(&view, options.images, MathPass::Collect(Vec::new()));
    let mut chapters = Vec::with_capacity(docs.len());
    let mut documents = Vec::with_capacity(docs.len());
    for (index, doc) in docs.iter().enumerate() {
        ctx.math = MathPass::Fill {
            items: std::mem::take(&mut maths[index].items),
            next: 0,
        };
        ctx.ids = std::mem::take(&mut ids[index]);
        ctx.has_math = false;
        let name = format!("entry-{}.xhtml", index + 1);
        let body = entry_html(doc, &mut ctx, options.sections);
        documents.push((
            name.clone(),
            document_xhtml(&doc.meta.title, &doc.meta.lang, &body),
        ));
        let properties = if ctx.has_math { "mathml" } else { "" }.to_owned();
        chapters.push(Chapter {
            name: name.clone(),
            title: doc.meta.title.clone(),
            properties,
            nav: nav::nav_items(doc, &name, &view, options.sections, &ctx.ids),
        });
    }
    warnings.extend(std::mem::take(&mut ctx.warnings));

    let mut stylesheet = css::stylesheet(&view);
    if !math_css.is_empty() {
        stylesheet.push_str("\n\n/* Typst's MathML rules, from its HTML export */\n");
        stylesheet.push_str(&math_css);
    }

    let mut container = container::Container::new().map_err(zip_error)?;
    container
        .add("META-INF/container.xml", package::CONTAINER_XML.as_bytes())
        .map_err(zip_error)?;
    container
        .add("OEBPS/style.css", stylesheet.as_bytes())
        .map_err(zip_error)?;
    for (name, text) in &documents {
        container
            .add(&format!("OEBPS/{name}"), text.as_bytes())
            .map_err(zip_error)?;
    }
    for (file, name) in &ctx.assets {
        let bytes = std::fs::read(file).map_err(|err| EpubError::Asset {
            file: file.display().to_string(),
            reason: err.to_string(),
        })?;
        container
            .add(&format!("OEBPS/{name}"), &bytes)
            .map_err(zip_error)?;
    }

    let nav = if chapters.len() == 1 {
        let only = &chapters[0];
        if only.nav.is_empty() {
            format!(
                "<ol><li><a href=\"{}\">{}</a></li></ol>",
                only.name,
                xhtml::esc(&only.title)
            )
        } else {
            only.nav.clone()
        }
    } else {
        format!(
            "<ol>{}</ol>",
            chapters
                .iter()
                .map(|c| format!(
                    "<li><a href=\"{}\">{}</a>{}</li>",
                    c.name,
                    xhtml::esc(&c.title),
                    c.nav
                ))
                .collect::<String>()
        )
    };
    container
        .add(
            "OEBPS/nav.xhtml",
            document_xhtml(
                "Contents",
                &options.volume.unwrap_or(&docs[0].meta).lang,
                &format!("<nav epub:type=\"toc\" id=\"toc\"><h1>Contents</h1>{nav}</nav>"),
            )
            .as_bytes(),
        )
        .map_err(zip_error)?;
    container
        .add(
            "OEBPS/content.opf",
            package::content_opf(docs, &chapters, &ctx.assets, options.volume).as_bytes(),
        )
        .map_err(zip_error)?;

    Ok(Epub {
        bytes: container.finish().map_err(zip_error)?,
        warnings,
    })
}

fn zip_error(err: zip::result::ZipError) -> EpubError {
    EpubError::Container(err.to_string())
}
