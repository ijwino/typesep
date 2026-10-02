use std::collections::HashSet;
use std::path::Path;

use super::schema::{Knob, Schema, generic, knob, knobs};
use super::value::Value;
use super::yaml;
use typesep_html::js;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct StyleError(pub String);

pub const DEFAULT_PRESET: &str = "halfletter";
pub const PRESET_KEY: &str = "preset";

pub const PRESETS: [(&str, &str); 7] = [
    (
        "a4-reading",
        include_str!("../../../styles/a4-reading.yaml"),
    ),
    ("a5-book", include_str!("../../../styles/a5-book.yaml")),
    ("compact", include_str!("../../../styles/compact.yaml")),
    ("ereader", include_str!("../../../styles/ereader.yaml")),
    (
        "halfletter",
        include_str!("../../../styles/halfletter.yaml"),
    ),
    ("lwp", include_str!("../../../styles/lwp.yaml")),
    (
        "manuscript",
        include_str!("../../../styles/manuscript.yaml"),
    ),
];

pub fn list_presets() -> Vec<&'static str> {
    PRESETS.iter().map(|(name, _)| *name).collect()
}

pub fn preset_text(name: &str) -> Option<&'static str> {
    PRESETS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, text)| *text)
}

pub fn preset_catalog() -> Vec<(&'static str, String)> {
    PRESETS
        .iter()
        .map(|(name, text)| {
            let summary = text
                .split('\n')
                .find(|l| l.starts_with('#'))
                .map(|l| js::trim_start(&l[1..]).to_owned())
                .unwrap_or_default();
            (*name, summary)
        })
        .collect()
}

fn available() -> String {
    list_presets().join(", ")
}

pub fn load_preset(name: &str) -> Result<Value, StyleError> {
    if name.is_empty() || name == "none" {
        return Ok(Value::map());
    }
    let text = preset_text(name).ok_or_else(|| {
        StyleError(format!(
            "unknown preset \"{name}\"\n  available: {}",
            available()
        ))
    })?;
    parse_structured(text).map_err(|e| StyleError(format!("preset {name}: {e}")))
}

fn parse_structured(text: &str) -> Result<Value, yaml::YamlError> {
    Ok(match yaml::parse(text)? {
        Value::Null => Value::map(),
        other => other,
    })
}

/// Every option of `schema` at its default.
pub fn defaults(schema: &Schema) -> Value {
    let mut style = Value::map();
    for knob in schema.knobs() {
        style.set_path(&knob.path, knob.default.clone());
    }
    style
}

fn flatten(obj: &Value, prefix: &str, out: &mut Vec<(String, Value)>) {
    let Some(map) = obj.as_map() else { return };
    for (key, value) in map.iter() {
        let p = if prefix.is_empty() {
            key.to_owned()
        } else {
            format!("{prefix}.{key}")
        };
        if knob(&p).is_some() {
            out.push((p, value.clone()));
        } else if *value == Value::Null && is_group(&p) {
            continue;
        } else if !value.is_tree() {
            out.push((p, value.clone()));
        } else {
            flatten(value, &p, out);
        }
    }
}

fn is_group(p: &str) -> bool {
    let prefix = format!("{p}.");
    knobs().iter().any(|k| k.path.starts_with(&prefix))
}

fn distance(a: &[u16], b: &[u16]) -> usize {
    let mut dp: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = dp[0];
        dp[0] = i;
        for j in 1..=b.len() {
            let tmp = dp[j];
            let cost = usize::from(a[i - 1] != b[j - 1]);
            dp[j] = (dp[j] + 1).min(dp[j - 1] + 1).min(prev + cost);
            prev = tmp;
        }
    }
    dp[b.len()]
}

