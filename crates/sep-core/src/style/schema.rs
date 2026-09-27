use std::collections::HashMap;
use std::sync::LazyLock;

use super::units::{LENGTH_UNITS, PAPER_SIZES, match_length, paper_size};
use super::value::{Map, Value};
use crate::parse::js::{self, lazy_regex};
use crate::sections::ALL_REGIONS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnobType {
    Boolean,
    Number,
    Integer,
    String,
    Length,
    FactorOrLength,
    PaperSize,
    Color,
    Font,
    StringList,
    Template,
    Enum(Vec<&'static str>),
    EnumList(Vec<&'static str>),
    Min(Bound, Box<KnobType>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    NonNegative,
    Positive,
}

impl Bound {
    fn describe(self) -> &'static str {
        match self {
            Bound::NonNegative => "0 or more",
            Bound::Positive => "greater than 0",
        }
    }

    fn check(self, n: f64) -> Result<(), String> {
        let ok = match self {
            Bound::NonNegative => n >= 0.0,
            Bound::Positive => n > 0.0,
        };
        if ok {
            Ok(())
        } else {
            Err(format!("must be {}", self.describe()))
        }
    }

    fn check_value(self, v: &Value) -> Result<(), String> {
        let n = match v {
            Value::Number(n) => *n,
            Value::String(text) => match_length(text)
                .and_then(|(n, _)| js::number(n))
                .unwrap_or(f64::NAN),
            _ => f64::NAN,
        };
        self.check(n)
    }
}

pub fn match_ignoring_case<'a>(values: &[&'a str], input: &str) -> Option<&'a str> {
    let s = js::trim(input);
    if let Some(hit) = values.iter().find(|v| **v == s) {
        return Some(hit);
    }
    let lower = s.to_lowercase();
    let hits: Vec<&str> = values
        .iter()
        .copied()
        .filter(|v| v.to_lowercase() == lower)
        .collect();
    match hits.as_slice() {
        [one] => Some(one),
        _ => None,
    }
}

fn coerce_boolean(v: &Value) -> Result<Value, String> {
    match v {
        Value::Bool(b) => Ok(Value::Bool(*b)),
        Value::String(s) if s == "true" || s == "yes" || s == "1" => Ok(Value::Bool(true)),
        Value::String(s) if s == "false" || s == "no" || s == "0" => Ok(Value::Bool(false)),
        _ => Err("expected true or false".to_owned()),
    }
}

fn coerce_number(v: &Value) -> Result<f64, String> {
    let n = match v {
        Value::Number(n) => *n,
        Value::String(s) if js::trim(s).is_empty() => f64::NAN,
        other => js::number(&other.js_string()).unwrap_or(f64::NAN),
    };
    if n.is_finite() {
        Ok(n)
    } else {
        Err("expected a number".to_owned())
    }
}

fn coerce_integer(v: &Value) -> Result<f64, String> {
    let n = coerce_number(v)?;
    if js::is_integer(n) {
        Ok(n)
    } else {
        Err("expected a whole number".to_owned())
    }
}

fn coerce_length(v: &Value) -> Result<String, String> {
    let text = v.js_string();
    match match_length(js::trim(&text)) {
        Some((n, unit)) => Ok(format!("{n}{unit}")),
        None => Err("expected a length like \"0.6in\" or \"12pt\"".to_owned()),
    }
}

fn coerce_factor_or_length(v: &Value) -> Result<Value, String> {
    let text = v.js_string();
    let s = Value::str(js::trim(&text));
    if match_length(js::trim(&text)).is_some() {
        Ok(Value::String(coerce_length(&s)?))
    } else {
        Ok(Value::Number(coerce_number(&s)?))
    }
}

lazy_regex!(
    SIZE_RE = format!(
        r"^{ws}*([0-9.]+{ws}*[a-zA-Z]{{2}}){ws}*[xX×]{ws}*([0-9.]+{ws}*[a-zA-Z]{{2}}){ws}*$",
        ws = js::WS
    )
);

fn coerce_paper_size(v: &Value) -> Result<String, String> {
    let text = v.js_string();
    let s: String = js::trim(&text)
        .to_lowercase()
        .chars()
        .filter(|c| !js::is_space(*c))
        .collect();
    if paper_size(&s).is_some() {
        return Ok(s);
    }
    let Some(m) = SIZE_RE.captures(&text) else {
        return Err(format!("unknown paper size \"{text}\""));
    };
    let w = coerce_length(&Value::str(&m[1]))?;
    let h = coerce_length(&Value::str(&m[2]))?;
    for dim in [&w, &h] {
        Bound::Positive
            .check_value(&Value::str(dim))
            .map_err(|_| format!("paper dimensions must be greater than 0, got \"{text}\""))?;
    }
    Ok(format!("{w}x{h}"))
}

