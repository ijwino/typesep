use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use typesep_style::resolve::nearest;
use typst::foundations::Bytes;
use typst::text::{Font, FontBook, FontInfo};
use typst::utils::LazyHash;
use typst_kit::fonts::{FontPath, FontStore};
use unicode_script::{Script, UnicodeScript};

use typesep_style::View;

pub const EMBEDDED: [(&str, &[u8]); 13] = [
    (
        "texgyretermes-regular.otf",
        include_bytes!("../fonts/texgyretermes-regular.otf"),
    ),
    (
        "texgyretermes-bold.otf",
        include_bytes!("../fonts/texgyretermes-bold.otf"),
    ),
    (
        "texgyretermes-italic.otf",
        include_bytes!("../fonts/texgyretermes-italic.otf"),
    ),
    (
        "texgyretermes-bolditalic.otf",
        include_bytes!("../fonts/texgyretermes-bolditalic.otf"),
    ),
    (
        "texgyretermes-math.otf",
        include_bytes!("../fonts/texgyretermes-math.otf"),
    ),
    (
        "texgyreheros-regular.otf",
        include_bytes!("../fonts/texgyreheros-regular.otf"),
    ),
    (
        "texgyreheros-bold.otf",
        include_bytes!("../fonts/texgyreheros-bold.otf"),
    ),
    (
        "texgyreheros-italic.otf",
        include_bytes!("../fonts/texgyreheros-italic.otf"),
    ),
    (
        "texgyreheros-bolditalic.otf",
        include_bytes!("../fonts/texgyreheros-bolditalic.otf"),
    ),
    (
        "texgyrecursor-regular.otf",
        include_bytes!("../fonts/texgyrecursor-regular.otf"),
    ),
    (
        "texgyrecursor-bold.otf",
        include_bytes!("../fonts/texgyrecursor-bold.otf"),
    ),
    (
        "texgyrecursor-italic.otf",
        include_bytes!("../fonts/texgyrecursor-italic.otf"),
    ),
    (
        "texgyrecursor-bolditalic.otf",
        include_bytes!("../fonts/texgyrecursor-bolditalic.otf"),
    ),
];

pub const FONTS_ENV: &str = "TYPESEP_FONTS";
pub const FONTS_DIR: &str = "fonts";

pub fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os(FONTS_ENV) {
        let dir = PathBuf::from(dir);
        if dir.is_dir() {
            dirs.push(dir);
        }
    }
    let local = PathBuf::from(FONTS_DIR);
    if local.is_dir() {
        dirs.push(local);
    }
    dirs
}

pub struct Fonts {
    store: FontStore,
}

impl Fonts {
    pub fn discover(dirs: &[PathBuf], system: bool) -> Fonts {
        let mut store = FontStore::new();
        for dir in dirs {
            store.extend(typst_kit::fonts::scan(dir));
        }
        for (_, data) in EMBEDDED {
            store.extend(Font::iter(Bytes::new(data)).map(|font| {
                let info = font.info().clone();
                (font, info)
            }));
        }
        if system {
            store.extend(typst_kit::fonts::system());
        }
        Fonts { store }
    }

    pub fn embedded() -> Fonts {
        Fonts::discover(&[], false)
    }

    pub fn book(&self) -> &LazyHash<FontBook> {
        self.store.book()
    }

    pub fn font(&self, index: usize) -> Option<Font> {
        self.store.font(index)
    }

    pub fn families(&self) -> Vec<String> {
        self.book()
            .families()
            .map(|(family, _)| family.to_owned())
            .collect()
    }

    pub fn has_family(&self, family: &str) -> bool {
        self.book().contains_family(&family.to_lowercase())
    }

    /// Whether a face of `family` has a glyph for `c`, as the font's own character map says.
    pub fn family_covers(&self, family: &str, c: char) -> bool {
        let book = self.book();
        book.select_family(&family.to_lowercase())
            .filter_map(|index| book.info(index))
            .any(|info| info.coverage.contains(c as u32))
    }

    /// Whether any font at hand has a glyph for `c`: where Typst looks last, when no family
    /// a text names has it.
    pub fn any_covers(&self, c: char) -> bool {
        let book = self.book();
        book.families()
            .flat_map(|(_, faces)| faces)
            .filter_map(|index| book.info(index))
            .any(|info| info.coverage.contains(c as u32))
    }

