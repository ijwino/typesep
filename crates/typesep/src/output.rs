use std::path::{Path, PathBuf};

use typesep_html::lazy_regex;

lazy_regex!(UNSAFE_RE = r#"[\\/:*?"<>|\x00-\x1f]+"#.to_owned());
lazy_regex!(SPACES_RE = format!("{}+", typesep_html::js::WS));
lazy_regex!(EDGE_RE = r"^[-. ]+|[-. ]+$".to_owned());

pub fn file_name_for(name: &str) -> String {
    let cleaned = UNSAFE_RE.replace_all(name, "-");
    let cleaned = SPACES_RE.replace_all(&cleaned, " ");
    let cleaned = EDGE_RE.replace_all(&cleaned, "");
    if cleaned.is_empty() {
        "untitled".to_owned()
    } else {
        cleaned.into_owned()
    }
}

pub fn output_file(name: &str, kind: &str, out_dir: &Path) -> PathBuf {
    out_dir.join(format!("{}.{kind}", file_name_for(name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe_for_the_file_system() {
        assert_eq!(file_name_for("turing-machine"), "turing-machine");
        assert_eq!(
            file_name_for("a/b: c?"),
            "a-b- c",
            "the trailing dash is trimmed with the dots and spaces"
        );
        assert_eq!(file_name_for("  ..dots..  "), "dots");
        assert_eq!(file_name_for("///"), "untitled");
        assert_eq!(
            file_name_for("Philosophy   of  Logic"),
            "Philosophy of Logic"
        );
        assert_eq!(
            file_name_for("Tractatus_Logico-Philosophicus_(English)"),
            "Tractatus_Logico-Philosophicus_(English)",
            "parentheses are safe"
        );
        assert_eq!(
            file_name_for("محاضرة_في_علم_الأخلاق"),
            "محاضرة_في_علم_الأخلاق",
            "so is any script"
        );
        assert_eq!(
            output_file("Logic: I", "pdf", Path::new("out")),
            PathBuf::from("out/Logic- I.pdf")
        );
    }
}
