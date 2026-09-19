use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sep_core::library::Library;
use sep_core::model::EntryDoc;

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
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
    let head = head.trim_start();
    if head.starts_with("<?xml") || head.starts_with("<svg") || head.starts_with("<!DOCTYPE svg") {
        return Some("svg");
    }
    None
}

fn head_of(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut head = vec![0; 512];
    let read = file.read(&mut head).ok()?;
    head.truncate(read);
    Some(head)
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
        for file in candidates.into_iter().flatten() {
            let Some(format) = head_of(&file).as_deref().and_then(sniff) else {
                continue;
            };
            out.insert(url.clone(), Image { file, format });
            placed = true;
            break;
        }
        if !placed && path.is_file() {
            warn(format!(
                "figure {url} is in a format Typst does not read and the library holds no converted copy; leaving it out"
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
    }
}
