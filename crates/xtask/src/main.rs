use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{Args, Parser, Subcommand};
use serde_json::Value;

mod checks;
mod licenses;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root")
}

#[derive(Parser)]
#[command(about = "typesep's development checks", disable_version_flag = true)]
struct Cli {
    #[command(subcommand)]
    command: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Compare the model snapshots with what `dump model` prints, and report matching / total.
    Parity(Parity),
    /// Typeset every fixture entry and report rendered / total.
    RenderCheck(Walk),
    /// Build every fixture entry as an EPUB, validate it with EPUBCheck, and report passing / total.
    EpubCheck(Walk),
    /// Write THIRD-PARTY-LICENSES.txt, or resolve every license expression without writing it.
    Licenses(Licenses),
}

#[derive(Args)]
struct Walk {
    /// The binary to drive.
    #[arg(long, value_name = "PATH", default_value = "target/release/typesep")]
    exec: PathBuf,
    /// Where the outputs go while they are being checked.
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
    /// Walk the whole library instead of the fixture set.
    #[arg(long)]
    all: bool,
    /// Leave the files behind instead of deleting them as they pass.
    #[arg(long)]
    keep: bool,
    /// The library the binary reads; the corpus's own by default.
    #[arg(long, value_name = "DIR")]
    library: Option<PathBuf>,
    /// Say what is happening, entry by entry.
    #[arg(long)]
    verbose: bool,
}

#[derive(Args)]
struct Licenses {
    /// The platform the notice describes; without it, the union of all four.
    #[arg(long, value_name = "TARGET")]
    target: Option<String>,
    /// Resolve every license expression for every target and write nothing.
    #[arg(long)]
    validate: bool,
    /// Where to write the notice.
    #[arg(long, value_name = "FILE")]
    out: Option<PathBuf>,
}

#[derive(Parser)]
struct Parity {
    /// Where the `<slug>.json` model snapshots are.
    #[arg(long, value_name = "DIR")]
    snapshots: Option<PathBuf>,
    /// Read candidates from `DIR/<slug>.json` instead of running a command.
    #[arg(long, value_name = "DIR", conflicts_with = "exec")]
    candidates: Option<PathBuf>,
    /// The command whose stdout is the candidate model; `{}` is replaced by the slug.
    #[arg(long, value_name = "CMD")]
    exec: Option<String>,
    /// Only the slugs this regular expression matches.
    #[arg(long, value_name = "REGEX")]
    only: Option<String>,
    /// The library the default command reads; ignored when --exec is given.
    #[arg(long, value_name = "DIR")]
    library: Option<PathBuf>,
    /// The summary line and nothing else.
    #[arg(long)]
    quiet: bool,
}

const DEFAULT_EXEC: &str = "target/debug/typesep dump model {} --sections all --quiet";

/// The corpus is thirty SEP entries and the models they parse to. It is not in this
/// repository: the entries are copyright their authors and the SEP's Terms of Use grant no
/// right to redistribute them, so it lives outside the tree. `TYPESEP_TEST_CORPUS` names the
/// directory; it holds `library/` and `snapshots/model/`.
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

fn corpus_subdir(name: &str) -> PathBuf {
    let dir = corpus_dir().join(name);
    if !dir.is_dir() {
        fail(&format!(
            "no corpus at {}. The thirty fixture entries and their models are not in this \
             repository; point TYPESEP_TEST_CORPUS at the directory holding library/ and \
             snapshots/model/.",
            dir.display()
        ));
    }
    dir
}

fn main() -> std::process::ExitCode {
    let root = root();
    match Cli::parse().command {
        Task::Parity(args) => parity(args),
        Task::RenderCheck(args) => checks::render_check(walk(args, &root, "render-check")),
        Task::EpubCheck(args) => {
            let walk = walk(args, &root, "epub-check");
            checks::epub_check(walk, &root)
        }
        Task::Licenses(args) => licenses::run(&root, args.target, args.validate, args.out),
    }
}

fn walk(args: Walk, root: &Path, name: &str) -> checks::CheckArgs {
    checks::CheckArgs {
        binary: root.join(&args.exec),
        library: args.library.unwrap_or_else(|| corpus_subdir("library")),
        out: args
            .out
            .map_or_else(|| root.join("out").join(name), |out| root.join(out)),
        snapshots: corpus_subdir("snapshots/model"),
        all: args.all,
        keep: args.keep,
        verbose: args.verbose,
    }
}

enum Outcome {
    Match,
    Missing(String),
    Invalid(String),
    Differs(Difference),
}

struct Difference {
    path: String,
    snapshot: Option<Value>,
    candidate: Option<Value>,
}

