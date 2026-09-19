use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// The corpus is thirty SEP entries and the models they parse to. It is not in this
/// repository: the entries are copyright their authors and the SEP's Terms of Use grant no
/// right to redistribute them, so the corpus lives outside the tree and this test runs only
/// where it is present. `TYPESEP_TEST_CORPUS` names the directory; it holds `library/` and
/// `snapshots/model/`.
fn corpus_dir() -> PathBuf {
    let named = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|dir| !dir.as_os_str().is_empty())
    };
    if let Some(dir) = named("TYPESEP_TEST_CORPUS") {
        return dir;
    }
    named("XDG_DATA_HOME")
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("typesep")
        .join("test-corpus")
}

#[test]
fn every_model_is_what_the_parser_still_produces() {
    let corpus = corpus_dir();
    let models = corpus.join("snapshots/model");
    let library = corpus.join("library");

    if !models.is_dir() || !library.is_dir() {
        println!(
            "no corpus at {}: skipping. Point TYPESEP_TEST_CORPUS at a directory holding \
             library/ and snapshots/model/ to run this.",
            corpus.display()
        );
        return;
    }

    let mut files: Vec<_> = fs::read_dir(&models)
        .expect("snapshots/model is readable")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();

    let mut failures: Vec<String> = Vec::new();
    for file in &files {
        let slug = file
            .file_stem()
            .expect("a slug")
            .to_string_lossy()
            .into_owned();
        let run = Command::new(env!("CARGO_BIN_EXE_typesep"))
            .args(["dump", "model", &slug, "--sections", "all", "--quiet"])
            .arg("--library")
            .arg(&library)
            .env_remove("TYPESEP_LIBRARY")
            .output()
            .expect("the binary runs");
        if !run.status.success() {
            let stderr = String::from_utf8_lossy(&run.stderr);
            failures.push(format!(
                "{slug}: exited {:?}: {}",
                run.status.code(),
                stderr.trim()
            ));
            continue;
        }
        let got = String::from_utf8_lossy(&run.stdout);
        let want = fs::read_to_string(file).expect("a snapshot is readable");
        if got == want {
            continue;
        }
        let (a, b): (Vec<&str>, Vec<&str>) = (got.lines().collect(), want.lines().collect());
        let at = (0..a.len().max(b.len()))
            .find(|&i| a.get(i) != b.get(i))
            .unwrap_or(0);
        failures.push(format!(
            "{slug}: differs at line {}:\n    snapshot: {:?}\n    now:      {:?}",
            at + 1,
            b.get(at),
            a.get(at)
        ));
    }

    assert_eq!(
        files.len(),
        30,
        "a model snapshot has gone missing from {}",
        models.display()
    );
    assert!(
        failures.is_empty(),
        "the parser no longer reproduces the models:\n{}",
        failures.join("\n")
    );
}
