use typesep_html::js;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    List(Vec<Value>),
    Map(Map),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map(Vec<(String, Value)>);

impl Map {
    pub fn new() -> Self {
        Map(Vec::new())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        match self.get_mut(&key) {
            Some(slot) => *slot = value,
            None => self.0.push((key, value)),
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let at = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(at).1)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn extend_from(&mut self, other: &Map) {
        for (k, v) in other.iter() {
            self.insert(k, v.clone());
        }
    }
}

impl FromIterator<(String, Value)> for Map {
    fn from_iter<I: IntoIterator<Item = (String, Value)>>(iter: I) -> Self {
        let mut map = Map::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

impl Value {
    pub fn map() -> Value {
        Value::Map(Map::new())
    }

    pub fn str(text: impl Into<String>) -> Value {
        Value::String(text.into())
    }

    pub fn as_map(&self) -> Option<&Map> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_map_mut(&mut self) -> Option<&mut Map> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(items) => Some(items),
            _ => None,
        }
    }

    pub fn is_tree(&self) -> bool {
        matches!(self, Value::Map(_))
    }

    pub fn get_path(&self, dotted: &str) -> Option<&Value> {
        dotted
            .split('.')
            .try_fold(self, |node, key| node.as_map()?.get(key))
    }

    pub fn set_path(&mut self, dotted: &str, value: Value) {
        let mut keys: Vec<&str> = dotted.split('.').collect();
        let last = keys.pop().expect("split yields at least one segment");
        let mut cur = self;
        for key in keys {
            let map = match cur {
                Value::Map(m) => m,
                _ => unreachable!("set_path only walks into trees"),
            };
            if !map.get(key).is_some_and(Value::is_tree) {
                map.insert(key, Value::map());
            }
            cur = map.get_mut(key).expect("just inserted");
        }
        match cur {
            Value::Map(m) => m.insert(last, value),
            _ => unreachable!("set_path only walks into trees"),
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => !(*n == 0.0 || n.is_nan()),
            Value::String(s) => !s.is_empty(),
            Value::List(_) | Value::Map(_) => true,
        }
    }

    pub fn js_string(&self) -> String {
        match self {
            Value::Null => "null".to_owned(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => js::number_to_string(*n),
            Value::String(s) => s.clone(),
            Value::List(items) => items
                .iter()
                .map(|item| match item {
                    Value::Null => String::new(),
                    other => other.js_string(),
                })
                .collect::<Vec<_>>()
                .join(","),
            Value::Map(_) => "[object Object]".to_owned(),
        }
    }

    pub fn to_json(&self) -> String {
        match self {
            Value::Null => "null".to_owned(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) if n.is_finite() => js::number_to_string(*n),
            Value::Number(_) => "null".to_owned(),
            Value::String(s) => json_string(s),
            Value::List(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(Value::to_json)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Value::Map(map) => format!(
                "{{{}}}",
                map.iter()
                    .map(|(k, v)| format!("{}:{}", json_string(k), v.to_json()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
}

pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Value {
        let mut v = Value::map();
        v.set_path("page.margins.top", Value::str("20mm"));
        v.set_path("page.size", Value::str("a5"));
        v.set_path("page.margins.bottom", Value::str("22mm"));
        v
    }

    #[test]
    fn paths_read_and_write_in_insertion_order() {
        let mut v = tree();
        assert_eq!(v.get_path("page.margins.top"), Some(&Value::str("20mm")));
        assert_eq!(v.get_path("page.nope"), None);
        assert_eq!(v.get_path("page.size.x"), None, "a scalar is not a tree");
        let page = v.get_path("page").unwrap().as_map().unwrap();
        let keys: Vec<&str> = page.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["margins", "size"]);
        v.set_path("page.size.width", Value::Number(1.0));
        assert!(
            v.get_path("page.size").unwrap().is_tree(),
            "a scalar on the way is replaced by a tree"
        );
        let mut m = Map::new();
        m.insert("b", Value::Number(1.0));
        m.insert("a", Value::Number(2.0));
        m.insert("b", Value::Number(3.0));
        assert_eq!(
            m.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            ["b", "a"],
            "assigning an existing key keeps its place"
        );
        m.remove("b");
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn javascript_conversions() {
        assert_eq!(Value::Number(1.5).js_string(), "1.5");
        assert_eq!(Value::Number(3.0).js_string(), "3");
        assert_eq!(Value::Bool(true).js_string(), "true");
        assert_eq!(Value::Null.js_string(), "null");
        assert_eq!(
            Value::List(vec![Value::str("a"), Value::Null, Value::Number(2.0)]).js_string(),
            "a,,2"
        );
        assert_eq!(Value::map().js_string(), "[object Object]");
        assert!(!Value::str("").truthy());
        assert!(!Value::Number(0.0).truthy());
        assert!(Value::List(vec![]).truthy());
        assert!(Value::map().truthy());

        let mut font = Map::new();
        font.insert("family", Value::str("TeX Gyre Termes"));
        let mut features = Map::new();
        features.insert("Ligatures", Value::str("TeX"));
        font.insert("features", Value::Map(features));
        assert_eq!(
            Value::Map(font).to_json(),
            r#"{"family":"TeX Gyre Termes","features":{"Ligatures":"TeX"}}"#
        );
        assert_eq!(
            Value::List(vec![Value::str("\\textbullet"), Value::str("a\"b\n")]).to_json(),
            r#"["\\textbullet","a\"b\n"]"#
        );
        assert_eq!(json_string("\u{1}é\u{7f}"), "\"\\u0001é\u{7f}\"");
        assert_eq!(Value::Number(f64::NAN).to_json(), "null");
    }
}
