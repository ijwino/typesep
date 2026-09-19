use crate::parse::js::{self, lazy_regex};

fn to_pt_factor(unit: &str) -> Option<f64> {
    Some(match unit {
        "pt" => 1.0,
        "bp" => 72.27 / 72.0,
        "mm" => 72.27 / 25.4,
        "cm" => 72.27 / 2.54,
        "in" => 72.27,
        "pc" => 12.0,
        "sp" => 1.0 / 65536.0,
        _ => return None,
    })
}

pub const LENGTH_UNITS: [&str; 9] = ["pt", "bp", "mm", "cm", "in", "pc", "em", "ex", "sp"];

lazy_regex!(
    LENGTH_RE = format!(
        r"^(-?[0-9]*\.?[0-9]+){}*(pt|bp|mm|cm|in|pc|em|ex|sp)$",
        js::WS
    )
);

pub fn match_length(text: &str) -> Option<(&str, &str)> {
    let m = LENGTH_RE.captures(text)?;
    Some((
        m.get(1).expect("number group").as_str(),
        m.get(2).expect("unit group").as_str(),
    ))
}

pub const PAPER_SIZES: [(&str, (&str, &str)); 9] = [
    ("a3", ("297mm", "420mm")),
    ("a4", ("210mm", "297mm")),
    ("a5", ("148mm", "210mm")),
    ("a6", ("105mm", "148mm")),
    ("b5", ("176mm", "250mm")),
    ("letter", ("8.5in", "11in")),
    ("legal", ("8.5in", "14in")),
    ("halfletter", ("5.5in", "8.5in")),
    ("digest", ("5.5in", "8.5in")),
];

pub fn paper_size(name: &str) -> Option<(&'static str, &'static str)> {
    PAPER_SIZES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, dims)| *dims)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FactorOrLength<'a> {
    Factor(f64),
    Length(&'a str),
}

pub fn to_pt(value: FactorOrLength<'_>, base: f64) -> Result<f64, String> {
    let text = match value {
        FactorOrLength::Factor(f) => return Ok(f * base),
        FactorOrLength::Length(text) => text,
    };
    let (n, unit) = match_length(js::trim(text)).ok_or_else(|| format!("not a length: {text}"))?;
    let n: f64 = js::number(n).expect("the regex admits only a decimal number");
    Ok(match unit {
        "em" => n * base,
        "ex" => n * base * 0.45,
        other => n * to_pt_factor(other).expect("the regex admits only known units"),
    })
}

pub fn fmt_pt(n: f64) -> String {
    let fixed: f64 = format!("{n:.3}")
        .parse()
        .expect("a fixed decimal reads back");
    js::number_to_string(fixed)
}

pub fn paper_dimensions(size: &str) -> (String, String) {
    if let Some((w, h)) = paper_size(size) {
        return (w.to_owned(), h.to_owned());
    }
    let mut parts = size.splitn(2, 'x');
    let w = parts.next().unwrap_or("").to_owned();
    let h = parts.next().unwrap_or("").to_owned();
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_convert_to_points_against_a_body_size() {
        let len = FactorOrLength::Length;
        assert_eq!(to_pt(len("72.27pt"), 10.0).unwrap(), 72.27);
        assert_eq!(to_pt(len("1in"), 10.0).unwrap(), 72.27);
        assert_eq!(to_pt(len("2em"), 11.0).unwrap(), 22.0);
        assert_eq!(to_pt(FactorOrLength::Factor(1.5), 10.0).unwrap(), 15.0);
        assert_eq!(to_pt(len("1ex"), 10.0).unwrap(), 4.5);
        assert_eq!(
            to_pt(len("3furlongs"), 10.0).unwrap_err(),
            "not a length: 3furlongs"
        );
        assert_eq!(match_length("12 pt"), Some(("12", "pt")));
        assert_eq!(match_length("-.5em"), Some(("-.5", "em")));
        assert_eq!(match_length("12PT"), None, "units are lower case");
        assert_eq!(match_length("1.in"), None);
        assert_eq!(
            paper_dimensions("a5"),
            ("148mm".to_owned(), "210mm".to_owned())
        );
        assert_eq!(
            paper_dimensions("5.5inx8.5in"),
            ("5.5in".to_owned(), "8.5in".to_owned())
        );
        assert_eq!(fmt_pt(20.0), "20");
        assert_eq!(fmt_pt(14.4540001), "14.454");
        assert_eq!(fmt_pt(1.0 / 3.0), "0.333");
    }
}