    pub fn add_file(&mut self, path: &Path) -> Result<String, String> {
        let data = std::fs::read(path)
            .map_err(|e| format!("cannot read font file {}: {e}", path.display()))?;
        let mut family = None;
        for (index, info) in FontInfo::iter(&data).enumerate() {
            family.get_or_insert_with(|| info.family.to_string());
            self.store.push((
                FontPath {
                    path: path.to_owned(),
                    index: index as u32,
                },
                info,
            ));
        }
        family.ok_or_else(|| format!("{} holds no font Typst can read", path.display()))
    }
}

/// The scripts Noto has a serif face for, whose name is "Noto Serif" and the script's.
const NOTO_SERIF_SCRIPTS: [Script; 22] = [
    Script::Armenian,
    Script::Bengali,
    Script::Ethiopic,
    Script::Georgian,
    Script::Gujarati,
    Script::Gurmukhi,
    Script::Kannada,
    Script::Khmer,
    Script::Lao,
    Script::Malayalam,
    Script::Myanmar,
    Script::Oriya,
    Script::Sinhala,
    Script::Tamil,
    Script::Telugu,
    Script::Thai,
    Script::Tibetan,
    Script::Devanagari,
    Script::Hebrew,
    Script::Balinese,
    Script::Tangut,
    Script::Yezidi,
];

/// A family that has the characters of `script`, to name when no font at hand has them.
pub fn suggested_family(script: Script) -> String {
    match script {
        Script::Han | Script::Bopomofo => "Noto Serif CJK SC".to_owned(),
        Script::Hiragana | Script::Katakana => "Noto Serif CJK JP".to_owned(),
        Script::Hangul => "Noto Serif CJK KR".to_owned(),
        Script::Arabic => "Noto Naskh Arabic".to_owned(),
        Script::Latin | Script::Greek | Script::Cyrillic => "Noto Serif".to_owned(),
        Script::Common | Script::Inherited | Script::Unknown => "Noto Sans Symbols".to_owned(),
        script if NOTO_SERIF_SCRIPTS.contains(&script) => {
            format!("Noto Serif {}", script.full_name().replace('_', " "))
        }
        script => format!("Noto Sans {}", script.full_name().replace('_', " ")),
    }
}

/// The script whose fonts would have `c`: its own, or for a character many scripts share (a
/// punctuation mark, a combining accent) the first of those that use it in an order of the
/// scripts whose fonts most often have such marks, or the one of the block it is in.
pub fn script_of(c: char) -> Script {
    let script = c.script();
    if !matches!(script, Script::Common | Script::Inherited) {
        return script;
    }
    let users = c.script_extension();
    let shared_by_all = users.is_common() || users.is_inherited();
    for known in [
        Script::Latin,
        Script::Greek,
        Script::Cyrillic,
        Script::Han,
        Script::Hiragana,
        Script::Katakana,
        Script::Hangul,
        Script::Arabic,
        Script::Hebrew,
        Script::Devanagari,
    ] {
        if !shared_by_all && users.contains_script(known) {
            return known;
        }
    }
    match c as u32 {
        0x0000..=0x052F
        | 0x1AB0..=0x1AFF
        | 0x1D00..=0x1DFF
        | 0x1E00..=0x1FFF
        | 0x2000..=0x206F
        | 0x2C60..=0x2C7F
        | 0xA720..=0xA7FF
        | 0xAB30..=0xAB6F => Script::Latin,
        0x2E80..=0x2FFF | 0x3000..=0x303F | 0x3200..=0x33FF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF => {
            Script::Han
        }
        _ => script,
    }
}

/// Characters that are not set with a glyph of their own, which no font needs to have:
/// spaces, controls, and those Unicode says are ignorable (joiners, marks of direction,
/// variation selectors).
pub fn takes_no_glyph(c: char) -> bool {
    c.is_whitespace() || c.is_control() || typst::text::is_default_ignorable(c)
}

/// Where the characters of a text are to come from: the families of `fallback` the text
/// needs, each that has a character of it that the main and maths families lack and that no
/// family before it has; and the characters no font at hand has at all, by script.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Coverage {
    pub needed: Vec<String>,
    /// By script, in the order of the first character of each.
    pub missing: Vec<(Script, Vec<char>)>,
}

