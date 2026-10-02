//! The inline `style` of an element, read as the declarations a browser would apply.

use scraper::ElementRef;

/// The declarations of a `style` attribute, in order, each property in lower case and each
/// value trimmed. A declaration without a colon is left out, as a browser leaves it out.
pub fn declarations(style: &str) -> Vec<(String, String)> {
    style
        .split(';')
        .filter_map(|part| {
            let (property, value) = part.split_once(':')?;
            let property = property.trim().to_ascii_lowercase();
            let value = value.trim();
            (!property.is_empty() && !value.is_empty()).then(|| (property, value.to_owned()))
        })
        .collect()
}

/// The declarations of `el`'s `style` attribute.
pub fn style_of(el: &ElementRef) -> Vec<(String, String)> {
    el.attr("style").map(declarations).unwrap_or_default()
}

/// The value `el`'s style gives `property` last, as the one that wins.
pub fn get(el: &ElementRef, property: &str) -> Option<String> {
    style_of(el)
        .into_iter()
        .rev()
        .find(|(name, _)| name == property)
        .map(|(_, value)| value)
}

/// A length in ems, as `2em` or `1.5em`: its number. Other units are not ems.
pub fn ems(value: &str) -> Option<f64> {
    let number = value.trim().to_ascii_lowercase();
    let number = number.strip_suffix("em")?;
    number.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Whether a border declaration's value draws a line: not `none` or `hidden`, and not of no
/// width.
pub fn draws_border(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    !value
        .split_whitespace()
        .any(|word| matches!(word, "none" | "hidden" | "0" | "0px" | "0pt" | "0em"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_style_is_read_as_a_browser_reads_it() {
        assert_eq!(
            declarations(" Text-Align: right ;margin-left:3em;; broken ; color: "),
            [
                ("text-align".to_owned(), "right".to_owned()),
                ("margin-left".to_owned(), "3em".to_owned())
            ]
        );
        assert_eq!(
            declarations("text-decoration: overline; aria-label="),
            [("text-decoration".to_owned(), "overline".to_owned())],
            "a stray attribute in the style is no declaration"
        );
        assert_eq!(ems("2em"), Some(2.0));
        assert_eq!(ems(" 1.5EM"), Some(1.5));
        assert_eq!(ems("-3em"), Some(-3.0));
        assert_eq!(ems("20px"), None);
        assert!(draws_border("1px solid black"));
        assert!(!draws_border("none"));
        assert!(!draws_border("0 solid black"));
    }
}
