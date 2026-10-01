use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

/// The runs the render snapshots are made of, and the one place they are listed. Each run
/// typesets its entries to PDF and EPUB with its arguments; the harness adds --library, -o,
/// --format, --keep-typst and -v. The manifest records every invocation, and the test in
/// crates/sep-cli/tests/render.rs replays what the manifest says.
const RUNS: [Run; 4] = [
    // Every corpus entry, the default style, every section: what the model snapshots hold.
    Run {
        name: "default",
        entries: Entries::Corpus,
        args: &["--sections", "all"],
    },
    // The e-reader preset: its page, type and furniture, and the default sections, which
    // leave the academic tools and the supplements out.
    Run {
        name: "ereader",
        entries: Entries::Each(&["turing-machine", "game-evolutionary"]),
        args: &["--preset", "ereader"],
    },
    // Three entries with notes as one volume: one cover and colophon, labels kept apart per
    // entry, the merged rights and identifier.
    Run {
        name: "merge",
        entries: Entries::Merged(&["abner-burgos", "bradley-regress", "compositionality"]),
        args: &["--sections", "all", "--merge", "Miscellany"],
    },
    // Options off their defaults that take other branches: endnotes and link footnotes in
    // the emitter, the LaTeX contents, no cover, a page counter that starts elsewhere, and in
    // the preamble two sides and two columns, grid tables, roman numbers, underlined links,
    // no footnote rule, ragged text, and a fallback list with one embedded family and one
    // that is never installed.
    Run {
        name: "knobs",
        entries: Entries::Each(&["turing-machine", "epistemic-game"]),
        args: &[
            "--sections",
            "all",
            "-s",
            "notes.style=endnote",
            "-s",
            "links.mode=footnote",
            "-s",
            "toc.style=latex",
            "-s",
            "cover.style=none",
            "-s",
            "pageFurniture.numbering.style=roman",
            "-s",
            "pageFurniture.numbering.startAt=3",
            "-s",
            "page.twoside=true",
            "-s",
            "page.columns=2",
            "-s",
            "blocks.table.style=grid",
            "-s",
            "links.underline=true",
            "-s",
            "blocks.footnote.rule=false",
            "-s",
            "typography.justify=false",
            "-s",
            "typography.hyphenate=false",
            "-s",
            "fonts.fallback=TeX Gyre Heros,Noto Serif",
        ],
    },
];

struct Run {
    name: &'static str,
    entries: Entries,
    args: &'static [&'static str],
}

enum Entries {
    /// Every entry with a model snapshot, one by one.
    Corpus,
    /// These entries, one by one.
    Each(&'static [&'static str]),
    /// These entries, in one invocation (the run's arguments carry the --merge).
    Merged(&'static [&'static str]),
}

/// One invocation of the binary, which writes one output.
struct Invocation {
    run: &'static str,
    args: Vec<String>,
}

/// What the manifest says about one output.
struct Line {
    run: &'static str,
    name: String,
    pages: usize,
    recovered: bool,
    args: Vec<String>,
}

pub struct BaselineArgs {
    pub binary: PathBuf,
    pub library: PathBuf,
    pub models: PathBuf,
    pub dest: PathBuf,
    pub work: PathBuf,
    pub bless: bool,
    pub keep: bool,
    pub verbose: bool,
}

const MANIFEST: &str = "manifest.tsv";

/// The EPUB's dcterms:modified is the time it was built; the snapshot holds this instead.
const MODIFIED_PLACEHOLDER: &str = "(build time)";

