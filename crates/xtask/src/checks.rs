use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

/// A work to check: its id in its source, which the binary is told with --source.
struct Target {
    source: String,
    id: String,
    edition: Option<String>,
}

impl Target {
    /// Where the binary writes it: a directory of its source's, so works of two sources with
    /// one name are kept apart.
    fn dir(&self, out: &Path) -> PathBuf {
        out.join(&self.source)
    }

    fn file(&self, out: &Path, kind: &str) -> PathBuf {
        let name = match &self.edition {
            Some(edition) if edition != "live" => format!("{}-{edition}.{kind}", self.id),
            _ => format!("{}.{kind}", self.id),
        };
        self.dir(out).join(name)
    }

    /// How the report names it: its source and its id.
    fn label(&self) -> String {
        format!("{}/{}", self.source, self.id)
    }
}

#[derive(Default)]
struct Tally {
    order: Vec<String>,
    counts: HashMap<String, usize>,
}

impl Tally {
    fn add(&mut self, key: &str) {
        match self.counts.get_mut(key) {
            Some(count) => *count += 1,
            None => {
                self.order.push(key.to_owned());
                self.counts.insert(key.to_owned(), 1);
            }
        }
    }

    fn total(&self) -> usize {
        self.counts.values().sum()
    }

    fn ranked(&self) -> Vec<(&str, usize)> {
        let mut rows: Vec<(&str, usize)> = self
            .order
            .iter()
            .map(|key| (key.as_str(), self.counts[key]))
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.1));
        rows
    }

    fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// The works the check walks, source by source in the order the binary names them: with
/// `all`, every work the library holds, as `typesep list --source` lists it; otherwise every
/// work with a model snapshot, the first source's at the top of `snapshots` and each other
/// source's in the directory named by its key.
fn targets(
    binary: &Path,
    library: &Path,
    snapshots: &Path,
    all: bool,
) -> Result<Vec<Target>, String> {
    let mut targets = Vec::new();
    for (index, source) in source_keys(binary)?.into_iter().enumerate() {
        if all {
            targets.extend(listed(&typesep_list(binary, library, &source)?, &source));
            continue;
        }
        let dir = if index == 0 {
            snapshots.to_owned()
        } else {
            snapshots.join(&source)
        };
        if index > 0 && !dir.is_dir() {
            continue;
        }
        let mut ids: Vec<String> = fs::read_dir(&dir)
            .map_err(|err| format!("{}: {err}", dir.display()))?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                name.strip_suffix(".json").map(str::to_owned)
            })
            .collect();
        ids.sort();
        targets.extend(ids.into_iter().map(|id| Target {
            source: source.clone(),
            id,
            edition: None,
        }));
    }
    Ok(targets)
}

/// The keys --source takes, the default first, as `typesep list --help` names them.
fn source_keys(binary: &Path) -> Result<Vec<String>, String> {
    let help = Command::new(binary)
        .args(["list", "--help"])
        .output()
        .map_err(|err| format!("{}: {err}", binary.display()))?;
    keys_in_help(&String::from_utf8_lossy(&help.stdout)).ok_or_else(|| {
        format!(
            "{} list --help names no sources for --source; build it again (make release)",
            binary.display()
        )
    })
}

/// The possible values the help gives --source, wherever it wraps the lines.
fn keys_in_help(help: &str) -> Option<Vec<String>> {
    let help = help.split_whitespace().collect::<Vec<_>>().join(" ");
    let values = Regex::new(r"--source <\w+> .*?\[possible values: ([^\]]+)\]").expect("regex");
    let keys: Vec<String> = values.captures(&help)?[1]
        .split(',')
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .collect();
    (!keys.is_empty()).then_some(keys)
}

/// What `typesep list --source <source>` prints for the library.
fn typesep_list(binary: &Path, library: &Path, source: &str) -> Result<String, String> {
    let listed = Command::new(binary)
        .arg("list")
        .arg("--library")
        .arg(library)
        .args(["--source", source])
        .output()
        .map_err(|err| format!("{}: {err}", binary.display()))?;
    if !listed.status.success() {
        let stderr = String::from_utf8_lossy(&listed.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            "typesep list failed".to_owned()
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&listed.stdout).into_owned())
}

