use std::collections::HashMap;

use crate::lazy_regex;
use typesep_model::MacroTable;

pub const COMPAT_PRELUDE: &str = concat!(
    r"\newcommand{\style}[2]{#2}",
    r"\newcommand{\class}[2]{#2}",
    r"\newcommand{\cssId}[2]{#2}",
    r"\newcommand{\require}[1]{}",
    r"\newcommand{\bbox}[1]{#1}",
    r"\newcommand{\sc}[1]{\textrm{#1}}",
    r"\newcommand{\italic}[1]{\mathit{#1}}",
    r"\newcommand{\mbox}[1]{\text{#1}}",
    r"\newcommand{\lower}[2]{#2}",
    r"\newcommand{\raise}[2]{#2}",
);

pub fn macros_prelude(macros: &MacroTable) -> String {
    let mut out = String::new();
    for (name, def) in macros {
        let signature = if def.args > 0 {
            format!("[{}]", def.args)
        } else {
            String::new()
        };
        out.push_str(&format!(
            "\\newcommand{{\\{name}}}{signature}{{{}}}",
            def.body
        ));
    }
    out
}

lazy_regex!(LABEL_RE = r"\\label\{[^}]*\}".to_owned());
lazy_regex!(NONUMBER_RE = r"\\nonumber\b".to_owned());
lazy_regex!(BLANK_LINES_RE = r"\n[ \t]*\n".to_owned());
lazy_regex!(
    DISPLAY_ENV_RE =
        r"^\s*\\begin\{(align|alignat|gather|multline|flalign|eqnarray|equation)(\*?)\}".to_owned()
);
lazy_regex!(
    HAS_ARRAY_RE =
        r"\\begin\{(array|matrix|pmatrix|bmatrix|vmatrix|smallmatrix|cases)\}".to_owned()
);
lazy_regex!(HLINE_RE = r"\\hline\s*".to_owned());
lazy_regex!(TRAILING_BREAK_RE = r"\\\\\s*(\\end\{)".to_owned());
lazy_regex!(
    UNICODE_RE =
        r"\\unicode\s*(?:\[[^\]]*\]\s*)*\{\s*(?:0?(x[0-9a-fA-F]+)|([0-9]+))\s*\}".to_owned()
);
lazy_regex!(
    STARRED_ENV_RE =
        r"\\(begin|end)\{(align|alignat|gather|multline|flalign|eqnarray|equation)\*\}".to_owned()
);

pub fn normalize_display_math(tex: &str) -> String {
    let mut body = tex.trim_start().to_owned();
    loop {
        let trimmed = body.trim_end();
        if trimmed.len() == body.len() {
            break;
        }
        if trimmed.ends_with('\\') {
            body.truncate(trimmed.len() + 1);
            break;
        }
        body.truncate(trimmed.len());
    }
    body = LABEL_RE.replace_all(&body, "").into_owned();
    body = NONUMBER_RE.replace_all(&body, "").into_owned();
    body = BLANK_LINES_RE.replace_all(&body, "\n").into_owned();

    let Some(env) = DISPLAY_ENV_RE.captures(&body) else {
        return body;
    };
    let name = env[1].to_owned();
    let starred = &env[2] == "*";
    if !HAS_ARRAY_RE.is_match(&body) {
        body = HLINE_RE.replace_all(&body, "").into_owned();
    }
    body = TRAILING_BREAK_RE.replace(&body, "$1").into_owned();
    if !starred {
        body = body
            .replace(
                &format!("\\begin{{{name}}}"),
                &format!("\\begin{{{name}*}}"),
            )
            .replace(&format!("\\end{{{name}}}"), &format!("\\end{{{name}*}}"));
    }
    body
}

pub fn expand_unicode(tex: &str) -> String {
    UNICODE_RE
        .replace_all(tex, |caps: &regex::Captures| {
            let n = match (caps.get(1), caps.get(2)) {
                (Some(hex), _) => u32::from_str_radix(&hex.as_str()[1..], 16).ok(),
                (_, Some(decimal)) => decimal.as_str().parse().ok(),
                _ => None,
            };
            n.and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_else(|| caps[0].to_owned())
        })
        .into_owned()
}

