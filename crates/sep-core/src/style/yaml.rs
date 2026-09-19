use std::collections::{HashMap, HashSet};

use saphyr_parser::{Event, Parser, ScalarStyle, Tag};

use super::value::{Map, Value, json_string};
use crate::parse::js::{self, lazy_regex};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct YamlError(pub String);

enum Frame {
    Seq {
        items: Vec<Value>,
        anchor: usize,
    },
    Map {
        map: Map,
        key: Option<String>,
        anchor: usize,
    },
}

struct Builder {
    stack: Vec<Frame>,
    root: Option<Value>,
    anchors: HashMap<usize, Value>,
}

impl Builder {
    fn place(
        &mut self,
        value: Value,
        anchor: usize,
        line: usize,
        col: usize,
    ) -> Result<(), YamlError> {
        if anchor != 0 {
            self.anchors.insert(anchor, value.clone());
        }
        match self.stack.last_mut() {
            None => {
                self.root = Some(value);
                Ok(())
            }
            Some(Frame::Seq { items, .. }) => {
                items.push(value);
                Ok(())
            }
            Some(Frame::Map { map, key, .. }) => match key.take() {
                Some(k) => {
                    map.insert(k, value);
                    Ok(())
                }
                None => {
                    let k = match &value {
                        Value::Null => String::new(),
                        Value::List(_) | Value::Map(_) => {
                            return Err(YamlError(format!(
                                "a collection cannot be a mapping key at line {line} column {col}"
                            )));
                        }
                        scalar => scalar.js_string(),
                    };
                    if map.contains_key(&k) {
                        return Err(YamlError(format!(
                            "Map keys must be unique at line {line} column {col}: \"{k}\""
                        )));
                    }
                    *key = Some(k);
                    Ok(())
                }
            },
        }
    }
}

pub fn parse(text: &str) -> Result<Value, YamlError> {
    let mut b = Builder {
        stack: Vec::new(),
        root: None,
        anchors: HashMap::new(),
    };
    let mut documents = 0;
    for item in Parser::new_from_str(text) {
        let (event, span) = item.map_err(|e| YamlError(e.to_string()))?;
        let (line, col) = (span.start.line(), span.start.col() + 1);
        match event {
            Event::Nothing | Event::StreamStart | Event::StreamEnd | Event::DocumentEnd => {}
            Event::DocumentStart(_) => {
                documents += 1;
                if documents > 1 {
                    return Err(YamlError(
                        "Source contains multiple documents; a style file holds one".to_owned(),
                    ));
                }
            }
            Event::Scalar(text, style, anchor, tag) => {
                let value = resolve_scalar(&text, style, tag.as_deref());
                b.place(value, anchor, line, col)?;
            }
            Event::SequenceStart(anchor, _) => b.stack.push(Frame::Seq {
                items: Vec::new(),
                anchor,
            }),
            Event::SequenceEnd => match b.stack.pop() {
                Some(Frame::Seq { items, anchor }) => {
                    b.place(Value::List(items), anchor, line, col)?
                }
                _ => return Err(YamlError("unbalanced sequence".to_owned())),
            },
            Event::MappingStart(anchor, _) => b.stack.push(Frame::Map {
                map: Map::new(),
                key: None,
                anchor,
            }),
            Event::MappingEnd => match b.stack.pop() {
                Some(Frame::Map {
                    map,
                    key: None,
                    anchor,
                    ..
                }) => {
                    b.place(Value::Map(map), anchor, line, col)?;
                }
                Some(Frame::Map { key: Some(k), .. }) => {
                    return Err(YamlError(format!("key \"{k}\" has no value")));
                }
                _ => return Err(YamlError("unbalanced mapping".to_owned())),
            },
            Event::Alias(id) => {
                let value = b.anchors.get(&id).cloned().ok_or_else(|| {
                    YamlError(format!(
                        "alias to an unknown anchor at line {line} column {col}"
                    ))
                })?;
                b.place(value, 0, line, col)?;
            }
        }
    }
    Ok(b.root.unwrap_or(Value::Null))
}

lazy_regex!(NULL_RE = r"^(?:~|[Nn]ull|NULL)?$");
lazy_regex!(BOOL_RE = r"^(?:[Tt]rue|TRUE|[Ff]alse|FALSE)$");
lazy_regex!(INT_OCT_RE = r"^0o[0-7]+$");
lazy_regex!(INT_RE = r"^[-+]?[0-9]+$");
lazy_regex!(INT_HEX_RE = r"^0x[0-9a-fA-F]+$");
lazy_regex!(FLOAT_NAN_RE = r"^(?:[-+]?\.(?:inf|Inf|INF)|\.nan|\.NaN|\.NAN)$");
lazy_regex!(FLOAT_EXP_RE = r"^[-+]?(?:\.[0-9]+|[0-9]+(?:\.[0-9]*)?)[eE][-+]?[0-9]+$");
lazy_regex!(FLOAT_RE = r"^[-+]?(?:\.[0-9]+|[0-9]+\.[0-9]*)$");