/// The works a listing of `source` names: each line its id, its revision, an edition other
/// than the live one in brackets, and its title.
fn listed(text: &str, source: &str) -> Vec<Target> {
    let edition = Regex::new(r" {2,}\[(\w+)\] ").expect("regex");
    text.lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let id = line.split_whitespace().next()?.to_owned();
            let rest = &line[id.len()..];
            let edition = edition.captures(rest).map(|caps| caps[1].to_owned());
            Some(Target {
                source: source.to_owned(),
                id,
                edition,
            })
        })
        .collect()
}

fn note_line(line: &str) -> Option<String> {
    let note =
        Regex::new(r"^ {2}(maths|figure|font|unknown|advanced|typography)\b.*").expect("regex");
    note.find(line.trim_end())
        .map(|m| m.as_str().trim().to_owned())
}

pub struct CheckArgs {
    pub binary: PathBuf,
    pub library: PathBuf,
    pub out: PathBuf,
    pub snapshots: PathBuf,
    /// Where the library is copied for the binary to read; removed when the check is done.
    pub copy: PathBuf,
    pub all: bool,
    pub keep: bool,
    pub verbose: bool,
}

pub fn render_check(args: CheckArgs) -> std::process::ExitCode {
    let args = reading_a_copy(args);
    let targets = match targets(&args.binary, &args.library, &args.snapshots, args.all) {
        Ok(targets) => targets,
        Err(message) => super::fail(&message),
    };
    if let Err(err) = fs::create_dir_all(&args.out) {
        super::fail(&format!("{}: {err}", args.out.display()));
    }

    let pages_re = Regex::new(r"^ {2}(\d+) pages$").expect("regex");
    let moved_re = Regex::new(
        r"^ {2}(?:\S+: )?note \d+ is ([\d.]+) text heights and is set at the end of the entry$",
    )
    .expect("regex");
    let more_re = Regex::new(r"^ {2}and (\d+) more notes$").expect("regex");

    let mut failures: Vec<(String, String)> = Vec::new();
    let mut notes = Tally::default();
    let (mut rendered, mut pages) = (0usize, 0usize);
    let (mut moved_notes, mut moved_works, mut not_listed) = (0usize, 0usize, 0usize);
    let mut tallest = 0f64;

    for (index, target) in targets.iter().enumerate() {
        let run = typeset(&args, target, "pdf");
        let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
        let mut moved_here = 0usize;
        for line in stderr.lines() {
            let line = line.trim_end();
            if let Some(note) = note_line(line) {
                notes.add(&note);
            }
            if let Some(caps) = pages_re.captures(line) {
                pages += caps[1].parse::<usize>().unwrap_or(0);
            }
            if let Some(caps) = moved_re.captures(line) {
                moved_here += 1;
                tallest = tallest.max(caps[1].parse::<f64>().unwrap_or(0.0));
            }
            if let Some(caps) = more_re.captures(line) {
                not_listed += caps[1].parse::<usize>().unwrap_or(0);
            }
        }
        moved_notes += moved_here;
        if moved_here > 0 {
            moved_works += 1;
        }
        let asked = super::asked_the_network(&stderr);
        let ok = run.status.success() && asked.is_none();
        if ok {
            rendered += 1;
            if !args.keep {
                let _ = fs::remove_file(target.file(&args.out, "pdf"));
            }
        } else {
            let message = asked.unwrap_or_else(|| super::last_lines(&stderr, 2));
            failures.push((target.label(), message));
        }
        if args.verbose {
            eprintln!(
                "[{}/{}] {} {}",
                index + 1,
                targets.len(),
                target.label(),
                if ok { "ok" } else { "FAILED" }
            );
        }
    }

    if !failures.is_empty() {
        eprintln!("\n{} works did not render:", failures.len());
        for (label, message) in &failures {
            eprintln!("  {label}: {message}");
        }
    }
    report_notes(&notes);
    if moved_notes > 0 {
        eprintln!(
            "\n{moved_notes} notes in {moved_works} works are set at the end of the work \
             (notes.maxHeight); the tallest is {} text heights",
            trim_float(tallest)
        );
        if not_listed > 0 {
            eprintln!("  {not_listed} further renderer notes were not listed by the CLI");
        }
    }

    println!("{rendered} / {}", targets.len());
    if pages > 0 {
        eprintln!("{pages} pages in all");
    }
    if !args.keep && args.out.is_dir() {
        let _ = fs::remove_dir_all(&args.out);
    }
    let _ = fs::remove_dir_all(&args.copy);
    exit_code(rendered == targets.len())
}

