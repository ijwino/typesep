use sep_core::model::MacroTable;
use typst_html::{HtmlDocument, HtmlElement, HtmlNode};

use crate::escape::escape_string;
use crate::fonts::Fonts;
use crate::math::Converter;
use crate::world::{MITEX_SCOPE_PATH, SepWorld};

const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";

const SCOPE_OVERRIDES: &str = r#"#let fs-html-align(..args) = if args.pos().len() > 0 { math.op(math.display(args.pos().first())) } else { math.zws }
#let fs-html-array(arg0: none, ..args) = {
  let rows = args.pos().map(row => if type(row) == array { row } else { (row,) })
  let width = calc.max(..rows.map(row => row.len()))
  let columns = if type(arg0) == str { arg0.replace(regex("[^lcr]"), "") } else { "" }
  let align-map = ("l": left, "c": center, "r": right)
  math.mat(
    delim: none,
    align: align-map.at(if columns.len() > 0 { columns.first() } else { "c" }),
    ..rows.map(row => row + (width - row.len()) * (none,)),
  )
}
#let fs-math-scope = fs-math-scope + (aligned: fs-html-align, alignedat: (arg0: none, ..args) => fs-html-align(..args), mitexarray: fs-html-array)
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formula {
    pub tex: String,
    pub display: bool,
}

#[derive(Debug, Default)]
pub struct Maths {
    pub items: Vec<Result<String, String>>,
    pub css: String,
    pub warnings: Vec<String>,
}

const EXPORT_NOTICE: &str = "html export is under active development and incomplete";

pub fn convert(formulas: &[Formula], macros: &MacroTable, fonts: &Fonts) -> Maths {
    if formulas.is_empty() {
        return Maths::default();
    }
    let converter = Converter::new(macros);
    let mut items: Vec<Result<String, String>> = Vec::with_capacity(formulas.len());
    let mut calls = Vec::new();
    for (index, formula) in formulas.iter().enumerate() {
        match converter.convert(&formula.tex, formula.display) {
            Ok(code) => {
                calls.push((
                    index,
                    format!(
                        "#fs-formula({index}, {}, block: {})\n",
                        escape_string(&code),
                        formula.display
                    ),
                ));
                items.push(Err(
                    "Typst's HTML export produced no MathML for it".to_owned()
                ));
            }
            Err(reason) => items.push(Err(reason)),
        }
    }

    let mut maths = Maths::default();
    typeset(&calls, fonts, &mut items, &mut maths);
    typst::comemo::evict(0);
    maths.warnings.sort();
    maths.warnings.dedup();
    maths.items = items;
    maths
}

fn typeset(
    calls: &[(usize, String)],
    fonts: &Fonts,
    items: &mut [Result<String, String>],
    maths: &mut Maths,
) {
    if calls.is_empty() {
        return;
    }
    let mut source = format!("#import \"{MITEX_SCOPE_PATH}\": mitex-scope\n");
    source.push_str(
        "#let fs-math-scope = mitex-scope + (mitexref: it => it, mitexcite: it => it, mitexlabel: it => none)\n",
    );
    source.push_str(SCOPE_OVERRIDES);
    source.push_str("#let fs-formula(n, src, block: false) = html.elem(\"div\", attrs: (id: \"fs-formula-\" + str(n)), math.equation(block: block, eval(\"$\" + src + \"$\", scope: fs-math-scope)))\n");
    for (_, call) in calls {
        source.push_str(call);
    }

    let world = SepWorld::new(source, &[], fonts).for_html();
    let warned = typst::compile::<HtmlDocument>(&world);
    let document = match warned.output {
        Ok(document) => document,
        Err(errors) => {
            if let [(index, _)] = calls {
                items[*index] = Err(errors
                    .first()
                    .map(|d| d.message.to_string())
                    .unwrap_or_else(|| "the maths could not be compiled".to_owned()));
            } else {
                let (head, tail) = calls.split_at(calls.len() / 2);
                typeset(head, fonts, items, maths);
                typeset(tail, fonts, items, maths);
            }
            return;
        }
    };
    maths.warnings.extend(
        warned
            .warnings
            .iter()
            .map(|d| d.message.to_string())
            .filter(|m| m != EXPORT_NOTICE)
            .map(|m| format!("maths: {m}")),
    );

    let mut found = std::collections::BTreeMap::new();
    collect_formulas(document.root_node(), &mut found);
    for (index, math) in found {
        items[index] = Ok(math);
    }
    if maths.css.is_empty() {
        maths.css = user_agent_css(document.root_node());
    }
}

