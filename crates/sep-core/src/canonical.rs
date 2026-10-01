use serde::Serialize;
use serde_json::Value;

use crate::model::Doc;

pub fn canonical_json<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<String> {
    let value = serde_json::to_value(value)?;
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text)
}

pub fn model_json(doc: &Doc) -> serde_json::Result<String> {
    let mut value = serde_json::to_value(doc)?;
    if let Some(origin) = value
        .pointer_mut("/meta/origin")
        .and_then(Value::as_object_mut)
    {
        origin.remove("retrieved");
    }
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Meta, Origin};

    #[test]
    fn keys_are_sorted_and_the_timestamp_is_dropped() {
        let doc = Doc {
            meta: Meta {
                id: "turing-machine".into(),
                origin: Origin {
                    url: "https://plato.stanford.edu/entries/turing-machine/".into(),
                    retrieved: "2026-09-03".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let text = model_json(&doc).unwrap();
        assert_eq!(
            text,
            r#"{
  "attachments": [],
  "back": [],
  "body": [],
  "front": [],
  "links": {
    "notes": [],
    "supplements": []
  },
  "meta": {
    "byline": "",
    "contributors": [],
    "dateline": "",
    "id": "turing-machine",
    "lang": "",
    "macros": {},
    "origin": {
      "edition": "",
      "editionLabel": "",
      "publisher": "",
      "revision": "",
      "source": "",
      "url": "https://plato.stanford.edu/entries/turing-machine/"
    },
    "rights": {
      "notices": [],
      "statement": "",
      "terms": ""
    },
    "title": ""
  },
  "toc": [],
  "warnings": []
}
"#
        );
        assert!(
            canonical_json(&doc)
                .unwrap()
                .contains("\"retrieved\": \"2026-09-03\""),
            "canonical_json keeps every field"
        );
        assert_eq!(
            canonical_json(&serde_json::json!({"b": 1, "a": [true, null]})).unwrap(),
            "{\n  \"a\": [\n    true,\n    null\n  ],\n  \"b\": 1\n}\n"
        );
    }
}
