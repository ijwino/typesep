use std::path::{Path, PathBuf};

use sep_core::style::resolve::nearest;
use typst::foundations::Bytes;
use typst::text::{Font, FontBook, FontInfo};
use typst::utils::LazyHash;
use typst_kit::fonts::{FontPath, FontStore};

use crate::style::View;

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

pub fn load_font_files(
    style: &mut sep_core::style::Style,
    fonts: &mut Fonts,
) -> Result<(), String> {
    for role in FONT_ROLES {
        let path = format!("fonts.{role}");
        let file = match style.get(&format!("{path}.file")) {
            Some(value) if value.truthy() => value.js_string(),
            _ => continue,
        };
        let family = fonts.add_file(Path::new(&file))?;
        style.tree.set_path(
            &format!("{path}.family"),
            sep_core::style::Value::str(family),
        );
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