pub const RENAMED_SYMBOLS: [(&str, &str); 37] = [
    ("sect", "inter"),
    ("sect.big", "inter.big"),
    ("sect.double", "inter.double"),
    ("sect.sq", "inter.sq"),
    ("angle.l", "chevron.l"),
    ("angle.r", "chevron.r"),
    ("bracket.l.double", "bracket.l.stroked"),
    ("bracket.r.double", "bracket.r.stroked"),
    ("arrow.l.dash", "arrow.l.open"),
    ("arrow.r.dash", "arrow.r.open"),
    ("ast.circle", "ast.o"),
    ("dash.circle", "dash.o"),
    ("circle.nested", "compose.o"),
    ("dot.circle", "dot.o"),
    ("dot.circle.big", "dot.o.big"),
    ("minus.circle", "minus.o"),
    ("plus.circle", "plus.o"),
    ("plus.circle.big", "plus.o.big"),
    ("times.circle", "times.o"),
    ("times.circle.big", "times.o.big"),
    ("diff", "partial"),
    ("ohm.inv", "Omega.inv"),
    ("planck.reduce", "planck"),
    ("tack.r.double", "tack.rr"),
    ("tack.r.double.not", "tack.rr.not"),
    ("tack.l.double", "tack.ll"),
    ("tack.t.double", "tack.tt"),
    ("tack.b.double", "tack.bb"),
    ("gt.tri", "gt.closed"),
    ("gt.tri.eq", "gt.closed.eq"),
    ("gt.tri.eq.not", "gt.closed.eq.not"),
    ("gt.tri.not", "gt.closed.not"),
    ("lt.tri", "lt.closed"),
    ("lt.tri.eq", "lt.closed.eq"),
    ("lt.tri.eq.not", "lt.closed.eq.not"),
    ("lt.tri.not", "lt.closed.not"),
    ("join", "bowtie.big"),
];

lazy_regex!(IDENTIFIER_RE = r"[A-Za-z][A-Za-z0-9]*(?:\.[A-Za-z0-9]+)*".to_owned());

pub fn rename_symbols(code: &str) -> String {
    let table: std::collections::HashMap<&str, &str> = RENAMED_SYMBOLS.iter().copied().collect();
    let mut out = String::with_capacity(code.len());
    let bytes = code.as_bytes();
    let mut depth = 0usize;
    let mut at = 0usize;
    while at < bytes.len() {
        let rest = &code[at..];
        let c = rest.chars().next().expect("non-empty");
        if c == '\\' {
            let width = rest.chars().take(2).map(char::len_utf8).sum::<usize>();
            out.push_str(&code[at..at + width]);
            at += width;
            continue;
        }
        if c == '[' {
            depth += 1;
        } else if c == ']' {
            depth = depth.saturating_sub(1);
        }
        if depth == 0
            && c.is_ascii_alphabetic()
            && let Some(m) = IDENTIFIER_RE.find(rest)
            && m.start() == 0
        {
            let word = m.as_str();
            out.push_str(table.get(word).copied().unwrap_or(word));
            at += word.len();
            continue;
        }
        out.push(c);
        at += c.len_utf8();
    }
    out
}

pub fn unstar_environments(tex: &str) -> String {
    STARRED_ENV_RE.replace_all(tex, "\\${1}{${2}}").into_owned()
}

lazy_regex!(
    KERN_RE = r"\\(?:mkern|mskip|kern|hskip|vskip)\s*[+-]?(?:\d+\.?\d*|\.\d+)\s*(?:mu|em|ex|pt|pc|bp|in|cm|mm|dd|cc|sp)\b"
        .to_owned()
);

pub fn drop_kerns(tex: &str) -> String {
    KERN_RE.replace_all(tex, "").into_owned()
}

lazy_regex!(SPACE_ARG_RE = r"\\([hv]space)(\*?)\s*\{([^{}]*)\}".to_owned());
lazy_regex!(LENGTH_RE = r"^\s*([+-]?(?:\d+\.?\d*|\.\d+))\s*([a-z]+)\s*$".to_owned());

fn typst_length(text: &str) -> String {
    let Some(caps) = LENGTH_RE.captures(text) else {
        return text.to_owned();
    };
    let Ok(n) = caps[1].parse::<f64>() else {
        return text.to_owned();
    };
    let (scale, unit) = match &caps[2] {
        "ex" => (0.5, "em"),
        "mu" => (1.0 / 18.0, "em"),
        "bp" => (1.0, "pt"),
        "pc" => (12.0, "pt"),
        _ => return text.to_owned(),
    };
    format!("{}{unit}", typesep_style::units::fmt_pt(n * scale))
}