fn collect_formulas(node: &HtmlNode, out: &mut std::collections::BTreeMap<usize, String>) {
    let HtmlNode::Element(element) = node else {
        return;
    };
    if let Some(index) = element
        .attrs
        .0
        .iter()
        .find(|(key, _)| key.resolve().as_str() == "id")
        .and_then(|(_, value)| value.strip_prefix("fs-formula-"))
        .and_then(|n| n.parse::<usize>().ok())
        && let Some(math) = first_math(element)
    {
        out.insert(index, write_xml(math, true));
        return;
    }
    for child in &element.children {
        collect_formulas(child, out);
    }
}

fn first_math(element: &HtmlElement) -> Option<&HtmlElement> {
    if element.tag.resolve().as_str() == "math" {
        return Some(element);
    }
    element.children.iter().find_map(|child| match child {
        HtmlNode::Element(child) => first_math(child),
        _ => None,
    })
}

fn user_agent_css(node: &HtmlNode) -> String {
    let HtmlNode::Element(element) = node else {
        return String::new();
    };
    if element.tag.resolve().as_str() == "style" {
        let css: String = element
            .children
            .iter()
            .filter_map(|child| match child {
                HtmlNode::Text(text, _) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        return css.replace(" i]", "]");
    }
    element
        .children
        .iter()
        .map(user_agent_css)
        .find(|css| !css.is_empty())
        .unwrap_or_default()
}

const TOKEN_TAGS: [&str; 7] = ["mi", "mn", "mo", "mtext", "ms", "mspace", "annotation"];
const STYLE_TAGS: [&str; 3] = ["math", "mstyle", "mtable"];
const CELL_TAGS: [&str; 2] = ["mtr", "mtd"];
const STYLE_ATTRS: [&str; 2] = ["displaystyle", "scriptlevel"];

fn escape_xml(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
}

fn write_xml(element: &HtmlElement, root: bool) -> String {
    let tag = element.tag.resolve();
    let tag = tag.as_str();
    let mut out = format!("<{tag}");
    if root {
        out.push_str(&format!(" xmlns=\"{MATHML_NS}\""));
    }
    let mut moved: Vec<(&'static str, &str)> = Vec::new();
    for (key, value) in &element.attrs.0 {
        let name = key.resolve();
        let name = name.as_str();
        if let Some(attr) = STYLE_ATTRS.iter().find(|a| **a == name).copied()
            && !STYLE_TAGS.contains(&tag)
        {
            if !CELL_TAGS.contains(&tag) {
                moved.push((attr, value));
            }
            continue;
        }
        out.push_str(&format!(" {name}=\""));
        escape_xml(value, &mut out);
        out.push('"');
    }
    let token = TOKEN_TAGS.contains(&tag);
    let mut children = String::new();
    for child in &element.children {
        match child {
            HtmlNode::Element(child) => children.push_str(&write_xml(child, false)),
            HtmlNode::Text(text, _) => {
                if token || text.trim().is_empty() {
                    escape_xml(text, &mut children);
                } else {
                    children.push_str("<mtext>");
                    escape_xml(text, &mut children);
                    children.push_str("</mtext>");
                }
            }
            HtmlNode::Tag(_) | HtmlNode::Frame(_) => {}
        }
    }
    if children.is_empty() {
        out.push_str("/>");
    } else {
        out.push('>');
        out.push_str(&children);
        out.push_str(&format!("</{tag}>"));
    }
    if moved.is_empty() {
        return out;
    }
    let mut wrapper = "<mstyle".to_owned();
    for (name, value) in moved {
        wrapper.push_str(&format!(" {name}=\""));
        escape_xml(value, &mut wrapper);
        wrapper.push('"');
    }
    format!("{wrapper}>{out}</mstyle>")
}