pub fn run(args: BaselineArgs) -> std::process::ExitCode {
    if args.dest.exists() && !args.bless {
        super::fail(&format!(
            "{} already holds render snapshots; pass --bless to replace them",
            args.dest.display()
        ));
    }
    if !args.binary.is_file() {
        super::fail(&format!(
            "{} is not there; build it first (make release)",
            args.binary.display()
        ));
    }
    let invocations = invocations(&args.models);
    let name = args
        .dest
        .file_name()
        .unwrap_or_else(|| super::fail("--out names no directory"))
        .to_string_lossy()
        .into_owned();
    let staging = args.dest.with_file_name(format!("{name}.new"));
    for dir in [&staging, &args.work] {
        if dir.exists() {
            fs::remove_dir_all(dir)
                .unwrap_or_else(|err| super::fail(&format!("{}: {err}", dir.display())));
        }
        fs::create_dir_all(dir)
            .unwrap_or_else(|err| super::fail(&format!("{}: {err}", dir.display())));
    }
    write_font_config(&args.work);
    // A build writes to the library it reads, so it reads a copy and the corpus stays as the
    // snapshots were made from it.
    let copy = args.work.join("library");
    copy_dir(&args.library, &copy)
        .unwrap_or_else(|err| super::fail(&format!("copying {}: {err}", args.library.display())));
    let args = BaselineArgs {
        library: copy,
        ..args
    };

    let mut lines: Vec<Line> = Vec::new();
    let mut failures: Vec<(String, String)> = Vec::new();
    for (index, invocation) in invocations.iter().enumerate() {
        let out = args.work.join(format!("{}-{index}", invocation.run));
        let label = format!("{} {}", invocation.run, invocation.args.join(" "));
        match generate(&args, invocation, &out, &staging) {
            Ok(line) => {
                if args.verbose {
                    eprintln!(
                        "[{}/{}] {}/{}: {} pages{}",
                        index + 1,
                        invocations.len(),
                        line.run,
                        line.name,
                        line.pages,
                        if line.recovered { ", recovered" } else { "" }
                    );
                }
                lines.push(line);
            }
            Err(message) => {
                if args.verbose {
                    eprintln!("[{}/{}] {label} FAILED", index + 1, invocations.len());
                }
                failures.push((label, message));
            }
        }
    }

    if !failures.is_empty() {
        eprintln!("\n{} invocations failed:", failures.len());
        for (label, message) in &failures {
            eprintln!("  {label}: {message}");
        }
        let _ = fs::remove_dir_all(&staging);
        eprintln!("{} is left as it was", args.dest.display());
        return std::process::ExitCode::FAILURE;
    }

    let manifest = manifest_text(&lines);
    if let Err(err) = fs::write(staging.join(MANIFEST), manifest) {
        super::fail(&format!("{}: {err}", staging.display()));
    }
    replace(&staging, &args.dest);
    if !args.keep {
        let _ = fs::remove_dir_all(&args.work);
    }

    let recovered: Vec<String> = lines
        .iter()
        .filter(|line| line.recovered)
        .map(|line| format!("{}/{}", line.run, line.name))
        .collect();
    if !recovered.is_empty() {
        eprintln!(
            "the Typst source of {} came out of the recovery loop: {}",
            recovered.len(),
            recovered.join(", ")
        );
    }
    println!("{} outputs in {}", lines.len(), args.dest.display());
    std::process::ExitCode::SUCCESS
}

fn invocations(models: &Path) -> Vec<Invocation> {
    let mut corpus: Vec<String> = fs::read_dir(models)
        .unwrap_or_else(|err| super::fail(&format!("{}: {err}", models.display())))
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            name.strip_suffix(".json").map(str::to_owned)
        })
        .collect();
    corpus.sort();
    if corpus.is_empty() {
        super::fail(&format!("no model snapshots in {}", models.display()));
    }

    let with = |entries: &[&str], args: &[&str]| -> Vec<String> {
        entries
            .iter()
            .chain(args)
            .map(|arg| (*arg).to_owned())
            .collect()
    };
    let mut out = Vec::new();
    for run in &RUNS {
        let one_by_one: Vec<&str> = match run.entries {
            Entries::Corpus => corpus.iter().map(String::as_str).collect(),
            Entries::Each(entries) => entries.to_vec(),
            Entries::Merged(entries) => {
                out.push(Invocation {
                    run: run.name,
                    args: with(entries, run.args),
                });
                continue;
            }
        };
        for entry in one_by_one {
            if !corpus.iter().any(|slug| slug == entry) {
                super::fail(&format!("run {}: {entry} is not in the corpus", run.name));
            }
            out.push(Invocation {
                run: run.name,
                args: with(&[entry], run.args),
            });
        }
    }
    out
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