lazy_regex!(HEX_COLOR_RE = r"^#[0-9a-fA-F]{6}$");
lazy_regex!(NAMED_COLOR_RE = r"^[a-zA-Z]+$");

fn coerce_color(v: &Value) -> Result<String, String> {
    let text = v.js_string();
    let s = js::trim(&text);
    if HEX_COLOR_RE.is_match(s) || NAMED_COLOR_RE.is_match(s) {
        Ok(s.to_owned())
    } else {
        Err("expected a color name or #rrggbb".to_owned())
    }
}

fn spread_into(map: &mut Map, value: &Value) {
    match value {
        Value::Map(m) => map.extend_from(m),
        Value::List(items) => {
            for (i, item) in items.iter().enumerate() {
                map.insert(i.to_string(), item.clone());
            }
        }
        Value::String(s) => {
            for (i, c) in s.encode_utf16().enumerate() {
                map.insert(i.to_string(), Value::str(String::from_utf16_lossy(&[c])));
            }
        }
        _ => {}
    }
}

fn truthy(map: &Map, key: &str) -> bool {
    map.get(key).is_some_and(Value::truthy)
}

fn coerce_font(v: &Value) -> Result<Value, String> {
    let mut spec = Map::new();
    match v {
        Value::String(family) => spec.insert("family", Value::str(family)),
        other => spread_into(&mut spec, other),
    }
    if !truthy(&spec, "family") && !truthy(&spec, "file") {
        return Err("a font needs a family or a file".to_owned());
    }
    if let Some(scale) = spec.get("scale") {
        let n = coerce_number(scale)?;
        Bound::Positive
            .check(n)
            .map_err(|e| format!("a font scale {e}"))?;
        spec.insert("scale", Value::Number(n));
    }
    if let Some(features) = spec.get("features")
        && !matches!(features, Value::Map(_) | Value::List(_) | Value::Null)
    {
        return Err("font features must be a mapping, e.g. {Numbers: OldStyle}".to_owned());
    }
    Ok(Value::Map(spec))
}

fn merge_font(current: Option<&Value>, raw: &Value) -> Value {
    let Value::Map(patch) = raw else {
        return raw.clone();
    };
    let mut merged = Map::new();
    if let Some(current) = current {
        spread_into(&mut merged, current);
    }
    merged.extend_from(patch);
    let current_features = current
        .and_then(|c| c.as_map())
        .and_then(|m| m.get("features"));
    if let (Some(cf), Some(pf)) = (current_features, patch.get("features"))
        && cf.truthy()
        && pf.truthy()
    {
        let mut features = Map::new();
        spread_into(&mut features, cf);
        spread_into(&mut features, pf);
        merged.insert("features", Value::Map(features));
    }
    if truthy(patch, "family") && !truthy(patch, "file") {
        merged.remove("file");
    }
    if truthy(patch, "file") && !truthy(patch, "family") {
        merged.remove("family");
    }
    Value::Map(merged)
}

fn scalar(v: &Value) -> Result<(), String> {
    match v {
        Value::Map(_) => Err("expected a single value, not a mapping".to_owned()),
        Value::List(_) => Err("expected a single value, not a list".to_owned()),
        _ => Ok(()),
    }
}

fn coerce_text(v: &Value) -> Result<String, String> {
    match v {
        Value::Null => Ok(String::new()),
        Value::Map(_) => Err(
            "expected text, not a mapping (quote text that starts with a brace, e.g. '{page}')"
                .to_owned(),
        ),
        Value::List(_) => Err("expected text, not a list".to_owned()),
        other => Ok(other.js_string()),
    }
}

fn coerce_string_list(v: &Value) -> Result<Vec<String>, String> {
    match v {
        Value::Null => Ok(Vec::new()),
        Value::List(items) => items
            .iter()
            .filter(|item| **item != Value::Null)
            .map(coerce_text)
            .collect(),
        Value::Map(_) => Err("expected a list, not a mapping".to_owned()),
        other => Ok(other
            .js_string()
            .split(',')
            .map(|s| js::trim(s).to_owned())
            .filter(|s| !s.is_empty())
            .collect()),
    }
}

fn string_list(items: Vec<String>) -> Value {
    Value::List(items.into_iter().map(Value::String).collect())
}

