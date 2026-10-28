use std::fs;
use std::path::Path;
use std::process::{Command, Output};

/// A library of one made-up SEP entry and one made-up text of the Wittgenstein Project, each
/// with the `meta.json` a load records: what `typesep list` reads, and all it reads.
fn write_library(dir: &Path) {
    let entry = dir.join("sep/entries/made-up");
    fs::create_dir_all(&entry).unwrap();
    fs::write(entry.join("index.html"), "<html></html>").unwrap();
    let meta = serde_json::json!({
        "url": "https://plato.stanford.edu/entries/made-up/",
        "title": "A Made-Up Entry",
        "revision": "Mon Oct 5, 2026",
        "files": { "index.html": { "fetchedAt": "2026-10-05T09:00:00.000Z" } }
    });
    fs::write(entry.join("meta.json"), meta.to_string()).unwrap();

    let text = dir.join("lwp/pages/Some_Remarks");
    fs::create_dir_all(&text).unwrap();
    fs::write(text.join("parse.json"), "{}").unwrap();
    let meta = serde_json::json!({
        "url": "https://www.wittgensteinproject.org/w/index.php/Some_Remarks",
        "title": "Some Remarks",
        "revision": "101",
        "files": { "parse.json": { "fetchedAt": "2026-10-05T09:00:00.000Z" } }
    });
    fs::write(text.join("meta.json"), meta.to_string()).unwrap();
}

fn list(library: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_typesep"))
        .arg("list")
        .arg("--library")
        .arg(library)
        .args(args)
        .env_remove("TYPESEP_LIBRARY")
        .output()
        .unwrap()
}

/// The first word of each line: the ids listed.
fn ids(run: &Output) -> Vec<String> {
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_list_holds_every_sources_works_and_with_a_source_that_sources_alone() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    write_library(&library);

    assert_eq!(ids(&list(&library, &[])), ["made-up", "Some_Remarks"]);
    assert_eq!(ids(&list(&library, &["--source", "sep"])), ["made-up"]);
    assert_eq!(ids(&list(&library, &["--source", "lwp"])), ["Some_Remarks"]);

    let unknown = list(&library, &["--source", "nope"]);
    assert!(!unknown.status.success());
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("[possible values: sep, lwp]"),
        "{}",
        String::from_utf8_lossy(&unknown.stderr)
    );
}

#[test]
fn a_source_with_nothing_in_the_library_is_said_to_have_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    fs::create_dir_all(library.join("sep")).unwrap();

    let run = list(&library, &["--source", "lwp"]);
    assert!(run.status.success());
    assert!(run.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.starts_with("nothing of lwp in the library yet at "),
        "{stderr}"
    );
}
