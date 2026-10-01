use std::collections::BTreeSet;

use sep_core::model::Doc;
use sep_core::style::Style;

use crate::compile::{CompileError, Compiled, compile_pdf, typesets};
use crate::emit::{EmitOptions, Rendered, emit_document};
use crate::fonts::Fonts;
use crate::world::SepWorld;

pub const COMPILE_BUDGET: usize = 48;

pub struct Narrowed {
    pub formulas: Option<BTreeSet<usize>>,
    pub attempts: usize,
}

fn with(base: &BTreeSet<usize>, more: impl IntoIterator<Item = usize>) -> BTreeSet<usize> {
    base.iter().copied().chain(more).collect()
}

struct Search<'f> {
    compiles: &'f mut dyn FnMut(&BTreeSet<usize>) -> bool,
    attempts: usize,
}

impl Search<'_> {
    fn ok(&mut self, raw: &BTreeSet<usize>) -> bool {
        self.attempts += 1;
        (self.compiles)(raw)
    }

    fn minimize(&mut self, base: &BTreeSet<usize>, items: &[usize]) -> BTreeSet<usize> {
        if items.len() <= 1 || self.attempts >= COMPILE_BUDGET {
            return items.iter().copied().collect();
        }
        let (head, tail) = items.split_at(items.len().div_ceil(2));
        if self.ok(&with(base, head.iter().copied())) {
            return self.minimize(base, head);
        }
        if self.ok(&with(base, tail.iter().copied())) {
            return self.minimize(base, tail);
        }
        let mut found = self.minimize(&with(base, tail.iter().copied()), head);
        let rest = self.minimize(&with(base, found.iter().copied()), tail);
        found.extend(rest);
        found
    }
}

pub fn narrow(total: usize, compiles: &mut dyn FnMut(&BTreeSet<usize>) -> bool) -> Narrowed {
    let all: Vec<usize> = (0..total).collect();
    let mut search = Search {
        compiles,
        attempts: 0,
    };
    if !search.ok(&all.iter().copied().collect()) {
        return Narrowed {
            formulas: None,
            attempts: search.attempts,
        };
    }
    let formulas = search.minimize(&BTreeSet::new(), &all);
    Narrowed {
        formulas: Some(formulas),
        attempts: search.attempts,
    }
}

pub struct Outcome {
    pub rendered: Rendered,
    pub result: Result<Compiled, CompileError>,
    pub notes: Vec<String>,
}

fn give_up(rendered: Rendered, failure: CompileError) -> Outcome {
    Outcome {
        rendered,
        result: Err(failure),
        notes: Vec::new(),
    }
}

fn excerpt(tex: &str) -> String {
    let flat = tex.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(72) {
        Some((at, _)) => format!("{}…", &flat[..at]),
        None => flat,
    }
}

pub fn compile_document(
    docs: &[&Doc],
    style: &Style,
    opts: EmitOptions<'_>,
    fonts: &Fonts,
    creator: &str,
    progress: &mut dyn FnMut(String),
) -> Outcome {
    let outcome = compile_and_recover(docs, style, opts, fonts, creator, progress);
    typst::comemo::evict(0);
    outcome
}

fn compile_and_recover(
    docs: &[&Doc],
    style: &Style,
    opts: EmitOptions<'_>,
    fonts: &Fonts,
    creator: &str,
    progress: &mut dyn FnMut(String),
) -> Outcome {
    let build = |rendered: &Rendered| {
        compile_pdf(
            &SepWorld::new(rendered.source.clone(), &rendered.figures, fonts),
            creator,
        )
    };

    let rendered = emit_document(docs, style, opts);
    let failure = match build(&rendered) {
        Ok(compiled) => {
            return Outcome {
                rendered,
                result: Ok(compiled),
                notes: Vec::new(),
            };
        }
        Err(failure) => failure,
    };
    if rendered.formulas.is_empty() {
        return give_up(rendered, failure);
    }

    progress("Typst refused the document; looking for the formulas at fault".to_owned());
    let laid_out = typesets(&SepWorld::new(
        rendered.source.clone(),
        &rendered.figures,
        fonts,
    ));
    let mut compiles = |raw: &BTreeSet<usize>| {
        let attempt = emit_document(
            docs,
            style,
            EmitOptions {
                raw_math: raw,
                ..opts
            },
        );
        if laid_out {
            build(&attempt).is_ok()
        } else {
            typesets(&SepWorld::new(attempt.source, &attempt.figures, fonts))
        }
    };
    let narrowed = narrow(rendered.formulas.len(), &mut compiles);
    let Some(culprits) = narrowed.formulas else {
        progress(format!(
            "the maths was not the cause ({} compile{})",
            narrowed.attempts,
            if narrowed.attempts == 1 { "" } else { "s" }
        ));
        return give_up(rendered, failure);
    };

    let notes: Vec<String> = culprits
        .iter()
        .map(|&at| {
            format!(
                "maths left as LaTeX, Typst could not evaluate what mitex made of it: {}",
                excerpt(&rendered.formulas[at])
            )
        })
        .collect();
    progress(format!(
        "{} of {} formulas left as LaTeX, found in {} compiles",
        culprits.len(),
        rendered.formulas.len(),
        narrowed.attempts
    ));

    let recovered = emit_document(
        docs,
        style,
        EmitOptions {
            raw_math: &culprits,
            ..opts
        },
    );
    match build(&recovered) {
        Ok(compiled) => Outcome {
            rendered: recovered,
            result: Ok(compiled),
            notes,
        },
        Err(_) => give_up(rendered, failure),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(total: usize, bad: &[usize]) -> Narrowed {
        let mut compiles = |raw: &BTreeSet<usize>| bad.iter().all(|b| raw.contains(b));
        narrow(total, &mut compiles)
    }

    #[test]
    fn one_bad_formula_is_found_by_halving() {
        let found = search(336, &[201]);
        assert_eq!(found.formulas, Some(BTreeSet::from([201])));
        assert!(found.attempts <= 20, "{} compiles", found.attempts);
    }

    #[test]
    fn several_bad_formulas_are_all_found() {
        for bad in [vec![0], vec![15], vec![0, 15], vec![3, 4], vec![1, 8, 13]] {
            let found = search(16, &bad);
            assert_eq!(
                found.formulas,
                Some(bad.iter().copied().collect()),
                "bad: {bad:?}"
            );
        }
    }

    #[test]
    fn maths_that_is_not_the_cause_is_reported_as_such() {
        let mut never = |_: &BTreeSet<usize>| false;
        let found = narrow(64, &mut never);
        assert_eq!(found.formulas, None);
        assert_eq!(found.attempts, 1, "one compile settles it");
    }

    #[test]
    fn the_budget_holds_and_what_it_returns_still_compiles() {
        let bad: Vec<usize> = (0..128).step_by(2).collect();
        let found = search(128, &bad);
        let formulas = found.formulas.expect("all of them as LaTeX compiles");
        assert!(found.attempts <= COMPILE_BUDGET + 1, "{}", found.attempts);
        assert!(
            bad.iter().all(|b| formulas.contains(b)),
            "the set it gives up with still compiles"
        );
    }

    #[test]
    fn a_formula_is_quoted_on_one_short_line() {
        assert_eq!(excerpt("a \n  b"), "a b");
        let long = excerpt(&"x".repeat(100));
        assert_eq!(long.chars().count(), 73);
        assert!(long.ends_with('…'));
    }
}
