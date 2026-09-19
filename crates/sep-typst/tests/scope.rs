use sep_typst::fonts::Fonts;
use sep_typst::world::{MITEX_FILES, SepWorld};

fn names() -> Vec<String> {
    let text = MITEX_FILES
        .iter()
        .find(|(path, _)| path.ends_with("standard.typ"))
        .expect("the standard package")
        .1;
    let re = regex::Regex::new(r#"define-sym\(\s*"([^"]+)""#).expect("regex");
    let word = regex::Regex::new(r"^[a-zA-Z][a-zA-Z0-9]*(\.[a-zA-Z0-9]+)*$").expect("regex");
    let mut out: Vec<String> = re
        .captures_iter(text)
        .map(|c| c[1].to_owned())
        .filter(|n| word.is_match(n))
        .collect();
    out.sort();
    out.dedup();
    out
}

#[test]
fn every_alias_in_the_mitex_scope_resolves() {
    let all = names();
    let fonts = Fonts::embedded();
    let mut unknown: Vec<String> = Vec::new();
    for name in &all {
        let name = sep_typst::math::rename_symbols(name);
        let source = format!(
            "#import \"/mitex/specs/mod.typ\": mitex-scope\n\
             #set text(font: \"TeX Gyre Termes\")\n\
             #show math.equation: set text(font: \"TeX Gyre Termes Math\")\n\
             #eval(\"${name}$\", scope: mitex-scope)\n"
        );
        let world = SepWorld::new(source, &[], &fonts);
        match sep_typst::compile_pdf(&world, "test") {
            Err(err) => unknown.push(format!(
                "{name}: {}",
                err.lines.join(" / ").replace('\n', " ")
            )),
            Ok(compiled) if !compiled.warnings.is_empty() => unknown.push(format!(
                "{name}: {}",
                compiled.warnings.join(" / ").replace('\n', " ")
            )),
            Ok(_) => {}
        }
    }
    assert!(
        unknown.is_empty(),
        "{} of {} aliases do not resolve:\n{}",
        unknown.len(),
        all.len(),
        unknown.join("\n")
    );
}

#[test]
fn no_stale_symbol_name_is_left_in_the_scope_files() {
    let alias = regex::Regex::new(r#"define-sym\("[^"]*""#).expect("regex");
    let key = regex::Regex::new(r#"^\s*"?[A-Za-z0-9_]+"?\s*:"#).expect("regex");
    let words: Vec<(regex::Regex, &str, &str)> = sep_typst::math::RENAMED_SYMBOLS
        .iter()
        .map(|(was, is)| {
            let word = format!(r"\b{}\b", regex::escape(was));
            (regex::Regex::new(&word).expect("regex"), *was, *is)
        })
        .collect();
    let mut stale: Vec<String> = Vec::new();
    for (path, text) in MITEX_FILES {
        let text = alias.replace_all(text, "define-sym(");
        for (line, source) in text.lines().enumerate() {
            let source = key.replace(source, "");
            for (word, was, is) in &words {
                if word.is_match(&source) {
                    stale.push(format!("{path}:{}: {was} is now {is}", line + 1));
                }
            }
        }
    }
    assert!(stale.is_empty(), "{}", stale.join("\n"));
}
