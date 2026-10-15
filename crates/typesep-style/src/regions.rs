/// The parts of every document that come before what a source names: the front matter and
/// the body.
const LEADING: [&str; 2] = ["preamble", "body"];
/// The parts of every document that come after: the notes and the supplements.
const TRAILING: [&str; 2] = ["notes", "supplements"];

/// What the sources in use add to the style table: the sections they can select, which
/// `links.sections` may then name, and those of them it names when it is not set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceSections {
    /// The keys of the sections, in the order the sources list them.
    pub keys: Vec<&'static str>,
    /// The keys whose links `links.mode` turns into notes unless the style says otherwise.
    pub linked: Vec<&'static str>,
}

impl SourceSections {
    /// What `links.sections` can name: the front matter and the body, the sources' sections,
    /// then the notes and supplements, each once.
    pub fn regions(&self) -> Vec<&'static str> {
        in_order(&self.keys)
    }

    /// What `links.sections` names when it is not set: the front matter, the body, the
    /// sources' sections that ask for it, the notes and the supplements.
    pub fn linked_regions(&self) -> Vec<&'static str> {
        in_order(&self.linked)
    }
}

fn in_order(keys: &[&'static str]) -> Vec<&'static str> {
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
        let none = SourceSections::default();
        assert_eq!(none.regions(), ["preamble", "body", "notes", "supplements"]);
        assert_eq!(none.linked_regions(), none.regions());
        let some = SourceSections {
            keys: vec!["bib", "notes", "remarks", "bib", "body", "supplements"],
            linked: vec!["remarks"],
        };
        assert_eq!(
            some.regions(),
            ["preamble", "body", "bib", "remarks", "notes", "supplements"]
        );
        assert_eq!(
            some.linked_regions(),
            ["preamble", "body", "remarks", "notes", "supplements"]
        );
    }
}