pub fn typst_lengths(tex: &str) -> String {
    SPACE_ARG_RE
        .replace_all(tex, |caps: &regex::Captures| {
            format!("\\{}{}{{{}}}", &caps[1], &caps[2], typst_length(&caps[3]))
        })
        .into_owned()
}

fn brace_pairs(text: &str) -> (HashMap<usize, usize>, HashMap<usize, usize>) {
    let (mut opens, mut closes) = (HashMap::new(), HashMap::new());
    let mut stack: Vec<usize> = Vec::new();
    let bytes = text.as_bytes();
    let mut at = 0usize;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 1,
            b'{' => stack.push(at),
            b'}' => {
                if let Some(open) = stack.pop() {
                    opens.insert(at, open);
                    closes.insert(open, at);
                }
            }
            _ => {}
        }
        at += 1;
    }
    (opens, closes)
}

lazy_regex!(OVER_RE = r"\\over(?:[^a-zA-Z]|$)".to_owned());

fn one_fraction_bar(text: &str) -> Option<String> {
    if !OVER_RE.is_match(text) {
        return None;
    }
    let (opens, closes) = brace_pairs(text);
    for found in OVER_RE.find_iter(text) {
        let before = text[..found.start()].trim_end();
        let after = found.start() + "\\over".len();
        let after = text.len() - text[after..].trim_start().len();
        if !before.ends_with('}') || !text[after..].starts_with('{') {
            continue;
        }
        let (Some(&open), Some(&end)) = (opens.get(&(before.len() - 1)), closes.get(&after)) else {
            continue;
        };
        return Some(format!(
            "{}\\frac{{{}}}{{{}}}{}",
            &text[..open],
            &text[open + 1..before.len() - 1],
            &text[after + 1..end],
            &text[end + 1..]
        ));
    }
    None
}

pub fn fraction_bars(tex: &str) -> String {
    let mut text = tex.to_owned();
    while let Some(rewritten) = one_fraction_bar(&text) {
        text = rewritten;
    }
    text
}

lazy_regex!(ARRAY_RE = r"\\begin\{(?:array|tabular|tabularx)\*?\}".to_owned());
lazy_regex!(
    SPACING_RE = r"(?:\\hspace\*?\{[^{}]*\}|\\extracolsep\{[^{}]*\}|\\(?:qquad|quad|hfill|hfil|thinspace|enspace)|\\[,;:! ]|\s)+"
        .to_owned()
);

fn spacing_only(text: &str) -> bool {
    SPACING_RE.replace_all(text, "").is_empty()
}

pub fn drop_column_spacing(tex: &str) -> String {
    let mut text = tex.to_owned();
    'again: loop {
        if !ARRAY_RE.is_match(&text) {
            return text;
        }
        let (_, closes) = brace_pairs(&text);
        for found in ARRAY_RE.find_iter(&text) {
            let mut at = found.end();
            if text[at..].starts_with('[')
                && let Some(shut) = text[at..].find(']')
            {
                at += shut + 1;
            }
            let Some(&spec) = closes.get(&at) else {
                continue;
            };
            for (offset, _) in text[at..spec].match_indices("@{") {
                let sep = at + offset;
                let Some(&end) = closes.get(&(sep + 1)) else {
                    continue;
                };
                if !spacing_only(&text[sep + 2..end]) {
                    continue;
                }
                text = format!("{}{}", &text[..sep], &text[end + 1..]);
                continue 'again;
            }
        }
        return text;
    }
}

pub fn for_mitex(tex: &str) -> String {
    let text = unstar_environments(tex);
    let text = drop_kerns(&text);
    let text = typst_lengths(&text);
    let text = fraction_bars(&text);
    drop_column_spacing(&text)
}

pub struct Converter {
    prelude: String,
}

impl Converter {
    pub fn new(macros: &MacroTable) -> Self {
        Converter {
            prelude: expand_unicode(&format!("{COMPAT_PRELUDE}{}", macros_prelude(macros))),
        }
    }

    fn parses(code: &str) -> Result<(), String> {
        let root = typst::syntax::parse(&format!("${code}$"));
        let (errors, _) = root.errors_and_warnings();
        match errors.first() {
            Some(error) => Err(error.message.to_string()),
            None => Ok(()),
        }
    }