pub fn epub_check(args: CheckArgs, root: &Path) -> std::process::ExitCode {
    let jar = match epubcheck_jar(root) {
        Some(jar) => jar,
        None => super::fail(
            "EPUBCheck is not in tools/; put it there:\n  \
             curl -L -o /tmp/epubcheck.zip \
             https://github.com/w3c/epubcheck/releases/download/v5.3.0/epubcheck-5.3.0.zip\n  \
             unzip -d tools /tmp/epubcheck.zip",
        ),
    };
    let args = reading_a_copy(args);
    let targets = match targets(&args.binary, &args.library, &args.snapshots, args.all) {
        Ok(targets) => targets,
        Err(message) => super::fail(&message),
    };
    if let Err(err) = fs::create_dir_all(&args.out) {
        super::fail(&format!("{}: {err}", args.out.display()));
    }

    let said_re = Regex::new(r"^(ERROR|WARNING|FATAL)\(([A-Z]+-\d+)\)").expect("regex");
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut notes = Tally::default();
    let mut complaints = Tally::default();
    let mut passing = 0usize;

    for (index, target) in targets.iter().enumerate() {
        let run = typeset(&args, target, "epub");
        let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
        for line in stderr.lines() {
            if let Some(note) = note_line(line.trim_end()) {
                notes.add(&note);
            }
        }
        let file = target.file(&args.out, "epub");
        let asked = super::asked_the_network(&stderr);
        if !run.status.success() || !file.exists() || asked.is_some() {
            let message = match asked {
                Some(asked) => asked,
                None if run.status.success() => "the EPUB was not built".to_owned(),
                None => super::last_lines(&stderr, 2),
            };
            failures.push((target.label(), message));
            if args.verbose {
                eprintln!(
                    "[{}/{}] {} NOT BUILT",
                    index + 1,
                    targets.len(),
                    target.label()
                );
            }
            continue;
        }
        let checked = Command::new("java")
            .arg("-jar")
            .arg(&jar)
            .arg("--failonwarnings")
            .arg(&file)
            .output()
            .unwrap_or_else(|err| super::fail(&format!("java: {err}")));
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&checked.stdout),
            String::from_utf8_lossy(&checked.stderr)
        );
        if checked.status.success() {
            passing += 1;
            if !args.keep {
                let _ = fs::remove_file(&file);
            }
        } else {
            for line in said.lines() {
                if let Some(caps) = said_re.captures(line.trim()) {
                    complaints.add(&format!("{}({})", &caps[1], &caps[2]));
                }
            }
            let first: Vec<&str> = said
                .lines()
                .map(str::trim)
                .filter(|line| said_re.is_match(line))
                .take(3)
                .collect();
            let message = if first.is_empty() {
                said.trim().lines().next().unwrap_or("").to_owned()
            } else {
                first.join("\n    ")
            };
            failures.push((target.label(), message));
        }
        if args.verbose {
            eprintln!(
                "[{}/{}] {} {}",
                index + 1,
                targets.len(),
                target.label(),
                if checked.status.success() {
                    "ok"
                } else {
                    "FAILED"
                }
            );
        }
    }

    if !failures.is_empty() {
        eprintln!("\n{} works did not pass:", failures.len());
        for (label, message) in &failures {
            eprintln!("  {label}: {message}");
        }
    }
    if !complaints.is_empty() {
        eprintln!("\nwhat EPUBCheck said:");
        for (what, count) in complaints.ranked() {
            eprintln!("  {count:>4}  {what}");
        }
    }
    report_notes(&notes);

    println!("{passing} / {}", targets.len());
    if !args.keep && args.out.exists() {
        let _ = fs::remove_dir_all(&args.out);
    }
    let _ = fs::remove_dir_all(&args.copy);
    exit_code(passing == targets.len())
}

