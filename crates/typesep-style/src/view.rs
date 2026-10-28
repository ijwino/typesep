use crate::units::{FactorOrLength, paper_dimensions, to_pt};
use crate::{Style, Value};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FontSpec {
    pub family: String,
    pub scale: Option<f64>,
    pub features: Vec<(String, String)>,
    pub file: Option<String>,
}

pub struct View<'a> {
    style: &'a Style,
    body: f64,
}

impl<'a> View<'a> {
    pub fn new(style: &'a Style) -> Self {
        let body = style
            .get("typography.fontSize")
            .and_then(Value::as_str)
            .and_then(|s| to_pt(FactorOrLength::Length(s), 10.0).ok())
            .unwrap_or(10.0);
        View { style, body }
    }

    pub fn style(&self) -> &'a Style {
        self.style
    }

    pub fn body(&self) -> f64 {
        self.body
    }

    pub fn value(&self, path: &str) -> Option<&'a Value> {
        self.style.get(path)
    }

    pub fn str(&self, path: &str) -> String {
        match self.value(path) {
            None | Some(Value::Null) => String::new(),
            Some(v) => v.js_string(),
        }
    }

    pub fn bool(&self, path: &str) -> bool {
        self.value(path).is_some_and(Value::truthy)
    }

    pub fn num(&self, path: &str) -> f64 {
        match self.value(path) {
            Some(Value::Number(n)) => *n,
            Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    pub fn int(&self, path: &str) -> i64 {
        self.num(path) as i64
    }

    pub fn list(&self, path: &str) -> Vec<String> {
        match self.value(path) {
            Some(Value::List(items)) => items.iter().map(Value::js_string).collect(),
            Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        }
    }

    pub fn pt(&self, path: &str) -> f64 {
        self.pt_against(path, self.body)
    }

    pub fn pt_against(&self, path: &str, base: f64) -> f64 {
        match self.value(path) {
            Some(Value::Number(n)) => n * base,
            Some(Value::String(s)) => to_pt(FactorOrLength::Length(s), base).unwrap_or(0.0),
            _ => 0.0,
        }
    }

    pub fn size_pt(&self, path: &str) -> f64 {
        self.pt_against(path, self.body)
    }

    pub fn font(&self, role: &str) -> FontSpec {
        let path = format!("fonts.{role}");
        match self.value(&path) {
            Some(Value::Map(map)) => FontSpec {
                family: map.get("family").map(Value::js_string).unwrap_or_default(),
                scale: map.get("scale").and_then(Value::as_f64),
                features: match map.get("features") {
                    Some(Value::Map(features)) => features
                        .iter()
                        .map(|(k, v)| (k.to_owned(), v.js_string()))
                        .collect(),
                    _ => Vec::new(),
                },
                file: map.get("file").filter(|v| v.truthy()).map(Value::js_string),
            },
            Some(Value::String(family)) => FontSpec {
                family: family.clone(),
                ..FontSpec::default()
            },
            _ => FontSpec::default(),
        }
    }

    pub fn paper_pt(&self) -> (f64, f64) {
        let (w, h) = paper_dimensions(&self.str("page.size"));
        let pt = |s: &str| to_pt(FactorOrLength::Length(s), self.body).unwrap_or(0.0);
        (pt(&w), pt(&h))
    }

    pub fn text_width_pt(&self) -> f64 {
        let (paper, _) = self.paper_pt();
        let width = paper
            - self.pt("page.margins.inner")
            - self.pt("page.margins.outer")
            - self.pt("page.bindingOffset");
        let columns = self.int("page.columns").max(1) as f64;
        (width - (columns - 1.0) * self.pt("page.columnSep")) / columns
    }

    pub fn text_height_pt(&self) -> f64 {
        let (_, paper) = self.paper_pt();
        paper - self.pt("page.margins.top") - self.pt("page.margins.bottom")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResolveOptions, resolve_style};

    fn halfletter() -> Style {
        resolve_style(ResolveOptions {
            preset: Some("halfletter"),
            file: None,
            sets: vec![],
            schema: None,
            default_preset: None,
        })
        .unwrap()
    }

    #[test]
    fn the_view_reads_lengths_fonts_and_paper() {
        let style = halfletter();
        let v = View::new(&style);
        assert_eq!(v.body(), 10.0);
        assert_eq!(v.str("page.size"), "5.5inx8.5in");
        assert_eq!(v.pt("typography.parIndent"), 12.0);
        assert_eq!(v.size_pt("typography.leading"), 15.0);
        assert_eq!(v.size_pt("headings.h1.size"), 18.0);
        assert!(v.bool("typography.justify"));
        assert_eq!(v.int("toc.depth"), 3);
        assert_eq!(v.font("main").family, "TeX Gyre Termes");
        assert_eq!(v.font("sans").scale, Some(0.95));
        assert_eq!(v.list("cover.fields").len(), 5);
        let (w, h) = v.paper_pt();
        assert!((w - 5.5 * 72.27).abs() < 1e-9 && (h - 8.5 * 72.27).abs() < 1e-9);
        assert!((v.text_width_pt() - (5.5 - 1.08) * 72.27).abs() < 1e-6);
    }
}