impl KnobType {
    pub fn describe(&self) -> String {
        match self {
            KnobType::Boolean => "true or false".to_owned(),
            KnobType::Number => "a number".to_owned(),
            KnobType::Integer => "a whole number".to_owned(),
            KnobType::String => "text".to_owned(),
            KnobType::Length => format!(
                "a length such as 0.6in, 12pt, 1.5em (units: {})",
                LENGTH_UNITS.join(", ")
            ),
            KnobType::FactorOrLength => {
                "a multiplier like 1.15, or an absolute length like 13pt".to_owned()
            }
            KnobType::PaperSize => format!(
                "a named size ({}) or \"WIDTHxHEIGHT\" like 5.5inx8.5in",
                PAPER_SIZES
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            KnobType::Color => {
                "a named color (black, gray, blue, …) or a hex value like #334455".to_owned()
            }
            KnobType::Font => "a family name, or {family, scale, features, file}".to_owned(),
            KnobType::StringList => "a list of strings".to_owned(),
            KnobType::Template => {
                "text with {title} {author} {section} {page} {pages} {edition} {date} tokens"
                    .to_owned()
            }
            KnobType::Enum(values) => format!("one of: {}", values.join(", ")),
            KnobType::EnumList(values) => format!("a list drawn from: {}", values.join(", ")),
            KnobType::Min(bound, inner) => format!("{}, {}", inner.describe(), bound.describe()),
        }
    }

    pub fn values(&self) -> Option<&[&'static str]> {
        match self {
            KnobType::Enum(values) | KnobType::EnumList(values) => Some(values),
            KnobType::Min(_, inner) => inner.values(),
            _ => None,
        }
    }

    pub fn is_boolean(&self) -> bool {
        *self == KnobType::Boolean
    }

    pub fn coerce(&self, v: &Value) -> Result<Value, String> {
        if !matches!(
            self,
            KnobType::Font
                | KnobType::StringList
                | KnobType::EnumList(_)
                | KnobType::String
                | KnobType::Template
        ) {
            scalar(v)?;
        }
        match self {
            KnobType::Boolean => coerce_boolean(v),
            KnobType::Number => coerce_number(v).map(Value::Number),
            KnobType::Integer => coerce_integer(v).map(Value::Number),
            KnobType::String | KnobType::Template => coerce_text(v).map(Value::String),
            KnobType::Length => coerce_length(v).map(Value::String),
            KnobType::FactorOrLength => coerce_factor_or_length(v),
            KnobType::PaperSize => coerce_paper_size(v).map(Value::String),
            KnobType::Color => coerce_color(v).map(Value::String),
            KnobType::Font => coerce_font(v),
            KnobType::StringList => coerce_string_list(v).map(string_list),
            KnobType::Enum(values) => match match_ignoring_case(values, &v.js_string()) {
                Some(hit) => Ok(Value::str(hit)),
                None => Err(format!("expected one of: {}", values.join(", "))),
            },
            KnobType::EnumList(values) => {
                let items = coerce_string_list(v)?;
                for item in &items {
                    if !values.contains(&item.as_str()) {
                        return Err(format!("\"{item}\" is not one of: {}", values.join(", ")));
                    }
                }
                Ok(string_list(items))
            }
            KnobType::Min(bound, inner) => {
                let typed = inner.coerce(v)?;
                bound.check_value(&typed)?;
                Ok(typed)
            }
        }
    }

    pub fn merge(&self, current: Option<&Value>, raw: &Value) -> Value {
        match self {
            KnobType::Font => merge_font(current, raw),
            _ => raw.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Knob {
    pub path: String,
    pub kind: KnobType,
    pub default: Value,
    pub doc: String,
}

fn k(path: impl Into<String>, kind: KnobType, default: Value, doc: impl Into<String>) -> Knob {
    Knob {
        path: path.into(),
        kind,
        default,
        doc: doc.into(),
    }
}

fn en(values: &[&'static str]) -> KnobType {
    KnobType::Enum(values.to_vec())
}

fn positive(kind: KnobType) -> KnobType {
    KnobType::Min(Bound::Positive, Box::new(kind))
}

fn non_negative(kind: KnobType) -> KnobType {
    KnobType::Min(Bound::NonNegative, Box::new(kind))
}

fn text(s: &str) -> Value {
    Value::str(s)
}

fn num(n: f64) -> Value {
    Value::Number(n)
}

fn list(items: &[&str]) -> Value {
    Value::List(items.iter().map(|s| Value::str(*s)).collect())
}

fn font(family: &str, scale: Option<f64>, features: &[(&str, &str)]) -> Value {
    let mut spec = Map::new();
    spec.insert("family", text(family));
    if let Some(scale) = scale {
        spec.insert("scale", num(scale));
    }
    if !features.is_empty() {
        let mut f = Map::new();
        for (key, value) in features {
            f.insert(*key, text(value));
        }
        spec.insert("features", Value::Map(f));
    }
    Value::Map(spec)
}

struct HeadingDefaults {
    size: f64,
    style: &'static str,
    space_before: &'static str,
    space_after: &'static str,
}

fn heading_knobs(level: &str, d: HeadingDefaults) -> Vec<Knob> {
    let faces = en(&["regular", "bold", "italic", "bolditalic", "smallcaps"]);
    vec![
        k(
            format!("headings.{level}.size"),
            positive(KnobType::FactorOrLength),
            num(d.size),
            format!("Font size of {level} (multiplier of body size, or absolute)."),
        ),
        k(
            format!("headings.{level}.style"),
            faces,
            text(d.style),
            format!("Face used for {level}."),
        ),
        k(
            format!("headings.{level}.case"),
            en(&["none", "upper", "lower"]),
            text("none"),
            format!("Letter case transform applied to {level}."),
        ),
        k(
            format!("headings.{level}.align"),
            en(&["left", "center", "right"]),
            text("left"),
            format!("Alignment of {level}."),
        ),
        k(
            format!("headings.{level}.font"),
            en(&["main", "sans", "mono"]),
            text("main"),
            format!("Which configured font family {level} uses."),
        ),
        k(
            format!("headings.{level}.color"),
            KnobType::Color,
            text("black"),
            format!("Color of {level}."),
        ),
        k(
            format!("headings.{level}.spaceBefore"),
            KnobType::Length,
            text(d.space_before),
            if level == "h1" {
                "Vertical space above h1. Kept even at the top of a page, where the entry title sits.".to_owned()
            } else {
                format!("Vertical space above {level}.")
            },
        ),
        k(
            format!("headings.{level}.spaceAfter"),
            KnobType::Length,
            text(d.space_after),
            format!("Vertical space below {level}."),
        ),
        k(
            format!("headings.{level}.pageBreakBefore"),
            KnobType::Boolean,
            Value::Bool(false),
            format!("Start a new page before every {level}."),
        ),
    ]
}

#[rustfmt::skip]
fn build_knobs() -> Vec<Knob> {
    use KnobType::*;
    let mut knobs = vec![
        k("page.size", PaperSize, text("a5"), "Paper size."),
        k("page.margins.top", non_negative(Length), text("20mm"), "Top margin."),
        k("page.margins.bottom", non_negative(Length), text("22mm"), "Bottom margin."),
        k("page.margins.inner", non_negative(Length), text("18mm"), "Inner margin (left on one-sided documents)."),
        k("page.margins.outer", non_negative(Length), text("18mm"), "Outer margin (right on one-sided documents)."),
        k("page.twoside", Boolean, Value::Bool(false), "Alternate inner/outer margins and page numbers for double-sided printing."),
        k("page.bindingOffset", non_negative(Length), text("0pt"), "Extra inner margin reserved for binding."),
        k("page.columns", positive(Integer), num(1.0), "Number of text columns."),
        k("page.columnSep", non_negative(Length), text("1.2em"), "Gap between columns."),

        k("typography.fontSize", positive(Length), text("10pt"), "Body font size."),
        k("typography.leading", positive(FactorOrLength), num(1.15), "Line spacing, as a multiplier or an absolute baseline distance."),
        k("typography.justify", Boolean, Value::Bool(true), "Justify body text (off gives a ragged right edge)."),
        k("typography.hyphenate", Boolean, Value::Bool(true), "Allow hyphenation at line breaks."),
        k("typography.language", String, text("english"), "Babel language, which selects the hyphenation patterns."),
        k("typography.parIndent", Length, text("1.2em"), "First-line indent of paragraphs."),
        k("typography.parSkip", Length, text("0pt"), "Extra vertical space between paragraphs."),
        k("typography.raggedBottom", Boolean, Value::Bool(true), "Let the last line of a page float instead of stretching to the bottom."),
        k("typography.microtype.protrusion", Boolean, Value::Bool(true), "Margin kerning (characters hang slightly into the margin)."),
        k("typography.microtype.expansion", Boolean, Value::Bool(true), "Subtle glyph width adjustment to improve justification."),
        k("typography.penalties.widow", Integer, num(10000.0), "Penalty for a widow line. 10000 forbids it."),
        k("typography.penalties.club", Integer, num(10000.0), "Penalty for an orphan line. 10000 forbids it."),
        k("typography.penalties.hyphen", Integer, num(50.0), "Penalty for breaking a word. Higher means fewer hyphens."),
        k("typography.tolerance", Integer, num(2000.0), "How much looseness TeX tolerates in a line before complaining."),
        k("typography.emergencyStretch", non_negative(Length), text("3em"), "Extra stretch allowed on a final pass to avoid overfull lines."),

        k("fonts.main", Font, font("TeX Gyre Termes", None, &[("Ligatures", "TeX")]), "Body font."),
        k("fonts.sans", Font, font("TeX Gyre Heros", Some(0.95), &[]), "Sans-serif font."),
        k("fonts.mono", Font, font("TeX Gyre Cursor", Some(0.9), &[]), "Monospaced font."),
        k("fonts.math", Font, font("TeX Gyre Termes Math", None, &[]), "Math font (must be an OpenType math font)."),
        k("fonts.fallback", StringList,
            list(&["Noto Serif", "Noto Serif CJK SC", "Noto Serif Devanagari", "Noto Naskh Arabic", "Noto Serif Hebrew", "Noto Sans Symbols"]),
            "Fonts tried, in order, for characters the main font lacks (CJK, Devanagari, ...). Names not installed are skipped; empty disables fallback."),
    ];
    knobs.extend(heading_knobs("h1", HeadingDefaults { size: 1.9, style: "regular", space_before: "0pt", space_after: "0.8em" }));
    knobs.extend(heading_knobs("h2", HeadingDefaults { size: 1.3, style: "bold", space_before: "1.6em", space_after: "0.7em" }));
    knobs.extend(heading_knobs("h3", HeadingDefaults { size: 1.1, style: "bold", space_before: "1.2em", space_after: "0.5em" }));
    knobs.extend(heading_knobs("h4", HeadingDefaults { size: 1.0, style: "italic", space_before: "1em", space_after: "0.4em" }));
    knobs.extend([
        k("blocks.blockquote.leftIndent", Length, text("1.5em"), "Left indent of block quotations."),
        k("blocks.blockquote.rightIndent", Length, text("1em"), "Right indent of block quotations."),
        k("blocks.blockquote.size", positive(FactorOrLength), num(0.95), "Font size of block quotations."),
        k("blocks.blockquote.spaceAround", Length, text("0.6em"), "Vertical space above and below block quotations."),
        k("blocks.list.itemSep", Length, text("0.25em"), "Space between list items."),
        k("blocks.list.leftMargin", Length, text("1.4em"), "Indent of list items."),
        k("blocks.list.bullets", StringList, list(&["\\textbullet", "\\textopenbullet", "\\textendash"]), "Bullet markers, outermost level first."),
        k("blocks.code.size", positive(FactorOrLength), num(0.9), "Font size of code, inline and preformatted."),
        k("blocks.table.style", en(&["booktabs", "grid", "plain"]), text("booktabs"), "Table rule style."),
        k("blocks.table.size", positive(FactorOrLength), num(0.9), "Font size inside tables."),
        k("blocks.figure.maxWidth", positive(Number), num(1.0), "Maximum figure width as a fraction of the text width."),
        k("blocks.figure.align", en(&["left", "center", "right"]), text("center"), "Figure alignment."),
        k("blocks.figure.captionSize", positive(FactorOrLength), num(0.9), "Figure caption size."),
        k("blocks.bibliography.hangingIndent", Length, text("1.5em"), "Hanging indent of bibliography entries."),
        k("blocks.bibliography.itemSep", Length, text("0.4em"), "Space between bibliography entries."),
        k("blocks.bibliography.size", positive(FactorOrLength), num(0.95), "Font size of the bibliography."),
        k("blocks.footnote.size", positive(FactorOrLength), num(0.85), "Font size of footnotes and endnotes."),
        k("blocks.footnote.rule", Boolean, Value::Bool(true), "Draw the rule above footnotes."),

        k("pageFurniture.header.left", Template, text(""), "Header, left slot. With twoside it mirrors: left on recto, right on verso (the inner edge)."),
        k("pageFurniture.header.center", Template, text(""), "Header, center slot."),
        k("pageFurniture.header.right", Template, text(""), "Header, right slot. With twoside it mirrors: right on recto, left on verso (the outer edge)."),
        k("pageFurniture.footer.left", Template, text(""), "Footer, left slot. Mirrors like the header when twoside."),
        k("pageFurniture.footer.center", Template, text(""), "Footer, center slot."),
        k("pageFurniture.footer.right", Template, text("{page}"), "Footer, right slot. Mirrors like the header when twoside."),
        k("pageFurniture.footer.skip", non_negative(FactorOrLength), num(0.5),
            "Distance from the bottom of the text block down to the footer baseline. A multiplier is a fraction of the bottom margin (0.5 puts the page number midway between the text and the paper edge); a length is used as is."),
        k("pageFurniture.rules.header", Boolean, Value::Bool(false), "Draw a rule under the header."),
        k("pageFurniture.rules.footer", Boolean, Value::Bool(false), "Draw a rule above the footer."),
        k("pageFurniture.size", positive(FactorOrLength), num(0.85), "Font size of headers and footers."),
        k("pageFurniture.numbering.style", en(&["arabic", "roman", "Roman", "alph", "Alph"]), text("arabic"), "Page number style."),
        k("pageFurniture.numbering.startAt", Integer, num(1.0), "First page number."),

        k("toc.style", en(&["bullet", "latex", "none"]), text("bullet"), "Table of contents style. \"bullet\" mirrors the SEP web layout."),
        k("toc.depth", positive(Integer), num(3.0), "Deepest heading level listed."),
        k("toc.pageNumbers", Boolean, Value::Bool(false), "Show page numbers in the bullet table of contents. The latex style always shows them."),
        k("toc.title", String, text(""), "Heading above the table of contents. Empty means no heading."),

        k("links.mode", en(&["inline", "footnote", "endnote", "none"]), text("inline"), "How external links are rendered."),
        k("links.sections", EnumList(ALL_REGIONS.to_vec()), list(&["preamble", "body", "oir", "notes", "supplements"]),
            "Parts of the entry where links.mode footnote or endnote applies; links elsewhere (the bibliography, say) stay inline."),
        k("links.color", Color, text("black"), "Link color."),
        k("links.underline", Boolean, Value::Bool(false), "Underline links."),

        k("notes.style", en(&["footnote", "endnote", "appendix"]), text("footnote"),
            "How the entry's notes (notes.html) are set: as footnotes, as endnotes after the body, or as an appendix that the markers link to."),
        k("notes.maxHeight", non_negative(FactorOrLength), num(1.0),
            "How tall a numbered note may be as a footnote: a multiplier of the text height, or a length. A taller note is set at the end of the entry instead, with a one-line footnote in its place giving the note number and the page. 0 turns this off."),

        k("cover.style", en(&["simple", "none", "custom"]), text("simple"), "Title page style."),
        k("cover.template", String, text(""), "Path to a Typst title page, used when cover.style is \"custom\"."),
        k("cover.fields", StringList, list(&["title", "author", "dates", "sourceUrl", "copyright"]), "Which fields the simple cover shows, in order. Known fields: title, author, dates, sourceUrl, copyright. The copyright field is the source, the retrieval date and the copyright line; the terms the entry is read under are on the colophon, the last page, which no setting removes."),

        k("output.bookmarks", Boolean, Value::Bool(true), "Write a PDF outline from the headings."),

        k("advanced.documentClass", String, text(""), "Ignored: Typst has no document class. The option is accepted, and a non-empty value is reported as a warning."),
        k("advanced.preamble", String, text(""), "Raw Typst appended to the preamble. The escape hatch for anything not covered above."),
        k("advanced.hooks.beforeBody", String, text(""), "Raw Typst inserted before the body, after the cover and the table of contents."),
        k("advanced.hooks.afterBody", String, text(""), "Raw Typst inserted at the end of the document, after the last entry's body."),
    ]);
    knobs
}

static KNOBS: LazyLock<Vec<Knob>> = LazyLock::new(build_knobs);
static KNOBS_BY_PATH: LazyLock<HashMap<&'static str, usize>> = LazyLock::new(|| {
    KNOBS
        .iter()
        .enumerate()
        .map(|(i, knob)| (knob.path.as_str(), i))
        .collect()
});

pub fn knobs() -> &'static [Knob] {
    &KNOBS
}

pub fn knob(path: &str) -> Option<&'static Knob> {
    KNOBS_BY_PATH.get(path).map(|&i| &KNOBS[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> Value {
        Value::str(x)
    }

    #[test]
    fn the_knob_table_is_complete_and_its_paths_are_unique() {
        assert_eq!(knobs().len(), 113);
        assert!(knob("notes.maxHeight").is_some());
        assert_eq!(knob("page.size").unwrap().default, s("a5"));
        assert_eq!(knob("headings.h4.style").unwrap().default, s("italic"));
        assert!(knob("headings.h1.doc").is_none());
        let paths: Vec<&str> = knobs().iter().map(|k| k.path.as_str()).collect();
        assert_eq!(paths[0], "page.size");
        assert_eq!(paths[paths.len() - 1], "advanced.hooks.afterBody");
        assert_eq!(
            paths.iter().filter(|p| p.starts_with("headings.")).count(),
            36
        );
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), paths.len(), "paths are unique");
    }

    #[test]
    fn each_type_coerces_the_values_it_accepts() {
        use KnobType::*;
        assert_eq!(Boolean.coerce(&s("yes")).unwrap(), Value::Bool(true));
        assert_eq!(Boolean.coerce(&s("0")).unwrap(), Value::Bool(false));
        assert_eq!(
            Boolean.coerce(&Value::Number(1.0)).unwrap_err(),
            "expected true or false"
        );
        assert_eq!(Number.coerce(&s(" 1.5 ")).unwrap(), Value::Number(1.5));
        assert_eq!(
            Number.coerce(&s(" ")).unwrap_err(),
            "expected a number",
            "unlike Number(''), an empty value is not 0"
        );
        assert_eq!(Number.coerce(&s("two")).unwrap_err(), "expected a number");
        assert_eq!(
            Integer.coerce(&s("2.5")).unwrap_err(),
            "expected a whole number"
        );
        assert_eq!(Integer.coerce(&s("1e3")).unwrap(), Value::Number(1000.0));
        assert_eq!(Length.coerce(&s(" 12 pt ")).unwrap(), s("12pt"));
        assert_eq!(
            Length.coerce(&Value::Number(12.0)).unwrap_err(),
            "expected a length like \"0.6in\" or \"12pt\""
        );
        assert_eq!(
            FactorOrLength.coerce(&s("1.15")).unwrap(),
            Value::Number(1.15)
        );
        assert_eq!(FactorOrLength.coerce(&s("13pt")).unwrap(), s("13pt"));
        assert_eq!(
            FactorOrLength.coerce(&Value::Number(2.0)).unwrap(),
            Value::Number(2.0)
        );
        assert_eq!(PaperSize.coerce(&s(" A4 ")).unwrap(), s("a4"));
        assert_eq!(
            PaperSize.coerce(&s("Half Letter")).unwrap(),
            s("halfletter")
        );
        assert_eq!(
            PaperSize.coerce(&s("5.5in x 8.5in")).unwrap(),
            s("5.5inx8.5in")
        );
        assert_eq!(PaperSize.coerce(&s("90mm×120mm")).unwrap(), s("90mmx120mm"));
        assert_eq!(
            PaperSize.coerce(&s("letterish")).unwrap_err(),
            "unknown paper size \"letterish\""
        );
        assert_eq!(
            PaperSize.coerce(&s("5.5INx8.5IN")).unwrap_err(),
            "expected a length like \"0.6in\" or \"12pt\"",
            "the size regex ignores case, the length regex does not"
        );
        assert_eq!(Color.coerce(&s("#334455")).unwrap(), s("#334455"));
        assert_eq!(Color.coerce(&s("Black")).unwrap(), s("Black"));
        assert_eq!(
            Color.coerce(&s("#33445")).unwrap_err(),
            "expected a color name or #rrggbb"
        );
        assert_eq!(
            StringList.coerce(&s("a, b,,c")).unwrap(),
            Value::List(vec![s("a"), s("b"), s("c")])
        );
        assert_eq!(
            StringList
                .coerce(&Value::List(vec![Value::Number(1.0), Value::Null]))
                .unwrap(),
            Value::List(vec![s("1")]),
            "a null item is dropped"
        );
        assert_eq!(
            StringList.coerce(&Value::Null).unwrap(),
            Value::List(vec![])
        );
        assert_eq!(
            StringList.coerce(&Value::map()).unwrap_err(),
            "expected a list, not a mapping"
        );
        assert_eq!(
            StringList
                .coerce(&Value::List(vec![Value::List(vec![])]))
                .unwrap_err(),
            "expected text, not a list"
        );
        assert_eq!(Template.coerce(&Value::Null).unwrap(), s(""));
        assert_eq!(String.coerce(&Value::Null).unwrap(), s(""));
        let token = super::super::yaml::parse("{page}").unwrap();
        assert_eq!(
            Template.coerce(&token).unwrap_err(),
            "expected text, not a mapping (quote text that starts with a brace, e.g. '{page}')"
        );
        assert_eq!(
            String.coerce(&Value::List(vec![s("a")])).unwrap_err(),
            "expected text, not a list"
        );
        assert_eq!(
            PaperSize.coerce(&Value::List(vec![s("a4")])).unwrap_err(),
            "expected a single value, not a list"
        );
        assert_eq!(
            Number.coerce(&Value::map()).unwrap_err(),
            "expected a single value, not a mapping"
        );
        assert_eq!(String.coerce(&Value::Number(1.5)).unwrap(), s("1.5"));
        let notes = knob("notes.style").unwrap();
        assert_eq!(notes.kind.coerce(&s("ENDNOTE")).unwrap(), s("endnote"));
        assert_eq!(
            notes.kind.coerce(&s("margin")).unwrap_err(),
            "expected one of: footnote, endnote, appendix"
        );
        let numbering = knob("pageFurniture.numbering.style").unwrap();
        assert_eq!(numbering.kind.coerce(&s("Roman")).unwrap(), s("Roman"));
        assert!(
            numbering.kind.coerce(&s("ROMAN")).is_err(),
            "case tells roman and Roman apart"
        );
        let columns = &knob("page.columns").unwrap().kind;
        assert_eq!(columns.coerce(&s("2")).unwrap(), Value::Number(2.0));
        assert_eq!(
            columns.coerce(&s("0")).unwrap_err(),
            "must be greater than 0"
        );
        assert_eq!(columns.coerce(&s("")).unwrap_err(), "expected a number");
        assert_eq!(columns.describe(), "a whole number, greater than 0");
        let margin = &knob("page.margins.top").unwrap().kind;
        assert_eq!(margin.coerce(&s("0pt")).unwrap(), s("0pt"));
        assert_eq!(margin.coerce(&s("-5mm")).unwrap_err(), "must be 0 or more");
        let leading = &knob("typography.leading").unwrap().kind;
        assert_eq!(
            leading.coerce(&s("0")).unwrap_err(),
            "must be greater than 0"
        );
        assert_eq!(
            leading.coerce(&s("-1pt")).unwrap_err(),
            "must be greater than 0"
        );
        assert_eq!(leading.coerce(&s("14pt")).unwrap(), s("14pt"));
        assert_eq!(
            knob("typography.fontSize")
                .unwrap()
                .kind
                .coerce(&s("0pt"))
                .unwrap_err(),
            "must be greater than 0"
        );
        assert_eq!(
            PaperSize.coerce(&s("0mmx0mm")).unwrap_err(),
            "paper dimensions must be greater than 0, got \"0mmx0mm\""
        );
        let sections = knob("links.sections").unwrap();
        assert_eq!(
            sections.kind.coerce(&s("body,notes")).unwrap(),
            Value::List(vec![s("body"), s("notes")])
        );
        assert_eq!(
            sections.kind.coerce(&s("body,Notes")).unwrap_err(),
            "\"Notes\" is not one of: preamble, body, bib, academic, oir, related, notes, supplements"
        );
        assert_eq!(
            Length.describe(),
            "a length such as 0.6in, 12pt, 1.5em (units: pt, bp, mm, cm, in, pc, em, ex, sp)"
        );
        assert_eq!(
            PaperSize.describe(),
            "a named size (a3, a4, a5, a6, b5, letter, legal, halfletter, digest) or \"WIDTHxHEIGHT\" like 5.5inx8.5in"
        );
    }

    #[test]
    fn fonts_coerce_and_merge() {
        let main = knob("fonts.main").unwrap();
        let current = &main.default;
        assert_eq!(
            main.kind.coerce(&s("EB Garamond")).unwrap().to_json(),
            r#"{"family":"EB Garamond"}"#
        );
        assert_eq!(
            main.kind.merge(Some(current), &s("EB Garamond")),
            s("EB Garamond")
        );
        let patch = super::super::yaml::parse("family: EB Garamond").unwrap();
        let merged = main.kind.merge(Some(current), &patch);
        assert_eq!(
            merged.to_json(),
            r#"{"family":"EB Garamond","features":{"Ligatures":"TeX"}}"#
        );
        let patch = super::super::yaml::parse("scale: 0.9").unwrap();
        let merged = main
            .kind
            .coerce(&main.kind.merge(Some(current), &patch))
            .unwrap();
        assert_eq!(
            merged.to_json(),
            r#"{"family":"TeX Gyre Termes","features":{"Ligatures":"TeX"},"scale":0.9}"#
        );
        let patch = super::super::yaml::parse("features: {Numbers: OldStyle}").unwrap();
        let merged = main.kind.merge(Some(current), &patch);
        assert_eq!(
            merged.to_json(),
            r#"{"family":"TeX Gyre Termes","features":{"Ligatures":"TeX","Numbers":"OldStyle"}}"#
        );
        let patch = super::super::yaml::parse("file: ./Fonts/Foo.otf").unwrap();
        let merged = main.kind.merge(Some(current), &patch);
        assert_eq!(
            merged.to_json(),
            r#"{"features":{"Ligatures":"TeX"},"file":"./Fonts/Foo.otf"}"#,
            "a file drops the family"
        );
        assert_eq!(
            main.kind.coerce(&Value::map()).unwrap_err(),
            "a font needs a family or a file"
        );
        assert_eq!(
            main.kind.coerce(&Value::Null).unwrap_err(),
            "a font needs a family or a file"
        );
        let bad = super::super::yaml::parse("{family: X, scale: big}").unwrap();
        assert_eq!(main.kind.coerce(&bad).unwrap_err(), "expected a number");
        let bad = super::super::yaml::parse("{family: X, scale: 0}").unwrap();
        assert_eq!(
            main.kind.coerce(&bad).unwrap_err(),
            "a font scale must be greater than 0"
        );
        let bad = super::super::yaml::parse("{family: X, features: TeX}").unwrap();
        assert_eq!(
            main.kind.coerce(&bad).unwrap_err(),
            "font features must be a mapping, e.g. {Numbers: OldStyle}"
        );
        let extra = super::super::yaml::parse("{family: X, weight: bold}").unwrap();
        assert_eq!(
            main.kind.coerce(&extra).unwrap().to_json(),
            r#"{"family":"X","weight":"bold"}"#,
            "unknown fields are kept"
        );
    }

    #[test]
    fn enumerations_match_ignoring_case_unless_ambiguous() {
        let values = ["arabic", "roman", "Roman"];
        assert_eq!(match_ignoring_case(&values, " roman "), Some("roman"));
        assert_eq!(match_ignoring_case(&values, "ARABIC"), Some("arabic"));
        assert_eq!(match_ignoring_case(&values, "ROMAN"), None);
    }
}