fn radix_number(digits: &str, radix: u32) -> f64 {
    digits.chars().fold(0f64, |acc, c| {
        acc * f64::from(radix) + f64::from(c.to_digit(radix).expect("the regex admits only digits"))
    })
}

fn core_scalar(text: &str) -> Value {
    if NULL_RE.is_match(text) {
        return Value::Null;
    }
    if BOOL_RE.is_match(text) {
        return Value::Bool(text.starts_with(['t', 'T']));
    }
    if INT_OCT_RE.is_match(text) {
        return Value::Number(radix_number(&text[2..], 8));
    }
    if INT_RE.is_match(text) {
        return Value::Number(text.parse().expect("decimal digits read as a double"));
    }
    if INT_HEX_RE.is_match(text) {
        return Value::Number(radix_number(&text[2..], 16));
    }
    if FLOAT_NAN_RE.is_match(text) {
        return Value::Number(if text.ends_with(['n', 'N']) {
            f64::NAN
        } else if text.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if FLOAT_EXP_RE.is_match(text) || FLOAT_RE.is_match(text) {
        return Value::Number(text.parse().expect("the regex admits only a decimal"));
    }
    Value::String(text.to_owned())
}

fn resolve_scalar(text: &str, style: ScalarStyle, tag: Option<&Tag>) -> Value {
    if let Some(tag) = tag {
        if tag.is_yaml_core_schema() {
            let resolved = core_scalar(text);
            let fits = match tag.suffix.as_str() {
                "str" => return Value::String(text.to_owned()),
                "null" => matches!(resolved, Value::Null),
                "bool" => matches!(resolved, Value::Bool(_)),
                "int" | "float" => matches!(resolved, Value::Number(_)),
                _ => false,
            };
            return if fits {
                resolved
            } else {
                Value::String(text.to_owned())
            };
        }
        return Value::String(text.to_owned());
    }
    match style {
        ScalarStyle::Plain => core_scalar(text),
        _ => Value::String(text.to_owned()),
    }
}

const LINE_WIDTH: usize = 80;
const MIN_CONTENT_WIDTH: usize = 20;
const INDENT_STEP: &str = "  ";
const DOUBLE_QUOTED_MIN_MULTI_LINE_LENGTH: usize = 40;

const NL: u16 = b'\n' as u16;
const SP: u16 = b' ' as u16;
const TAB: u16 = b'\t' as u16;
const BS: u16 = b'\\' as u16;
const DQ: u16 = b'"' as u16;
const SQ: u16 = b'\'' as u16;

fn u16s(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn from_u16s(units: &[u16]) -> String {
    String::from_utf16(units).expect("the emitter never splits a surrogate pair")
}

fn is(c: Option<&u16>, ch: u8) -> bool {
    c == Some(&(ch as u16))
}

pub fn stringify(value: &Value) -> String {
    let mut out = stringify_node(value, "", None);
    out.push('\n');
    out
}

fn number_text(n: f64) -> String {
    if n.is_nan() {
        ".nan".to_owned()
    } else if n.is_infinite() {
        if n < 0.0 { "-.inf" } else { ".inf" }.to_owned()
    } else if n == 0.0 && n.is_sign_negative() {
        "-0".to_owned()
    } else {
        js::number_to_string(n)
    }
}

fn stringify_node(value: &Value, indent: &str, indent_at_start: Option<usize>) -> String {
    match value {
        Value::Map(map) if !map.is_empty() => block_map(map, indent),
        Value::Map(_) => "{}".to_owned(),
        Value::List(items) if !items.is_empty() => block_seq(items, indent),
        Value::List(_) => "[]".to_owned(),
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number_text(*n),
        Value::String(s) => stringify_string(s, indent, false, indent_at_start),
    }
}

fn block_map(map: &Map, indent: &str) -> String {
    map.iter()
        .map(|(k, v)| pair(k, v, indent))
        .collect::<Vec<_>>()
        .join(&format!("\n{indent}"))
}

fn block_seq(items: &[Value], indent: &str) -> String {
    let item_indent = format!("{indent}{INDENT_STEP}");
    items
        .iter()
        .map(|item| format!("- {}", stringify_node(item, &item_indent, None)))
        .collect::<Vec<_>>()
        .join(&format!("\n{indent}"))
}

fn pair(key: &str, value: &Value, indent: &str) -> String {
    let nested = format!("{indent}{INDENT_STEP}");
    let key_text = stringify_string(key, &nested, true, None);
    let key_width = key_text.encode_utf16().count();
    let value_text = match value {
        Value::Map(m) if !m.is_empty() => format!("\n{nested}{}", block_map(m, &nested)),
        Value::List(l) if !l.is_empty() => format!("\n{nested}{}", block_seq(l, &nested)),
        Value::Map(_) => " {}".to_owned(),
        Value::List(_) => " []".to_owned(),
        scalar => {
            let text = stringify_node(scalar, &nested, Some(key_width + 2));
            if text.is_empty() || text.starts_with('\n') {
                text
            } else {
                format!(" {text}")
            }
        }
    };
    format!("{key_text}:{value_text}")
}

struct Ctx<'a> {
    indent: &'a str,
    implicit_key: bool,
    indent_at_start: Option<usize>,
}

fn stringify_string(
    value: &str,
    indent: &str,
    implicit_key: bool,
    indent_at_start: Option<usize>,
) -> String {
    let ctx = Ctx {
        indent,
        implicit_key,
        indent_at_start,
    };
    let v = u16s(value);
    if v.iter()
        .any(|&c| matches!(c, 0x00..=0x08 | 0x0b..=0x1f | 0x7f..=0x9f))
    {
        return from_u16s(&double_quoted(&v, &ctx));
    }
    from_u16s(&plain_string(&v, &ctx))
}

fn contains_document_marker(v: &[u16]) -> bool {
    let text = from_u16s(v);
    text.lines()
        .any(|l| l.starts_with('%') || l.starts_with("---") || l.starts_with("..."))
}

fn not_plain(v: &[u16]) -> bool {
    let Some(&first) = v.first() else {
        return false;
    };
    let indicator = b"\n\t ,[]{}#&*!|>'\"%@`";
    if indicator.iter().any(|&c| c as u16 == first) {
        return true;
    }
    let qm = b'?' as u16;
    let dash = b'-' as u16;
    if (first == qm || first == dash)
        && (v.len() == 1 || v.get(1) == Some(&SP) || v.get(1) == Some(&TAB))
    {
        return true;
    }
    for w in v.windows(2) {
        let (a, b) = (w[0], w[1]);
        let hash = b'#' as u16;
        let colon = b':' as u16;
        if (a == NL || a == colon) && (b == SP || b == TAB) {
            return true;
        }
        if (a == SP || a == TAB) && b == NL {
            return true;
        }
        if (a == NL || a == TAB || a == SP) && b == hash {
            return true;
        }
    }
    let last = *v.last().expect("non-empty");
    last == NL || last == TAB || last == SP || last == b':' as u16
}

fn reads_as_another_type(v: &[u16]) -> bool {
    let text = from_u16s(v);
    [
        &*NULL_RE,
        &*BOOL_RE,
        &*INT_OCT_RE,
        &*INT_RE,
        &*INT_HEX_RE,
        &*FLOAT_NAN_RE,
        &*FLOAT_EXP_RE,
        &*FLOAT_RE,
    ]
    .iter()
    .any(|re| re.is_match(&text))
}

fn plain_string(v: &[u16], ctx: &Ctx) -> Vec<u16> {
    let has_newline = v.contains(&NL);
    if ctx.implicit_key && has_newline {
        return quoted(v, ctx);
    }
    if not_plain(v) {
        return if ctx.implicit_key || !has_newline {
            quoted(v, ctx)
        } else {
            block_string(v, ctx)
        };
    }
    if !ctx.implicit_key && has_newline {
        return block_string(v, ctx);
    }
    if contains_document_marker(v) && ctx.implicit_key && ctx.indent == INDENT_STEP {
        return quoted(v, ctx);
    }
    if reads_as_another_type(v) {
        return quoted(v, ctx);
    }
    if ctx.implicit_key {
        v.to_vec()
    } else {
        fold_flow_lines(v, ctx.indent, Fold::Flow, ctx.indent_at_start, None)
    }
}

fn quoted(v: &[u16], ctx: &Ctx) -> Vec<u16> {
    let has_double = v.contains(&DQ);
    let has_single = v.contains(&SQ);
    if has_double && !has_single {
        single_quoted(v, ctx)
    } else {
        double_quoted(v, ctx)
    }
}

fn single_quoted(v: &[u16], ctx: &Ctx) -> Vec<u16> {
    let has_newline = v.contains(&NL);
    let space_at_newline = v.windows(2).any(|w| {
        ((w[0] == SP || w[0] == TAB) && w[1] == NL) || (w[0] == NL && (w[1] == SP || w[1] == TAB))
    });
    if (ctx.implicit_key && has_newline) || space_at_newline {
        return double_quoted(v, ctx);
    }
    let indent = u16s(ctx.indent);
    let mut res = vec![SQ];
    let mut i = 0;
    while i < v.len() {
        let c = v[i];
        if c == SQ {
            res.extend([SQ, SQ]);
            i += 1;
        } else if c == NL {
            let mut j = i;
            while j < v.len() && v[j] == NL {
                j += 1;
            }
            res.extend(&v[i..j]);
            res.push(NL);
            res.extend(&indent);
            i = j;
        } else {
            res.push(c);
            i += 1;
        }
    }
    res.push(SQ);
    if ctx.implicit_key {
        res
    } else {
        fold_flow_lines(&res, ctx.indent, Fold::Flow, ctx.indent_at_start, None)
    }
}

fn double_quoted(v: &[u16], ctx: &Ctx) -> Vec<u16> {
    let json = u16s(&json_string(&from_u16s(v)));
    let indent = u16s(ctx.indent);
    let mut out: Vec<u16> = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < json.len() {
        let mut ch = json[i];
        if ch == SP && is(json.get(i + 1), b'\\') && is(json.get(i + 2), b'n') {
            out.extend(&json[start..i]);
            out.extend(u16s("\\ "));
            i += 1;
            start = i;
            ch = BS;
        }
        if ch == BS {
            match json.get(i + 1).map(|&c| c as u8) {
                Some(b'u') => {
                    out.extend(&json[start..i]);
                    let code = from_u16s(&json[i + 2..i + 6]);
                    let short = match code.as_str() {
                        "0000" => Some("\\0"),
                        "0007" => Some("\\a"),
                        "000b" => Some("\\v"),
                        "001b" => Some("\\e"),
                        "0085" => Some("\\N"),
                        "00a0" => Some("\\_"),
                        "2028" => Some("\\L"),
                        "2029" => Some("\\P"),
                        _ => None,
                    };
                    match short {
                        Some(s) => out.extend(u16s(s)),
                        None if code.starts_with("00") => {
                            out.extend(u16s(&format!("\\x{}", &code[2..])))
                        }
                        None => out.extend(&json[i..i + 6]),
                    }
                    i += 5;
                    start = i + 1;
                }
                Some(b'n') => {
                    if ctx.implicit_key
                        || is(json.get(i + 2), b'"')
                        || json.len() < DOUBLE_QUOTED_MIN_MULTI_LINE_LENGTH
                    {
                        i += 1;
                    } else {
                        out.extend(&json[start..i]);
                        out.extend([NL, NL]);
                        while is(json.get(i + 2), b'\\')
                            && is(json.get(i + 3), b'n')
                            && !is(json.get(i + 4), b'"')
                        {
                            out.push(NL);
                            i += 2;
                        }
                        out.extend(&indent);
                        if is(json.get(i + 2), b' ') {
                            out.push(BS);
                        }
                        i += 1;
                        start = i + 1;
                    }
                }
                _ => i += 1,
            }
        }
        i += 1;
    }
    let text = if start > 0 {
        out.extend(&json[start.min(json.len())..]);
        out
    } else {
        json
    };
    if ctx.implicit_key {
        text
    } else {
        fold_flow_lines(&text, ctx.indent, Fold::Quoted, ctx.indent_at_start, None)
    }
}

fn line_length_over_limit(v: &[u16], indent_len: usize) -> bool {
    let limit = LINE_WIDTH - indent_len;
    if v.len() <= limit {
        return false;
    }
    let mut start = 0;
    for (i, &c) in v.iter().enumerate() {
        if c == NL {
            if i - start > limit {
                return true;
            }
            start = i + 1;
            if v.len() - start <= limit {
                return false;
            }
        }
    }
    true
}

fn indent_after_newlines(v: &[u16], indent: &[u16], not_at_end: bool) -> Vec<u16> {
    let mut out = Vec::with_capacity(v.len());
    let mut i = 0;
    while i < v.len() {
        if v[i] == NL {
            let mut j = i;
            while j < v.len() && v[j] == NL {
                j += 1;
            }
            out.extend(&v[i..j]);
            if !(not_at_end && j == v.len()) {
                out.extend(indent);
            }
            i = j;
        } else {
            out.push(v[i]);
            i += 1;
        }
    }
    out
}

fn keep_more_indented_lines(t: &[u16]) -> Vec<u16> {
    let is_ws = |c: u16| c == NL || c == TAB || c == SP;
    let is_blank = |c: u16| c == TAB || c == SP;
    let mut out = Vec::with_capacity(t.len());
    let mut pos = 0;
    let mut m = 0;
    while m < t.len() {
        let line_start = if m == 0 && is_blank(t[0]) {
            Some(0)
        } else if t[m] == NL && t.get(m + 1).is_some_and(|&c| is_blank(c)) {
            Some(m + 1)
        } else {
            None
        };
        let Some(ls) = line_start else {
            m += 1;
            continue;
        };
        let le = (ls..t.len()).find(|&k| t[k] == NL).unwrap_or(t.len());
        let re = (le..t.len()).find(|&k| !is_ws(t[k])).unwrap_or(t.len());
        out.extend(&t[pos..m]);
        out.extend(&t[ls..le]);
        let end = if re > le && t[re - 1] == NL {
            out.extend(&t[le..re - 1]);
            re
        } else {
            le
        };
        pos = end;
        m = end;
    }
    out.extend(&t[pos..]);
    out
}

fn block_string(v: &[u16], ctx: &Ctx) -> Vec<u16> {
    if let Some(last_nl) = v.iter().rposition(|&c| c == NL)
        && last_nl + 1 < v.len()
        && v[last_nl + 1..].iter().all(|&c| c == TAB || c == SP)
    {
        return quoted(v, ctx);
    }
    let indent_text = ctx.indent;
    let indent = u16s(indent_text);
    let literal = !line_length_over_limit(v, indent.len());
    if v.is_empty() {
        return u16s(if literal { "|\n" } else { ">\n" });
    }
    let mut value = v.to_vec();
    let end_start = value
        .iter()
        .rposition(|&c| c != NL && c != TAB && c != SP)
        .map_or(0, |p| p + 1);
    let mut end: Vec<u16> = value[end_start..].to_vec();
    let chomp = match end.iter().position(|&c| c == NL) {
        None => "-",
        Some(p) if end_start == 0 || p != end.len() - 1 => "+",
        Some(_) => "",
    };
    if !end.is_empty() {
        value.truncate(end_start);
        if end.last() == Some(&NL) {
            end.pop();
        }
        end = indent_after_newlines(&end, &indent, true);
    }
    let mut start_with_space = false;
    let mut start_nl_pos: Option<usize> = None;
    let mut start_end = 0;
    while start_end < value.len() {
        match value[start_end] {
            SP => start_with_space = true,
            NL => start_nl_pos = Some(start_end),
            _ => break,
        }
        start_end += 1;
    }
    let start_len = start_nl_pos.map_or(0, |p| p + 1);
    let mut start: Vec<u16> = value[..start_len].to_vec();
    if !start.is_empty() {
        value.drain(..start_len);
        start = indent_after_newlines(&start, &indent, false);
    }
    let indent_size = if indent.is_empty() { "1" } else { "2" };
    let header = format!("{}{chomp}", if start_with_space { indent_size } else { "" });
    if !literal {
        let mut doubled = Vec::with_capacity(value.len());
        let mut i = 0;
        while i < value.len() {
            if value[i] == NL {
                let mut j = i;
                while j < value.len() && value[j] == NL {
                    j += 1;
                }
                doubled.push(NL);
                doubled.extend(&value[i..j]);
                i = j;
            } else {
                doubled.push(value[i]);
                i += 1;
            }
        }
        let folded = indent_after_newlines(&keep_more_indented_lines(&doubled), &indent, false);
        let mut text = start.clone();
        text.extend(&folded);
        text.extend(&end);
        let mut overflow = false;
        let body = fold_flow_lines(
            &text,
            indent_text,
            Fold::Block,
            Some(indent.len()),
            Some(&mut overflow),
        );
        if !overflow {
            let mut out = u16s(&format!(">{header}\n{indent_text}"));
            out.extend(body);
            return out;
        }
    }
    let value = indent_after_newlines(&value, &indent, false);
    let mut out = u16s(&format!("|{header}\n{indent_text}"));
    out.extend(start);
    out.extend(value);
    out.extend(end);
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fold {
    Flow,
    Block,
    Quoted,
}

fn consume_more_indented_lines(text: &[u16], i: i64, indent: usize) -> i64 {
    let at = |k: i64| -> Option<u16> { usize::try_from(k).ok().and_then(|k| text.get(k).copied()) };
    let mut i = i;
    let mut end = i;
    let mut start = i + 1;
    let mut ch = at(start);
    while ch == Some(SP) || ch == Some(TAB) {
        if i < start + indent as i64 {
            i += 1;
            ch = at(i);
        } else {
            loop {
                i += 1;
                ch = at(i);
                if ch.is_none() || ch == Some(NL) {
                    break;
                }
            }
            end = i;
            start = i + 1;
            ch = at(start);
        }
    }
    end
}

fn fold_flow_lines(
    text: &[u16],
    indent: &str,
    mode: Fold,
    indent_at_start: Option<usize>,
    on_overflow: Option<&mut bool>,
) -> Vec<u16> {
    let indent_len = indent.encode_utf16().count();
    let end_step = std::cmp::max(1 + MIN_CONTENT_WIDTH, 1 + LINE_WIDTH - indent_len) as i64;
    if text.len() as i64 <= end_step {
        return text.to_vec();
    }
    let at = |k: i64| -> Option<u16> { usize::try_from(k).ok().and_then(|k| text.get(k).copied()) };
    let mut folds: Vec<i64> = Vec::new();
    let mut escaped_folds: HashSet<i64> = HashSet::new();
    let mut end = (LINE_WIDTH - indent_len) as i64;
    if let Some(ias) = indent_at_start {
        if ias > LINE_WIDTH - std::cmp::max(2, MIN_CONTENT_WIDTH) {
            folds.push(0);
        } else {
            end = (LINE_WIDTH - ias) as i64;
        }
    }
    let mut split: Option<i64> = None;
    let mut prev: Option<u16> = None;
    let mut overflow = false;
    let mut i: i64 = -1;
    let mut esc_start: i64 = -1;
    let mut esc_end: i64 = -1;
    if mode == Fold::Block {
        i = consume_more_indented_lines(text, i, indent_len);
        if i != -1 {
            end = i + end_step;
        }
    }
    loop {
        i += 1;
        let Some(mut ch) = at(i) else { break };
        if mode == Fold::Quoted && ch == BS {
            esc_start = i;
            i += match at(i + 1).map(|c| c as u8) {
                Some(b'x') => 3,
                Some(b'u') => 5,
                Some(b'U') => 9,
                _ => 1,
            };
            esc_end = i;
        }
        if ch == NL {
            if mode == Fold::Block {
                i = consume_more_indented_lines(text, i, indent_len);
            }
            end = i + indent_len as i64 + end_step;
            split = None;
        } else {
            if ch == SP
                && prev.is_some_and(|p| p != SP && p != NL && p != TAB)
                && at(i + 1).is_some_and(|n| n != SP && n != NL && n != TAB)
            {
                split = Some(i);
            }
            if i >= end {
                if let Some(s) = split {
                    folds.push(s);
                    end = s + end_step;
                    split = None;
                } else if mode == Fold::Quoted {
                    while prev == Some(SP) || prev == Some(TAB) {
                        prev = Some(ch);
                        i += 1;
                        ch = at(i).unwrap_or(0);
                        overflow = true;
                    }
                    let j = if i > esc_end + 1 {
                        i - 2
                    } else {
                        esc_start - 1
                    };
                    if escaped_folds.contains(&j) {
                        return text.to_vec();
                    }
                    folds.push(j);
                    escaped_folds.insert(j);
                    end = j + end_step;
                    split = None;
                } else {
                    overflow = true;
                }
            }
        }
        prev = Some(ch);
    }
    if overflow && let Some(flag) = on_overflow {
        *flag = true;
    }
    if folds.is_empty() {
        return text.to_vec();
    }
    let indent = u16s(indent);
    let slice = |from: i64, to: i64| -> &[u16] {
        let from = from.clamp(0, text.len() as i64) as usize;
        let to = to.clamp(0, text.len() as i64) as usize;
        &text[from..to.max(from)]
    };
    let mut res: Vec<u16> = slice(0, folds[0]).to_vec();
    for (k, &fold) in folds.iter().enumerate() {
        let end = folds.get(k + 1).copied().unwrap_or(text.len() as i64);
        if fold == 0 {
            res = vec![NL];
            res.extend(&indent);
            res.extend(slice(0, end));
        } else {
            if mode == Fold::Quoted && escaped_folds.contains(&fold) {
                res.push(text[fold as usize]);
                res.push(BS);
            }
            res.push(NL);
            res.extend(&indent);
            res.extend(slice(fold + 1, end));
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(x: f64) -> Value {
        Value::Number(x)
    }
    fn s(x: &str) -> Value {
        Value::str(x)
    }
    fn one(text: &str) -> Value {
        let doc = parse(&format!("a: {text}")).unwrap();
        doc.as_map().unwrap().get("a").unwrap().clone()
    }

    #[test]
    fn plain_scalars_follow_the_core_schema_as_yaml_2_does() {
        assert_eq!(one("yes"), s("yes"));
        assert_eq!(one("on"), s("on"));
        assert_eq!(one("1_000"), s("1_000"));
        assert_eq!(one("0o17"), n(15.0));
        assert_eq!(one("0x1F"), n(31.0));
        assert_eq!(one("0b11"), s("0b11"));
        assert_eq!(one("~"), Value::Null);
        assert_eq!(one(""), Value::Null);
        assert_eq!(one("null"), Value::Null);
        assert_eq!(one("Null"), Value::Null);
        assert_eq!(one(".inf"), n(f64::INFINITY));
        assert_eq!(one("-.inf"), n(f64::NEG_INFINITY));
        assert!(one(".nan").as_f64().unwrap().is_nan());
        assert_eq!(one("1."), n(1.0));
        assert_eq!(one("+1"), n(1.0));
        assert_eq!(one("00"), n(0.0));
        assert_eq!(one("007"), n(7.0));
        assert_eq!(one("1e3"), n(1000.0));
        assert_eq!(one("1.50"), n(1.5));
        assert_eq!(one(".5"), n(0.5));
        assert_eq!(one("1.5e2"), n(150.0));
        assert_eq!(one("1_0.5"), s("1_0.5"));
        assert_eq!(one("12345678901234567890"), n(12345678901234567000.0));
        assert_eq!(one("True"), Value::Bool(true));
        assert_eq!(one("TRUE"), Value::Bool(true));
        assert_eq!(one("tRue"), s("tRue"));
        assert_eq!(one("\"1\""), s("1"));
        assert_eq!(one("'yes'"), s("yes"));
        assert_eq!(one("!!str 1"), s("1"));
        assert_eq!(one("!!int \"12\""), n(12.0));
        assert_eq!(one("!!int abc"), s("abc"));
        assert_eq!(one("!local x"), s("x"));
        assert_eq!(one("10pt"), s("10pt"));
        assert_eq!(one("5.5inx8.5in"), s("5.5inx8.5in"));
        assert_eq!(one("'{page}'"), s("{page}"));
        assert_eq!(one("[1, two]"), Value::List(vec![n(1.0), s("two")]));
        let m = one("{b: 1, c: {d: e}}");
        assert_eq!(m.get_path("b"), Some(&n(1.0)));
        assert_eq!(m.get_path("c.d"), Some(&s("e")));
    }

    #[test]
    fn documents_keys_and_anchors() {
        assert_eq!(parse("").unwrap(), Value::Null);
        assert_eq!(parse("# comment\n").unwrap(), Value::Null);
        let doc = parse("b: 1\na: 2\n1: x\n").unwrap();
        let keys: Vec<&str> = doc.as_map().unwrap().iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["b", "a", "1"], "insertion order, keys as strings");
        let err = parse("a: 1\na: 2\n").unwrap_err();
        assert!(
            err.0
                .starts_with("Map keys must be unique at line 2 column 1"),
            "{err}"
        );
        let err = parse("a: 1\n---\nb: 2\n").unwrap_err();
        assert!(err.0.contains("multiple documents"), "{err}");
        let doc = parse("base: &b {x: 1}\ncopy: *b\n").unwrap();
        assert_eq!(doc.get_path("copy.x"), Some(&n(1.0)));
        assert!(parse("a: [1, 2").is_err());
        let json = parse("{\"page\": {\"size\": \"a4\", \"columns\": 2}}").unwrap();
        assert_eq!(json.get_path("page.columns"), Some(&n(2.0)));
        assert_eq!(parse("a:\n").unwrap().get_path("a"), Some(&Value::Null));
        assert_eq!(
            parse("- a\n- b\n").unwrap(),
            Value::List(vec![s("a"), s("b")])
        );
    }

    fn doc(pairs: &[(&str, Value)]) -> String {
        let mut m = Map::new();
        for (k, v) in pairs {
            m.insert(*k, v.clone());
        }
        stringify(&Value::Map(m))
    }

    #[test]
    fn scalars_print_as_yaml_2_prints_them() {
        let cases: &[(Value, &str)] = &[
            (s("a5"), "a5"),
            (s(""), "\"\""),
            (s("{page}"), "\"{page}\""),
            (s("{page} / {pages}"), "\"{page} / {pages}\""),
            (s("#334455"), "\"#334455\""),
            (s("Contents: a list"), "\"Contents: a list\""),
            (s("a #b"), "\"a #b\""),
            (s("1"), "\"1\""),
            (s("1e3"), "\"1e3\""),
            (s("0x1F"), "\"0x1F\""),
            (s("true"), "\"true\""),
            (s("null"), "\"null\""),
            (s("~"), "\"~\""),
            (s("yes"), "yes"),
            (s("Roman"), "Roman"),
            (s(" lead"), "\" lead\""),
            (s("-x"), "-x"),
            (s("-"), "\"-\""),
            (s("- x"), "\"- x\""),
            (s("it's single"), "it's single"),
            (s("has \"double\" quotes"), "has \"double\" quotes"),
            (s("both ' and \""), "both ' and \""),
            (s("a: "), "\"a: \""),
            (s("a\r\nb"), "\"a\\r\\nb\""),
            (s("\\textbullet"), "\\textbullet"),
            (s("a\tb"), "a\tb"),
            (s("tab\u{1}"), "\"tab\\x01\""),
            (s("nul\u{0}"), "\"nul\\0\""),
            (s("x:"), "\"x:\""),
            (s("x: y"), "\"x: y\""),
            (s("trailing "), "\"trailing \""),
            (s("한글 제목"), "한글 제목"),
            (n(1.0), "1"),
            (n(0.95), "0.95"),
            (n(-0.0), "-0"),
            (n(f64::INFINITY), ".inf"),
            (n(f64::NAN), ".nan"),
            (Value::Bool(false), "false"),
            (Value::Null, "null"),
            (Value::List(vec![]), "[]"),
            (Value::map(), "{}"),
        ];
        for (value, expected) in cases {
            assert_eq!(
                doc(&[("k", value.clone())]),
                format!("k: {expected}\n"),
                "{value:?}"
            );
        }
    }

    #[test]
    fn long_and_multiline_strings_fold_and_block_as_yaml_2_does() {
        let long = "\\usepackage{foo} % a rather long comment that goes on and on and on past eighty characters wide, and then some more words to fold twice over";
        let mut advanced = Map::new();
        advanced.insert("preamble", s(long));
        let mut hooks = Map::new();
        hooks.insert("beforeBody", s("line one\nline two"));
        hooks.insert("afterBody", s("has \"double\" quotes"));
        advanced.insert("hooks", Value::Map(hooks));
        assert_eq!(
            doc(&[("advanced", Value::Map(advanced))]),
            "advanced:\n  preamble: \\usepackage{foo} % a rather long comment that goes on and on and on\n    past eighty characters wide, and then some more words to fold twice over\n  hooks:\n    beforeBody: |-\n      line one\n      line two\n    afterBody: has \"double\" quotes\n"
        );
        assert_eq!(doc(&[("k", s("a\n\nb\n"))]), "k: |\n  a\n\n  b\n");
        assert_eq!(doc(&[("k", s("a\nb\n\n"))]), "k: |+\n  a\n  b\n\n");
        assert_eq!(
            doc(&[("k", s("  lead\nnext"))]),
            "k: |2-\n    lead\n  next\n"
        );
        assert_eq!(doc(&[("k", s("a\n  b\nc"))]), "k: |-\n  a\n    b\n  c\n");
        assert_eq!(doc(&[("k", s("x \ny"))]), "k: |-\n  x \n  y\n");
        assert_eq!(
            doc(&[(
                "k",
                s(
                    "long quoted with a newline\nand a second line that is long enough to force the folder to fold the text here"
                )
            )]),
            "k: >-\n  long quoted with a newline\n\n  and a second line that is long enough to force the folder to fold the text\n  here\n"
        );
        assert_eq!(doc(&[("k", s("a\n "))]), "k: \"a\\n \"\n");
        assert_eq!(doc(&[("k", s("- a\nb"))]), "k: |-\n  - a\n  b\n");
        let wide = "one line that is long enough to go past the eighty character limit of the folder x\nshort";
        assert_eq!(
            doc(&[("k", s(wide))]),
            "k: >-\n  one line that is long enough to go past the eighty character limit of the\n  folder x\n\n  short\n"
        );
        let wide_indented = "one line that is long enough to go past the eighty character limit of the folder x\n  more\nlast";
        assert_eq!(
            doc(&[("k", s(wide_indented))]),
            "k: >-\n  one line that is long enough to go past the eighty character limit of the\n  folder x\n    more\n  last\n"
        );
        let words = "word ".repeat(40);
        assert_eq!(
            doc(&[("k", s(words.trim_end()))]),
            format!("k: {}\n", {
                let w: Vec<&str> = vec!["word"; 40];
                let mut lines = Vec::new();
                let mut line = String::new();
                let mut budget = 80 - 3;
                for word in w {
                    if !line.is_empty() && line.len() + 1 + word.len() > budget {
                        lines.push(std::mem::take(&mut line));
                        budget = 80 - 2;
                    }
                    if !line.is_empty() {
                        line.push(' ');
                    }
                    line.push_str(word);
                }
                lines.push(line);
                lines.join("\n  ")
            })
        );
        let long_quoted =
            "a string with a colon: here that is long enough to need folding across the line";
        assert_eq!(
            doc(&[("k", s(long_quoted))]),
            "k: \"a string with a colon: here that is long enough to need folding across the\n  line\"\n"
        );
        let long_key = "k".repeat(70);
        assert_eq!(doc(&[(&long_key, s("v"))]), format!("{long_key}: v\n"));
        assert_eq!(
            doc(&[(
                &long_key,
                s("a value that would have to wrap because the key is long")
            )]),
            format!("{long_key}: a value that would have to wrap because the key is long\n"),
            "a value shorter than the step is never folded, however long the key"
        );
    }

    #[test]
    fn collections_lay_out_as_yaml_2_does() {
        let mut fonts = Map::new();
        let mut main = Map::new();
        main.insert("family", s("TeX Gyre Termes"));
        let mut features = Map::new();
        features.insert("Ligatures", s("TeX"));
        main.insert("features", Value::Map(features));
        fonts.insert("main", Value::Map(main));
        fonts.insert(
            "fallback",
            Value::List(vec![s("Noto Serif"), s("Noto Sans Symbols")]),
        );
        fonts.insert("empty", Value::List(vec![]));
        fonts.insert("none", Value::map());
        fonts.insert(
            "nested",
            Value::List(vec![
                Value::List(vec![n(1.0), n(2.0)]),
                Value::Map({
                    let mut m = Map::new();
                    m.insert("a", n(1.0));
                    m.insert("b", s("two"));
                    m
                }),
            ]),
        );
        assert_eq!(
            doc(&[("fonts", Value::Map(fonts))]),
            "fonts:\n  main:\n    family: TeX Gyre Termes\n    features:\n      Ligatures: TeX\n  fallback:\n    - Noto Serif\n    - Noto Sans Symbols\n  empty: []\n  none: {}\n  nested:\n    - - 1\n      - 2\n    - a: 1\n      b: two\n"
        );
        assert_eq!(
            doc(&[("1", s("x")), ("a b", s("y")), ("c: d", s("z"))]),
            "\"1\": x\na b: y\n\"c: d\": z\n"
        );
    }
}
