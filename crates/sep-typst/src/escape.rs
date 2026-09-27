use std::fmt::Write;

pub fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut chars = text.chars().peekable();
    let mut in_space = false;
    while let Some(c) = chars.next() {
        if c.is_whitespace() && c != '\u{a0}' && c != '\u{202f}' {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
            continue;
        }
        in_space = false;
        match c {
            '\\' | '*' | '_' | '`' | '#' | '$' | '<' | '>' | '@' | '~' | '[' | ']' => {
                out.push('\\');
                out.push(c);
            }
            '/' if matches!(chars.peek(), Some('/') | Some('*')) => out.push_str("\\/"),
            '-' if chars.peek() == Some(&'?') => out.push_str("\\-"),
            '\u{200b}' | '\u{2060}' => {}
            _ => out.push(c),
        }
    }
    out
}

pub fn escape_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' | '*' | '_' | '`' | '#' | '$' | '<' | '>' | '@' | '~' | '[' | ']' | '\'' | '"'
            | '-' | '.' => {
                out.push('\\');
                out.push(c);
            }
            '/' if matches!(chars.peek(), Some('/') | Some('*')) => out.push_str("\\/"),
            '\n' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

pub fn escape_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn guard_line_start(text: &str) -> String {
    let start = text.len() - text.trim_start().len();
    let rest = &text[start..];
    let mut chars = rest.chars();
    let Some(first) = chars.next() else {
        return text.to_owned();
    };
    let guard = match first {
        '-' | '+' | '/' => chars.next().is_none_or(char::is_whitespace),
        '=' => rest
            .trim_start_matches('=')
            .chars()
            .next()
            .is_none_or(char::is_whitespace),
        c if c.is_ascii_digit() => {
            let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
            rest[digits..]
                .strip_prefix('.')
                .is_some_and(|after| after.chars().next().is_none_or(char::is_whitespace))
        }
        _ => false,
    };
    if guard {
        format!("{}\\{}", &text[..start], rest)
    } else {
        text.to_owned()
    }
}

pub fn safe_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn url_for_display(url: &str) -> String {
    let mut out = String::with_capacity(url.len() * 2);
    for c in url.chars() {
        out.push(c);
        if matches!(c, '/' | '.' | '-' | '_' | '?' | '&' | '=' | '#' | '%' | ':') {
            out.push('\u{200b}');
        }
    }
    escape_text_keep_zwsp(&out)
}

fn escape_text_keep_zwsp(text: &str) -> String {
    text.split('\u{200b}')
        .map(escape_text)
        .collect::<Vec<_>>()
        .join("\u{200b}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_specials_are_escaped_and_whitespace_collapsed() {
        assert_eq!(
            escape_text("a *b* _c_ #d $e$"),
            "a \\*b\\* \\_c\\_ \\#d \\$e\\$"
        );
        assert_eq!(
            escape_text("x<y> @z ~ [k] `q`"),
            "x\\<y\\> \\@z \\~ \\[k\\] \\`q\\`"
        );
        assert_eq!(
            escape_text("see https://x.test/a//b"),
            "see https:\\//x.test/a\\//b"
        );
        assert_eq!(escape_text("one\n\n  two\tthree"), "one two three");
        assert_eq!(escape_text("no\u{a0}break\u{200b}"), "no\u{a0}break");
        assert_eq!(
            escape_text("don't \"quote\" -- dash"),
            "don't \"quote\" -- dash"
        );
        assert_eq!(escape_text("C:\\dir"), "C:\\\\dir");
        assert_eq!(escape_text("soft -? no"), "soft \\-? no");
    }

    #[test]
    fn code_keeps_quotes_and_shorthands_literal_on_one_line() {
        assert_eq!(
            escape_code("it's \"x\" --y..."),
            "it\\'s \\\"x\\\" \\-\\-y\\.\\.\\."
        );
        assert_eq!(escape_code("a\nb"), "a b");
    }

    #[test]
    fn strings_and_labels() {
        assert_eq!(escape_string("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
        assert_eq!(safe_id("sec 1.2/α"), "sec-1.2--");
        assert_eq!(
            url_for_display("https://x.test/a"),
            "https:\u{200b}/\u{200b}/\u{200b}x.\u{200b}test/\u{200b}a"
        );
    }

    #[test]
    fn line_starts_that_are_markup_are_guarded() {
        assert_eq!(guard_line_start("- item"), "\\- item");
        assert_eq!(guard_line_start("  + item"), "  \\+ item");
        assert_eq!(guard_line_start("= heading"), "\\= heading");
        assert_eq!(guard_line_start("== heading"), "\\== heading");
        assert_eq!(guard_line_start("-\u{a0}item"), "\\-\u{a0}item");
        assert_eq!(guard_line_start("3.\u{a0}three"), "\\3.\u{a0}three");
        assert_eq!(guard_line_start("=> arrow"), "=> arrow");
        assert_eq!(guard_line_start("/ term"), "\\/ term");
        assert_eq!(guard_line_start("12. Twelve"), "\\12. Twelve");
        assert_eq!(guard_line_start("-5 degrees"), "-5 degrees");
        assert_eq!(guard_line_start("1.5 litres"), "1.5 litres");
        assert_eq!(guard_line_start("plain"), "plain");
        assert_eq!(guard_line_start(""), "");
    }
}
