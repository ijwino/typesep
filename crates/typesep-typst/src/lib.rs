pub mod colors;
pub mod compile;
pub mod emit;
pub mod escape;
pub mod fonts;
pub mod images;
pub mod lang;
pub mod math;
pub mod mathml;
pub mod preamble;
pub mod recover;
pub mod style;
pub mod text;
pub mod world;

pub use compile::{CompileError, Compiled, MovedNote, compile_pdf};
pub use emit::{EmitOptions, Rendered, emit_document};
pub use fonts::{
    FontProblem, Fonts, describe_missing_fonts, font_dirs, font_problems, load_font_files,
};
pub use recover::{Outcome, compile_document};
pub use world::TypesetWorld;

pub const TYPST_VERSION: &str = "0.15.1";

macro_rules! lazy_regex {
    ($name:ident = $pattern:expr) => {
        static $name: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            let pattern = $pattern;
            regex::Regex::new(pattern.as_ref()).expect(concat!("regex ", stringify!($name)))
        });
    };
}
pub(crate) use lazy_regex;
