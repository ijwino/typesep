use typesep_model::{Dir, Doc, Meta};
use typesep_typst::text::fetch_date_span;

use crate::xhtml::esc;

pub const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;

pub fn media_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, ext)| ext.to_lowercase()) {
        Some(ext) => match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "webp" => "image/webp",
            _ => "application/octet-stream",
        },
        None => "application/octet-stream",
    }
}

pub struct Chapter {
    pub name: String,
    pub title: String,
    pub properties: String,
    pub nav: String,
}

fn now_utc() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

/// The package document; `nav_math` says the navigation document holds MathML, as a
/// contents line with a formula sets it.
pub fn content_opf(
    docs: &[&Doc],
    chapters: &[Chapter],
    nav_math: bool,
    assets: &[(std::path::PathBuf, String)],
    volume: Option<&Meta>,
) -> String {
    let first = &docs[0].meta;
    let merged = docs.len() > 1;
    let volume = volume.unwrap_or(first);
    let title = &volume.title;
    let lang = esc(&volume.lang);
    // A volume of several entries is the publisher's; one entry, even typeset as a volume,
    // is its authors'. A single entry's statement is a line on a page and needs its stop
    // here; a volume's is a sentence already.
    let (author, held) = if merged {
        (&volume.origin.publisher, volume)
    } else {
        (&first.byline, first)
    };
    let statement = match held.rights.statement.as_str() {
        "" => String::new(),
        line if merged => line.to_owned(),
        line => format!("{line}."),
    };
    let publisher = match held.origin.publisher.as_str() {
        "" => String::new(),
        name => format!("{name}."),
    };
    let rights = [
        statement.as_str(),
        publisher.as_str(),
        held.rights.terms.as_str(),
    ]
    .iter()
    .filter(|part| !part.is_empty())
    .cloned()
    .collect::<Vec<_>>()
    .join(" ");
    let metas: Vec<&Meta> = docs.iter().map(|d| &d.meta).collect();
    let retrieved = fetch_date_span(&metas).map(|(_, last)| last);
    let identifier = if merged {
        docs.iter()
            .map(|d| d.meta.id.clone())
            .collect::<Vec<_>>()
            .join("+")
    } else if first.id.is_empty() {
        "entry".to_owned()
    } else {
        first.id.clone()
    };

    let nav_properties = if nav_math { "nav mathml" } else { "nav" };
    let mut manifest = vec![
        format!(
            "<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"{nav_properties}\"/>"
        ),
        "<item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>".to_owned(),
    ];
    manifest.extend(chapters.iter().enumerate().map(|(i, c)| {
        let properties = if c.properties.is_empty() {
            String::new()
        } else {
            format!(" properties=\"{}\"", c.properties)
        };
        format!(
            "<item id=\"c{i}\" href=\"{}\" media-type=\"application/xhtml+xml\"{properties}/>",
            c.name
        )
    }));
    manifest.extend(assets.iter().enumerate().map(|(i, (_, name))| {
        format!(
            "<item id=\"img{i}\" href=\"{}\" media-type=\"{}\"/>",
            esc(name),
            media_type(name)
        )
    }));
    let spine: String = (0..chapters.len())
        .map(|i| format!("<itemref idref=\"c{i}\"/>"))
        .collect();
    // A book in a script written right to left turns its pages the other way.
    let progression = if volume.dir == Dir::Rtl {
        " page-progression-direction=\"rtl\""
    } else {
        ""
    };
    let date = match retrieved {
        Some(date) => format!("\n<dc:date>{date}</dc:date>"),
        None => String::new(),
    };
    // A work has the address it was read from; a document made of two, as a bilingual
    // edition is, has none of its own, and an empty source is no source.
    let source = match first.origin.url.as_str() {
        "" => String::new(),
        url => format!("\n<dc:source>{}</dc:source>", esc(url)),
    };

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id" xml:lang="{lang}">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:identifier id="pub-id">urn:typesep:{}</dc:identifier>
<dc:title>{}</dc:title>
<dc:language>{lang}</dc:language>
<dc:creator>{}</dc:creator>{source}{date}
<dc:rights>{}</dc:rights>
<meta property="dcterms:modified">{}</meta>
</metadata>
<manifest>{}</manifest>
<spine{progression}>{spine}</spine>
</package>"#,
        esc(&identifier),
        esc(title),
        esc(author),
        esc(&rights),
        now_utc(),
        manifest.join("\n"),
    )
}