pub fn nearest(name: &str, candidates: &[&str], limit: usize) -> Vec<String> {
    let needle = js::trim(name).to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let needle_units: Vec<u16> = needle.encode_utf16().collect();
    let mut scored: Vec<(usize, &str)> = candidates
        .iter()
        .map(|c| {
            let hay = c.to_lowercase();
            let score = if hay.contains(&needle) || needle.contains(&hay) {
                1
            } else {
                distance(&needle_units, &hay.encode_utf16().collect::<Vec<_>>())
            };
            (score, *c)
        })
        .collect();
    scored.sort_by_key(|(score, _)| *score);
    let bound = f64::max(3.0, needle_units.len() as f64 / 2.0);
    scored
        .into_iter()
        .filter(|(score, _)| (*score as f64) <= bound)
        .take(limit)
        .map(|(_, c)| c.to_owned())
        .collect()
}

fn suggest(unknown_path: &str) -> Vec<String> {
    let paths: Vec<&str> = knobs().iter().map(|k| k.path.as_str()).collect();
    nearest(unknown_path, &paths, 3)
}

fn unknown_option(verb: &str, p: &str, source: Option<&str>) -> StyleError {
    let near = suggest(p);
    let mut message = format!("{verb} style option \"{p}\"");
    if let Some(source) = source {
        message.push_str(&format!(" ({source})"));
    }
    if !near.is_empty() {
        message.push_str(&format!("\n  did you mean: {}?", near.join(", ")));
    }
    StyleError(message)
}

fn apply_layer(
    schema: &Schema,
    style: &mut Value,
    layer: &Value,
    label: &dyn Fn(&str) -> String,
) -> Result<(), StyleError> {
    let mut leaves = Vec::new();
    flatten(layer, "", &mut leaves);
    let mut seen = HashSet::new();
    if let Some((p, _)) = leaves.iter().find(|(p, _)| !seen.insert(p.as_str())) {
        return Err(StyleError(format!(
            "style option \"{p}\" is set twice ({})",
            label(p)
        )));
    }
    for (p, raw) in leaves {
        let knob = schema
            .knob(&p)
            .ok_or_else(|| unknown_option("unknown", &p, Some(&label(&p))))?;
        let value = knob.kind.merge(style.get_path(&p), &raw);
        match knob.kind.coerce(&value) {
            Ok(typed) => style.set_path(&p, typed),
            Err(message) => {
                return Err(StyleError(format!(
                    "invalid value for \"{p}\" ({}): {message}\n  {}\n  default: {}",
                    label(&p),
                    knob.kind.describe(),
                    knob.default.to_json()
                )));
            }
        }
    }
    Ok(())
}

fn read_structured(file: &Path, shown: &str) -> Result<Value, StyleError> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| StyleError(format!("cannot read style file {shown}: {e}")))?;
    let tree = parse_structured(&text).map_err(|e| StyleError(format!("{shown}: {e}")))?;
    if !tree.is_tree() {
        return Err(StyleError(format!(
            "{shown}: a style file is a mapping of options, like \"page: {{size: a4}}\""
        )));
    }
    Ok(tree)
}

