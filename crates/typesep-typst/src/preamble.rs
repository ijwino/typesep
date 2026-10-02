use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::colors::color_expr;
use crate::escape::{escape_string, escape_text, guard_line_start};
use crate::fonts::Fonts;
use crate::style::typst_pt;
use typesep_model::Dir;
use typesep_style::view::{FontSpec, View};

pub const HEADING_KEYS: [&str; 4] = ["h1", "h2", "h3", "h4"];

#[derive(Debug, Clone, Default)]
pub struct PreambleInput {
    pub title: String,
    pub byline: String,
    pub edition: String,
    pub date: String,
    pub subject: String,
    /// The language the document names, an ISO 639 code such as "de", which
    /// `typography.language: auto` follows.
    pub lang: String,
    /// The direction the document's script is written in.
    pub dir: Dir,
    /// The helpers the document's own constructs need, which the preamble defines after the
    /// ones every document has.
    pub helpers: BTreeSet<Helper>,
    /// The characters the document prints, which decide the fallback families its font
    /// lists name.
    pub chars: BTreeSet<char>,
}

/// A helper only some documents need. Each is defined only when the document uses its
/// construct, so a document without any of them gets the preamble it always had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Helper {
    Spaced,
    MarginNote,
    OutlineOnly,
    RemarkNumber,
    Aside,
    Columns,
    Verse,
    PlainTable,
    SizedFigure,
}

pub struct Emitter {
    pub name: &'static str,
    pub needs: &'static [&'static str],
    pub typst: &'static str,
    emit: fn(&Ctx<'_>) -> Vec<String>,
}

pub struct Ctx<'a> {
    pub view: &'a View<'a>,
    pub input: &'a PreambleInput,
    pub fonts: Option<&'a Fonts>,
    warnings: std::cell::RefCell<Vec<String>>,
}

impl<'a> Ctx<'a> {
    pub fn new(view: &'a View<'a>, input: &'a PreambleInput, fonts: Option<&'a Fonts>) -> Self {
        Ctx {
            view,
            input,
            fonts,
            warnings: std::cell::RefCell::new(Vec::new()),
        }
    }

    fn warn(&self, message: impl Into<String>) {
        self.warnings.borrow_mut().push(message.into());
    }

    pub fn into_warnings(self) -> Vec<String> {
        self.warnings.into_inner()
    }

    fn color(&self, path: &str) -> String {
        let value = self.view.str(path);
        color_expr(&value, &mut |m| self.warn(m))
    }
}

/// The families of `fonts.fallback` the document's font lists name: with the fonts at hand,
/// those installed that have a character of the document the main and maths families lack,
/// each only if no family before it has all of those it has; without them, every one. The
/// characters no font has are said, once.
fn fallback_families(ctx: &Ctx<'_>) -> Vec<String> {
    let listed = ctx.view.list("fonts.fallback");
    let Some(fonts) = ctx.fonts else {
        return listed;
    };
    let installed: Vec<String> = listed
        .into_iter()
        .filter(|family| fonts.has_family(family))
        .collect();
    let main = ctx.view.font("main").family;
    let math = ctx.view.font("math").family;
    let found = crate::fonts::coverage(fonts, &ctx.input.chars, &[&main, &math], &installed);
    if let Some(warning) = crate::fonts::describe_missing_characters(&found.missing) {
        ctx.warn(warning);
    }
    found.needed
}

fn font_list(spec: &FontSpec, fallback: &[String]) -> Vec<String> {
    let mut families = Vec::new();
    if !spec.family.is_empty() {
        families.push(spec.family.clone());
    }
    for family in fallback {
        if !families.iter().any(|f| f.eq_ignore_ascii_case(family)) {
            families.push(family.clone());
        }
    }
    families
}

fn font_array(families: &[String]) -> String {
    let items: Vec<String> = families.iter().map(|f| escape_string(f)).collect();
    if items.len() == 1 {
        format!("({},)", items[0])
    } else {
        format!("({})", items.join(", "))
    }
}

fn feature_pairs(spec: &FontSpec, ctx: &Ctx<'_>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (key, value) in &spec.features {
        let tag = match (key.as_str(), value.as_str()) {
            ("Ligatures", "TeX") => continue,
            ("Ligatures", "Common" | "Standard") => "liga",
            ("Ligatures", "Rare" | "Discretionary") => "dlig",
            ("Ligatures", "Historic") => "hlig",
            ("Numbers", "OldStyle") => "onum",
            ("Numbers", "Lining") => "lnum",
            ("Numbers", "Proportional") => "pnum",
            ("Numbers", "Tabular") => "tnum",
            ("Letters", "SmallCaps") => "smcp",
            ("Letters", "UppercaseSmallCaps") => "c2sc",
            (tag, _) if tag.len() == 4 && tag.chars().all(|c| c.is_ascii_lowercase()) => tag,
            _ => {
                ctx.warn(format!(
                    "font feature {key}={value} has no Typst equivalent and is ignored"
                ));
                continue;
            }
        };
        let number: Option<f64> = value.parse().ok();
        out.push((
            tag.to_owned(),
            number.map_or("1".to_owned(), typesep_style::units::fmt_pt),
        ));
    }
    out
}

fn features_arg(spec: &FontSpec, ctx: &Ctx<'_>) -> String {
    let pairs = feature_pairs(spec, ctx);
    if pairs.is_empty() {
        return String::new();
    }
    let items: Vec<String> = pairs
        .iter()
        .map(|(tag, value)| format!("\"{tag}\": {value}"))
        .collect();
    format!(", features: ({})", items.join(", "))
}

const ALIGN: [(&str, &str); 3] = [("left", "left"), ("center", "center"), ("right", "right")];

fn face_args(face: &str) -> &'static str {
    match face {
        "bold" => ", weight: \"bold\", style: \"normal\"",
        "italic" => ", weight: \"regular\", style: \"italic\"",
        "bolditalic" => ", weight: \"bold\", style: \"italic\"",
        "smallcaps" => ", weight: \"regular\", style: \"normal\"",
        _ => ", weight: \"regular\", style: \"normal\"",
    }
}

fn family_var(role: &str) -> &'static str {
    match role {
        "sans" => "fs-sans",
        "mono" => "fs-mono",
        _ => "fs-main",
    }
}

fn scale_var(role: &str) -> &'static str {
    match role {
        "sans" => "fs-sans-scale",
        "mono" => "fs-mono-scale",
        _ => "fs-main-scale",
    }
}