/// Typesets `target` to `format`, every section, with no way to the network and -v, so that
/// whatever the library lacks shows as a request instead of being fetched.
fn typeset(args: &CheckArgs, target: &Target, format: &str) -> std::process::Output {
    let mut command = Command::new(&args.binary);
    super::offline(&mut command)
        .arg(&target.id)
        .args(["--source", &target.source, "--sections", "all"])
        .args(["--format", format, "-v", "--library"])
        .arg(&args.library)
        .arg("-o")
        .arg(target.dir(&args.out));
    if let Some(edition) = &target.edition {
        command.args(["--edition", edition]);
    }
    command
        .output()
        .unwrap_or_else(|err| super::fail(&format!("{}: {err}", args.binary.display())))
}

/// The arguments with the library replaced by a fresh copy of it at `args.copy`. A build
/// writes to the library it reads: it records the work it read, fetches a figure the library
/// lacks, and moves a library of an older layout into the present one. So the check reads the
/// copy, and the corpus stays as the render snapshots were made from it.
fn reading_a_copy(args: CheckArgs) -> CheckArgs {
    let library = args
        .library
        .canonicalize()
        .unwrap_or_else(|err| super::fail(&format!("{}: {err}", args.library.display())));
    if args.copy.starts_with(&library) {
        super::fail(&format!(
            "the library would be copied into itself, at {}",
            args.copy.display()
        ));
    }
    if args.copy.exists() {
        fs::remove_dir_all(&args.copy)
            .unwrap_or_else(|err| super::fail(&format!("{}: {err}", args.copy.display())));
    }
    super::copy_dir(&library, &args.copy)
        .unwrap_or_else(|err| super::fail(&format!("copying {}: {err}", library.display())));
    CheckArgs {
        library: args.copy.clone(),
        ..args
    }
}

fn report_notes(notes: &Tally) {
    if notes.is_empty() {
        return;
    }
    eprintln!("\n{} notes from the renderer:", notes.total());
    for (note, count) in notes.ranked() {
        eprintln!("  {count:>4}  {note}");
    }
}

fn epubcheck_jar(root: &Path) -> Option<PathBuf> {
    let tools = root.join("tools");
    let mut found: Vec<PathBuf> = fs::read_dir(tools)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("epubcheck"))
        .map(|entry| entry.path().join("epubcheck.jar"))
        .filter(|jar| jar.is_file())
        .collect();
    found.sort();
    found.into_iter().next()
}

fn trim_float(value: f64) -> String {
    let text = format!("{value}");
    text.trim_end_matches(".0").to_owned()
}

