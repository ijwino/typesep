use typesep_html::js::{DOT, WS};
use typesep_html::lazy_regex;
use typesep_model::{MacroDef, MacroTable};

lazy_regex!(MACROS_BLOCK = format!(r"(?-u:\b)(?i-u:macros){WS}*:{WS}*\{{"));

lazy_regex!(
    ENTRY = format!(
        r#"([A-Za-z][A-Za-z0-9]*){WS}*:{WS}*(?:'((?:[^\\']|\\{DOT})*)'|"((?:[^\\"]|\\{DOT})*)"|\[{WS}*(?:'((?:[^\\']|\\{DOT})*)'|"((?:[^\\"]|\\{DOT})*)"){WS}*(?:,{WS}*([0-9]+){WS}*)?\])"#
    )
);

fn unescape_js(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
            && !matches!(next, '\n' | '\r' | '\u{2028}' | '\u{2029}')
        {
            chars.next();
            out.push(match next {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                other => other,
            });
        } else {
            out.push(c);
        }
    }
    out
}

pub fn extract_macros(js: &str) -> MacroTable {
    let Some(start) = MACROS_BLOCK.find(js) else {
        return MacroTable::new();
    };
    let from = start.end() - 1;
    let mut depth = 0usize;
    let mut end = None;
    let mut quote = None;
    let mut escaped = false;
    for (i, byte) in js.bytes().enumerate().skip(from) {
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == open {
                quote = None;
            }
            continue;
        }
        match byte {
            b'"' | b'\'' | b'`' => quote = Some(byte),
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(end) = end else {
        return MacroTable::new();
    };

    let mut macros = MacroTable::new();
    for m in ENTRY.captures_iter(&js[from + 1..end]) {
        let Some(raw) = (2..=5).find_map(|group| m.get(group)) else {
            continue;
        };
        let args = m
            .get(6)
            .map_or(0, |count| count.as_str().parse().unwrap_or(u32::MAX));
        macros.insert(
            m[1].to_owned(),
            MacroDef {
                body: unescape_js(raw.as_str()),
                args,
            },
        );
    }
    macros
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(body: &str, args: u32) -> MacroDef {
        MacroDef {
            body: body.to_owned(),
            args,
        }
    }

    #[test]
    fn macro_tables_come_out_of_local_js() {
        let v2 = r#"
          MathJax.Hub.Config({
            TeX: { Macros: {
              opL: "\\mathbin{L}",
              vect: ["\\mathbf{#1}", 1],
              quoted: '\'a\' \"b\"\\n{x}',
              nested: "{ { } }"
            } }
          });"#;
        assert_eq!(
            extract_macros(v2),
            MacroTable::from([
                ("opL".to_owned(), def("\\mathbin{L}", 0)),
                ("vect".to_owned(), def("\\mathbf{#1}", 1)),
                ("quoted".to_owned(), def("'a' \"b\"\\n{x}", 0)),
                ("nested".to_owned(), def("{ { } }", 0)),
            ])
        );
        let v3 =
            "window.MathJax = { tex: { macros: { RR: '{\\\\bf R}', bold: ['{\\\\bf #1}', 1] } } };";
        assert_eq!(
            extract_macros(v3),
            MacroTable::from([
                ("RR".to_owned(), def("{\\bf R}", 0)),
                ("bold".to_owned(), def("{\\bf #1}", 1)),
            ])
        );
        let one_element = "Macros: {\n  amp: [\"\\\\mathbin{\\\\&}\"],\n  cb: [\"{\\\\color{DarkMagenta}{#1}}\", 1],\n  q: \"it's \\\"quoted\\\"\"\n}";
        assert_eq!(
            extract_macros(one_element),
            MacroTable::from([
                ("amp".to_owned(), def("\\mathbin{\\&}", 0)),
                ("cb".to_owned(), def("{\\color{DarkMagenta}{#1}}", 1)),
                ("q".to_owned(), def("it's \"quoted\"", 0)),
            ]),
            "a one-element array is a macro of no arguments and does not swallow the next entry"
        );
        let unbalanced = r#"Macros: { rb: "\\right\\}", lb: '\\left\\{', opL: "\\mathbin{L}" }"#;
        assert_eq!(
            extract_macros(unbalanced),
            MacroTable::from([
                ("rb".to_owned(), def("\\right\\}", 0)),
                ("lb".to_owned(), def("\\left\\{", 0)),
                ("opL".to_owned(), def("\\mathbin{L}", 0)),
            ]),
            "a brace inside a string does not open or close the block"
        );
        assert!(extract_macros("var x = 1;").is_empty(), "no macros block");
        assert!(
            extract_macros("macros: { a: 'b'").is_empty(),
            "an unclosed block yields nothing"
        );
        assert!(
            extract_macros("submacros: { a: 'b' }").is_empty(),
            "the word boundary holds"
        );
        assert_eq!(
            unescape_js("a\\\nb"),
            "a\\\nb",
            "a backslash before a newline stays"
        );
        assert_eq!(unescape_js("\\\\ \\t"), "\\ \t");
    }
}
