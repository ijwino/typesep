use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The render snapshots are what the corpus typesets to, downstream of the model: the Typst
/// source each output compiled, its EPUB, its page count. They are in the corpus, under
/// `snapshots/render/` (model.rs describes the corpus and its layout); these tests replay
/// the arguments its `manifest.tsv` lists and compare what comes out.
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

struct Corpus {
    library: PathBuf,
    render: PathBuf,
    baselines: Vec<Baseline>,
}

/// One line of the manifest: one output of one run.
struct Baseline {
    run: String,
    name: String,
    pages: usize,
    recovered: bool,
    args: Vec<String>,
}

impl Baseline {
    fn file(&self, suffix: &str) -> String {
        format!("{}/{}{suffix}", self.run, self.name)
    }
}

fn corpus() -> Option<Corpus> {
    let corpus = corpus_dir();
    let render = corpus.join("snapshots/render");
    let library = corpus.join("library");
    let manifest = render.join("manifest.tsv");
    if !manifest.is_file() || !library.is_dir() {
        println!(
            "no render snapshots at {}: skipping. Point TYPESEP_TEST_CORPUS at a directory \
             holding library/ and snapshots/render/ to run this.",
            corpus.display()
        );
        return None;
    }
    let text = fs::read_to_string(&manifest).expect("the manifest is readable");
    let baselines = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let [run, name, pages, source, args @ ..] = &fields[..] else {
                panic!("a manifest line without its four fields: {line:?}");
            };
            Baseline {
                run: (*run).to_owned(),
                name: (*name).to_owned(),
                pages: pages.parse().expect("a page count"),
                recovered: match *source {
                    "emitted" => false,
                    "recovered" => true,
                    other => panic!("{run}/{name}: a Typst source that is {other:?}"),
                },
                args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            }
        })
        .collect();
    Some(Corpus {
        library,
        render,
        baselines,
    })
}

/// Where the binary runs: no typesep.config.yaml or fonts/ in it, a fontconfig file that
/// hides the system fonts, so a fallback family is kept or skipped the same way on every
/// machine, and an unreachable proxy, so whatever the library lacks fails instead of being
/// fetched into it.
struct Harness {
    work: tempfile::TempDir,
    library: PathBuf,
}

impl Harness {
    fn new(library: &Path) -> Harness {
        let work = tempfile::tempdir().expect("a temporary directory");
        let empty = work.path().join("no-fonts");
        fs::create_dir(&empty).expect("an empty font directory");
        fs::write(
            work.path().join("fonts.conf"),
            format!(
                "<?xml version=\"1.0\"?>\n<fontconfig><dir>{}</dir></fontconfig>\n",
                empty.display()
            ),
        )
        .expect("a fontconfig file");
        Harness {
            work,
            library: library.to_owned(),
        }
    }

    fn typesep(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_typesep"));
        command
            .current_dir(self.work.path())
            .env("FONTCONFIG_FILE", self.work.path().join("fonts.conf"))
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .env_remove("NO_PROXY")
            .env_remove("no_proxy")
            .env_remove("TYPESEP_FONTS")
            .env_remove("TYPESEP_LIBRARY");
        command
    }