fn exit_code(ok: bool) -> std::process::ExitCode {
    if ok {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(source: &str, id: &str, edition: Option<&str>) -> Target {
        Target {
            source: source.to_owned(),
            id: id.to_owned(),
            edition: edition.map(str::to_owned),
        }
    }

    #[test]
    fn a_work_that_asked_the_network_for_anything_fails_with_what_it_asked_for() {
        let read = "library /tmp/library\nstyle   the default preset\nfonts   11 families\n\
                    reading made-up\n  typesetting made-up\n  59 pages\n  typeset in 0.51 s\n";
        assert_eq!(crate::asked_the_network(read), None);

        let fetched = "reading made-up\n  GET https://plato.stanford.edu/entries/made-up/a.svg\n  \
                       Connection refused; retrying in 5s\n  \
                       GET https://plato.stanford.edu/entries/made-up/a.svg (retry 1)\n  \
                       skipping image https://plato.stanford.edu/entries/made-up/a.svg: \
                       Connection refused\n  59 pages\n";
        assert_eq!(
            crate::asked_the_network(fetched).as_deref(),
            Some(
                "the library lacks what it asked the network for: \
                 \x20 GET https://plato.stanford.edu/entries/made-up/a.svg; \
                 \x20 GET https://plato.stanford.edu/entries/made-up/a.svg (retry 1)"
            )
        );
    }

    #[test]
    fn an_archived_edition_is_looked_for_under_its_own_name() {
        let out = Path::new("out");
        let kant = |edition| target("sep", "kant", edition);
        assert_eq!(kant(None).file(out, "pdf"), out.join("sep/kant.pdf"));
        assert_eq!(
            kant(Some("live")).file(out, "pdf"),
            out.join("sep/kant.pdf")
        );
        assert_eq!(
            kant(Some("sum2020")).file(out, "epub"),
            out.join("sep/kant-sum2020.epub")
        );
    }

    #[test]
    fn works_of_two_sources_with_one_name_are_written_apart() {
        let out = Path::new("out");
        let (sep, lwp) = (target("sep", "kant", None), target("lwp", "kant", None));
        assert_ne!(sep.file(out, "pdf"), lwp.file(out, "pdf"));
        assert_eq!(lwp.dir(out), out.join("lwp"));
        assert_eq!(
            (sep.label(), lwp.label()),
            ("sep/kant".into(), "lwp/kant".into())
        );
    }

    #[test]
    fn a_listing_names_each_work_with_the_source_it_was_asked_for() {
        // As `typesep list --source` prints them: the id padded to 28, the revision to 26.
        let sep = "made-up                      Fri Feb 28, 2025           A Made-Up Entry\n\
                   made-up                      Mon Mar 3, 2020            [sum2020] A Made-Up Entry\n\
                   undated                      -                          Undated\n";
        let works: Vec<(String, String, Option<String>)> = listed(sep, "sep")
            .into_iter()
            .map(|work| (work.source, work.id, work.edition))
            .collect();
        assert_eq!(
            works,
            [
                ("sep".into(), "made-up".into(), None),
                ("sep".into(), "made-up".into(), Some("sum2020".into())),
                ("sep".into(), "undated".into(), None),
            ]
        );

        // A title longer than the column, in parentheses and in a script of its own.
        let lwp = "Some_Rather_Long_Remarks_(English) 11293                      \
                   Some Rather Long Remarks (English)\n\
                   ملاحظات 101                        ملاحظات\n";
        let works: Vec<(String, String, Option<String>)> = listed(lwp, "lwp")
            .into_iter()
            .map(|work| (work.source, work.id, work.edition))
            .collect();
        assert_eq!(
            works,
            [
                (
                    "lwp".into(),
                    "Some_Rather_Long_Remarks_(English)".into(),
                    None
                ),
                ("lwp".into(), "ملاحظات".into(), None),
            ]
        );
    }

    #[test]
    fn the_sources_are_the_values_the_help_gives_source_however_it_wraps() {
        let help = "Usage: typesep list [OPTIONS]\n\n\
                    Options:\n      \
                    --library <dir>  Where fetched pages and figures are kept [default: \
                    TYPESEP_LIBRARY or the\n                       platform data directory]\n      \
                    --source <key>   Only the works of this source: sep (entries) or lwp \
                    (texts) [default: every\n                       source's] [possible \
                    values: sep,\n                       lwp]\n  \
                    -h, --help           Print help\n";
        assert_eq!(keys_in_help(help), Some(vec!["sep".into(), "lwp".into()]));
        assert_eq!(
            keys_in_help("Usage: typesep list [OPTIONS]\n      --library <dir>  Where\n"),
            None
        );
    }
}