    pub fn convert(&self, tex: &str, display: bool) -> Result<String, String> {
        let body = if display {
            normalize_display_math(tex)
        } else {
            tex.to_owned()
        };
        let input = format!("{}{}", self.prelude, for_mitex(&expand_unicode(&body)));
        let out = mitex::convert_math(&input, None).map_err(|e| {
            e.trim()
                .strip_prefix("error: ")
                .map(str::to_owned)
                .unwrap_or(e.trim().to_owned())
        })?;
        let code = rename_symbols(out.trim());
        Self::parses(&code)?;
        Ok(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use typesep_model::MacroDef;

    #[test]
    fn display_maths_is_normalised() {
        assert_eq!(
            normalize_display_math("\\begin{align} a &= b \\label{x} \\\\ \\end{align}"),
            "\\begin{align*} a &= b  \\end{align*}"
        );
        assert_eq!(
            normalize_display_math("  x = y \\nonumber \\\\"),
            "x = y  \\\\"
        );
        assert_eq!(normalize_display_math("a\n\n\nb"), "a\n\nb");
        assert_eq!(normalize_display_math("a\n  \nb"), "a\nb");
        assert_eq!(
            normalize_display_math("\\begin{align*} \\hline a \\end{align*}"),
            "\\begin{align*} a \\end{align*}"
        );
        assert_eq!(
            normalize_display_math(
                "\\begin{equation} \\begin{array}{c} \\hline a \\end{array} \\end{equation}"
            ),
            "\\begin{equation*} \\begin{array}{c} \\hline a \\end{array} \\end{equation*}"
        );
    }

    #[test]
    fn macros_and_compat_commands_expand() {
        let macros: MacroTable = BTreeMap::from([
            (
                "opL".to_owned(),
                MacroDef {
                    body: "\\mathbin{L}".into(),
                    args: 0,
                },
            ),
            (
                "pr".to_owned(),
                MacroDef {
                    body: "P(#1 \\mid #2)".into(),
                    args: 2,
                },
            ),
        ]);
        assert_eq!(
            macros_prelude(&macros),
            "\\newcommand{\\opL}{\\mathbin{L}}\\newcommand{\\pr}[2]{P(#1 \\mid #2)}"
        );
        let conv = Converter::new(&macros);
        let out = conv.convert("q \\opL \\pr{a}{b}", false).unwrap();
        assert!(out.contains("mathbin(L )"), "{out}");
        assert!(out.contains("divides"), "{out}");
        let out = conv
            .convert("\\style{color:red}{x} \\class{c}{y} \\sc{ab}", false)
            .unwrap();
        assert!(out.contains('x') && out.contains('y'), "{out}");
        assert_eq!(expand_unicode("\\unicode{x2200} \\unicode{8707}"), "∀ ∃");
        assert_eq!(
            expand_unicode("\\unicode{0x2200}"),
            "∀",
            "MathJax also writes 0x"
        );
        assert_eq!(expand_unicode("\\unicode[.,.]{x2200}"), "∀");
        assert_eq!(
            unstar_environments("\\begin{gather*} a \\end{gather*}"),
            "\\begin{gather} a \\end{gather}"
        );
        let conv = Converter::new(&Default::default());
        assert!(
            conv.convert("\\begin{gather} a \\\\ b \\end{gather}", true)
                .is_ok()
        );
        assert!(conv.convert("x \\mbox{(and so on)}", false).is_ok());
        let err = conv.convert("\\nosuchcommand{x}", false).unwrap_err();
        assert!(err.contains("nosuchcommand"), "{err}");
        assert_eq!(Converter::parses("frac(1, 2)"), Ok(()));
        assert!(Converter::parses("lr(( a").is_err());
        let out = conv
            .convert("\\begin{align} a &= b \\\\ c &= d \\end{align}", true)
            .unwrap();
        assert!(out.starts_with("aligned("), "{out}");
    }

    #[test]
    fn latex_mitex_cannot_read_is_rewritten_before_it_sees_it() {
        assert_eq!(
            drop_kerns(r"\alpha \not\mkern-7mu\vdash"),
            r"\alpha \not\vdash"
        );
        assert_eq!(drop_kerns(r"a \kern 2pt b"), "a  b");
        assert_eq!(
            drop_kerns(r"\kernel"),
            r"\kernel",
            "only a kern with a dimension"
        );
        assert_eq!(typst_lengths(r"a \hspace{3ex} b"), r"a \hspace{1.5em} b");
        assert_eq!(typst_lengths(r"\hspace*{9mu}"), r"\hspace*{0.5em}");
        assert_eq!(typst_lengths(r"\vspace{2bp}"), r"\vspace{2pt}");
        assert_eq!(
            typst_lengths(r"\hspace{2cm}"),
            r"\hspace{2cm}",
            "cm it already knows"
        );
        assert_eq!(fraction_bars(r"{dx} \over {dt}"), r"\frac{dx}{dt}");
        assert_eq!(
            fraction_bars(r"a + {{x} \over {y}} + b"),
            r"a + {\frac{x}{y}} + b"
        );
        assert_eq!(
            fraction_bars(r"\overline{x}"),
            r"\overline{x}",
            "not every \\over"
        );
        assert_eq!(
            fraction_bars(r"a \over b"),
            r"a \over b",
            "neither side is a group"
        );
        assert_eq!(
            drop_column_spacing(r"\begin{array}{r@{}c@{}r} a & b & c \end{array}"),
            r"\begin{array}{rcr} a & b & c \end{array}"
        );
        assert_eq!(
            drop_column_spacing(r"\begin{array}[t]{l@{\hspace{2cm}}l} a & b \end{array}"),
            r"\begin{array}[t]{ll} a & b \end{array}"
        );
        assert_eq!(
            drop_column_spacing(r"\begin{array}{rc@{\mbox{if\ \ }}l} a && b \end{array}"),
            r"\begin{array}{rc@{\mbox{if\ \ }}l} a && b \end{array}",
            "a separator with a word in it is the table's, not ours to drop"
        );
        assert_eq!(
            drop_column_spacing(r"\begin{array}{@{}l@{\mbox{if}}r@{\quad}l} a \end{array}"),
            r"\begin{array}{l@{\mbox{if}}rl} a \end{array}",
            "the ones that are only space go, the one that is not stays"
        );
    }

    #[test]
    fn the_formulas_that_took_a_document_down_convert() {
        let conv = Converter::new(&Default::default());
        for (name, display, tex) in [
            (
                "chaos",
                true,
                r"\begin{aligned} {dx} \over {dt} &= -2yt \\ {dy} \over {dt} &= 2xt \end{aligned}",
            ),
            (
                "formal-belief",
                false,
                r"\alpha \not\mkern-7mu\vdash \neg\gamma",
            ),
            (
                "logic-fuzzy",
                true,
                r"\frac{\Gamma_1 \Rightarrow \phi \hspace{3ex} \Gamma_2}{\Gamma_1, \Gamma_2}",
            ),
            (
                "logic-temporal",
                true,
                r"\begin{array}{l@{\hspace{2cm}}l} \langle L \rangle & a \end{array}",
            ),
            (
                "qm",
                true,
                r"\begin{align} \ket{B=\text{ground}}_{s^*} &\rightarrow x \end{align}",
            ),
        ] {
            let out = conv
                .convert(tex, display)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!out.is_empty(), "{name}");
            assert!(
                !out.contains("m u "),
                "{name}: the kern is still there: {out}"
            );
            assert!(
                !out.contains("e x "),
                "{name}: the ex unit is still there: {out}"
            );
        }
        assert!(
            conv.convert(r"\ket{B}", false).unwrap().contains("ket("),
            "the handler is still what converts it"
        );
    }

    #[test]
    fn symbols_typst_has_renamed_are_renamed_in_the_output() {
        assert_eq!(rename_symbols("a sect b"), "a inter b");
        assert_eq!(rename_symbols("sect.big"), "inter.big");
        assert_eq!(rename_symbols("angle.l x angle.r"), "chevron.l x chevron.r");
        assert_eq!(
            rename_symbols("angle"),
            "angle",
            "the bare angle is still an angle"
        );
        assert_eq!(
            rename_symbols("dissect"),
            "dissect",
            "only whole identifiers"
        );
        assert_eq!(
            rename_symbols("#textmath[the sect of]"),
            "#textmath[the sect of]",
            "words inside a content block are the author's text"
        );
        assert_eq!(
            rename_symbols(r"\[3 \] sect"),
            r"\[3 \] inter",
            "an escaped bracket opens nothing"
        );
        let conv = Converter::new(&Default::default());
        assert_eq!(conv.convert(r"A \cap B", false).unwrap(), "A  inter  B");
        assert_eq!(rename_symbols("tack.r.double.not"), "tack.rr.not");
        assert_eq!(
            rename_symbols("tack.r"),
            "tack.r",
            "only the deprecated spelling"
        );
    }
}
