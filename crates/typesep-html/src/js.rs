pub fn is_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

pub const WS: &str = r"[\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";
pub const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";
pub const WORD: &str = "[A-Za-z0-9_]";

pub fn trim(text: &str) -> &str {
    text.trim_matches(is_space)
}

pub fn trim_start(text: &str) -> &str {
    text.trim_start_matches(is_space)
}

pub fn trim_end(text: &str) -> &str {
    text.trim_end_matches(is_space)
}

pub fn is_blank(text: &str) -> bool {
    text.chars().all(is_space)
}

pub fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if is_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

pub fn number(text: &str) -> Option<f64> {
    let text = trim(text);
    if text.is_empty() {
        return Some(0.0);
    }
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if text.len() >= 2 && text[..2].eq_ignore_ascii_case(prefix) {
            let digits = &text[2..];
            if digits.is_empty() {
                return None;
            }
            return digits.chars().try_fold(0f64, |acc, c| {
                c.to_digit(radix)
                    .map(|d| acc * f64::from(radix) + f64::from(d))
            });
        }
    }
    match text {
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    if !text
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'))
    {
        return None;
    }
    text.parse::<f64>().ok()
}

pub fn is_integer(n: f64) -> bool {
    n.is_finite() && n.fract() == 0.0
}

pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_owned();
    }
    if n == 0.0 {
        return "0".to_owned();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    let sci = format!("{:e}", n.abs());
    let (mantissa, exponent) = sci.split_once('e').expect("LowerExp has an exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i64;
    let e: i64 = exponent.parse().expect("LowerExp exponent");
    let point = e + 1;
    let sign = if n < 0.0 { "-" } else { "" };
    let body = if k <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - k) as usize))
    } else if 0 < point && point <= 21 {
        let (int, frac) = digits.split_at(point as usize);
        format!("{int}.{frac}")
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let exp = point - 1;
        let exp_sign = if exp < 0 { "-" } else { "+" };
        if k == 1 {
            format!("{digits}e{exp_sign}{}", exp.abs())
        } else {
            let (first, rest) = digits.split_at(1);
            format!("{first}.{rest}e{exp_sign}{}", exp.abs())
        }
    };
    format!("{sign}{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_is_javascripts_set() {
        assert_eq!(trim("\u{FEFF} a \u{A0}\n"), "a");
        assert_eq!(
            trim("\u{85}a\u{85}"),
            "\u{85}a\u{85}",
            "NEL is not whitespace in JavaScript"
        );
        assert!(is_blank(" \t\u{3000}"));
        assert!(!is_blank(" x "));
        assert_eq!(collapse_spaces("a \n\t b\u{A0}c"), "a b c");
        assert_eq!(trim_start("  a  "), "a  ");
        assert_eq!(trim_end("  a  "), "  a");
        crate::lazy_regex!(WS_RUN = format!("^{WS}+$"));
        assert!(WS_RUN.is_match(" \u{FEFF}"));
        assert!(!WS_RUN.is_match("\u{85}"));
    }

    #[test]
    fn number_reads_as_javascript_does() {
        assert_eq!(number("3"), Some(3.0));
        assert_eq!(number(" 3 "), Some(3.0));
        assert_eq!(number(""), Some(0.0));
        assert_eq!(number("3.0"), Some(3.0));
        assert_eq!(number("1e2"), Some(100.0));
        assert_eq!(number("0x10"), Some(16.0));
        assert_eq!(number("0b101"), Some(5.0));
        assert_eq!(number("-2"), Some(-2.0));
        assert_eq!(number("+.5"), Some(0.5));
        assert_eq!(number("Infinity"), Some(f64::INFINITY));
        assert_eq!(number("inf"), None);
        assert_eq!(number("nan"), None);
        assert_eq!(number("0x"), None);
        assert_eq!(number("abc"), None);
        assert_eq!(number("1 2"), None);
        assert!(is_integer(3.0));
        assert!(!is_integer(2.5));
        assert!(!is_integer(f64::INFINITY));
    }

    #[test]
    fn numbers_print_as_javascript_prints_them() {
        for (n, text) in [
            (1.0, "1"),
            (0.0, "0"),
            (-0.0, "0"),
            (1.15, "1.15"),
            (0.95, "0.95"),
            (10000.0, "10000"),
            (-2.5, "-2.5"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1.5e21, "1.5e+21"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (1.2345e-7, "1.2345e-7"),
            (12345678901234567890.0, "12345678901234567000"),
            (0.1 + 0.2, "0.30000000000000004"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (f64::NAN, "NaN"),
        ] {
            assert_eq!(number_to_string(n), text, "{n:?}");
        }
    }
}