/// The Babel names typesep knows, by the ISO 639 code Typst takes for them.
#[rustfmt::skip]
const LANGUAGES: [(&str, &[&str]); 41] = [
    ("en", &["english", "american", "usenglish", "british", "ukenglish", "canadian", "australian", "newzealand"]),
    ("de", &["german", "ngerman", "austrian", "naustrian", "swissgerman"]),
    ("fr", &["french", "francais", "canadien"]),
    ("es", &["spanish"]),
    ("it", &["italian"]),
    ("pt", &["portuguese", "portuges", "brazilian", "brazil"]),
    ("nl", &["dutch"]),
    ("da", &["danish"]),
    ("sv", &["swedish"]),
    ("nb", &["norsk", "nynorsk", "norwegian"]),
    ("fi", &["finnish"]),
    ("is", &["icelandic"]),
    ("pl", &["polish"]),
    ("cs", &["czech"]),
    ("sk", &["slovak"]),
    ("hu", &["hungarian", "magyar"]),
    ("ro", &["romanian"]),
    ("hr", &["croatian"]),
    ("sl", &["slovene", "slovenian"]),
    ("sr", &["serbian"]),
    ("bg", &["bulgarian"]),
    ("ru", &["russian"]),
    ("uk", &["ukrainian"]),
    ("el", &["greek", "polutonikogreek"]),
    ("tr", &["turkish"]),
    ("la", &["latin"]),
    ("he", &["hebrew"]),
    ("ar", &["arabic"]),
    ("hi", &["hindi"]),
    ("ja", &["japanese"]),
    ("zh", &["chinese"]),
    ("ko", &["korean"]),
    ("ca", &["catalan"]),
    ("gl", &["galician"]),
    ("eu", &["basque"]),
    ("et", &["estonian"]),
    ("lv", &["latvian"]),
    ("lt", &["lithuanian"]),
    ("ga", &["irish"]),
    ("cy", &["welsh"]),
    ("eo", &["esperanto"]),
];

/// What `typography.language` means when the document's own language is to be followed.
pub const AUTO_LANGUAGE: &str = "auto";

fn lang_code(babel: &str) -> Option<&'static str> {
    let babel = babel.to_lowercase();
    LANGUAGES
        .iter()
        .find(|(_, names)| names.contains(&babel.as_str()))
        .map(|(code, _)| *code)
}