fn split_preset(tree: Value, file: &str) -> Result<(Option<String>, Value), StyleError> {
    let Value::Map(mut map) = tree else {
        return Ok((None, tree));
    };
    let Some(preset) = map.remove(PRESET_KEY) else {
        return Ok((None, Value::Map(map)));
    };
    match preset {
        Value::String(name) if !js::trim(&name).is_empty() => {
            Ok((Some(js::trim(&name).to_owned()), Value::Map(map)))
        }
        _ => Err(StyleError(format!(
            "{file}: \"{PRESET_KEY}\" must name a bundled preset, or be \"none\"\n  available: {}",
            available()
        ))),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    pub path: String,
    pub value: Value,
    pub source: String,
}

pub fn parse_set(assignment: &str) -> Result<Assignment, StyleError> {
    let Some((path, value)) = assignment.split_once('=') else {
        return Err(StyleError(format!(
            "--set expects path=value, got \"{assignment}\""
        )));
    };
    Ok(Assignment {
        path: js::trim(path).to_owned(),
        value: Value::str(value),
        source: "--set".to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum SetItem {
    Set(String),
    Assignment(Assignment),
}

impl From<&str> for SetItem {
    fn from(text: &str) -> Self {
        SetItem::Set(text.to_owned())
    }
}

impl From<String> for SetItem {
    fn from(text: String) -> Self {
        SetItem::Set(text)
    }
}

impl From<Assignment> for SetItem {
    fn from(a: Assignment) -> Self {
        SetItem::Assignment(a)
    }
}

#[derive(Debug, Default)]
pub struct ResolveOptions<'a> {
    pub preset: Option<&'a str>,
    pub file: Option<&'a Path>,
    pub sets: Vec<SetItem>,
    /// The table the style is read against, made for the sources in use; without one, the
    /// table for no source at all.
    pub schema: Option<&'a Schema>,
    /// The preset when neither `preset` nor the style file names one, as the source of the
    /// works typeset asks for; without one, `DEFAULT_PRESET`.
    pub default_preset: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub tree: Value,
}

impl Style {
    pub fn get(&self, path: &str) -> Option<&Value> {
        self.tree.get_path(path)
    }

    pub fn to_yaml(&self) -> String {
        yaml::stringify(&self.tree)
    }
}

pub fn resolve_style(opts: ResolveOptions<'_>) -> Result<Style, StyleError> {
    let schema = opts.schema.unwrap_or(generic());
    let mut style = defaults(schema);

    let mut from_file: Option<(Option<String>, Value, String)> = None;
    if let Some(file) = opts.file {
        let shown = file.display().to_string();
        if !file.exists() {
            return Err(StyleError(format!("style file not found: {shown}")));
        }
        let (preset, layer) = split_preset(read_structured(file, &shown)?, &shown)?;
        from_file = Some((preset, layer, shown));
    }

    let base = opts
        .preset
        .map(str::to_owned)
        .or_else(|| from_file.as_ref().and_then(|(p, _, _)| p.clone()))
        .unwrap_or_else(|| opts.default_preset.unwrap_or(DEFAULT_PRESET).to_owned());
    apply_layer(schema, &mut style, &load_preset(&base)?, &|_| {
        format!("preset {base}")
    })?;
    if let Some((_, layer, shown)) = &from_file {
        apply_layer(schema, &mut style, layer, &|_| shown.clone())?;
    }

    let mut origin: Vec<(String, String)> = Vec::new();
    for item in opts.sets {
        let Assignment {
            path,
            value,
            source,
        } = match item {
            SetItem::Set(text) => parse_set(&text)?,
            SetItem::Assignment(a) => a,
        };
        let mut layer = Value::map();
        layer.set_path(&path, value);
        match origin.iter_mut().find(|(p, _)| *p == path) {
            Some(entry) => entry.1 = source,
            None => origin.push((path, source)),
        }
        apply_layer(schema, &mut style, &layer, &|knob_path| {
            let mut seen = HashSet::new();
            let sources: Vec<&str> = origin
                .iter()
                .filter(|(p, _)| p == knob_path || p.starts_with(&format!("{knob_path}.")))
                .map(|(_, s)| s.as_str())
                .filter(|s| seen.insert(*s))
                .collect();
            if sources.is_empty() {
                "--set".to_owned()
            } else {
                sources.join(", ")
            }
        })?;
    }

    Ok(Style { tree: style })
}

#[derive(Debug, Clone, PartialEq)]
pub enum Explained<'a> {
    Knob {
        knob: &'a Knob,
        field: Option<String>,
    },
    Group {
        prefix: String,
        knobs: Vec<&'a Knob>,
    },
}

/// The option of `schema` that `dotted` names, the group of options under it, or the option
/// it is a field of.
pub fn explain_path<'a>(schema: &'a Schema, dotted: &str) -> Result<Explained<'a>, StyleError> {
    if let Some(exact) = schema.knob(dotted) {
        return Ok(Explained::Knob {
            knob: exact,
            field: None,
        });
    }
    let prefix = format!("{dotted}.");
    let under: Vec<&'a Knob> = schema
        .knobs()
        .iter()
        .filter(|k| k.path.starts_with(&prefix))
        .collect();
    if !under.is_empty() {
        return Ok(Explained::Group {
            prefix: dotted.to_owned(),
            knobs: under,
        });
    }
    let segments: Vec<&str> = dotted.split('.').collect();
    for i in (1..segments.len()).rev() {
        if let Some(k) = schema.knob(&segments[..i].join(".")) {
            return Ok(Explained::Knob {
                knob: k,
                field: Some(segments[i..].join(".")),
            });
        }
    }
    Err(unknown_option("no such", dotted, None))
}