pub fn coverage(
    fonts: &Fonts,
    chars: &BTreeSet<char>,
    base: &[&str],
    fallback: &[String],
) -> Coverage {
    let lacking: Vec<char> = chars
        .iter()
        .copied()
        .filter(|&c| !takes_no_glyph(c))
        .filter(|&c| !base.iter().any(|family| fonts.family_covers(family, c)))
        .collect();
    let mut needed: Vec<String> = Vec::new();
    let mut left: Vec<char> = lacking.clone();
    for family in fallback {
        if needed.iter().any(|f| f.eq_ignore_ascii_case(family)) {
            continue;
        }
        let (covered, rest): (Vec<char>, Vec<char>) =
            left.iter().partition(|&&c| fonts.family_covers(family, c));
        if !covered.is_empty() {
            needed.push(family.clone());
            left = rest;
        }
    }
    let mut missing: Vec<(Script, Vec<char>)> = Vec::new();
    for c in lacking.into_iter().filter(|&c| !fonts.any_covers(c)) {
        let script = script_of(c);
        match missing.iter_mut().find(|(known, _)| *known == script) {
            Some((_, chars)) => chars.push(c),
            None => missing.push((script, vec![c])),
        }
    }
    Coverage { needed, missing }
}

/// How many characters of a script the warning shows before it says there are more.
const SHOWN: usize = 12;

/// What the warning calls the characters of `script`.
fn script_label(script: Script) -> String {
    match script {
        Script::Common => "symbols".to_owned(),
        Script::Inherited => "combining marks".to_owned(),
        Script::Unknown => "unassigned".to_owned(),
        script => script.full_name().replace('_', " "),
    }
}

/// The warning that the characters in `missing` have no font, so are not printed, with a
/// family for each script that has them.
pub fn describe_missing_characters(missing: &[(Script, Vec<char>)]) -> Option<String> {
    let total: usize = missing.iter().map(|(_, chars)| chars.len()).sum();
    if total == 0 {
        return None;
    }
    let scripts: Vec<String> = missing
        .iter()
        .map(|(script, chars)| {
            let mut shown: String = chars.iter().take(SHOWN).collect();
            if chars.len() > SHOWN {
                shown.push('…');
            }
            format!(
                "{} {shown} (in {})",
                script_label(*script),
                suggested_family(*script)
            )
        })
        .collect();
    Some(format!(
        "{total} character{} of the text {} no font and {} not printed: {}; install a family that has them and typeset again (\"typesep fonts\" lists the families there are)",
        if total == 1 { "" } else { "s" },
        if total == 1 { "has" } else { "have" },
        if total == 1 { "is" } else { "are" },
        scripts.join(", ")
    ))
}

pub const FONT_ROLES: [&str; 4] = ["main", "sans", "mono", "math"];

#[derive(Debug, Clone, PartialEq)]
pub struct FontProblem {
    pub path: String,
    pub family: String,
    pub near: Vec<String>,
}

pub fn font_problems(view: &View, fonts: &Fonts) -> Vec<FontProblem> {
    let families = fonts.families();
    let candidates: Vec<&str> = families.iter().map(String::as_str).collect();
    FONT_ROLES
        .iter()
        .filter_map(|role| {
            let spec = view.font(role);
            if spec.family.is_empty() || spec.file.is_some() || fonts.has_family(&spec.family) {
                return None;
            }
            Some(FontProblem {
                path: format!("fonts.{role}"),
                family: spec.family.clone(),
                near: nearest(&spec.family, &candidates, 3),
            })
        })
        .collect()
}

pub fn load_font_files(style: &mut typesep_style::Style, fonts: &mut Fonts) -> Result<(), String> {
    for role in FONT_ROLES {
        let path = format!("fonts.{role}");
        let file = match style.get(&format!("{path}.file")) {
            Some(value) if value.truthy() => value.js_string(),
            _ => continue,
        };
        let family = fonts.add_file(Path::new(&file))?;
        style
            .tree
            .set_path(&format!("{path}.family"), typesep_style::Value::str(family));
    }
    Ok(())
}