fn parity(args: Parity) -> std::process::ExitCode {
    let root = root();
    let snapshots = args
        .snapshots
        .unwrap_or_else(|| corpus_subdir("snapshots/model"));
    let only = args.only.as_deref().map(|pattern| {
        regex::Regex::new(pattern).unwrap_or_else(|err| fail(&format!("--only: {err}")))
    });

    let entries = fs::read_dir(&snapshots)
        .unwrap_or_else(|_| fail(&format!("cannot read {}", snapshots.display())));
    let mut slugs: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let slug = name.strip_suffix(".json")?.to_owned();
            only.as_ref()
                .is_none_or(|re| re.is_match(&slug))
                .then_some(slug)
        })
        .collect();
    slugs.sort();
    if slugs.is_empty() {
        fail(&format!("no model snapshots in {}", snapshots.display()));
    }

    let exec = args.exec.clone().or_else(|| {
        let library = args
            .library
            .clone()
            .unwrap_or_else(|| corpus_subdir("library"));
        Some(format!(
            "{DEFAULT_EXEC} --library {}",
            shell_quote(&library)
        ))
    });

    let mut matching = 0usize;
    for slug in &slugs {
        let outcome = compare(slug, &snapshots, &args.candidates, exec.as_deref(), &root);
        if matches!(outcome, Outcome::Match) {
            matching += 1;
            continue;
        }
        if args.quiet {
            continue;
        }
        let label = format!("{slug:<28}");
        match outcome {
            Outcome::Match => unreachable!(),
            Outcome::Missing(detail) => println!("{label} candidate missing ({detail})"),
            Outcome::Invalid(detail) => println!("{label} candidate is not JSON ({detail})"),
            Outcome::Differs(diff) => println!(
                "{label} differs at {}: snapshot {} / candidate {}",
                if diff.path.is_empty() {
                    "(root)"
                } else {
                    &diff.path
                },
                show(diff.snapshot.as_ref()),
                show(diff.candidate.as_ref())
            ),
        }
    }

    println!("{matching} / {}", slugs.len());
    if matching == slugs.len() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

fn compare(
    slug: &str,
    snapshots: &Path,
    candidates: &Option<PathBuf>,
    exec: Option<&str>,
    root: &Path,
) -> Outcome {
    let snapshot_text = match fs::read_to_string(snapshots.join(format!("{slug}.json"))) {
        Ok(text) => text,
        Err(err) => fail(&format!("{slug}: {err}")),
    };
    let snapshot: Value = match serde_json::from_str(&snapshot_text) {
        Ok(value) => value,
        Err(err) => fail(&format!("{slug}: the snapshot is not JSON: {err}")),
    };
    let text = match candidate_text(slug, candidates, exec, root) {
        Ok(text) => text,
        Err(detail) => return Outcome::Missing(detail),
    };
    let candidate: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(err) => return Outcome::Invalid(err.to_string()),
    };
    match first_difference(Some(&snapshot), Some(&candidate), String::new()) {
        None => Outcome::Match,
        Some(diff) => Outcome::Differs(diff),
    }
}

fn candidate_text(
    slug: &str,
    candidates: &Option<PathBuf>,
    exec: Option<&str>,
    root: &Path,
) -> Result<String, String> {
    if let Some(dir) = candidates {
        let file = dir.join(format!("{slug}.json"));
        return fs::read_to_string(&file).map_err(|_| file.display().to_string());
    }
    let command = exec.unwrap_or(DEFAULT_EXEC).replace("{}", slug);
    let run = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .current_dir(root)
        .output()
        .map_err(|err| format!("{command}: {err}"))?;
    if !run.status.success() {
        let stderr = String::from_utf8_lossy(&run.stderr);
        let last = stderr
            .trim_end()
            .lines()
            .next_back()
            .unwrap_or("")
            .to_owned();
        let code = run
            .status
            .code()
            .map_or_else(|| "on a signal".to_owned(), |code| code.to_string());
        return Err(if last.is_empty() {
            format!("command exited {code}")
        } else {
            format!("command exited {code}: {last}")
        });
    }
    Ok(String::from_utf8_lossy(&run.stdout).into_owned())
}

fn first_difference(
    snapshot: Option<&Value>,
    candidate: Option<&Value>,
    path: String,
) -> Option<Difference> {
    match (snapshot, candidate) {
        (Some(Value::Array(a)), Some(Value::Array(b))) => (0..a.len().max(b.len()))
            .find_map(|i| first_difference(a.get(i), b.get(i), format!("{path}[{i}]"))),
        (Some(Value::Object(a)), Some(Value::Object(b))) => {
            let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            keys.into_iter().find_map(|key| {
                let below = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                first_difference(a.get(key), b.get(key), below)
            })
        }
        (Some(a), Some(b)) if same_scalar(a, b) => None,
        (None, None) => None,
        _ => Some(Difference {
            path,
            snapshot: snapshot.cloned(),
            candidate: candidate.cloned(),
        }),
    }
}

fn same_scalar(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => x == y,
        },
        _ => a == b,
    }
}

fn show(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return "(absent)".to_owned();
    };
    let text = serde_json::to_string(value).unwrap_or_else(|_| "(unprintable)".to_owned());
    if text.chars().count() > 80 {
        format!("{}...", text.chars().take(77).collect::<String>())
    } else {
        text
    }
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}

pub(crate) fn fail(message: &str) -> ! {
    eprintln!("xtask: {message}");
    std::process::exit(2)
}