pub fn explain(dotted: &str) -> Result<&'static Knob, StyleError> {
    match explain_path(generic(), dotted)? {
        Explained::Knob { knob, .. } => Ok(knob),
        Explained::Group { .. } => Err(unknown_option("no such", dotted, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shorthand::shorthand_assignments;
    use std::path::PathBuf;

    fn resolve(preset: Option<&str>, sets: &[&str]) -> Result<Style, StyleError> {
        resolve_style(ResolveOptions {
            preset,
            file: None,
            sets: sets.iter().map(|s| SetItem::from(*s)).collect(),
            schema: None,
            default_preset: None,
        })
    }

    #[test]
    fn a_sources_preset_is_the_base_only_when_nothing_names_another() {
        let base = |preset: Option<&str>, file: Option<&Path>| {
            resolve_style(ResolveOptions {
                preset,
                file,
                sets: Vec::new(),
                schema: None,
                default_preset: Some("ereader"),
            })
            .unwrap()
            .get("page.size")
            .cloned()
        };
        let ereader = Some(Value::str("90mmx120mm"));
        assert_eq!(base(None, None), ereader);
        assert_eq!(base(Some("a5-book"), None), Some(Value::str("a5")));
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s.yaml");
        std::fs::write(&file, "preset: halfletter\n").unwrap();
        assert_eq!(base(None, Some(&file)), Some(Value::str("5.5inx8.5in")));
        std::fs::write(&file, "typography: {fontSize: 9pt}\n").unwrap();
        assert_eq!(
            base(None, Some(&file)),
            ereader,
            "a file without a preset key"
        );
    }

    fn s(x: &str) -> Value {
        Value::str(x)
    }

    #[test]
    fn the_embedded_presets_are_the_files_of_styles() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../styles");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .expect("styles/ exists")
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter_map(|f| f.strip_suffix(".yaml").map(str::to_owned))
            .collect();
        on_disk.sort();
        assert_eq!(
            list_presets(),
            on_disk,
            "a preset was added or removed in styles/"
        );
        for (name, text) in PRESETS {
            let file = std::fs::read_to_string(dir.join(format!("{name}.yaml"))).unwrap();
            assert_eq!(text, file, "{name} is embedded from the wrong file");
        }
        let catalog = preset_catalog();
        assert_eq!(catalog[0].0, "a4-reading");
        assert_eq!(
            catalog[4],
            ("halfletter", "Half-letter (5.5x8.5in) single-column book page, Times-like serif, justified and hyphenated.".to_owned())
        );
    }

    #[test]
    fn defaults_cover_every_knob() {
        let style = defaults(generic());
        for knob in knobs() {
            assert!(
                style.get_path(&knob.path).is_some(),
                "{} has no default",
                knob.path
            );
        }
        let top: Vec<&str> = style.as_map().unwrap().iter().map(|(k, _)| k).collect();
        assert_eq!(
            top,
            [
                "page",
                "typography",
                "fonts",
                "headings",
                "blocks",
                "pageFurniture",
                "toc",
                "links",
                "notes",
                "cover",
                "output",
                "advanced"
            ]
        );
    }

    #[test]
    fn layers_merge_with_set_winning() {
        let style = resolve(
            Some("halfletter"),
            &["typography.leading=1.9", "page.margins.top=2in"],
        )
        .unwrap();
        assert_eq!(style.get("typography.leading"), Some(&Value::Number(1.9)));
        assert_eq!(style.get("page.margins.top"), Some(&s("2in")));
        assert_eq!(
            style.get("page.size"),
            Some(&s("5.5inx8.5in")),
            "untouched preset values survive"
        );
        assert!(
            style
                .to_yaml()
                .starts_with("page:\n  size: 5.5inx8.5in\n  margins:\n    top: 2in\n")
        );
        let bare = resolve(Some("none"), &[]).unwrap();
        assert_eq!(bare.get("page.size"), Some(&s("a5")));
        assert_eq!(
            resolve(Some(""), &[]).unwrap(),
            bare,
            "an empty preset name is the bare defaults"
        );
    }

    #[test]
    fn unknown_options_and_bad_values_are_reported_with_suggestions() {
        let err = resolve(Some("none"), &["typography.leadng=1.2"]).unwrap_err();
        assert_eq!(
            err.0,
            "unknown style option \"typography.leadng\" (--set)\n  did you mean: typography.leading, typography.language, typography.tolerance?"
        );
        let err = resolve(Some("none"), &["page.margins.top=banana"]).unwrap_err();
        assert_eq!(
            err.0,
            "invalid value for \"page.margins.top\" (--set): expected a length like \"0.6in\" or \"12pt\"\n  a length such as 0.6in, 12pt, 1.5em (units: pt, bp, mm, cm, in, pc, em, ex, sp), 0 or more\n  default: \"20mm\""
        );
        let err = resolve(Some("none"), &["fonts.main.scale=big"]).unwrap_err();
        assert_eq!(
            err.0,
            "invalid value for \"fonts.main\" (--set): expected a number\n  a family name, or {family, scale, features, file}\n  default: {\"family\":\"TeX Gyre Termes\",\"features\":{\"Ligatures\":\"TeX\"}}"
        );
        let err = resolve(Some("none"), &["page.size"]).unwrap_err();
        assert_eq!(err.0, "--set expects path=value, got \"page.size\"");
        let err = resolve(Some("nope"), &[]).unwrap_err();
        assert_eq!(
            err.0,
            "unknown preset \"nope\"\n  available: a4-reading, a5-book, compact, ereader, halfletter, lwp, manuscript"
        );
        let err = resolve_style(ResolveOptions {
            preset: Some("none"),
            file: None,
            sets: shorthand_assignments(&[("paper", s("letterish"))])
                .into_iter()
                .map(SetItem::from)
                .collect(),
            schema: None,
            default_preset: None,
        })
        .unwrap_err();
        assert!(
            err.0.contains("(--paper)") && !err.0.contains("--set"),
            "{err}"
        );
        let both = resolve_style(ResolveOptions {
            preset: Some("none"),
            file: None,
            sets: vec![
                SetItem::from(shorthand_assignments(&[("font", s("EB Garamond"))]).remove(0)),
                SetItem::from("fonts.main.scale=big"),
            ],
            schema: None,
            default_preset: None,
        })
        .unwrap_err();
        assert!(
            both.0.contains("(--font, --set)"),
            "every flag that touched the knob is named: {both}"
        );
    }

    #[test]
    fn every_bundled_preset_resolves() {
        for name in list_presets() {
            let style = resolve(Some(name), &[]).unwrap();
            assert!(style.get("page.size").is_some(), "{name}");
        }
        let ereader = resolve(Some("ereader"), &[]).unwrap();
        assert_eq!(ereader.get("page.size"), Some(&s("90mmx120mm")));
        let compact = resolve(Some("compact"), &[]).unwrap();
        assert_eq!(compact.get("headings.h1.size"), Some(&Value::Number(1.5)));
        assert_eq!(
            compact.get("headings.h1.style"),
            Some(&s("regular")),
            "the rest of the heading keeps its defaults"
        );
    }

    fn with_style_file<T>(text: &str, f: impl FnOnce(&Path) -> T) -> T {
        let dir = tempfile::tempdir().unwrap();
        let file: PathBuf = dir.path().join("style.yaml");
        std::fs::write(&file, text).unwrap();
        f(&file)
    }

    #[test]
    fn a_style_file_names_the_preset_it_builds_on_and_an_explicit_preset_overrides_it() {
        with_style_file("preset: a5-book\npage: {columns: 2}\n", |file| {
            let style = resolve_style(ResolveOptions {
                preset: None,
                file: Some(file),
                sets: vec![],
                schema: None,
                default_preset: None,
            })
            .unwrap();
            assert_eq!(style.get("page.twoside"), Some(&Value::Bool(true)));
            assert_eq!(style.get("page.columns"), Some(&Value::Number(2.0)));
            let explicit = resolve_style(ResolveOptions {
                preset: Some("none"),
                file: Some(file),
                sets: vec![],
                schema: None,
                default_preset: None,
            })
            .unwrap();
            assert_eq!(explicit.get("page.twoside"), Some(&Value::Bool(false)));
            assert_eq!(explicit.get("page.columns"), Some(&Value::Number(2.0)));
        });
        with_style_file("page: {columns: 2}\n", |file| {
            let style = resolve_style(ResolveOptions {
                preset: None,
                file: Some(file),
                sets: vec![],
                schema: None,
                default_preset: None,
            })
            .unwrap();
            assert_eq!(
                style.get("page.size"),
                Some(&s("5.5inx8.5in")),
                "without a preset key the default preset applies"
            );
        });
        with_style_file("preset: nope\n", |file| {
            let err = resolve_style(ResolveOptions {
                preset: None,
                file: Some(file),
                sets: vec![],
                schema: None,
                default_preset: None,
            })
            .unwrap_err();
            assert!(err.0.starts_with("unknown preset \"nope\""), "{err}");
        });
        with_style_file("preset: [1]\n", |file| {
            let err = resolve_style(ResolveOptions {
                preset: None,
                file: Some(file),
                sets: vec![],
                schema: None,
                default_preset: None,
            })
            .unwrap_err();
            assert!(
                err.0.contains("\"preset\" must name a bundled preset"),
                "{err}"
            );
        });
        with_style_file("{\"page\": {\"size\": \"a4\"}}", |file| {
            let style = resolve_style(ResolveOptions {
                preset: Some("none"),
                file: Some(file),
                sets: vec![],
                schema: None,
                default_preset: None,
            })
            .unwrap();
            assert_eq!(style.get("page.size"), Some(&s("a4")), "JSON is YAML");
        });
        with_style_file("# only a comment\n", |file| {
            let style = resolve_style(ResolveOptions {
                preset: Some("none"),
                file: Some(file),
                sets: vec![],
                schema: None,
                default_preset: None,
            })
            .unwrap();
            assert_eq!(
                style,
                resolve(Some("none"), &[]).unwrap(),
                "an empty file is an empty layer"
            );
        });
        with_style_file("typography:\n#  leading: 2\npage:\n  margins:\n", |file| {
            let style = resolve_style(ResolveOptions {
                preset: Some("none"),
                file: Some(file),
                sets: vec![],
                schema: None,
                default_preset: None,
            })
            .unwrap();
            assert_eq!(
                style,
                resolve(Some("none"), &[]).unwrap(),
                "a group with nothing under it is empty"
            );
        });
        with_style_file("page.size: a4\npage: {size: a3}\n", |file| {
            let err = resolve_style(ResolveOptions {
                preset: None,
                file: Some(file),
                sets: vec![],
                schema: None,
                default_preset: None,
            })
            .unwrap_err();
            assert!(
                err.0
                    .starts_with("style option \"page.size\" is set twice ("),
                "{err}"
            );
        });
        with_style_file("page: {size: a4, size: a5}\n", |file| {
            assert!(
                resolve_style(ResolveOptions {
                    preset: None,
                    file: Some(file),
                    sets: vec![],
                    schema: None,
                    default_preset: None,
                })
                .is_err()
            );
        });
        for text in ["- page.size: a4\n", "page.size=a4\n", "[]"] {
            with_style_file(text, |file| {
                let err = resolve_style(ResolveOptions {
                    preset: None,
                    file: Some(file),
                    sets: vec![],
                    schema: None,
                    default_preset: None,
                })
                .unwrap_err();
                assert!(
                    err.0.ends_with(
                        ": a style file is a mapping of options, like \"page: {size: a4}\""
                    ),
                    "{text:?}: {err}"
                );
            });
        }
        let err = resolve_style(ResolveOptions {
            preset: None,
            file: Some(Path::new("/nonexistent/style.yaml")),
            sets: vec![],
            schema: None,
            default_preset: None,
        })
        .unwrap_err();
        assert_eq!(err.0, "style file not found: /nonexistent/style.yaml");
    }

    #[test]
    fn a_dotted_font_field_edits_the_font_and_a_bare_name_replaces_it() {
        let edited = resolve(Some("halfletter"), &["fonts.main.scale=0.9"]).unwrap();
        assert_eq!(
            edited.get("fonts.main").unwrap().to_json(),
            r#"{"family":"TeX Gyre Termes","features":{"Ligatures":"TeX"},"scale":0.9}"#
        );
        let renamed = resolve(Some("halfletter"), &["fonts.main.family=EB Garamond"]).unwrap();
        assert_eq!(
            renamed.get("fonts.main").unwrap().to_json(),
            r#"{"family":"EB Garamond","features":{"Ligatures":"TeX"}}"#
        );
        let replaced = resolve(Some("halfletter"), &["fonts.main=EB Garamond"]).unwrap();
        assert_eq!(
            replaced.get("fonts.main").unwrap().to_json(),
            r#"{"family":"EB Garamond"}"#
        );
        let then_scaled = resolve(
            Some("halfletter"),
            &["fonts.main=EB Garamond", "fonts.main.scale=0.9"],
        )
        .unwrap();
        assert_eq!(
            then_scaled.get("fonts.main").unwrap().to_json(),
            r#"{"family":"EB Garamond","scale":0.9}"#,
            "assignments apply in order, so the field edits the named font"
        );
        let then_replaced = resolve(
            Some("halfletter"),
            &["fonts.main.scale=0.9", "fonts.main=EB Garamond"],
        )
        .unwrap();
        assert_eq!(
            then_replaced, replaced,
            "a later bare name replaces the font, scale and all"
        );
    }

    #[test]
    fn enumerations_accept_any_case_unless_case_tells_the_values_apart() {
        let style = resolve(
            Some("none"),
            &["notes.style=ENDNOTE", "pageFurniture.numbering.style=Roman"],
        )
        .unwrap();
        assert_eq!(style.get("notes.style"), Some(&s("endnote")));
        assert_eq!(
            style.get("pageFurniture.numbering.style"),
            Some(&s("Roman"))
        );
        assert!(resolve(Some("none"), &["pageFurniture.numbering.style=ROMAN"]).is_err());
    }

    #[test]
    fn explain_path_resolves_fields_and_groups_and_nearest_ranks_substrings_first() {
        match explain_path(generic(), "fonts.main.family").unwrap() {
            Explained::Knob { knob, field } => {
                assert_eq!(knob.path, "fonts.main");
                assert_eq!(field.as_deref(), Some("family"));
            }
            other => panic!("{other:?}"),
        }
        match explain_path(generic(), "page.margins").unwrap() {
            Explained::Group { prefix, knobs } => {
                assert_eq!(prefix, "page.margins");
                assert_eq!(knobs.len(), 4);
            }
            other => panic!("{other:?}"),
        }
        let err = explain_path(generic(), "page.sizzle").unwrap_err();
        assert_eq!(
            err.0,
            "no such style option \"page.sizzle\"\n  did you mean: page.size?"
        );
        assert_eq!(
            nearest("Garamond", &["TeX Gyre Termes", "EB Garamond"], 3),
            ["EB Garamond"]
        );
        assert_eq!(
            nearest("leading", &["typography.leading", "typography.language"], 3),
            ["typography.leading"]
        );
        assert_eq!(explain("page.size").unwrap().path, "page.size");
        assert!(explain("page.sizzle").is_err());
        assert!(explain("page.margins").is_err(), "a group is not one knob");
    }
}
