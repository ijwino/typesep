use std::collections::BTreeMap;
use std::path::PathBuf;

use sep_core::library::Library;
use sep_core::model::EntryDoc;
use typst::foundations::Bytes;
use typst::visualize::{ExchangeFormat, RasterImage, SvgImage};

pub use sep_core::figures::figure_urls;

pub const NATIVE: [&str; 6] = ["svg", "png", "jpg", "jpeg", "gif", "webp"];

pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(64 * 1024)]);
    let mut head = head.strip_prefix('\u{feff}').unwrap_or(&head).trim_start();
    loop {
        let skipped = if head.starts_with("<?") {
            head.split_once("?>")
        } else if head.starts_with("<!--") {
            head.split_once("-->")
        } else {
            None
        };
        match skipped {
            Some((_, rest)) => head = rest.trim_start(),
            None => break,
        }
    }
    if head.starts_with("<svg") || head.starts_with("<!DOCTYPE svg") {
        return Some("svg");
    }
    None
}

fn usable(bytes: Vec<u8>) -> Result<&'static str, String> {
    let format = sniff(&bytes).ok_or("is in a format Typst does not read")?;
    let bytes = Bytes::new(bytes);
    let decoded = match format {
        "png" => RasterImage::plain(bytes, ExchangeFormat::Png).map(drop),
        "jpg" => RasterImage::plain(bytes, ExchangeFormat::Jpg).map(drop),
        "gif" => RasterImage::plain(bytes, ExchangeFormat::Gif).map(drop),
        "webp" => RasterImage::plain(bytes, ExchangeFormat::Webp).map(drop),
        _ => SvgImage::new(bytes)
            .map(drop)
            .map_err(|_| "the SVG does not parse".into()),
    };
    decoded
        .map(|()| format)
        .map_err(|why| format!("does not decode as {format} ({why})"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub file: PathBuf,
    pub format: &'static str,
}

pub fn collect(
    doc: &EntryDoc,
    library: &Library,
    warn: &mut dyn FnMut(String),
) -> BTreeMap<String, Image> {
    let mut out = BTreeMap::new();
    for url in figure_urls(doc) {
        let Ok(path) = library.file_path(&url) else {
            continue;
        };
        let candidates = [
            Some(path.clone()).filter(|p| p.is_file()),
            library
                .derived_path(&url, ".png")
                .ok()
                .filter(|p| p.is_file()),
        ];
        let mut placed = false;
        let mut failure = None;
        for file in candidates.into_iter().flatten() {
            let checked = std::fs::read(&file)
                .map_err(|err| format!("could not be read ({err})"))
                .and_then(usable);
            match checked {
                Ok(format) => {
                    out.insert(url.clone(), Image { file, format });
                    placed = true;
                    break;
                }
                Err(why) => {
                    failure.get_or_insert(why);
                }
            }
        }
        if !placed && let Some(why) = failure {
            warn(format!(
                "figure {url} {why} and the library holds no usable converted copy; leaving it out"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_comes_from_the_bytes_not_the_name() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n\0\0"), Some("png"));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(sniff(b"GIF89a....."), Some("gif"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniff(b"<?xml version=\"1.0\"?><svg"), Some("svg"));
        assert_eq!(sniff(b"  <svg xmlns=\"\">"), Some("svg"));
        assert_eq!(sniff(b"not an image at all"), None);
        assert_eq!(sniff(b"<?xml version=\"1.0\"?><rss/>"), None);
        assert_eq!(sniff(b"\xEF\xBB\xBF<svg xmlns=\"\">"), Some("svg"));
        assert_eq!(
            sniff(b"<?xml version=\"1.0\"?>\n<!-- made by hand -->\n<!DOCTYPE svg><svg>"),
            Some("svg")
        );
    }

    const PNG: [u8; 70] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64,
        0x60, 0xf8, 0x5f, 0x0f, 0x00, 0x02, 0x87, 0x01, 0x80, 0xeb, 0x47, 0xba, 0x92, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"4\"/>";

    #[test]
    fn an_image_is_used_only_if_typst_can_decode_it() {
        assert_eq!(usable(PNG.to_vec()), Ok("png"));
        assert!(usable(PNG[..40].to_vec()).is_err());
        assert_eq!(usable(SVG.into()), Ok("svg"));
        assert_eq!(usable(format!("\u{feff}<!-- c -->{SVG}").into()), Ok("svg"));
        assert!(usable(b"<svg xmlns=\"http://www.w3.org/2000/svg\"><g>".to_vec()).is_err());
        assert!(usable(b"<?xml version=\"1.0\"?><rss/>".to_vec()).is_err());
    }
}