    fn dump_typst(&self, baseline: &Baseline) -> Result<String, String> {
        let run = self
            .typesep()
            .args(["dump", "typst"])
            .args(&baseline.args)
            .arg("--library")
            .arg(&self.library)
            .arg("--quiet")
            .output()
            .expect("the binary runs");
        if !run.status.success() {
            return Err(format!(
                "dump typst exited {:?}: {}",
                run.status.code(),
                String::from_utf8_lossy(&run.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&run.stdout).into_owned())
    }

    /// Builds the baseline in a directory of its own, and returns the directory and what the
    /// binary said, which is verbose: every request it makes is on a line of its own.
    fn build(
        &self,
        baseline: &Baseline,
        format: &[&str],
    ) -> Result<(tempfile::TempDir, String), String> {
        let out = tempfile::tempdir_in(self.work.path()).expect("an output directory");
        let run = self
            .typesep()
            .args(&baseline.args)
            .arg("--library")
            .arg(&self.library)
            .args(format)
            .arg("-v")
            .arg("-o")
            .arg(out.path())
            .output()
            .expect("the binary runs");
        let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
        if !run.status.success() {
            return Err(format!("exited {:?}: {}", run.status.code(), stderr.trim()));
        }
        let requests: Vec<&str> = stderr
            .lines()
            .filter(|line| line.starts_with("  GET "))
            .collect();
        if !requests.is_empty() {
            return Err(format!(
                "the library lacks what it asked the network for: {}",
                requests.join("; ")
            ));
        }
        Ok((out, stderr))
    }
}

/// Runs `check` over the baselines on as many threads as there are cores, and returns the
/// failures in manifest order.
fn failures(
    baselines: &[Baseline],
    check: impl Fn(&Baseline) -> Result<(), String> + Sync,
) -> Vec<String> {
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut found: Vec<(usize, String)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut found = Vec::new();
                    loop {
                        let at = next.fetch_add(1, Ordering::Relaxed);
                        let Some(baseline) = baselines.get(at) else {
                            return found;
                        };
                        if let Err(message) = check(baseline) {
                            found.push((at, message));
                        }
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("a worker finishes"))
            .collect()
    });
    found.sort();
    found.into_iter().map(|(_, message)| message).collect()
}

/// Where `got` first parts from `want`, by line and column, with what each has there. The
/// XHTML is a few long lines, so a long line is cut down to the stretch around the column.
fn differs_at(file: &str, got: &str, want: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (got.lines().collect(), want.lines().collect());
    let at = (0..a.len().max(b.len()))
        .find(|&i| a.get(i) != b.get(i))
        .unwrap_or(0);
    let column = match (a.get(at), b.get(at)) {
        (Some(x), Some(y)) => x.chars().zip(y.chars()).take_while(|(p, q)| p == q).count(),
        _ => 0,
    };
    format!(
        "{file}: differs at line {}, column {}:\n    snapshot: {}\n    now:      {}",
        at + 1,
        column + 1,
        stretch(b.get(at).copied(), column),
        stretch(a.get(at).copied(), column)
    )
}

fn stretch(line: Option<&str>, column: usize) -> String {
    let Some(line) = line else {
        return "(no such line)".to_owned();
    };
    let chars: Vec<char> = line.chars().collect();
    let start = column.saturating_sub(40).min(chars.len());
    let end = (column + 80).min(chars.len());
    format!(
        "{}{:?}{}",
        if start > 0 { "…" } else { "" },
        chars[start..end].iter().collect::<String>(),
        if end < chars.len() { "…" } else { "" }
    )
}

#[test]
fn every_typst_source_is_what_the_renderer_still_compiles() {
    let Some(corpus) = corpus() else {
        return;
    };
    let harness = Harness::new(&corpus.library);

    // A source the recovery loop did not touch is what dump typst prints, which compiles
    // nothing; one it re-emitted is only to be had from a build.
    let failed = failures(&corpus.baselines, |baseline| {
        let file = baseline.file(".typ");
        let got = if baseline.recovered {
            let (out, _) = harness
                .build(baseline, &["--format", "pdf", "--keep-typst"])
                .map_err(|err| format!("{file}: {err}"))?;
            fs::read_to_string(out.path().join(format!("{}.typ", baseline.name)))
                .map_err(|err| format!("{file}: the build kept no source: {err}"))?
        } else {
            harness
                .dump_typst(baseline)
                .map_err(|err| format!("{file}: {err}"))?
        };
        let want = fs::read_to_string(corpus.render.join(&file))
            .map_err(|err| format!("{file}: the snapshot is not readable: {err}"))?;
        if got == want {
            Ok(())
        } else {
            Err(differs_at(&file, &got, &want))
        }
    });

    let entries = corpus
        .baselines
        .iter()
        .filter(|baseline| baseline.run == "default")
        .count();
    assert_eq!(
        entries,
        30,
        "an entry has gone missing from {}",
        corpus.render.join("manifest.tsv").display()
    );
    assert!(
        failed.is_empty(),
        "the renderer no longer writes the Typst sources:\n{}",
        failed.join("\n")
    );
}