/// A fontconfig file that names one empty directory, so the binary finds no system fonts and
/// a fallback family is kept or skipped the same way on every machine.
fn write_font_config(work: &Path) {
    let empty = work.join("no-fonts");
    let config = format!(
        "<?xml version=\"1.0\"?>\n<fontconfig><dir>{}</dir></fontconfig>\n",
        empty.display()
    );
    fs::create_dir_all(&empty)
        .and_then(|()| fs::write(work.join("fonts.conf"), config))
        .unwrap_or_else(|err| super::fail(&format!("{}: {err}", work.display())));
}

/// The binary, run where no typesep.config.yaml or fonts/ is, with system fonts hidden and
/// the network unreachable: whatever the library lacks fails instead of being fetched.
fn typesep(args: &BaselineArgs) -> Command {
    let mut command = Command::new(&args.binary);
    command
        .current_dir(&args.work)
        .env("FONTCONFIG_FILE", args.work.join("fonts.conf"))
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .env_remove("TYPESEP_FONTS")
        .env_remove("TYPESEP_LIBRARY");
    command
}

fn generate(
    args: &BaselineArgs,
    invocation: &Invocation,
    out: &Path,
    staging: &Path,
) -> Result<Line, String> {
    let build = typesep(args)
        .args(&invocation.args)
        .arg("--library")
        .arg(&args.library)
        .args(["--format", "pdf,epub", "--keep-typst", "-v", "-o"])
        .arg(out)
        .output()
        .map_err(|err| format!("{}: {err}", args.binary.display()))?;
    let stderr = String::from_utf8_lossy(&build.stderr).into_owned();
    if !build.status.success() {
        return Err(last_lines(&stderr, 3));
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
    let pages_re = Regex::new(r"^ {2}(\d+) pages$").expect("regex");
    let pages: Vec<usize> = stderr
        .lines()
        .filter_map(|line| pages_re.captures(line.trim_end()))
        .filter_map(|caps| caps[1].parse().ok())
        .collect();
    let [pages] = pages[..] else {
        return Err(format!("{} page counts reported, not one", pages.len()));
    };

    let typ = only_file(out, "typ")?;
    let epub = only_file(out, "epub")?;
    let name = stem(&epub);
    if stem(&typ) != name {
        return Err(format!(
            "{} and {} are named differently; the snapshot needs one name",
            typ.display(),
            epub.display()
        ));
    }
    let compiled = fs::read_to_string(&typ).map_err(|err| format!("{}: {err}", typ.display()))?;

    let dump = typesep(args)
        .args(["dump", "typst"])
        .args(&invocation.args)
        .arg("--library")
        .arg(&args.library)
        .arg("--quiet")
        .output()
        .map_err(|err| format!("{}: {err}", args.binary.display()))?;
    if !dump.status.success() {
        let stderr = String::from_utf8_lossy(&dump.stderr);
        return Err(format!("dump typst: {}", last_lines(&stderr, 3)));
    }
    // The test reads a source the recovery loop left alone from dump typst, which compiles
    // nothing, so a source that differs from it has to be one the loop re-emitted.
    let recovered = String::from_utf8_lossy(&dump.stdout) != compiled;
    if recovered && !stderr.contains("looking for the formulas at fault") {
        return Err(
            "the kept source is not what dump typst prints, and the recovery loop did not run"
                .to_owned(),
        );
    }

    let dir = staging.join(invocation.run);
    fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    fs::write(dir.join(format!("{name}.typ")), &compiled)
        .map_err(|err| format!("{}: {err}", dir.display()))?;
    extract_epub(&epub, &dir, &name)?;

    Ok(Line {
        run: invocation.run,
        name,
        pages,
        recovered,
        args: invocation.args.clone(),
    })
}

fn last_lines(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines[lines.len().saturating_sub(count)..]
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ")
}

fn only_file(dir: &Path, extension: &str) -> Result<PathBuf, String> {
    let found: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|err| format!("{}: {err}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == extension))
        .collect();
    match &found[..] {
        [one] => Ok(one.clone()),
        _ => Err(format!(
            "{} .{extension} files in {}, not one",
            found.len(),
            dir.display()
        )),
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Writes the EPUB's members under `<name>.epub/`, and their order and compression, which
/// the directory cannot show, to `<name>.epub.members`.
fn extract_epub(epub: &Path, dir: &Path, name: &str) -> Result<(), String> {
    let file = fs::File::open(epub).map_err(|err| format!("{}: {err}", epub.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| format!("{}: {err}", epub.display()))?;
    let root = dir.join(format!("{name}.epub"));
    let mut members = String::new();
    for index in 0..archive.len() {
        let mut member = archive
            .by_index(index)
            .map_err(|err| format!("{}: {err}", epub.display()))?;
        let path = member
            .enclosed_name()
            .ok_or_else(|| format!("{}: a member named {}", epub.display(), member.name()))?;
        members.push_str(&format!(
            "{} {}\n",
            compression(member.compression()),
            member.name()
        ));
        let mut bytes = Vec::new();
        member
            .read_to_end(&mut bytes)
            .map_err(|err| format!("{}: {err}", epub.display()))?;
        if member.name() == "OEBPS/content.opf" {
            bytes = normalize_package(&String::from_utf8_lossy(&bytes)).into_bytes();
        }
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        fs::write(&target, bytes).map_err(|err| format!("{}: {err}", target.display()))?;
    }
    let listing = dir.join(format!("{name}.epub.members"));
    fs::write(&listing, members).map_err(|err| format!("{}: {err}", listing.display()))
}

fn compression(method: zip::CompressionMethod) -> String {
    match method {
        zip::CompressionMethod::Stored => "stored".to_owned(),
        zip::CompressionMethod::Deflated => "deflated".to_owned(),
        other => format!("{other:?}").to_lowercase(),
    }
}

fn normalize_package(text: &str) -> String {
    let modified =
        Regex::new(r#"<meta property="dcterms:modified">\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ</meta>"#)
            .expect("regex");
    modified
        .replace(
            text,
            format!(r#"<meta property="dcterms:modified">{MODIFIED_PLACEHOLDER}</meta>"#),
        )
        .into_owned()
}

fn manifest_text(lines: &[Line]) -> String {
    let mut text = String::from(
        "# The render snapshots, written by `make baselines`. One output per line, \
         tab-separated:\n# run, output name, PDF pages, whether its Typst source is what the \
         emitter wrote (emitted)\n# or what the recovery loop re-emitted (recovered), then \
         the arguments, one per field.\n",
    );
    for line in lines {
        let fields = [
            line.run.to_owned(),
            line.name.clone(),
            line.pages.to_string(),
            if line.recovered {
                "recovered"
            } else {
                "emitted"
            }
            .to_owned(),
        ];
        let all: Vec<&str> = fields
            .iter()
            .map(String::as_str)
            .chain(line.args.iter().map(String::as_str))
            .collect();
        text.push_str(&all.join("\t"));
        text.push('\n');
    }
    text
}

/// Puts the new snapshots where the old ones were, keeping the old ones until the new ones
/// are in place.
fn replace(staging: &Path, dest: &Path) {
    let old = dest.with_file_name(format!(
        "{}.old",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ));
    if old.exists() {
        let _ = fs::remove_dir_all(&old);
    }
    if dest.exists() {
        fs::rename(dest, &old)
            .unwrap_or_else(|err| super::fail(&format!("{}: {err}", dest.display())));
    }
    fs::rename(staging, dest)
        .unwrap_or_else(|err| super::fail(&format!("{}: {err}", dest.display())));
    if old.exists() {
        let _ = fs::remove_dir_all(&old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_build_time_is_taken_out_of_the_package() {
        let opf = "<dc:date>2026-09-13</dc:date>\n\
                   <meta property=\"dcterms:modified\">2026-10-01T09:19:28Z</meta>\n";
        assert_eq!(
            normalize_package(opf),
            "<dc:date>2026-09-13</dc:date>\n\
             <meta property=\"dcterms:modified\">(build time)</meta>\n"
        );
    }

    #[test]
    fn every_run_has_a_name_of_its_own() {
        let mut names: Vec<&str> = RUNS.iter().map(|run| run.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), RUNS.len());
    }
}
