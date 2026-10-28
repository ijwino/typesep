use std::collections::BTreeMap;

use typesep_typst::fonts::Fonts;
use typesep_typst::mathml::{Formula, convert};

fn maths(formulas: &[(&str, bool)]) -> Vec<Result<String, String>> {
    let list: Vec<Formula> = formulas
        .iter()
        .map(|(tex, display)| Formula {
            tex: (*tex).to_owned(),
            display: *display,
        })
        .collect();
    convert(&list, &BTreeMap::new(), &Fonts::embedded()).items
}

fn one(tex: &str, display: bool) -> String {
    maths(&[(tex, display)]).remove(0).expect("converts")
}

#[test]
fn a_formula_becomes_mathml_in_its_own_namespace() {
    let math = one(r"x^2 + \frac{a}{b}", false);
    assert!(math.starts_with(r#"<math xmlns="http://www.w3.org/1998/Math/MathML">"#));
    assert!(math.contains("<msup>"));
    assert!(math.contains("<mfrac>"));
    assert!(math.ends_with("</math>"));
}

#[test]
fn a_display_formula_says_so() {
    let math = one(r"\sum_{i=1}^{n} \alpha_i", true);
    assert!(math.contains(r#" display="block""#));
    assert!(math.contains("<munderover>"));
}

#[test]
fn the_three_scope_overrides_keep_an_environment_from_coming_out_empty() {
    for tex in [
        r"\begin{align} a &= b \\ c &= d \end{align}",
        r"\begin{equation} a = b \\ c = d \end{equation}",
        r"\begin{gather} a \\ b \end{gather}",
        r"\begin{array}{lc} a & b \\ c & d \end{array}",
    ] {
        let math = one(tex, true);
        assert!(math.contains("<mtable"), "{tex} gave {math}");
        assert!(math.contains("<mtd"), "{tex} gave {math}");
    }
}

#[test]
fn display_style_moves_onto_an_mstyle_where_mathml_3_keeps_it() {
    let math = one(r"\text{a} \dfrac{1}{2}", false);
    assert!(math.contains("<mstyle displaystyle="), "{math}");
    assert!(
        !math.contains("<mfrac displaystyle=") && !math.contains("<mrow displaystyle="),
        "{math}"
    );
}

#[test]
fn a_bare_character_becomes_an_mtext() {
    let math = one(r"\mathrm{`}X \rightarrow Y\text{'}", false);
    assert!(math.contains("<mtext>\u{2019}</mtext>"), "{math}");
}

#[test]
fn a_formula_mitex_cannot_read_says_why_and_the_rest_still_convert() {
    let items = maths(&[
        ("a + b", false),
        (r"\begin{multline} x \end{multline}", true),
        ("c + d", false),
    ]);
    assert!(items[0].is_ok());
    assert!(items[1].is_err(), "{items:?}");
    assert!(items[2].as_ref().expect("converts").contains("<mi>"));
}

#[test]
fn a_formula_typst_cannot_evaluate_fails_alone() {
    let items = maths(&[
        ("a + b", false),
        (r"\sqrt[]{x}", false),
        ("c + d", false),
        (r"\frac{1}{2}", true),
    ]);
    assert!(items[1].is_err(), "{items:?}");
    for at in [0, 2, 3] {
        assert!(
            items[at].as_ref().is_ok_and(|m| m.starts_with("<math")),
            "{items:?}"
        );
    }
}

#[test]
fn the_user_agent_stylesheet_comes_with_the_maths() {
    let converted = convert(
        &[Formula {
            tex: r"\begin{align} a &= b \end{align}".to_owned(),
            display: true,
        }],
        &BTreeMap::new(),
        &Fonts::embedded(),
    );
    assert!(converted.css.contains("mtable"));
    assert!(!converted.css.contains(" i]"), "{}", converted.css);
}