/// The language Typst sets a document in whose metadata names `code`, an ISO 639 code that
/// may carry a region (`de-AT`): the language itself when typesep knows it, else English,
/// which is said unless the document names no language at all.
fn document_lang(code: &str, ctx: &Ctx<'_>) -> &'static str {
    let primary = code
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if let Some((known, _)) = LANGUAGES.iter().find(|(known, _)| *known == primary) {
        return known;
    }
    if !code.trim().is_empty() {
        ctx.warn(format!(
            "the document's language \"{code}\" is not one typography.language auto knows; using english"
        ));
    }
    "en"
}

fn bullet_char(label: &str) -> String {
    let text = label.trim();
    let named = match text {
        "\\textbullet" | "\\bullet" => "•",
        "\\textopenbullet" | "\\circ" => "◦",
        "\\textendash" => "–",
        "\\textemdash" => "—",
        "\\textasteriskcentered" | "\\ast" => "∗",
        "\\textperiodcentered" | "\\cdot" => "·",
        "\\textdagger" | "\\dagger" => "†",
        "\\textdaggerdbl" | "\\ddagger" => "‡",
        "\\textbardbl" => "‖",
        "\\textminus" => "−",
        "\\triangleright" => "▹",
        "\\diamond" | "\\textdiamond" => "⋄",
        "\\square" => "□",
        "\\textasteriskcentered{}" => "∗",
        _ => {
            return match text.strip_prefix('\\') {
                Some(rest) => rest.trim_end_matches("{}").to_owned(),
                None => text.to_owned(),
            };
        }
    };
    named.to_owned()
}

fn size_arg(view: &View<'_>, path: &str) -> String {
    typst_pt(view.size_pt(path))
}

fn leading_em(view: &View<'_>) -> f64 {
    (view.size_pt("typography.leading") - view.body()) / view.body()
}

/// The space between two paragraphs, `pt` beyond what the leading sets between their lines.
pub(crate) fn gap(view: &View<'_>, pt: f64) -> String {
    format!(
        "{}em + {}",
        typesep_style::units::fmt_pt(leading_em(view)),
        typst_pt(pt)
    )
}

fn gap_knob(view: &View<'_>, path: &str) -> String {
    gap(view, view.pt(path))
}

fn page_numbering(view: &View<'_>) -> &'static str {
    match view.str("pageFurniture.numbering.style").as_str() {
        "roman" => "i",
        "Roman" => "I",
        "alph" => "a",
        "Alph" => "A",
        _ => "1",
    }
}

pub fn note_max_height_pt(view: &View<'_>) -> f64 {
    match view.value("notes.maxHeight") {
        Some(typesep_style::Value::Number(n)) => n * view.text_height_pt(),
        _ => view.pt("notes.maxHeight"),
    }
}

pub const EMITTERS: &[Emitter] = &[
    Emitter {
        name: "document",
        needs: &[
            "typography.fontSize",
            "page.twoside",
            "advanced.documentClass",
        ],
        typst: "set text(size:) with top-edge 1em and a baseline bottom edge, so the line pitch is the body size plus the leading whatever the font's own ascender; set document() for the PDF metadata. Typst has no document class, so advanced.documentClass is ignored",
        emit: |ctx| {
            let view = ctx.view;
            if !view.str("advanced.documentClass").is_empty() {
                ctx.warn("advanced.documentClass has no Typst equivalent and is ignored");
            }
            let input = ctx.input;
            let author = if input.byline.is_empty() {
                "()".to_owned()
            } else {
                format!("({},)", escape_string(&input.byline))
            };
            let description = if input.subject.is_empty() {
                "none".to_owned()
            } else {
                escape_string(&input.subject)
            };
            vec![
                format!(
                    "#set document(title: {}, author: {author}, description: {description})",
                    escape_string(&input.title),
                ),
                format!(
                    "#set text(size: {}, top-edge: 1em, bottom-edge: \"baseline\")",
                    typst_pt(view.body())
                ),
            ]
        },
    },
    Emitter {
        name: "fonts",
        needs: &[
            "fonts.main",
            "fonts.sans",
            "fonts.mono",
            "fonts.math",
            "fonts.fallback",
        ],
        typst: "set text(font:) with the fonts.fallback families the document needs appended to each family list (those installed that have a character of it the main and maths fonts lack), which is how Typst falls back for characters a font lacks, and a warning for characters no font has; show math.equation: set text(font:) for the maths font; a fonts.<role>.scale becomes a size factor where the role is selected, Typst having no per-family scale",
        emit: |ctx| {
            let view = ctx.view;
            let fallback = fallback_families(ctx);
            let mut out = Vec::new();
            for role in ["main", "sans", "mono"] {
                let spec = view.font(role);
                out.push(format!(
                    "#let {} = {}",
                    family_var(role),
                    font_array(&font_list(&spec, &fallback))
                ));
                out.push(format!(
                    "#let {} = {}",
                    scale_var(role),
                    typesep_style::units::fmt_pt(spec.scale.unwrap_or(1.0))
                ));
            }
            let math = view.font("math");
            out.push(format!(
                "#let fs-math-font = {}",
                font_array(&font_list(&math, &fallback))
            ));
            let main = view.font("main");
            out.push(format!(
                "#set text(font: fs-main{})",
                features_arg(&main, ctx)
            ));
            out.push(format!(
                "#show math.equation: set text(font: fs-math-font{})",
                features_arg(&math, ctx)
            ));
            out
        },
    },
    Emitter {
        name: "language and hyphenation",
        needs: &["typography.language", "typography.hyphenate"],
        typst: "set text(lang:) selects the hyphenation patterns, and set text(dir: rtl) the direction of a document written right to left; set text(hyphenate:) turns hyphenation off, as it is, with a note, for a language Typst has no patterns for",
        emit: |ctx| {
            let view = ctx.view;
            let babel = view.str("typography.language");
            let lang = if babel == AUTO_LANGUAGE {
                document_lang(&ctx.input.lang, ctx).to_owned()
            } else {
                lang_code(&babel).map(str::to_owned).unwrap_or_else(|| {
                if babel.len() == 2 && babel.chars().all(|c| c.is_ascii_alphabetic()) {
                    babel.to_lowercase()
                } else {
                    ctx.warn(format!(
                        "typography.language \"{babel}\" is not a language Typst knows; using english"
                    ));
                    "en".to_owned()
                }
                })
            };
            let mut hyphenate = view.bool("typography.hyphenate");
            if hyphenate && !crate::lang::has_hyphenation(&lang) {
                ctx.warn(format!(
                    "Typst has no hyphenation patterns for {lang}, the document's language, so it is not hyphenated"
                ));
                hyphenate = false;
            }
            let dir = match ctx.input.dir {
                Dir::Rtl => ", dir: rtl",
                Dir::Ltr => "",
            };
            vec![format!(
                "#set text(lang: \"{lang}\"{dir}, hyphenate: {hyphenate})"
            )]
        },
    },
    Emitter {
        name: "page geometry",
        needs: &[
            "page.size",
            "page.margins.*",
            "page.twoside",
            "page.bindingOffset",
            "pageFurniture.footer.skip",
        ],
        typst: "set page(width:, height:, margin:) with inside/outside margins when twoside, the binding offset added to the inner margin, and footer-descent from pageFurniture.footer.skip",
        emit: |ctx| {
            let view = ctx.view;
            let (width, height) = view.paper_pt();
            let twoside = view.bool("page.twoside");
            let inner = view.pt("page.margins.inner") + view.pt("page.bindingOffset");
            let outer = view.pt("page.margins.outer");
            let margin = if twoside {
                format!(
                    "(top: {}, bottom: {}, inside: {}, outside: {})",
                    typst_pt(view.pt("page.margins.top")),
                    typst_pt(view.pt("page.margins.bottom")),
                    typst_pt(inner),
                    typst_pt(outer)
                )
            } else {
                format!(
                    "(top: {}, bottom: {}, left: {}, right: {})",
                    typst_pt(view.pt("page.margins.top")),
                    typst_pt(view.pt("page.margins.bottom")),
                    typst_pt(inner),
                    typst_pt(outer)
                )
            };
            let bottom = view.pt("page.margins.bottom");
            let descent = match view.value("pageFurniture.footer.skip") {
                Some(typesep_style::Value::Number(n)) => {
                    format!("{}%", typesep_style::units::fmt_pt(n * 100.0))
                }
                _ => typst_pt(view.pt_against("pageFurniture.footer.skip", bottom)),
            };
            vec![format!(
                "#set page(width: {}, height: {}, margin: {margin}, footer-descent: {descent}, header-ascent: 40%)",
                typst_pt(width),
                typst_pt(height)
            )]
        },
    },
    Emitter {
        name: "columns",
        needs: &["page.columns", "page.columnSep"],
        typst: "set page(columns:) with set columns(gutter:)",
        emit: |ctx| {
            let view = ctx.view;
            let columns = view.int("page.columns");
            if columns <= 1 {
                return Vec::new();
            }
            vec![
                format!(
                    "#set columns(gutter: {})",
                    typst_pt(view.pt("page.columnSep"))
                ),
                format!("#set page(columns: {columns})"),
            ]
        },
    },
    Emitter {
        name: "paragraph shape",
        needs: &[
            "typography.leading",
            "typography.parIndent",
            "typography.parSkip",
            "typography.justify",
            "typography.raggedBottom",
            "typography.penalties.*",
            "typography.tolerance",
            "typography.emergencyStretch",
        ],
        typst: "set par(leading:, spacing:, justify:, first-line-indent:) and set text(costs:), where TeX's default penalties (150, 150, 50) are 100%. Typst never stretches a page, so typography.raggedBottom false, tolerance and emergencyStretch have nothing to drive",
        emit: |ctx| {
            let view = ctx.view;
            let body = view.body();
            let pitch = view.size_pt("typography.leading");
            let leading = (pitch - body) / body;
            let spacing = leading + view.pt("typography.parSkip") / body;
            let indent = view.pt("typography.parIndent");
            if !view.bool("typography.raggedBottom") {
                ctx.warn("typography.raggedBottom false has no Typst equivalent: Typst never stretches a page to its bottom margin");
            }
            for path in ["typography.tolerance", "typography.emergencyStretch"] {
                let _ = path;
            }
            let em = |n: f64| format!("{}em", typesep_style::units::fmt_pt(n));
            let percent = |penalty: f64, default: f64| {
                format!(
                    "{}%",
                    typesep_style::units::fmt_pt((penalty / default * 100.0).round())
                )
            };
            vec![
                format!(
                    "#set par(leading: {}, spacing: {}, justify: {}, first-line-indent: (amount: {}, all: false))",
                    em(leading),
                    em(spacing),
                    view.bool("typography.justify"),
                    typst_pt(indent)
                ),
                format!(
                    "#set text(costs: (widow: {}, orphan: {}, hyphenation: {}, runt: 100%))",
                    percent(view.num("typography.penalties.widow"), 150.0),
                    percent(view.num("typography.penalties.club"), 150.0),
                    percent(view.num("typography.penalties.hyphen"), 50.0)
                ),
            ]
        },
    },
    Emitter {
        name: "microtypography",
        needs: &[
            "typography.microtype.protrusion",
            "typography.microtype.expansion",
        ],
        typst: "set text(overhang:) for protrusion; Typst has no font expansion, so typography.microtype.expansion has nothing to drive",
        emit: |ctx| {
            let view = ctx.view;
            vec![format!(
                "#set text(overhang: {})",
                view.bool("typography.microtype.protrusion")
            )]
        },
    },
    Emitter {
        name: "colors",
        needs: &[
            "headings.h1.color",
            "headings.h2.color",
            "headings.h3.color",
            "headings.h4.color",
            "links.color",
        ],
        typst: "rgb() values bound to fs-heading-h1 … fs-heading-h4 and fs-link; a name is looked up among xcolor's base colors and the CSS names its svgnames option defined, so the presets keep their colors",
        emit: |ctx| {
            let mut out: Vec<String> = HEADING_KEYS
                .iter()
                .map(|level| {
                    format!(
                        "#let fs-heading-{level} = {}",
                        ctx.color(&format!("headings.{level}.color"))
                    )
                })
                .collect();
            out.push(format!("#let fs-link = {}", ctx.color("links.color")));
            out
        },
    },
    Emitter {
        name: "headings",
        needs: &["headings.*"],
        typst: "one show heading.where(level:) rule per level carrying size, face, case, alignment, family, color and the spaces, so every level knob applies exactly; pageBreakBefore is a weak pagebreak",
        emit: |ctx| {
            let view = ctx.view;
            let mut out = Vec::new();
            for (index, level) in HEADING_KEYS.iter().enumerate() {
                let n = index + 1;
                let prefix = format!("headings.{level}");
                let case = view.str(&format!("{prefix}.case"));
                let body = match case.as_str() {
                    "upper" => "upper(it.body)",
                    "lower" => "lower(it.body)",
                    _ => "it.body",
                };
                let role = view.str(&format!("{prefix}.font"));
                let align = ALIGN
                    .iter()
                    .find(|(name, _)| *name == view.str(&format!("{prefix}.align")))
                    .map(|(_, typst)| *typst)
                    .unwrap_or("left");
                let face = view.str(&format!("{prefix}.style"));
                let smallcaps = face == "smallcaps";
                let size = view.size_pt(&format!("{prefix}.size"));
                let space_before = typst_pt(view.pt(&format!("{prefix}.spaceBefore")));
                let page_break = if view.bool(&format!("{prefix}.pageBreakBefore")) {
                    "\n  pagebreak(weak: true)"
                } else {
                    ""
                };
                let above = if n == 1 {
                    format!(
                        "  v({}em + {space_before})\n  block(above: 0pt",
                        typesep_style::units::fmt_pt(leading_em(view))
                    )
                } else {
                    format!(
                        "  block(above: {}em + {space_before}",
                        typesep_style::units::fmt_pt(leading_em(view))
                    )
                };
                let content = if smallcaps {
                    format!("smallcaps({body})")
                } else {
                    body.to_owned()
                };
                out.push(format!(
                    "#show heading.where(level: {n}): it => {{{page_break}\n  set text(font: {}, size: {}, fill: fs-heading-{level}{})\n{above}, below: {below}, width: 100%)[\n    #set align({align})\n    #{content}\n  ]\n}}",
                    family_var(&role),
                    typst_pt(size * view.font(&role).scale.unwrap_or(1.0)),
                    face_args(&face),
                    below = gap(view, view.pt(&format!("{prefix}.spaceAfter")))
                ));
            }
            out
        },
    },
    Emitter {
        name: "blocks",
        needs: &["blocks.*"],
        typst: "set list/enum for the item spacing, indent and markers, and fs-quote, fs-bib, fs-tag-item, fs-code, fs-code-block, fs-table, fs-caption and fs-figure for the shapes LaTeX built environments for",
        emit: |ctx| {
            let view = ctx.view;
            let item_sep = gap_knob(view, "blocks.list.itemSep");
            let left_margin = view.pt("blocks.list.leftMargin");
            let body_indent = view.body();
            let indent = typst_pt((left_margin - body_indent).max(0.0));
            let markers: Vec<String> = view
                .list("blocks.list.bullets")
                .iter()
                .map(|b| escape_string(&bullet_char(b)))
                .collect();
            let marker = if markers.is_empty() {
                "(\"•\",)".to_owned()
            } else if markers.len() == 1 {
                format!("({},)", markers[0])
            } else {
                format!("({})", markers.join(", "))
            };
            let table_style = view.str("blocks.table.style");
            let mut out = vec![
                format!(
                    "#set list(spacing: {item_sep}, indent: {indent}, body-indent: {}, marker: {marker})",
                    typst_pt(body_indent)
                ),
                format!(
                    "#set enum(spacing: {item_sep}, indent: {indent}, body-indent: {})",
                    typst_pt(body_indent)
                ),
                format!(
                    "#let fs-quote(body) = block(above: {}, below: {}, width: 100%, inset: (left: {}, right: {}), text(size: {}, body))",
                    gap_knob(view, "blocks.blockquote.spaceAround"),
                    gap_knob(view, "blocks.blockquote.spaceAround"),
                    typst_pt(view.pt("blocks.blockquote.leftIndent")),
                    typst_pt(view.pt("blocks.blockquote.rightIndent")),
                    size_arg(view, "blocks.blockquote.size")
                ),
                format!(
                    "#let fs-bib(body) = block(above: {}, below: {}, width: 100%, {{ set par(hanging-indent: {}, first-line-indent: 0pt); text(size: {}, body) }})",
                    gap_knob(view, "blocks.bibliography.itemSep"),
                    gap_knob(view, "blocks.bibliography.itemSep"),
                    typst_pt(view.pt("blocks.bibliography.hangingIndent")),
                    size_arg(view, "blocks.bibliography.size")
                ),
                format!(
                    "#let fs-tag-item(label, body) = block(above: {item_sep}, below: {item_sep}, width: 100%, {{ set par(hanging-indent: 3.2em, first-line-indent: 0pt); box(width: 2.6em, label); h(0.6em); body }})"
                ),
                format!(
                    "#let fs-code(body) = text(font: fs-mono, size: {}, body)",
                    typst_pt(
                        view.size_pt("blocks.code.size") * view.font("mono").scale.unwrap_or(1.0)
                    )
                ),
                format!(
                    "#let fs-code-block(body) = block(above: {0}, below: {0}, width: 100%, {{ set par(justify: false, first-line-indent: 0pt, hanging-indent: 0pt); fs-code(body) }})",
                    gap(view, view.body() * 0.5)
                ),
                format!(
                    "#let fs-caption(body) = text(size: {}, body)",
                    size_arg(view, "blocks.figure.captionSize")
                ),
                format!(
                    "#let fs-table-size = {}",
                    size_arg(view, "blocks.table.size")
                ),
                format!(
                    "#let fs-figure-max-width = {}",
                    typesep_style::units::fmt_pt(view.num("blocks.figure.maxWidth"))
                ),
            ];
            let stroke = match table_style.as_str() {
                "grid" => "0.4pt",
                _ => "none",
            };
            let rules = match table_style.as_str() {
                "booktabs" => {
                    "\n  if header-rows > 0 and header-rows < rows { lines.push(table.hline(y: header-rows, stroke: 0.5pt)) }\n  lines.push(table.hline(y: 0, stroke: 1pt))\n  lines.push(table.hline(y: rows, stroke: 1pt))"
                }
                _ => "",
            };
            out.push(format!(
                "#let fs-table(caption: none, columns: 1, rows: 0, header-rows: 0, ..cells) = align(center, block(width: auto)[\n  #if caption != none {{ fs-caption(caption); v(0.4em, weak: true) }}\n  #{{\n  let lines = ()\n{rules}\n  text(size: fs-table-size, table(columns: columns, stroke: {stroke}, inset: 4pt, align: left, ..lines, ..cells))\n  }}\n])"
            ));
            let figure_align = ALIGN
                .iter()
                .find(|(name, _)| *name == view.str("blocks.figure.align"))
                .map(|(_, typst)| *typst)
                .unwrap_or("center");
            out.push(format!(
                "#let fs-figure(path, caption: none) = align({figure_align}, block(width: 100%, align({figure_align}, layout(size => {{\n  let natural = measure(image(path))\n  let factor = calc.min(1.0, fs-figure-max-width * size.width / natural.width, 0.8 * size.height / natural.height)\n  block[\n    #image(path, width: natural.width * factor)\n    #if caption != none {{ v(0.4em, weak: true); fs-caption(caption) }}\n  ]\n}}))))"
            ));
            out.push("#let fs-figure-inline(path) = box(image(path))".to_owned());
            out
        },
    },
    Emitter {
        name: "footnotes",
        needs: &[
            "blocks.footnote.size",
            "blocks.footnote.rule",
            "links.mode",
            "notes.style",
            "notes.maxHeight",
        ],
        typst: "show footnote.entry: set text() for the size and set footnote.entry(separator:) for the rule; endnotes (notes.style or links.mode endnote) are collected by the emitter and printed after the body under the h2 style, Typst needing no endnotes package. Typst sets footnotes inside tables and headings itself, so nothing reserves numbers for them. notes.maxHeight is measured at layout time: a numbered note taller than it is set at the end of the entry and a one-line footnote in its place names the page",
        emit: |ctx| {
            let view = ctx.view;
            let size = size_arg(view, "blocks.footnote.size");
            let separator = if view.bool("blocks.footnote.rule") {
                "line(length: 30%, stroke: 0.5pt)"
            } else {
                "none"
            };
            vec![
                format!("#let fs-note-size = {size}"),
                "#show footnote.entry: set text(size: fs-note-size)".to_owned(),
                "#show footnote.entry: set par(hanging-indent: 1em, first-line-indent: 0pt)"
                    .to_owned(),
                format!("#set footnote.entry(indent: 0pt, separator: {separator})"),
                "#set footnote(numbering: \"1\")".to_owned(),
                format!(
                    "#let fs-endnote(number, body) = block(above: {0}, below: {0}, width: 100%, {{ set par(hanging-indent: 1.5em, first-line-indent: 0pt); text(size: fs-note-size, {{ super(number); h(0.3em); body }}) }})",
                    gap(view, view.body() * 0.4)
                ),
                format!("#let fs-note-max-height = {}", typst_pt(note_max_height_pt(view))),
                format!("#let fs-text-height = {}", typst_pt(view.text_height_pt())),
                format!("#let fs-note-measure = {}", typst_pt(view.text_width_pt())),
                format!(
                    "#let fs-note(entry, number, body) = context {{\n  let height = if fs-note-max-height > 0pt {{ measure(text(size: fs-note-size, body), width: fs-note-measure).height }} else {{ 0pt }}\n  if height > fs-note-max-height {{\n    [#metadata((entry: entry, number: number, heights: calc.round(height / fs-text-height, digits: 2), body: body))<fs-moved-note>]\n    footnote[Note #number is set at the end of the entry, #context {{\n      let at = query(<fs-moved-note-at>).find(m => m.value.entry == entry and m.value.number == number)\n      if at == none {{ [at its end] }} else {{ link(at.location())[page #numbering(\"{numbering}\", ..counter(page).at(at.location()))] }}\n    }}.]\n  }} else {{ footnote(body) }}\n}}",
                    numbering = page_numbering(view)
                ),
                "#let fs-moved-notes(entry, title: none) = context {\n  let moved = query(<fs-moved-note>).filter(m => m.value.entry == entry)\n  if moved.len() > 0 {\n    if title != none { heading(level: 2, title) }\n    for m in moved {\n      [#metadata((entry: entry, number: m.value.number))<fs-moved-note-at>]\n      fs-endnote([#m.value.number], m.value.body)\n    }\n  }\n}".to_owned(),
            ]
        },
    },
    Emitter {
        name: "headers and footers",
        needs: &["pageFurniture.*", "page.twoside"],
        typst: "set page(header:, footer:) with a three-slot grid that mirrors on verso pages when twoside, and {token} expansion into counter(page), a query for the current section and the document metadata",
        emit: |ctx| {
            let view = ctx.view;
            let twoside = view.bool("page.twoside");
            let numbering = page_numbering(view);
            let size = size_arg(view, "pageFurniture.size");
            let mut out = vec![
                format!("#let fs-page-numbering = \"{numbering}\""),
                "#set page(numbering: fs-page-numbering)".to_owned(),
            ];
            for area in ["header", "footer"] {
                let slots: Vec<String> = ["left", "center", "right"]
                    .iter()
                    .map(|slot| {
                        template_expr(
                            &view.str(&format!("pageFurniture.{area}.{slot}")),
                            ctx.input,
                        )
                    })
                    .collect();
                if slots.iter().all(|s| s == "[]") {
                    out.push(format!("#set page({area}: none)"));
                    continue;
                }
                let rule = view.bool(&format!("pageFurniture.rules.{area}"));
                let grid = "grid(columns: (1fr, auto, 1fr), align: (left + horizon, center + horizon, right + horizon), column-gutter: 0.5em, ..slots)";
                // A document written right to left lays a grid out from the right, and its
                // mirrored pages are bound on the right, so its odd pages are the left-hand
                // ones: its slots are set left to right, and mirrored on the even pages.
                let rtl = ctx.input.dir == Dir::Rtl;
                let mirror = match (twoside, rtl) {
                    (true, false) => {
                        "\n    let slots = if calc.even(here().page()) { (right-slot, center-slot, left-slot) } else { (left-slot, center-slot, right-slot) }"
                    }
                    (true, true) => {
                        "\n    set text(dir: ltr)\n    let slots = if calc.odd(here().page()) { (right-slot, center-slot, left-slot) } else { (left-slot, center-slot, right-slot) }"
                    }
                    (false, false) => "\n    let slots = (left-slot, center-slot, right-slot)",
                    (false, true) => {
                        "\n    set text(dir: ltr)\n    let slots = (left-slot, center-slot, right-slot)"
                    }
                };
                let decoration = match (area, rule) {
                    ("header", true) => format!(
                        "{grid}\n    v(2pt, weak: true)\n    line(length: 100%, stroke: 0.4pt)"
                    ),
                    ("footer", true) => format!(
                        "line(length: 100%, stroke: 0.4pt)\n    v(2pt, weak: true)\n    {grid}"
                    ),
                    _ => grid.to_owned(),
                };
                out.push(format!(
                    "#set page({area}: context {{\n    set text(size: {size})\n    let left-slot = {}\n    let center-slot = {}\n    let right-slot = {}{mirror}\n    {decoration}\n  }})",
                    slots[0], slots[1], slots[2]
                ));
            }
            out
        },
    },
    Emitter {
        name: "links and bookmarks",
        needs: &["links.mode", "links.underline", "output.bookmarks"],
        typst: "show link: set text(fill:) with underline when asked; set heading(bookmarked:) writes the PDF outline from the headings",
        emit: |ctx| {
            let view = ctx.view;
            let mut out = vec![format!(
                "#set heading(bookmarked: {}, outlined: true, numbering: none)",
                view.bool("output.bookmarks")
            )];
            out.push("#show link: it => text(fill: fs-link, it)".to_owned());
            if view.bool("links.underline") {
                out.push("#show link: underline".to_owned());
            }
            out
        },
    },
    Emitter {
        name: "maths",
        needs: &[],
        typst: "the mitex scope (its LaTeX command tables as Typst functions) with fs-math, which evaluates the Typst maths mitex converts each formula into; the entry's own local.js macros are \\newcommand lines in front of every formula",
        emit: |_ctx| {
            vec![
                format!("#import \"{}\": mitex-scope", crate::world::MITEX_SCOPE_PATH),
                "#let fs-math-scope = mitex-scope + (mitexref: it => it, mitexcite: it => it, mitexlabel: it => none)".to_owned(),
                "#let fs-math(src, block: false) = math.equation(block: block, eval(\"$\" + src + \"$\", scope: fs-math-scope))".to_owned(),
                "#let fs-math-raw(src) = fs-code(src)".to_owned(),
            ]
        },
    },
];

/// The definition of `helper`. The margin note reads the page geometry from the style, so
/// it knows which margin is on the left of the page it lands on, and which column it is in.
/// It is set level with its line in the margin beside its column when there are columns,
/// else in the outer margin of mirrored pages (as LaTeX's `\marginpar` is), else in the one
/// on the side lines start from, the left one, or the right one in text written right to
/// left, whose mirrored pages are bound on the right; and in the line instead when it is
/// wider than that margin. `space` is the space
/// that followed it in the text, which it gives back only there. Where the note is placed is
/// read inside its box: a line may break between the box and what comes before it.
fn helper_definition(view: &View<'_>, helper: Helper) -> String {
    let around = gap(view, view.body() * 0.5);
    match helper {
        // Typst tracks between the letters of a run and not after its last one, so a comma
        // or a space follows a spaced word as it follows any other; CSS letter-spacing does
        // follow the last letter, which the EPUB's rule takes back.
        Helper::Spaced => "#let fs-spaced(body) = text(tracking: 0.2em, body)".to_owned(),
        Helper::MarginNote => {
            let (width, _) = view.paper_pt();
            let inner = view.pt("page.margins.inner") + view.pt("page.bindingOffset");
            let outer = view.pt("page.margins.outer");
            let twoside = view.bool("page.twoside");
            let beside_right = if view.int("page.columns") > 1 {
                format!("at.x > {} / 2", typst_pt(width))
            } else if twoside {
                "calc.odd(at.page) != rtl".to_owned()
            } else {
                "rtl".to_owned()
            };
            format!(
                "#let fs-margin-note(body, space: false) = context {{\n  set text(size: fs-note-size)\n  let rtl = text.dir == rtl\n  let margins(at) = if {twoside} and (calc.even(at.page) != rtl) {{ ({outer}, {inner}) }} else {{ ({inner}, {outer}) }}\n  let beside-right(at) = {beside_right}\n  let gap = 0.6em.to-absolute()\n  let at = here().position()\n  let (left-margin, right-margin) = margins(at)\n  let room = (if beside-right(at) {{ right-margin }} else {{ left-margin }}) - 2 * gap\n  if measure(body).width > room {{ if space {{ body + [ ] }} else {{ body }} }} else {{\n    box(width: 0pt, context {{\n      let at = here().position()\n      let (left-margin, right-margin) = margins(at)\n      if beside-right(at) {{\n        place(top + left, dx: {width} - right-margin + gap - at.x, dy: -1em, body)\n      }} else {{\n        place(top + right, dx: left-margin - gap - at.x, dy: -1em, body)\n      }}\n    }})\n  }}\n}}",
                width = typst_pt(width),
                inner = typst_pt(inner),
                outer = typst_pt(outer),
            )
        }
        Helper::OutlineOnly => {
            "#let fs-outline-only(body) = { show heading: none; body }".to_owned()
        }
        Helper::RemarkNumber => "#let fs-remark-number(number) = strong(number)".to_owned(),
        Helper::Aside => format!(
            "#let fs-aside(body) = block(above: {around}, below: {around}, width: 100%, inset: (x: 1em, y: 0.8em), stroke: 0.5pt + luma(60%), body)"
        ),
        Helper::Columns => format!(
            "#let fs-columns(..columns) = block(above: {around}, below: {around}, width: 100%, grid(columns: (1fr,) * columns.pos().len(), column-gutter: 1.5em, ..columns))"
        ),
        Helper::Verse => format!(
            "#let fs-verse(..lines) = block(above: {around}, below: {around}, width: 100%, {{\n  set par(first-line-indent: 0pt, hanging-indent: 1em, justify: false)\n  for (indent, body) in lines.pos() {{ block(above: {line}, below: {line}, inset: (left: indent * 1em), par(body)) }}\n}})",
            line = gap(view, 0.0)
        ),
        Helper::SizedFigure => {
            let figure_align = ALIGN
                .iter()
                .find(|(name, _)| *name == view.str("blocks.figure.align"))
                .map(|(_, typst)| *typst)
                .unwrap_or("center");
            format!(
                "#let fs-figure-at(path, width, caption: none) = align({figure_align}, block(width: 100%, align({figure_align}, layout(size => {{\n  let natural = measure(image(path))\n  let shown = calc.min(width, fs-figure-max-width * size.width, natural.width * (0.8 * size.height / natural.height))\n  block[\n    #image(path, width: shown)\n    #if caption != none {{ v(0.4em, weak: true); fs-caption(caption) }}\n  ]\n}}))))\n#let fs-figure-inline-at(path, width) = box(layout(size => image(path, width: calc.min(width, size.width))))"
            )
        }
        Helper::PlainTable => "#let fs-table-plain(caption: none, columns: 1, ..cells) = align(center, block(width: auto)[\n  #if caption != none { fs-caption(caption); v(0.4em, weak: true) }\n  #text(size: fs-table-size, table(columns: columns, stroke: none, inset: 4pt, align: left, ..cells))\n])".to_owned(),
    }
}

fn content_block(text: &str) -> String {
    format!("[{}]", guard_line_start(&escape_text(text)))
}

pub fn template_expr(text: &str, input: &PreambleInput) -> String {
    if text.is_empty() {
        return "[]".to_owned();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut literal = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        let token = &rest[open + 1..open + close];
        if !token.is_empty() && token.chars().all(|c| c.is_alphanumeric() || c == '_') {
            let expansion = match token {
                "page" => Some("counter(page).display(fs-page-numbering)".to_owned()),
                "pages" => Some("numbering(fs-page-numbering, ..counter(page).final())".to_owned()),
                "section" => Some(
                    "{ let found = query(selector(heading.where(level: 2)).before(here())); if found.len() > 0 { found.last().body } }"
                        .to_owned(),
                ),
                "title" => Some(content_block(&input.title)),
                "author" => Some(content_block(&input.byline)),
                "edition" => Some(content_block(&input.edition)),
                "date" => Some(content_block(&input.date)),
                _ => None,
            };
            if let Some(expansion) = expansion {
                literal.push_str(&rest[..open]);
                if !literal.is_empty() {
                    parts.push(content_block(&literal));
                    literal.clear();
                }
                parts.push(expansion);
                rest = &rest[open + close + 1..];
                continue;
            }
        }
        literal.push_str(&rest[..open + close + 1]);
        rest = &rest[open + close + 1..];
    }
    literal.push_str(rest);
    if !literal.is_empty() {
        parts.push(content_block(&literal));
    }
    match parts.len() {
        0 => "[]".to_owned(),
        1 => parts.pop().expect("one part"),
        _ => format!("{{ {} }}", parts.join("; ")),
    }
}

pub fn build_preamble(
    view: &View<'_>,
    input: &PreambleInput,
    fonts: Option<&Fonts>,
) -> (String, Vec<String>) {
    let ctx = Ctx::new(view, input, fonts);
    let mut text = String::new();
    for emitter in EMITTERS {
        let lines = (emitter.emit)(&ctx);
        if lines.is_empty() {
            continue;
        }
        let _ = writeln!(text, "// ---- {} ----", emitter.name);
        for line in lines {
            let _ = writeln!(text, "{line}");
        }
    }
    if !input.helpers.is_empty() {
        let _ = writeln!(text, "// ---- constructs ----");
        for helper in &input.helpers {
            let _ = writeln!(text, "{}", helper_definition(view, *helper));
        }
    }
    let advanced = view.str("advanced.preamble");
    if !advanced.trim().is_empty() {
        let _ = writeln!(text, "// ---- advanced.preamble ----\n{}", advanced.trim());
    }
    (text, ctx.into_warnings())
}

pub fn need_matches(need: &str, knob_path: &str) -> bool {
    match need.strip_suffix('*') {
        Some(prefix) => knob_path.starts_with(prefix),
        None => need == knob_path,
    }
}

pub fn emitter_for(knob_path: &str) -> Vec<&'static Emitter> {
    EMITTERS
        .iter()
        .filter(|e| e.needs.iter().any(|n| need_matches(n, knob_path)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use typesep_style::{ResolveOptions, Style, resolve_style};

    fn style_with(sets: &[&str]) -> Style {
        resolve_style(ResolveOptions {
            preset: Some("halfletter"),
            file: None,
            sets: sets.iter().map(|s| (*s).into()).collect(),
            schema: None,
            default_preset: None,
        })
        .unwrap()
    }

    #[test]
    fn the_preamble_sets_the_page_the_pitch_and_the_fonts() {
        let style = style_with(&[]);
        let view = View::new(&style);
        let (text, warnings) = build_preamble(&view, &PreambleInput::default(), None);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(
            text.contains("#set text(size: 9.963pt, top-edge: 1em, bottom-edge: \"baseline\")"),
            "10 TeX pt is 9.963 Typst pt"
        );
        assert!(
            text.contains("#set page(width: 396pt, height: 612pt"),
            "{text}"
        );
        assert!(text.contains("left: 38.88pt, right: 38.88pt"), "{text}");
        assert!(
            text.contains("#set par(leading: 0.5em, spacing: 0.5em, justify: true"),
            "{text}"
        );
        assert!(
            text.contains("#let fs-main = (\"TeX Gyre Termes\","),
            "{text}"
        );
        assert!(
            text.contains("#let fs-math-font = (\"TeX Gyre Termes Math\","),
            "{text}"
        );
        assert!(text.contains("#show heading.where(level: 1)"));
        assert!(text.contains("#let fs-heading-h1 = rgb(\"#000000\")"));
        assert!(text.contains("footer-descent: 50%"), "{text}");
        assert!(
            !text.contains("#set page(columns:"),
            "one column asks for nothing"
        );
        assert!(
            text.contains("#set document(title: \"\", author: (), description: none)"),
            "{text}"
        );
        let input = PreambleInput {
            title: "Pleasure".into(),
            byline: "A. Author".into(),
            subject: "A \"Publisher\"".into(),
            ..PreambleInput::default()
        };
        let (text, _) = build_preamble(&view, &input, None);
        assert!(
            text.contains(
                "#set document(title: \"Pleasure\", author: (\"A. Author\",), description: \"A \\\"Publisher\\\"\")"
            ),
            "the publisher is the PDF's subject: {text}"
        );
    }

    #[test]
    fn a_font_list_names_the_fallback_families_the_document_needs() {
        let style = style_with(&["fonts.fallback=Noto Serif,TeX Gyre Termes Math,TeX Gyre Heros"]);
        let view = View::new(&style);
        let fonts = Fonts::embedded();
        let lists = |text: &str| {
            let input = PreambleInput {
                chars: text.chars().collect(),
                ..PreambleInput::default()
            };
            let (preamble, warnings) = build_preamble(&view, &input, Some(&fonts));
            let lists: Vec<String> = preamble
                .lines()
                .filter(|line| {
                    line.starts_with("#let fs-main =") || line.starts_with("#let fs-math-font =")
                })
                .map(str::to_owned)
                .collect();
            (lists, warnings)
        };
        assert_eq!(
            lists("Plain text, € and –."),
            (
                vec![
                    "#let fs-main = (\"TeX Gyre Termes\",)".to_owned(),
                    "#let fs-math-font = (\"TeX Gyre Termes Math\",)".to_owned()
                ],
                vec![]
            ),
            "what the main font has needs no fallback, and Noto Serif is not installed"
        );
        let (needed, warnings) = lists("Plain text ℵ ʃ");
        assert_eq!(
            needed,
            [
                "#let fs-main = (\"TeX Gyre Termes\",)",
                "#let fs-math-font = (\"TeX Gyre Termes Math\",)"
            ],
            "the maths font is one of the main fonts, which Typst falls back to of itself"
        );
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with(
                "1 character of the text has no font and is not printed: Latin ʃ (in Noto Serif)"
            ),
            "{warnings:?}"
        );
        let maths_first = style_with(&[
            "fonts.math=TeX Gyre Heros",
            "fonts.fallback=TeX Gyre Termes Math",
        ]);
        let input = PreambleInput {
            chars: "ℵ".chars().collect(),
            ..PreambleInput::default()
        };
        let (preamble, _) = build_preamble(&View::new(&maths_first), &input, Some(&fonts));
        assert!(
            preamble.contains("#let fs-main = (\"TeX Gyre Termes\", \"TeX Gyre Termes Math\")"),
            "{preamble}"
        );
        let (unchecked, _) = build_preamble(&view, &input, None);
        assert!(
            unchecked.contains("#let fs-main = (\"TeX Gyre Termes\", \"Noto Serif\", \"TeX Gyre Termes Math\", \"TeX Gyre Heros\")"),
            "without the fonts at hand, every family is named: {unchecked}"
        );
    }

    #[test]
    fn the_long_note_policy_reaches_typst() {
        let style = style_with(&[]);
        let view = View::new(&style);
        assert!((note_max_height_pt(&view) - view.text_height_pt()).abs() < 1e-9);
        let (text, _) = build_preamble(&view, &PreambleInput::default(), None);
        assert!(text.contains("#let fs-note-max-height = 518.4pt"), "{text}");
        assert!(text.contains("#let fs-text-height = 518.4pt"), "{text}");
        assert!(text.contains("#let fs-note-measure = 318.24pt"), "{text}");
        assert!(
            text.contains("#let fs-note(entry, number, body) = context"),
            "{text}"
        );
        assert!(
            text.contains("#let fs-moved-notes(entry, title: none)"),
            "{text}"
        );

        let half = style_with(&["notes.maxHeight=0.5"]);
        let half = View::new(&half);
        assert!((note_max_height_pt(&half) - half.text_height_pt() / 2.0).abs() < 1e-9);
        let length = style_with(&["notes.maxHeight=200pt"]);
        assert_eq!(note_max_height_pt(&View::new(&length)), 200.0);
        let off = style_with(&["notes.maxHeight=0"]);
        assert_eq!(note_max_height_pt(&View::new(&off)), 0.0);

        let roman = style_with(&["pageFurniture.numbering.style=roman"]);
        let (text, _) = build_preamble(&View::new(&roman), &PreambleInput::default(), None);
        assert!(
            text.contains("page #numbering(\"i\", ..counter(page).at("),
            "{text}"
        );
    }

    #[test]
    fn twoside_columns_and_penalties_reach_typst() {
        let style = style_with(&[
            "page.twoside=true",
            "page.columns=2",
            "page.bindingOffset=10pt",
            "typography.penalties.widow=300",
            "typography.justify=false",
        ]);
        let view = View::new(&style);
        let (text, _) = build_preamble(&view, &PreambleInput::default(), None);
        assert!(
            text.contains("inside: 48.843pt, outside: 38.88pt"),
            "{text}"
        );
        assert!(text.contains("#set page(columns: 2)"));
        assert!(text.contains("widow: 200%"), "{text}");
        assert!(text.contains("justify: false"));
    }

    #[test]
    fn options_without_a_typst_equivalent_say_so() {
        let style = style_with(&[
            "typography.raggedBottom=false",
            "advanced.documentClass=book",
        ]);
        let view = View::new(&style);
        let (_, warnings) = build_preamble(&view, &PreambleInput::default(), None);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("advanced.documentClass"))
        );
        assert!(warnings.iter().any(|w| w.contains("raggedBottom")));
    }

    #[test]
    fn header_tokens_expand_to_typst_expressions() {
        let input = PreambleInput {
            title: "Turing Machines".into(),
            byline: "David Barker-Plummer".into(),
            ..PreambleInput::default()
        };
        assert_eq!(template_expr("", &input), "[]");
        assert_eq!(
            template_expr("{page}", &input),
            "counter(page).display(fs-page-numbering)"
        );
        assert_eq!(template_expr("{title}", &input), "[Turing Machines]");
        assert_eq!(
            template_expr("{page} of {pages}", &input),
            "{ counter(page).display(fs-page-numbering); [ of ]; numbering(fs-page-numbering, ..counter(page).final()) }"
        );
        assert_eq!(
            template_expr("{page} / {pages}", &input),
            "{ counter(page).display(fs-page-numbering); [ \\/ ]; numbering(fs-page-numbering, ..counter(page).final()) }",
            "a slash at the start of a block is a term list to Typst (the manuscript preset's footer)"
        );
        assert_eq!(
            template_expr("{unknown}", &input),
            "[{unknown}]",
            "braces are not markup in Typst"
        );
    }

    #[test]
    fn every_knob_the_latex_emitters_read_still_has_an_emitter() {
        for knob in typesep_style::knobs() {
            let ours = emitter_for(&knob.path);
            let optional = [
                "toc.",
                "cover.",
                "notes.style",
                "links.sections",
                "blocks.remark",
                "advanced.hooks",
                "advanced.preamble",
                "pageFurniture.numbering",
            ];
            if ours.is_empty() && !optional.iter().any(|p| knob.path.starts_with(p)) {
                panic!("{} drives nothing", knob.path);
            }
        }
        assert!(
            emitter_for("page.margins.top")
                .iter()
                .any(|e| e.name == "page geometry")
        );
        assert!(
            emitter_for("headings.h2.size")
                .iter()
                .any(|e| e.name == "headings")
        );
        assert!(emitter_for("links.mode").len() >= 2);
    }

    #[test]
    fn bullets_and_languages_map_onto_typst() {
        assert_eq!(bullet_char("\\textbullet"), "•");
        assert_eq!(bullet_char("\\textopenbullet"), "◦");
        assert_eq!(bullet_char("\\textendash"), "–");
        assert_eq!(bullet_char("*"), "*");
        assert_eq!(bullet_char("\\nosuch"), "nosuch");
        assert_eq!(lang_code("english"), Some("en"));
        assert_eq!(lang_code("NGerman"), Some("de"));
        assert_eq!(lang_code("klingon"), None);
    }

    #[test]
    fn the_language_follows_the_document_unless_the_style_names_one() {
        let lang_line = |sets: &[&str], lang: &str| {
            let style = style_with(sets);
            let view = View::new(&style);
            let input = PreambleInput {
                lang: lang.to_owned(),
                ..PreambleInput::default()
            };
            let (text, warnings) = build_preamble(&view, &input, None);
            let line = text
                .lines()
                .find(|line| line.starts_with("#set text(lang:"))
                .expect("a language line")
                .to_owned();
            (line, warnings)
        };
        let set = |code: &str| format!("#set text(lang: \"{code}\", hyphenate: true)");
        for (lang, code) in [
            ("en", "en"),
            ("de", "de"),
            ("fr", "fr"),
            ("it", "it"),
            ("es", "es"),
            ("hi", "hi"),
        ] {
            assert_eq!(lang_line(&[], lang), (set(code), vec![]), "{lang}");
        }
        assert_eq!(lang_line(&[], "de-AT"), (set("de"), vec![]));
        assert_eq!(
            lang_line(&[], ""),
            (set("en"), vec![]),
            "a document that names no language is set in English without a word"
        );
        assert_eq!(
            lang_line(&[], "tlh"),
            (
                set("en"),
                vec![
                    "the document's language \"tlh\" is not one typography.language auto knows; using english"
                        .to_owned()
                ]
            )
        );
        assert_eq!(
            lang_line(&[], "ar"),
            (
                "#set text(lang: \"ar\", hyphenate: false)".to_owned(),
                vec![
                    "Typst has no hyphenation patterns for ar, the document's language, so it is not hyphenated"
                        .to_owned()
                ]
            ),
            "a language without patterns is not hyphenated, which is said"
        );
        assert_eq!(
            lang_line(&["typography.hyphenate=false"], "ar"),
            (
                "#set text(lang: \"ar\", hyphenate: false)".to_owned(),
                vec![]
            ),
            "and needs no word when hyphenation is off anyway"
        );
        let style = style_with(&[]);
        let input = PreambleInput {
            lang: "ar".to_owned(),
            dir: Dir::Rtl,
            ..PreambleInput::default()
        };
        let (text, _) = build_preamble(&View::new(&style), &input, None);
        assert!(
            text.contains("#set text(lang: \"ar\", dir: rtl, hyphenate: false)\n"),
            "a document written right to left says so: {text}"
        );
        assert_eq!(
            lang_line(&["typography.language=french"], "de"),
            (set("fr"), vec![]),
            "a language the style names wins"
        );
        assert_eq!(
            lang_line(&["typography.language=auto"], "it"),
            (set("it"), vec![])
        );
    }
}
