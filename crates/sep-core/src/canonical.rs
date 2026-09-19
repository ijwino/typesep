use serde::Serialize;
use serde_json::Value;

use crate::model::EntryDoc;

pub fn canonical_json<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<String> {
    let value = serde_json::to_value(value)?;
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text)
}

pub fn model_json(doc: &EntryDoc) -> serde_json::Result<String> {
    let mut value = serde_json::to_value(doc)?;
    if let Some(meta) = value.get_mut("meta").and_then(Value::as_object_mut) {
        meta.remove("fetchedAt");
    }
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntryMeta;

    #[test]
    fn keys_are_sorted_and_the_timestamp_is_dropped() {
        let doc = EntryDoc {
            meta: EntryMeta {
                slug: "turing-machine".into(),
                fetched_at: "2026-09-03".into(),
                edition: "".into(),
                source_url: "https://plato.stanford.edu/entries/turing-machine/".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let text = model_json(&doc).unwrap();
        assert_eq!(
            text,
            r#"{
  "attachments": [],
  "body": [],
  "links": {
    "notes": [],
    "supplements": []
  },
  "meta": {
    "authorLine": "",
    "authors": [],
    "copyrightYear": "",
    "edition": "",
    "editionLabel": "",
    "firstPublished": "",
    "macros": {},
    "revision": "",
    "slug": "turing-machine",
    "sourceUrl": "https://plato.stanford.edu/entries/turing-machine/",
    "title": ""
  },
  "preamble": [],
  "sections": {},
  "toc": [],
  "warnings": []
}
"#
        );
        assert!(
            canonical_json(&doc)
                .unwrap()
                .contains("\"fetchedAt\": \"2026-09-03\""),
            "canonical_json keeps every field"
        );
        assert_eq!(
            canonical_json(&serde_json::json!({"b": 1, "a": [true, null]})).unwrap(),
            "{\n  \"a\": [\n    true,\n    null\n  ],\n  \"b\": 1\n}\n"
        );
    }
}
