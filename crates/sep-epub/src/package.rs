use sep_core::model::EntryDoc;
use sep_typst::text::{
    MERGED_RIGHTS, PERSONAL_USE_NOTICE, SEP_NAME, copyright_line, fetch_date_span,
};

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

pub fn content_opf(
    docs: &[&EntryDoc],
    chapters: &[Chapter],
    assets: &[(std::path::PathBuf, String)],
    volume_title: Option<&str>,
) -> String {
    let first = &docs[0].meta;
    let merged = docs.len() > 1;
    let title = volume_title.unwrap_or(&first.title);
    let author = if merged {
        SEP_NAME.to_owned()
    } else {
        first.author_line.clone()
    };
    let copyright = if merged {
        MERGED_RIGHTS.to_owned()
    } else {
        match copyright_line(first) {
            line if line.is_empty() => String::new(),
            line => format!("{line}."),
        }
    };
    let rights = [
        copyright.as_str(),
        &format!("{SEP_NAME}."),
        PERSONAL_USE_NOTICE,
    ]
    .iter()
    .filter(|part| !part.is_empty())
    .cloned()
    .collect::<Vec<_>>()
    .join(" ");
    let metas: Vec<&sep_core::model::EntryMeta> = docs.iter().map(|d| &d.meta).collect();
    let retrieved = fetch_date_span(&metas).map(|(_, last)| last);
    let identifier = if merged {
        docs.iter()
            .map(|d| d.meta.slug.clone())
            .collect::<Vec<_>>()
            .join("+")
    } else if first.slug.is_empty() {
        "entry".to_owned()
    } else {
        first.slug.clone()
    };

    let mut manifest = vec![
        "<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>".to_owned(),
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
    let date = match retrieved {
        Some(date) => format!("\n<dc:date>{date}</dc:date>"),
        None => String::new(),
    };

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id" xml:lang="en">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:identifier id="pub-id">urn:typesep:{}</dc:identifier>
<dc:title>{}</dc:title>
<dc:language>en</dc:language>
<dc:creator>{}</dc:creator>
<dc:source>{}</dc:source>{date}
<dc:rights>{}</dc:rights>
<meta property="dcterms:modified">{}</meta>
</metadata>
<manifest>{}</manifest>
<spine>{spine}</spine>
</package>"#,
        esc(&identifier),
        esc(title),
        esc(&author),
        esc(&first.source_url),
        esc(&rights),
        now_utc(),
        manifest.join("\n"),
    )
}
