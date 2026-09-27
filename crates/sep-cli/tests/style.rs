use std::fs;
use std::path::Path;
use std::process::Command;

struct Snapshot {
    name: String,
    args: Vec<String>,
    cwd_tmp: bool,
    exit: i32,
    stdout: String,
    stderr: String,
}

fn parse(text: &str) -> Snapshot {
    let mut name = String::new();
    let mut args = Vec::new();
    let mut cwd_tmp = false;
    let mut exit = 0;
    let mut lines = text.lines();
    for line in lines.by_ref() {
        if line == "--- stdout ---" {
            break;
        }
        let (key, value) = line.split_once(": ").unwrap_or((line, ""));
        match key {
            "name" => name = value.to_owned(),
            "arg" => args.push(serde_json::from_str::<String>(value).expect("an arg is JSON")),
            "cwd" => cwd_tmp = value == "tmp",
            "exit" => exit = value.parse().expect("an exit status is a number"),
            other => panic!("unknown key in a style snapshot: {other}"),
        }
    }
    let mut stdout: Vec<&str> = Vec::new();
    let mut stderr: Vec<&str> = Vec::new();
    let mut into_stderr = false;
    for line in lines {
        match line {
            "--- stderr ---" => into_stderr = true,
            "--- end ---" => break,
            _ if into_stderr => stderr.push(line),
            _ => stdout.push(line),
        }
    }
    Snapshot {
        name,
        args,
        cwd_tmp,
        exit,
        stdout: stdout.join("\n"),
        stderr: stderr.join("\n"),
    }
}

#[test]
fn the_style_surface_answers_as_it_was_frozen() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let tmp = dir.path();
    fs::write(
        tmp.join("book.yaml"),
        "preset: a5-book\npage: {columns: 2}\nfonts: {main: {family: EB Garamond}, math: TeX Gyre Pagella Math}\n",
    )
    .expect("write book.yaml");
    fs::write(
        tmp.join("style.json"),
        r#"{"page":{"size":"letter","margins":{"top":"1in"}},"toc":{"title":"Contents"}}"#,
    )
    .expect("write style.json");
    fs::write(
        tmp.join("typesep.config.yaml"),
        "preset: manuscript\ntypography: {leading: 1.8}\n",
    )
    .expect("write the config file");
    fs::write(tmp.join("bad.yaml"), "preset: [1]\n").expect("write bad.yaml");
    fs::write(
        tmp.join("braces.yaml"),
        "pageFurniture: {footer: {center: {page}}}\n",
    )
    .expect("write braces.yaml");

    let elsewhere = tempfile::tempdir().expect("a second temporary directory");

    let dir_text = tmp.to_str().expect("a UTF-8 temporary path");
    let cases = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/style");
    let mut files: Vec<_> = fs::read_dir(&cases)
        .expect("tests/snapshots/style is readable")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "txt"))
        .collect();
    files.sort();

    let mut failures: Vec<String> = Vec::new();
    for file in &files {
        let snapshot = parse(&fs::read_to_string(file).expect("a snapshot is readable"));
        let args: Vec<String> = snapshot
            .args
            .iter()
            .map(|arg| arg.replace("<tmp>", dir_text))
            .collect();
        let cwd = if snapshot.cwd_tmp {
            tmp
        } else {
            elsewhere.path()
        };
        let run = Command::new(env!("CARGO_BIN_EXE_typesep"))
            .args(&args)
            .current_dir(cwd)
            .env_remove("TYPESEP_LIBRARY")
            .output()
            .expect("the binary runs");
        let stdout = String::from_utf8_lossy(&run.stdout).replace(dir_text, "<tmp>");
        let stderr = String::from_utf8_lossy(&run.stderr).replace(dir_text, "<tmp>");
        let exit = run.status.code().unwrap_or(-1);
        if exit != snapshot.exit {
            failures.push(format!(
                "{}: exit {exit}, frozen as {}",
                snapshot.name, snapshot.exit
            ));
        }
        for (stream, got, want) in [
            ("stdout", &stdout, &snapshot.stdout),
            ("stderr", &stderr, &snapshot.stderr),
        ] {
            if got == want {
                continue;
            }
            let (a, b): (Vec<&str>, Vec<&str>) = (got.lines().collect(), want.lines().collect());
            let at = (0..a.len().max(b.len()))
                .find(|&i| a.get(i) != b.get(i))
                .unwrap_or(0);
            failures.push(format!(
                "{}: {stream} differs at line {}:\n    frozen: {:?}\n    now:    {:?}",
                snapshot.name,
                at + 1,
                b.get(at),
                a.get(at)
            ));
        }
    }

    assert_eq!(files.len(), 28, "a style snapshot has gone missing");
    assert!(
        failures.is_empty(),
        "the style surface has moved under its snapshots:\n{}",
        failures.join("\n")
    );
}
