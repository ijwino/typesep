use std::collections::BTreeSet;

use typesep_model::Doc;
use typesep_style::Style;

use crate::compile::{CompileError, Compiled, Typeset, compile_pdf, typesets};
use crate::emit::{EmitOptions, Layout, Rendered, emit_document, emit_with, facing};
use crate::fonts::Fonts;
use crate::world::TypesetWorld;

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
            &TypesetWorld::new(rendered.source.clone(), &rendered.figures, fonts),
            creator,
        )
    };
    let facing = facing(docs, style, &opts);
    // A document on facing pages is compiled as it is planned, and its PDF is made from the
    // last of those compiles; any other is compiled here.
    let emit = |raw_tex: &BTreeSet<String>, raw_math: &BTreeSet<usize>| {
        let opts = EmitOptions { raw_math, ..opts };
        match facing {
            Some(doc) => {
                let (rendered, typeset) = crate::facing::emit(doc, style, opts, fonts, raw_tex);
                let typeset = typeset.unwrap_or_else(|| Typeset::new(&rendered, fonts));
                (rendered, typeset)
            }
            None => {
                let rendered = emit_document(docs, style, opts);
                let typeset = Typeset::new(&rendered, fonts);
                (rendered, typeset)
            }
        }
    };

    let (rendered, typeset) = emit(&BTreeSet::new(), opts.raw_math);
    let laid_out = typeset.document().is_some();
    let failure = match typeset.pdf(creator) {
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
    // A formula Typst cannot evaluate fails wherever the page puts it, so the formulas of a
    // document on facing pages are looked for in its rows set one after another, which takes a
    // compile each time where the facing pages take several. What is found is the formulas'
    // TeX, since the spreads set them in another order.
    let probe = |raw: &BTreeSet<usize>| match facing {
        Some(doc) => emit_with(
            &[doc],
            style,
            EmitOptions {
                raw_math: raw,
                ..opts
            },
            &Layout::Flow,
        ),
        None => emit_document(
            docs,
            style,
            EmitOptions {
                raw_math: raw,
                ..opts
            },
        ),
    };
    let mut compiles = |raw: &BTreeSet<usize>| {
        let attempt = probe(raw);
        if laid_out {
            build(&attempt).is_ok()
        } else {
            typesets(&TypesetWorld::new(attempt.source, &attempt.figures, fonts))
        }
    };
    let (formulas, checked) = match facing {
        Some(_) => {
            // The rows set one after another must fail as the facing pages did, or the search
            // would blame the first formula for what is not the maths.
            if compiles(opts.raw_math) {
                progress("the maths was not the cause (1 compile)".to_owned());
                return give_up(rendered, failure);
            }
            (probe(opts.raw_math).formulas, 1)
        }
        None => (rendered.formulas.clone(), 0),
    };
    let narrowed = narrow(formulas.len(), &mut compiles);
    let attempts = checked + narrowed.attempts;
    let Some(culprits) = narrowed.formulas else {
        progress(format!(
            "the maths was not the cause ({attempts} compile{})",
            if attempts == 1 { "" } else { "s" }
        ));
        return give_up(rendered, failure);
    };
    progress(format!(
        "{} of {} formulas left as LaTeX, found in {attempts} compiles",
        culprits.len(),
        formulas.len(),
    ));

    let (recovered, typeset) = match facing {
        Some(_) => {
            let tex: BTreeSet<String> = culprits.iter().map(|&at| formulas[at].clone()).collect();
            emit(&tex, opts.raw_math)
        }
        None => emit(&BTreeSet::new(), &culprits),
    };
    let notes: Vec<String> = recovered
        .formulas
        .iter()
        .enumerate()
        .filter(|(at, tex)| match facing {
            Some(_) => culprits.iter().any(|&culprit| formulas[culprit] == **tex),
            None => culprits.contains(at),
        })
        .map(|(_, tex)| {
            format!(
                "maths left as LaTeX, Typst could not evaluate what mitex made of it: {}",
                excerpt(tex)
            )
        })
        .collect();
    match typeset.pdf(creator) {
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
    use std::cell::Cell;

    use typesep_model::{Block, Inline, MathMode, Meta};
    use typesep_style::{ResolveOptions, resolve_style};

    use super::*;
    use crate::compile::COMPILES;

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

    /// TeX mitex makes code of that Typst cannot evaluate, a root with an empty index.
    const BAD: &str = r"\sqrt[]{x}";

    /// What `f` returns, and how many compiles it made.
    fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
        let before = COMPILES.with(Cell::get);
        let out = f();
        (out, COMPILES.with(Cell::get) - before)
    }

    fn para(text: &str, tex: Option<&str>) -> Block {
        let mut children = vec![Inline::Text {
            value: text.to_owned(),
        }];
        children.extend(tex.map(|tex| Inline::Math {
            mode: MathMode::Inline,
            tex: tex.to_owned(),
        }));
        Block::Para {
            children,
            align: None,
            indent_em: 0,
        }
    }

    /// A bilingual edition of twelve remarks, each a paragraph with a formula on either side,
    /// the formula `tex` gives the side and row of, or none.
    fn edition(tex: impl Fn(usize, usize) -> Option<String>) -> Doc {
        let side = |row: usize, side: usize| {
            let (lang, words) = [("de", "Ein Satz"), ("en", "A sentence")][side];
            vec![Block::Lang {
                lang: lang.into(),
                dir: None,
                children: vec![Block::Remark {
                    number: format!("{}", row + 1),
                    id: Some(format!("{lang}-{row}")),
                    children: vec![para(&words.repeat(20), tex(row, side).as_deref())],
                }],
            }]
        };
        Doc {
            meta: Meta {
                id: "both".into(),
                title: "Both".into(),
                lang: "de".into(),
                ..Meta::default()
            },
            body: (0..12)
                .map(|row| Block::Columns {
                    columns: vec![side(row, 0), side(row, 1)],
                })
                .collect(),
            ..Doc::default()
        }
    }

    fn style(layout: &str) -> Style {
        resolve_style(ResolveOptions {
            preset: Some("lwp"),
            file: None,
            sets: vec![format!("parallel.layout={layout}").into()],
            schema: None,
            default_preset: None,
        })
        .unwrap()
    }

    /// What `doc` compiles to, with what was said of it on the way and how many compiles it
    /// took.
    fn compiled(doc: &Doc, style: &Style, fonts: &Fonts) -> (Outcome, Vec<String>, usize) {
        let mut said = Vec::new();
        let (outcome, compiles) = counted(|| {
            compile_document(
                &[doc],
                style,
                EmitOptions {
                    fonts: Some(fonts),
                    parallel: true,
                    ..Default::default()
                },
                fonts,
                "typesep tests",
                &mut |message| said.push(message),
            )
        });
        (outcome, said, compiles)
    }

    /// The number of compiles `progress` says the search took.
    fn search_compiles(said: &[String]) -> usize {
        let last = said.last().expect("the search says what it found");
        let count = last
            .rsplit("found in ")
            .next()
            .and_then(|rest| rest.split(' ').next())
            .and_then(|count| count.parse().ok());
        count.unwrap_or_else(|| panic!("{said:?}"))
    }

    #[test]
    fn the_pdf_of_facing_pages_is_made_from_their_last_check_without_compiling_again() {
        let fonts = Fonts::embedded();
        let style = style("facing");
        let doc = edition(|row, side| Some(format!("x_{{{row}{side}}}")));
        let (source, planned) = counted(|| {
            emit_document(
                &[&doc],
                &style,
                EmitOptions {
                    fonts: Some(&fonts),
                    parallel: true,
                    ..Default::default()
                },
            )
            .source
        });
        assert!(planned >= 2, "measured and checked: {planned}");
        let (outcome, said, compiles) = compiled(&doc, &style, &fonts);
        assert!(outcome.result.is_ok(), "{said:?}");
        assert_eq!(outcome.rendered.source, source);
        assert_eq!(compiles, planned, "the PDF takes no compile of its own");
    }

    #[test]
    fn the_bad_formulas_of_facing_pages_are_found_in_their_rows_set_one_after_another() {
        let fonts = Fonts::embedded();
        let style = style("facing");
        let bad = |row: usize, side: usize| match (row, side) {
            (3, 1) => Some(format!(r"{BAD} + 1")),
            (8, 0) => Some(format!(r"{BAD} + 2")),
            _ => Some(format!("x_{{{row}{side}}}")),
        };
        let doc = edition(bad);
        let (outcome, said, compiles) = compiled(&doc, &style, &fonts);
        assert!(outcome.result.is_ok(), "{said:?}");
        assert_eq!(
            outcome.notes,
            [
                format!(
                    r"maths left as LaTeX, Typst could not evaluate what mitex made of it: {BAD} + 1"
                ),
                format!(
                    r"maths left as LaTeX, Typst could not evaluate what mitex made of it: {BAD} + 2"
                ),
            ]
        );
        assert_eq!(
            said.last()
                .map(|said| said.split(", found").next().unwrap()),
            Some("2 of 24 formulas left as LaTeX")
        );
        assert_eq!(
            outcome.rendered.source.matches("#fs-math-raw(\"").count(),
            2,
            "the two bad formulas, and only they, are left as LaTeX"
        );

        // The facing pages are planned twice, for the document as it is, whose measuring
        // fails and so whose source is compiled once more, and with the bad formulas left out;
        // every other compile is of the rows set one after another, one to a search.
        let raw: BTreeSet<String> = [format!(r"{BAD} + 1"), format!(r"{BAD} + 2")].into();
        let opts = EmitOptions {
            fonts: Some(&fonts),
            parallel: true,
            ..Default::default()
        };
        let ((recovered, typeset), planned) =
            counted(|| crate::facing::emit(&doc, &style, opts, &fonts, &raw));
        assert!(typeset.is_some());
        assert_eq!(recovered.source, outcome.rendered.source);
        assert_eq!(compiles, 2 + search_compiles(&said) + planned);
    }

    #[test]
    fn the_bad_formulas_of_a_document_are_found_in_a_compile_each() {
        let fonts = Fonts::embedded();
        let style = style("columns");
        let doc = edition(|row, side| {
            Some(if (row, side) == (5, 1) {
                BAD.to_owned()
            } else {
                format!("x_{{{row}{side}}}")
            })
        });
        let (outcome, said, compiles) = compiled(&doc, &style, &fonts);
        assert!(outcome.result.is_ok(), "{said:?}");
        assert_eq!(
            outcome.notes,
            [format!(
                r"maths left as LaTeX, Typst could not evaluate what mitex made of it: {BAD}"
            )]
        );
        assert_eq!(
            compiles,
            1 + search_compiles(&said) + 1,
            "the document, the search and the document with the formula left out"
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