/// The EPUB's members in order, each with how it is compressed, and the package's build time
/// replaced as the snapshot has it.
fn epub_members(file: &Path) -> Result<Vec<(String, String, Vec<u8>)>, String> {
    let bytes = fs::read(file).map_err(|err| format!("{}: {err}", file.display()))?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|err| format!("{}: {err}", file.display()))?;
    let modified = regex::Regex::new(
        r#"<meta property="dcterms:modified">\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ</meta>"#,
    )
    .expect("regex");
    let mut members = Vec::new();
    for index in 0..archive.len() {
        let mut member = archive
            .by_index(index)
            .map_err(|err| format!("{}: {err}", file.display()))?;
        let method = match member.compression() {
            zip::CompressionMethod::Stored => "stored".to_owned(),
            zip::CompressionMethod::Deflated => "deflated".to_owned(),
            other => format!("{other:?}").to_lowercase(),
        };
        let mut bytes = Vec::new();
        member
            .read_to_end(&mut bytes)
            .map_err(|err| format!("{}: {err}", file.display()))?;
        if member.name() == "OEBPS/content.opf" {
            let text = String::from_utf8_lossy(&bytes);
            bytes = modified
                .replace(
                    &text,
                    r#"<meta property="dcterms:modified">(build time)</meta>"#,
                )
                .into_owned()
                .into_bytes();
        }
        members.push((member.name().to_owned(), method, bytes));
    }
    Ok(members)
}

#[test]
fn every_epub_is_what_the_builder_still_writes() {
    let Some(corpus) = corpus() else {
        return;
    };
    let harness = Harness::new(&corpus.library);

    let failed = failures(&corpus.baselines, |baseline| {
        let epub = baseline.file(".epub");
        let (out, _) = harness
            .build(baseline, &["--format", "epub"])
            .map_err(|err| format!("{epub}: {err}"))?;
        let members = epub_members(&out.path().join(format!("{}.epub", baseline.name)))
            .map_err(|err| format!("{epub}: {err}"))?;

        let listing = baseline.file(".epub.members");
        let got: String = members
            .iter()
            .map(|(name, method, _)| format!("{method} {name}\n"))
            .collect();
        let want = fs::read_to_string(corpus.render.join(&listing))
            .map_err(|err| format!("{listing}: the snapshot is not readable: {err}"))?;
        if got != want {
            return Err(differs_at(&listing, &got, &want));
        }

        for (name, _, bytes) in &members {
            let file = format!("{epub}/{name}");
            let want = fs::read(corpus.render.join(&file))
                .map_err(|err| format!("{file}: the snapshot is not readable: {err}"))?;
            if *bytes == want {
                continue;
            }
            let texts = (std::str::from_utf8(bytes), std::str::from_utf8(&want));
            return Err(match texts {
                (Ok(got), Ok(want)) => differs_at(&file, got, want),
                _ => format!(
                    "{file}: differs, {} bytes in the snapshot and {} now",
                    want.len(),
                    bytes.len()
                ),
            });
        }
        Ok(())
    });

    assert!(
        failed.is_empty(),
        "the EPUB builder no longer writes the EPUBs:\n{}",
        failed.join("\n")
    );
}

#[test]
#[ignore = "typesets every baseline to PDF, which takes minutes in a debug build; make baselines-pages"]
fn every_pdf_still_has_as_many_pages() {
    let Some(corpus) = corpus() else {
        return;
    };
    let harness = Harness::new(&corpus.library);
    let pages_re = regex::Regex::new(r"^ {2}(\d+) pages$").expect("regex");

    let failed = failures(&corpus.baselines, |baseline| {
        let file = baseline.file(".pdf");
        let (_, stderr) = harness
            .build(baseline, &["--format", "pdf"])
            .map_err(|err| format!("{file}: {err}"))?;
        let pages: Vec<usize> = stderr
            .lines()
            .filter_map(|line| pages_re.captures(line.trim_end()))
            .filter_map(|caps| caps[1].parse().ok())
            .collect();
        match pages[..] {
            [pages] if pages == baseline.pages => Ok(()),
            [pages] => Err(format!(
                "{file}: {} pages in the snapshot, {pages} now",
                baseline.pages
            )),
            _ => Err(format!(
                "{file}: {} page counts reported, not one",
                pages.len()
            )),
        }
    });

    assert!(
        failed.is_empty(),
        "the PDFs no longer have the pages they had:\n{}",
        failed.join("\n")
    );
}
