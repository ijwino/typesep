use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The corpus is thirty SEP entries and what they parse and typeset to. It is not in this
/// repository: the entries are copyright their authors and the SEP's Terms of Use grant no
/// right to redistribute them, so the corpus lives outside the tree and this test runs only
/// where it is present. `TYPESEP_TEST_CORPUS` names the directory (by default
/// `$XDG_DATA_HOME/typesep/test-corpus`); it holds
///
/// - `library/`: the entries as fetched, which the binary reads with `--library`, in the
///   layout of any library: `sep/` holds the SEP's files at their paths on the site, each unit
///   (`sep/entries/<slug>/`, an archived entry's `sep/archives/<edition>/entries/<slug>/`, and
///   `sep/` itself for the site's root) with its `meta.json`, and the converted copies of its
///   files under `sep/.derived/`. The tests and `make baselines` read a copy of it, so the
///   corpus is never written to; a corpus in the layout before sources had directories
///   (`entries/` and `meta.json` at the top) still works, since the copy is moved into
///   `sep/` the first time typesep opens it, and the corpus itself is moved by any typesep
///   pointed at it, `typesep list --library <corpus>/library` say. The snapshots are made
///   from it as it is, fetch dates included, so refetching anything in it means making them
///   again.
/// - `snapshots/model/<slug>.json`: what `dump model <slug> --sections all` prints, checked
///   here and by `make parity`.
/// - `snapshots/render/`: what the entries typeset to, checked by render.rs and written by
///   `make baselines`, which replaces them only when given `ARGS=--bless`:
///   - `manifest.tsv`: one output per line: its run, its name, its PDF's page count, whether
///     its Typst source is what the emitter wrote (`emitted`) or what the recovery loop
///     re-emitted (`recovered`), and the arguments that made it. The runs are listed in
///     crates/xtask/src/baselines.rs.
///   - `<run>/<name>.typ`: the Typst source the PDF was compiled from, as `--keep-typst`
///     writes it.
///   - `<run>/<name>.epub/`: the EPUB's members, with the build time in `content.opf`
///     replaced by `(build time)`, and `<run>/<name>.epub.members` their order and
///     compression.
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

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
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

    // Opening a library can move it into the present layout, so the binary reads a copy.
    let work = tempfile::tempdir().expect("a temporary directory");
    let library = work.path().join("library");
    copy_dir(&corpus.join("library"), &library).expect("a copy of the library");

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