pub fn describe_missing_fonts(problems: &[FontProblem]) -> String {
    let mut lines: Vec<String> = problems
        .iter()
        .map(|p| {
            let near = if p.near.is_empty() {
                String::new()
            } else {
                format!("\n  did you mean: {}?", p.near.join(", "))
            };
            format!("font \"{}\" ({}) is not installed{near}", p.family, p.path)
        })
        .collect();
    lines.push(
        "  \"typesep fonts\" lists every family available; the README explains how to add your own"
            .to_owned(),
    );
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_fonts_form_the_default_book() {
        let fonts = Fonts::embedded();
        let families = fonts.families();
        assert_eq!(
            families,
            [
                "TeX Gyre Cursor",
                "TeX Gyre Heros",
                "TeX Gyre Termes",
                "TeX Gyre Termes Math"
            ]
        );
        assert!(fonts.has_family("tex gyre termes"));
        assert!(!fonts.has_family("Noto Serif"));
        assert_eq!(
            EMBEDDED.len(),
            13,
            "four faces each of Termes, Heros and Cursor, and Termes Math"
        );
        assert!(fonts.font(0).is_some(), "the first embedded face loads");
    }

    fn chars(text: &str) -> BTreeSet<char> {
        text.chars().collect()
    }

    fn families(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_fallback_family_is_needed_only_for_characters_the_main_fonts_lack() {
        let fonts = Fonts::embedded();
        let list = families(&["TeX Gyre Heros", "TeX Gyre Termes Math", "Noto Serif"]);
        let termes = ["TeX Gyre Termes"];
        let plain = coverage(&fonts, &chars("plain words, € and → too"), &termes, &list);
        assert_eq!(plain, Coverage::default(), "the main font has them all");
        let aleph = coverage(&fonts, &chars("the set ℵ and ⇒"), &termes, &list);
        assert_eq!(
            aleph.needed,
            ["TeX Gyre Termes Math"],
            "only the maths font has them"
        );
        assert!(aleph.missing.is_empty());
        let with_maths = coverage(
            &fonts,
            &chars("ℵ"),
            &["TeX Gyre Termes", "TeX Gyre Termes Math"],
            &list,
        );
        assert!(
            with_maths.needed.is_empty(),
            "the maths font is where Typst finds them already"
        );
        let first = coverage(
            &fonts,
            &chars("abc"),
            &[],
            &families(&["TeX Gyre Termes", "TeX Gyre Heros"]),
        );
        assert_eq!(
            first.needed,
            ["TeX Gyre Termes"],
            "a family whose characters one before it has is never reached"
        );
    }

    #[test]
    fn characters_no_font_has_are_listed_by_script_with_a_family_that_has_them() {
        let fonts = Fonts::embedded();
        let found = coverage(
            &fonts,
            &chars("a ʃ ℵ 中文 、 ж \u{200d}\u{ad}"),
            &["TeX Gyre Termes"],
            &families(&["TeX Gyre Termes Math"]),
        );
        assert_eq!(found.needed, ["TeX Gyre Termes Math"]);
        assert_eq!(
            found.missing,
            vec![
                (Script::Latin, vec!['ʃ']),
                (Script::Cyrillic, vec!['ж']),
                (Script::Han, vec!['、', '中', '文']),
            ],
            "spaces, joiners and soft hyphens need no font, a shared mark goes with its script"
        );
        assert_eq!(
            describe_missing_characters(&found.missing).unwrap(),
            "5 characters of the text have no font and are not printed: Latin ʃ (in Noto Serif), \
             Cyrillic ж (in Noto Serif), Han 、中文 (in Noto Serif CJK SC); install a family that \
             has them and typeset again (\"typesep fonts\" lists the families there are)"
        );
        let one = [(Script::Thai, vec!['ก'])];
        assert!(describe_missing_characters(&one).unwrap().starts_with(
            "1 character of the text has no font and is not printed: Thai ก (in Noto Serif Thai);"
        ),);
        let many = [(Script::Han, ('一'..='丿').collect::<Vec<char>>())];
        assert!(
            describe_missing_characters(&many)
                .unwrap()
                .contains("Han 一丁丂七丄丅丆万丈三上下… (in Noto Serif CJK SC)"),
            "a long list is cut short"
        );
        assert_eq!(describe_missing_characters(&[]), None);
        assert_eq!(suggested_family(Script::Hiragana), "Noto Serif CJK JP");
        assert_eq!(suggested_family(Script::Arabic), "Noto Naskh Arabic");
        assert_eq!(suggested_family(Script::Cherokee), "Noto Sans Cherokee");
    }

    #[test]
    fn missing_families_are_reported_with_suggestions() {
        let problems = vec![FontProblem {
            path: "fonts.main".into(),
            family: "TeX Gyre Termis".into(),
            near: vec!["TeX Gyre Termes".into()],
        }];
        assert_eq!(
            describe_missing_fonts(&problems),
            "font \"TeX Gyre Termis\" (fonts.main) is not installed\n  did you mean: TeX Gyre Termes?\n  \"typesep fonts\" lists every family available; the README explains how to add your own"
        );
    }
}
