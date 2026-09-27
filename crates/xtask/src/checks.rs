use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

struct Target {
    slug: String,
    edition: Option<String>,
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

fn targets(
    binary: &Path,
    library: &Path,
    snapshots: &Path,
    all: bool,
) -> Result<Vec<Target>, String> {
    if !all {
        let mut slugs: Vec<String> = fs::read_dir(snapshots)
            .map_err(|err| format!("{}: {err}", snapshots.display()))?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                name.strip_suffix(".json").map(str::to_owned)
            })
            .collect();
        slugs.sort();
        return Ok(slugs
            .into_iter()
            .map(|slug| Target {
                slug,
                edition: None,
            })
            .collect());
    }
    let listed = Command::new(binary)
        .arg("list")
        .arg("--library")
        .arg(library)
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
    let edition = Regex::new(r" {2,}\[(\w+)\] ").expect("regex");
    Ok(String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let slug = line.split_whitespace().next()?.to_owned();
            let rest = &line[slug.len()..];
            let edition = edition.captures(rest).map(|caps| caps[1].to_owned());
            Some(Target { slug, edition })
        })
        .collect())
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
    pub all: bool,
    pub keep: bool,
    pub verbose: bool,
}

pub fn render_check(args: CheckArgs) -> std::process::ExitCode {
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
    let (mut moved_notes, mut moved_entries, mut not_listed) = (0usize, 0usize, 0usize);
    let mut tallest = 0f64;

    for (index, target) in targets.iter().enumerate() {
        let mut argv = vec![
            target.slug.clone(),
            "--sections".to_owned(),
            "all".to_owned(),
            "--library".to_owned(),
            args.library.display().to_string(),
            "-o".to_owned(),
            args.out.display().to_string(),
        ];
        if let Some(edition) = &target.edition {
            argv.push("--edition".to_owned());
            argv.push(edition.clone());
        }
        let run = Command::new(&args.binary)
            .args(&argv)
            .output()
            .unwrap_or_else(|err| super::fail(&format!("{}: {err}", args.binary.display())));
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
            moved_entries += 1;
        }
        if run.status.success() {
            rendered += 1;
            if !args.keep {
                let _ = fs::remove_file(args.out.join(format!("{}.pdf", target.slug)));
            }
        } else {
            failures.push((target.slug.clone(), first_lines(&stderr, 2)));
        }
        if args.verbose {
            eprintln!(
                "[{}/{}] {} {}",
                index + 1,
                targets.len(),
                target.slug,
                if run.status.success() { "ok" } else { "FAILED" }
            );
        }
    }

    if !failures.is_empty() {
        eprintln!("\n{} entries did not render:", failures.len());
        for (slug, message) in &failures {
            eprintln!("  {slug}: {message}");
        }
    }
    report_notes(&notes);
    if moved_notes > 0 {
        eprintln!(
            "\n{moved_notes} notes in {moved_entries} entries are set at the end of the entry \
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
        let mut argv = vec![
            target.slug.clone(),
            "--sections".to_owned(),
            "all".to_owned(),
            "--format".to_owned(),
            "epub".to_owned(),
            "--library".to_owned(),
            args.library.display().to_string(),
            "-o".to_owned(),
            args.out.display().to_string(),
        ];
        if let Some(edition) = &target.edition {
            argv.push("--edition".to_owned());
            argv.push(edition.clone());
        }
        let run = Command::new(&args.binary)
            .args(&argv)
            .output()
            .unwrap_or_else(|err| super::fail(&format!("{}: {err}", args.binary.display())));
        let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
        for line in stderr.lines() {
            if let Some(note) = note_line(line.trim_end()) {
                notes.add(&note);
            }
        }
        let file = args.out.join(format!("{}.epub", target.slug));
        if !run.status.success() || !file.exists() {
            let first = first_lines(&stderr, 2);
            failures.push((
                target.slug.clone(),
                if first.is_empty() {
                    "the EPUB was not built".to_owned()
                } else {
                    first
                },
            ));
            if args.verbose {
                eprintln!(
                    "[{}/{}] {} NOT BUILT",
                    index + 1,
                    targets.len(),
                    target.slug
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
            failures.push((target.slug.clone(), message));
        }
        if args.verbose {
            eprintln!(
                "[{}/{}] {} {}",
                index + 1,
                targets.len(),
                target.slug,
                if checked.status.success() {
                    "ok"
                } else {
                    "FAILED"
                }
            );
        }
    }

    if !failures.is_empty() {
        eprintln!("\n{} entries did not pass:", failures.len());
        for (slug, message) in &failures {
            eprintln!("  {slug}: {message}");
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
    exit_code(passing == targets.len())
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

fn first_lines(text: &str, count: usize) -> String {
    text.lines()
        .filter(|line| !line.is_empty())
        .take(count)
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned()
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
