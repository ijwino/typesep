use super::resolve::Assignment;
use super::schema::{Knob, KnobType, knob, knobs};
use super::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shorthand {
    pub flag: &'static str,
    pub path: &'static str,
    pub doc: &'static str,
    pub placeholder: Option<&'static str>,
}

const fn sh(
    flag: &'static str,
    path: &'static str,
    placeholder: Option<&'static str>,
    doc: &'static str,
) -> Shorthand {
    Shorthand {
        flag,
        path,
        doc,
        placeholder,
    }
}

pub const SHORTHANDS: [Shorthand; 14] = [
    sh(
        "paper",
        "page.size",
        Some("<size>"),
        "paper size, a name like a4 or WIDTHxHEIGHT",
    ),
    sh(
        "margin",
        "page.margins.*",
        Some("<length>"),
        "all four margins",
    ),
    sh(
        "font",
        "fonts.main.family",
        Some("<family>"),
        "body font family",
    ),
    sh(
        "font-size",
        "typography.fontSize",
        Some("<length>"),
        "body font size",
    ),
    sh(
        "leading",
        "typography.leading",
        Some("<factor|length>"),
        "line spacing",
    ),
    sh(
        "columns",
        "page.columns",
        Some("<n>"),
        "number of text columns",
    ),
    sh(
        "twoside",
        "page.twoside",
        None,
        "lay out for double-sided printing",
    ),
    sh("justify", "typography.justify", None, "justify body text"),
    sh(
        "hyphenate",
        "typography.hyphenate",
        None,
        "hyphenate at line breaks",
    ),
    sh(
        "toc",
        "toc.style",
        Some("<style>"),
        "table of contents style",
    ),
    sh(
        "links",
        "links.mode",
        Some("<mode>"),
        "how external links are rendered",
    ),
    sh(
        "notes",
        "notes.style",
        Some("<style>"),
        "how the entry's notes are set",
    ),
    sh("cover", "cover.style", Some("<style>"), "title page style"),
    sh(
        "bookmarks",
        "output.bookmarks",
        None,
        "write a PDF outline from the headings",
    ),
];

pub fn shorthand(flag: &str) -> Option<&'static Shorthand> {
    SHORTHANDS.iter().find(|s| s.flag == flag)
}

pub fn target_paths(sh: &Shorthand) -> Vec<String> {
    match sh.path.strip_suffix(".*") {
        None => vec![sh.path.to_owned()],
        Some(prefix) => {
            let prefix = format!("{prefix}.");
            knobs()
                .iter()
                .filter(|k| k.path.starts_with(&prefix))
                .map(|k| k.path.clone())
                .collect()
        }
    }
}

pub fn knob_for(sh: &Shorthand) -> Option<&'static Knob> {
    let first = target_paths(sh).into_iter().next()?;
    let mut segments: Vec<&str> = first.split('.').collect();
    while !segments.is_empty() {
        if let Some(k) = knob(&segments.join(".")) {
            return Some(k);
        }
        segments.pop();
    }
    None
}

pub fn is_boolean(sh: &Shorthand) -> bool {
    knob_for(sh).is_some_and(|k| k.kind == KnobType::Boolean)
}

pub fn choices_for(sh: &Shorthand) -> Option<&'static [&'static str]> {
    knob(sh.path).and_then(|k| k.kind.values())
}

pub fn option_key(flag: &str) -> String {
    let mut out = String::with_capacity(flag.len());
    let mut upper = false;
    for c in flag.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

pub fn shorthand_assignments(values: &[(&str, Value)]) -> Vec<Assignment> {
    let mut out = Vec::new();
    for sh in &SHORTHANDS {
        let Some((_, value)) = values.iter().find(|(flag, _)| *flag == sh.flag) else {
            continue;
        };
        let source = if *value == Value::Bool(false) {
            format!("--no-{}", sh.flag)
        } else {
            format!("--{}", sh.flag)
        };
        for path in target_paths(sh) {
            out.push(Assignment {
                path,
                value: value.clone(),
                source: source.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shorthand_targets_a_real_knob_and_is_declared_consistently() {
        for sh in &SHORTHANDS {
            assert!(
                !target_paths(sh).is_empty(),
                "{} expands to nothing",
                sh.flag
            );
            assert!(knob_for(sh).is_some(), "{} points at no knob", sh.flag);
            assert_eq!(
                sh.placeholder.is_none(),
                is_boolean(sh),
                "{}: booleans take no value, others need a placeholder",
                sh.flag
            );
        }
        assert_eq!(
            choices_for(shorthand("toc").unwrap()),
            Some(&["bullet", "latex", "none"][..])
        );
        assert_eq!(choices_for(shorthand("font").unwrap()), None);
        assert_eq!(
            knob_for(shorthand("font").unwrap()).unwrap().path,
            "fonts.main"
        );
        assert_eq!(option_key("font-size"), "fontSize");
        assert_eq!(option_key("paper"), "paper");
    }

    #[test]
    fn shorthands_translate_to_assignments_that_remember_their_flag() {
        let sets = shorthand_assignments(&[
            ("paper", Value::str("a4")),
            ("font-size", Value::str("11pt")),
            ("justify", Value::Bool(false)),
            ("twoside", Value::Bool(true)),
            ("margin", Value::str("1in")),
        ]);
        let summary: Vec<(String, String, String)> = sets
            .iter()
            .map(|a| (a.path.clone(), a.value.js_string(), a.source.clone()))
            .collect();
        let expected: Vec<(String, String, String)> = [
            ("page.size", "a4", "--paper"),
            ("page.margins.top", "1in", "--margin"),
            ("page.margins.bottom", "1in", "--margin"),
            ("page.margins.inner", "1in", "--margin"),
            ("page.margins.outer", "1in", "--margin"),
            ("typography.fontSize", "11pt", "--font-size"),
            ("page.twoside", "true", "--twoside"),
            ("typography.justify", "false", "--no-justify"),
        ]
        .iter()
        .map(|(p, v, s)| (p.to_string(), v.to_string(), s.to_string()))
        .collect();
        assert_eq!(summary, expected);
    }
}
