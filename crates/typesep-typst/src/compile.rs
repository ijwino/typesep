use typst::WorldExt;
use typst::diag::{Severity, SourceDiagnostic};
use typst::foundations::{Label, Selector, Value};
use typst::introspection::Introspector;
use typst::utils::PicoStr;
use typst_layout::PagedDocument;
use typst_pdf::{PdfOptions, Timestamp};

use crate::world::TypesetWorld;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub lines: Vec<String>,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.lines.join("\n"))
    }
}

impl std::error::Error for CompileError {}

pub struct Compiled {
    pub pdf: Vec<u8>,
    pub pages: usize,
    pub warnings: Vec<String>,
    pub moved_notes: Vec<MovedNote>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MovedNote {
    pub entry: usize,
    pub number: i64,
    pub heights: f64,
}

impl std::fmt::Display for MovedNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "note {} is {} text heights and is set at the end of the entry",
            self.number, self.heights
        )
    }
}

fn moved_notes(document: &PagedDocument) -> Vec<MovedNote> {
    let Some(label) = Label::new(PicoStr::intern("fs-moved-note")) else {
        return Vec::new();
    };
    document
        .introspector()
        .query(&Selector::Label(label))
        .iter()
        .filter_map(|content| {
            let Ok(Value::Dict(dict)) = content.get_by_name("value") else {
                return None;
            };
            let number = match dict.get("number") {
                Ok(Value::Int(n)) => *n,
                _ => return None,
            };
            let entry = match dict.get("entry") {
                Ok(Value::Int(n)) => (*n).max(0) as usize,
                _ => 0,
            };
            let heights = match dict.get("heights") {
                Ok(Value::Float(h)) => *h,
                Ok(Value::Int(h)) => *h as f64,
                _ => 0.0,
            };
            Some(MovedNote {
                entry,
                number,
                heights,
            })
        })
        .collect()
}

fn format_diagnostic(world: &TypesetWorld, diag: &SourceDiagnostic) -> String {
    let mut text = diag.message.to_string();
    if let Some(range) = world.range(diag.span) {
        let source = world.main_source();
        if diag.span.id() == Some(source.id())
            && let Some((line, column)) = source.lines().byte_to_line_column(range.start)
        {
            text.push_str(&format!(" (main.typ:{}:{})", line + 1, column + 1));
        }
    }
    for hint in &diag.hints {
        text.push_str(&format!("\n  hint: {}", hint.v));
    }
    text
}

pub fn typesets(world: &TypesetWorld) -> bool {
    typst::compile::<PagedDocument>(world).output.is_ok()
}

pub fn compile_pdf(world: &TypesetWorld, creator: &str) -> Result<Compiled, CompileError> {
    let warned = typst::compile::<PagedDocument>(world);
    let warnings: Vec<String> = warned
        .warnings
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .map(|d| format_diagnostic(world, d))
        .collect();
    let document = warned.output.map_err(|errors| CompileError {
        lines: errors.iter().map(|d| format_diagnostic(world, d)).collect(),
    })?;

    let now = time::OffsetDateTime::now_utc();
    let timestamp = typst::foundations::Datetime::from_ymd_hms(
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
    )
    .map(Timestamp::new_utc);
    let options = PdfOptions {
        creator: typst::foundations::Smart::Custom(Some(creator.to_owned())),
        timestamp,
        ..PdfOptions::default()
    };
    let pdf = typst_pdf::pdf(&document, &options).map_err(|errors| CompileError {
        lines: errors.iter().map(|d| format_diagnostic(world, d)).collect(),
    })?;
    Ok(Compiled {
        pdf,
        pages: document.pages().len(),
        warnings,
        moved_notes: moved_notes(&document),
    })
}
