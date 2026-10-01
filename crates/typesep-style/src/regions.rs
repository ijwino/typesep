use std::sync::OnceLock;

/// The parts of every document that come before what a source names: the front matter and
/// the body.
const LEADING: [&str; 2] = ["preamble", "body"];
/// The parts of every document that come after: the notes and the supplements.
const TRAILING: [&str; 2] = ["notes", "supplements"];

static SOURCE_KEYS: OnceLock<Vec<&'static str>> = OnceLock::new();

/// Names the sections the sources in use can select, so that `links.sections` accepts them.
/// The style table is built once, the first time a style is read, so this is called before
/// that; the first call is the one that counts.
pub fn register(keys: Vec<&'static str>) {
    let _ = SOURCE_KEYS.set(keys);
}

/// What `links.sections` can name: the front matter and the body, the sections of the
/// sources registered, then the notes and supplements, each once.
pub fn regions() -> Vec<&'static str> {
    regions_with(SOURCE_KEYS.get().map_or(&[], Vec::as_slice))
}

fn regions_with(keys: &[&'static str]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = LEADING.to_vec();
    for key in keys {
        if !out.contains(key) && !TRAILING.contains(key) {
            out.push(key);
        }
    }
    out.extend(TRAILING);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sources_sections_go_between_the_body_and_the_notes() {
        assert_eq!(
            regions_with(&[]),
            ["preamble", "body", "notes", "supplements"]
        );
        assert_eq!(
            regions_with(&["bib", "notes", "remarks", "bib", "body", "supplements"]),
            ["preamble", "body", "bib", "remarks", "notes", "supplements"]
        );
    }
}
