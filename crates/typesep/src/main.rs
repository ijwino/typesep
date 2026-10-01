use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Arg, ArgAction, ArgMatches, Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use output::{file_name_for, job_name_for, output_file};
use source_sep::entry::{
    EntryTarget, LoadOptions, LoadedEntry, UpdateStatus, load_entry, normalize_entry,
    related_entry_urls,
};
use source_sep::parse::volume_meta;
use source_sep::sections::{ContentKey, default_sections_text, key_names, parse_content_keys};
use typesep_epub::{EpubOptions, build_epub};
use typesep_model::canonical::model_json;
use typesep_model::{Doc, Meta};
use typesep_net::fetch::{DEFAULT_DELAY_MS, DEFAULT_TIMEOUT_MS, Fetcher, Loudness};
use typesep_net::figures::fetch_figures;
use typesep_net::library::{Library, default_library_dir};
use typesep_net::net::Source;
use typesep_style::shorthand::{choices_for, is_boolean, shorthand};
use typesep_style::{
    Explained, PRESET_KEY, ResolveOptions, SHORTHANDS, SetItem, Style, StyleError, Value, View,
    explain_path, list_presets, match_ignoring_case, preset_catalog, preset_text, resolve_style,
    shorthand_assignments,
};
use typesep_typst::preamble::emitter_for;
use typesep_typst::{
    EmitOptions, Fonts, compile_document, describe_missing_fonts, emit_document, font_dirs,
    font_problems, images, load_font_files,
};

mod output;
mod reference;

const CONFIG_FILE: &str = "typesep.config.yaml";

const AFTER_HELP: &str = "\
Style flags are shortcuts for -s: --paper a4 means -s page.size=a4. When both touch the same
option, -s wins. Settings merge in this order, later winning: built-in defaults, the preset,
--style (or typesep.config.yaml in the current directory), then the flags above. The preset is
--preset when given, else the \"preset\" key of the style file, else halfletter.

--edition: SEP's robots.txt excludes /archives/, so use a fixed edition deliberately and sparingly.";

#[derive(Parser)]
#[command(
    name = "typesep",
    version,
    about = "Typeset Stanford Encyclopedia of Philosophy entries as PDF/EPUB (Rust port, in progress)",
    after_help = AFTER_HELP,
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(flatten)]
    build: BuildArgs,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Args)]
struct BuildArgs {
    /// Entry slugs or URLs, e.g. turing-machine
    #[arg(value_name = "entry")]
    entries: Vec<String>,

    #[command(flatten)]
    style: StyleOptions,

    #[command(flatten)]
    output: OutputOptions,

    #[command(flatten)]
    content: ContentOptions,

    #[command(flatten)]
    fetching: FetchOptions,

    #[command(flatten)]
    network: NetworkOptions,

    /// Also fetch entries linked under Related Entries, this many levels deep
    #[arg(
        long,
        value_name = "depth",
        default_value_t = 0,
        help_heading = "Fetching"
    )]
    follow_related: u32,

    #[command(flatten)]
    verbosity: Verbosity,
}

#[derive(Args, Clone)]
struct Verbosity {
    /// Print only output paths and errors
    #[arg(long, help_heading = "Options", conflicts_with = "verbose")]
    quiet: bool,

    /// Say more: the library and style in use, every request, and how long each stage took
    #[arg(short = 'v', long, help_heading = "Options")]
    verbose: bool,
}

#[derive(Args, Clone)]
#[command(next_help_heading = "Style")]
struct StyleOptions {
    /// Starting point; "typesep presets" lists them, "none" is the bare defaults [default: halfletter]
    #[arg(long, value_name = "name")]
    preset: Option<String>,

    /// YAML/JSON style file layered on top of the preset (may name its own with a "preset" key)
    #[arg(long, value_name = "file")]
    style: Option<PathBuf>,

    /// Override one style option, e.g. -s page.margins.top=1in (repeatable)
    #[arg(short = 's', long = "set", value_name = "path=value", action = ArgAction::Append)]
    set: Vec<String>,
}

#[derive(Args, Clone)]
#[command(next_help_heading = "Output")]
struct OutputOptions {
    /// Output directory
    #[arg(short = 'o', long, value_name = "dir", default_value = "out")]
    out: PathBuf,

    /// What to produce: pdf, epub, or both of them as a list
    #[arg(long, value_name = "list", value_parser = parse_formats, default_value = "pdf")]
    format: Formats,

    /// Combine all entries into one document
    #[arg(long, value_name = "name")]
    merge: Option<String>,

    /// Keep the generated .typ next to the PDF
    #[arg(long)]
    keep_typst: bool,
}

const FORMATS: [&str; 2] = ["pdf", "epub"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Formats {
    pdf: bool,
    epub: bool,
}

fn parse_formats(value: &str) -> Result<Formats, String> {
    let mut formats = Formats {
        pdf: false,
        epub: false,
    };
    for part in value.split(',') {
        let part = part.trim();
        match match_ignoring_case(&FORMATS, part) {
            Some("pdf") => formats.pdf = true,
            Some("epub") => formats.epub = true,
            _ => {
                return Err(format!(
                    "Allowed choices are {}, or a comma list of them.",
                    FORMATS.join(", ")
                ));
            }
        }
    }
    if !formats.pdf && !formats.epub {
        return Err(format!(
            "Allowed choices are {}, or a comma list of them.",
            FORMATS.join(", ")
        ));
    }
    Ok(formats)
}

#[derive(Args, Clone)]
#[command(next_help_heading = "Content")]
struct ContentOptions {
    /// bib,academic,oir,related,notes,supplements | all | none
    #[arg(long, value_name = "list", value_parser = parse_sections, default_value_t = default_sections_text())]
    sections: String,

    /// Leave images out
    #[arg(long = "no-images", action = ArgAction::SetTrue)]
    no_images: bool,
}

fn parse_sections(spec: &str) -> Result<String, String> {
    parse_content_keys(spec)
        .map(|_| spec.to_owned())
        .map_err(|e| e.to_string())
}

#[derive(Args, Clone)]
#[command(next_help_heading = "Fetching")]
struct FetchOptions {
    /// Where fetched pages and figures are kept, for good [default: TYPESEP_LIBRARY or the platform data directory]
    #[arg(long, value_name = "dir")]
    library: Option<PathBuf>,

    /// live, or one fixed archive edition such as sum2026
    #[arg(long, value_name = "name", default_value = "live")]
    edition: String,
}

impl FetchOptions {
    fn library_dir(&self) -> PathBuf {
        self.library.clone().unwrap_or_else(default_library_dir)
    }
}

#[derive(Args, Clone)]
#[command(next_help_heading = "Fetching")]
struct NetworkOptions {
    /// Revalidate against the server instead of trusting the library's copy
    #[arg(long)]
    refresh: bool,

    /// Gap between requests, per SEP's robots.txt
    #[arg(long, value_name = "ms", default_value_t = DEFAULT_DELAY_MS)]
    delay: u64,

    /// Read entries from a file, one per line (- for stdin; repeatable)
    #[arg(long, value_name = "file", action = ArgAction::Append)]
    from: Vec<PathBuf>,
}

impl NetworkOptions {
    fn fetcher(&self, verbosity: &Verbosity) -> Fetcher {
        let quiet = verbosity.quiet;
        let loud = verbosity.verbose;
        Fetcher::live(
            Duration::from_millis(self.delay),
            Duration::from_millis(DEFAULT_TIMEOUT_MS),
        )
        .with_log(move |loudness, message| {
            let worth_saying = match loudness {
                Loudness::Detail => loud,
                Loudness::Notice => !quiet,
            };
            if worth_saying {
                eprintln!("  {message}");
            }
        })
    }
}

fn entries_with_from(named: &[String], from: &[PathBuf]) -> Result<Vec<String>, ExitCode> {
    let mut entries = named.to_vec();
    for file in from {
        let text = if file.as_os_str() == "-" {
            let mut text = String::new();
            match std::io::Read::read_to_string(&mut std::io::stdin().lock(), &mut text) {
                Ok(_) => text,
                Err(err) => return Err(user_error(format!("cannot read entry list -: {err}"))),
            }
        } else {
            match std::fs::read_to_string(file) {
                Ok(text) => text,
                Err(_) => {
                    return Err(user_error(format!(
                        "cannot read entry list {}",
                        show_path(file)
                    )));
                }
            }
        };
        for line in text.split('\n') {
            let line = line.split('#').next().unwrap_or("").trim();
            if !line.is_empty() {
                entries.push(line.to_owned());
            }
        }
    }
    Ok(entries)
}

#[derive(Subcommand)]
enum Command {
    /// Typeset entries; the same as naming them with no command at all
    Build(BuildArgs),
    /// Fetch entries into the library without typesetting them
    Fetch(FetchCommandArgs),
    /// Ask the server whether the library's entries have been revised
    Update(UpdateArgs),
    /// Print the fully merged style as YAML
    Style(StyleCommandOptions),
    /// Print what a stage of the pipeline produced, and stop
    #[command(subcommand)]
    Dump(DumpCommand),
    /// List the bundled presets
    Presets,
    /// Copy a preset into the current directory as a starting point for --style
    Eject(EjectOptions),
    /// Describe style options and the Typst they drive
    Explain(ExplainOptions),
    /// List the font families available to --font
    Fonts,
    /// List the entries in the library with their revision dates
    List(ListOptions),
    /// Write a shell completion script to stdout
    Completions(CompletionsOptions),
    /// Write the manual page, as roff, to stdout
    #[command(hide = true)]
    Man,
    /// Write docs/style-reference.md, generated from the option table, to stdout
    #[command(hide = true)]
    Reference,
}

#[derive(Subcommand)]
enum DumpCommand {
    /// The parsed document model as canonical JSON. Takes no style flags: the model does not
    /// depend on the style.
    Model(DumpModelOptions),
    /// The Typst source the renderer would compile
    Typst(DumpTypstOptions),
}

#[derive(Args)]
struct FetchCommandArgs {
    /// Entry slugs or URLs, e.g. turing-machine
    #[arg(value_name = "entry")]
    entries: Vec<String>,

    #[command(flatten)]
    content: ContentOptions,

    #[command(flatten)]
    fetching: FetchOptions,

    #[command(flatten)]
    network: NetworkOptions,

    /// Also fetch entries linked under Related Entries, this many levels deep
    #[arg(
        long,
        value_name = "depth",
        default_value_t = 0,
        help_heading = "Fetching"
    )]
    follow_related: u32,

    #[command(flatten)]
    verbosity: Verbosity,
}

#[derive(Args)]
struct UpdateArgs {
    /// Entry slugs or URLs; with none, every live entry the library holds
    #[arg(value_name = "entry")]
    entries: Vec<String>,

    /// bib,academic,oir,related,notes,supplements | all | none
    #[arg(long, value_name = "list", value_parser = parse_sections, default_value_t = default_sections_text(), help_heading = "Content")]
    sections: String,

    #[command(flatten)]
    fetching: FetchOptions,

    #[command(flatten)]
    network: NetworkOptions,

    #[command(flatten)]
    verbosity: Verbosity,
}

#[derive(Args)]
struct StyleCommandOptions {
    #[command(flatten)]
    style: StyleOptions,
}

#[derive(Args)]
struct DumpModelOptions {
    /// Entry slugs or URLs, e.g. turing-machine
    #[arg(value_name = "entry")]
    entries: Vec<String>,

    /// bib,academic,oir,related,notes,supplements | all | none
    #[arg(long, value_name = "list", value_parser = parse_sections, default_value_t = default_sections_text(), help_heading = "Content")]
    sections: String,

    #[command(flatten)]
    fetching: FetchOptions,

    #[command(flatten)]
    verbosity: Verbosity,
}

#[derive(Args)]
struct DumpTypstOptions {
    /// Entry slugs or URLs, e.g. turing-machine
    #[arg(value_name = "entry")]
    entries: Vec<String>,

    #[command(flatten)]
    style: StyleOptions,

    #[command(flatten)]
    content: ContentOptions,

    #[command(flatten)]
    fetching: FetchOptions,

    #[command(flatten)]
    verbosity: Verbosity,

    /// Combine all entries into one document
    #[arg(long, value_name = "name", help_heading = "Output")]
    merge: Option<String>,
}

impl DumpTypstOptions {
    fn as_build(&self) -> BuildArgs {
        BuildArgs {
            entries: self.entries.clone(),
            style: self.style.clone(),
            output: OutputOptions {
                out: PathBuf::from("out"),
                format: Formats {
                    pdf: true,
                    epub: false,
                },
                merge: self.merge.clone(),
                keep_typst: false,
            },
            content: self.content.clone(),
            fetching: self.fetching.clone(),
            network: NetworkOptions {
                refresh: false,
                delay: DEFAULT_DELAY_MS,
                from: Vec::new(),
            },
            follow_related: 0,
            verbosity: self.verbosity.clone(),
        }
    }
}

#[derive(Args)]
struct CompletionsOptions {
    /// bash, zsh, fish, powershell or elvish
    #[arg(value_name = "shell")]
    shell: clap_complete::Shell,
}

#[derive(Args)]
struct EjectOptions {
    /// Name of a bundled preset
    #[arg(value_name = "preset")]
    preset: String,

    /// Overwrite an existing file of the same name
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct ExplainOptions {
    /// A style option such as typography.leading, a group such as page.margins, or a flag such as --font
    #[arg(value_name = "option", allow_hyphen_values = true)]
    option: String,
}

#[derive(Args)]
struct ListOptions {
    /// Where fetched pages and figures are kept [default: TYPESEP_LIBRARY or the platform data directory]
    #[arg(long, value_name = "dir")]
    library: Option<PathBuf>,
}

fn with_shorthands(mut cmd: clap::Command) -> clap::Command {
    for sh in &SHORTHANDS {
        if is_boolean(sh) {
            let off = format!("no-{}", sh.flag);
            cmd = cmd
                .arg(
                    Arg::new(sh.flag)
                        .long(sh.flag)
                        .action(ArgAction::SetTrue)
                        .overrides_with(off.clone())
                        .help(format!("{} (--no-{} turns it off)", sh.doc, sh.flag))
                        .help_heading("Style"),
                )
                .arg(
                    Arg::new(off.clone())
                        .long(off)
                        .action(ArgAction::SetTrue)
                        .overrides_with(sh.flag)
                        .hide(true)
                        .help_heading("Style"),
                );
        } else {
            let placeholder = sh.placeholder.unwrap_or("<value>");
            let mut arg = Arg::new(sh.flag)
                .long(sh.flag)
                .value_name(
                    placeholder
                        .trim_matches(|c| c == '<' || c == '>')
                        .to_owned(),
                )
                .help(sh.doc)
                .help_heading("Style");
            if let Some(choices) = choices_for(sh) {
                let quoted: Vec<String> = choices.iter().map(|c| format!("\"{c}\"")).collect();
                arg = arg.help(format!("{} (choices: {})", sh.doc, quoted.join(", ")));
                arg = arg.value_parser(move |value: &str| {
                    match_ignoring_case(choices, value)
                        .map(str::to_owned)
                        .ok_or_else(|| format!("Allowed choices are {}.", choices.join(", ")))
                });
            }
            cmd = cmd.arg(arg);
        }
    }
    cmd
}

fn shorthand_values(matches: &ArgMatches) -> Vec<(&'static str, Value)> {
    let mut out = Vec::new();
    for sh in &SHORTHANDS {
        if is_boolean(sh) {
            if matches.get_flag(&format!("no-{}", sh.flag)) {
                out.push((sh.flag, Value::Bool(false)));
            } else if matches.get_flag(sh.flag) {
                out.push((sh.flag, Value::Bool(true)));
            }
        } else if let Some(value) = matches.get_one::<String>(sh.flag) {
            out.push((sh.flag, Value::str(value)));
        }
    }
    out
}

fn show_path(path: &Path) -> String {
    match std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok().map(Path::to_owned))
    {
        Some(rel) if !rel.as_os_str().is_empty() => rel.display().to_string(),
        _ => path.display().to_string(),
    }
}

const USER_ERROR: u8 = 1;
const USAGE_ERROR: u8 = 2;

fn write_out(text: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    if let Err(err) = out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        if err.kind() == std::io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        eprintln!("typesep: cannot write to stdout: {err}");
        std::process::exit(USER_ERROR as i32);
    }
}

macro_rules! outln {
    ($($arg:tt)*) => { crate::write_out(&format!("{}\n", format_args!($($arg)*))) };
}
macro_rules! out {
    ($($arg:tt)*) => { crate::write_out(&format_args!($($arg)*).to_string()) };
}

fn user_error(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("typesep: {message}");
    ExitCode::from(USER_ERROR)
}

fn usage_error(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::from(USAGE_ERROR)
}

fn list(opts: &ListOptions) -> ExitCode {
    let dir = opts.library.clone().unwrap_or_else(default_library_dir);
    let entries = Library::new(&dir).list_entries();
    if entries.is_empty() {
        eprintln!("nothing in the library yet at {}", show_path(&dir));
        return ExitCode::SUCCESS;
    }
    for e in entries {
        let edition = if e.edition == "live" {
            String::new()
        } else {
            format!("[{}] ", e.edition)
        };
        let revision = if e.revision.is_empty() {
            "-"
        } else {
            e.revision.as_str()
        };
        outln!("{:<28} {:<26} {edition}{}", e.slug, revision, e.title);
    }
    ExitCode::SUCCESS
}

fn fonts_command() -> ExitCode {
    let families = Fonts::discover(&font_dirs(), true).families();
    if families.is_empty() {
        outln!("no fonts found");
    } else {
        outln!("{}", families.join("\n"));
    }
    ExitCode::SUCCESS
}

fn presets() -> ExitCode {
    for (name, summary) in preset_catalog() {
        outln!("{name:<14} {summary}");
    }
    ExitCode::SUCCESS
}

fn ejected_text(text: &str) -> String {
    let preset_line = format!("{PRESET_KEY}: none");
    let mut lines: Vec<&str> = text.split('\n').collect();
    let at = lines.iter().take_while(|l| l.starts_with('#')).count();
    lines.splice(at..at, [preset_line.as_str(), ""]);
    lines.join("\n")
}

fn eject(opts: &EjectOptions) -> ExitCode {
    let Some(text) = preset_text(&opts.preset) else {
        return user_error(format!(
            "unknown preset \"{}\"\n  available: {}",
            opts.preset,
            list_presets().join(", ")
        ));
    };
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(err) => return user_error(err),
    };
    let file_name = format!("{}.yaml", opts.preset);
    let dest = cwd.join(&file_name);
    if dest.exists() && !opts.force {
        return user_error(format!(
            "{} already exists; pass --force to overwrite it",
            show_path(&dest)
        ));
    }
    if let Err(err) = std::fs::write(&dest, ejected_text(text)) {
        return user_error(format!("cannot write {}: {err}", show_path(&dest)));
    }
    outln!(
        "{}\nedit it, then use: typesep <entry> --style {file_name}",
        show_path(&dest)
    );
    ExitCode::SUCCESS
}

fn path_for_flag(arg: &str) -> Result<Option<String>, String> {
    let name = arg.strip_prefix("--").unwrap_or(arg);
    if let Some(sh) = shorthand(name) {
        return Ok(Some(
            sh.path.strip_suffix(".*").unwrap_or(sh.path).to_owned(),
        ));
    }
    if !arg.starts_with("--") {
        return Ok(None);
    }
    Err(format!(
        "{arg} is not a style flag\n  style flags: {}",
        SHORTHANDS
            .iter()
            .map(|s| format!("--{}", s.flag))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn explain(opts: &ExplainOptions) -> ExitCode {
    let arg = opts.option.as_str();
    let dotted = match path_for_flag(arg) {
        Ok(Some(path)) => path,
        Ok(None) => arg.to_owned(),
        Err(message) => return user_error(message),
    };
    match explain_path(&dotted) {
        Err(err) => user_error(err),
        Ok(Explained::Group { prefix, knobs }) => {
            outln!("{prefix}.* covers {} options:", knobs.len());
            for k in knobs {
                outln!(
                    "  {:<34} {} (default {})",
                    k.path,
                    k.doc,
                    k.default.to_json()
                );
            }
            ExitCode::SUCCESS
        }
        Ok(Explained::Knob { knob, field }) => {
            if let Some(field) = field {
                let via = if arg != dotted {
                    format!(" ({arg})")
                } else {
                    String::new()
                };
                outln!("{dotted} is the \"{field}\" field of {}{via}\n", knob.path);
            } else if arg != dotted {
                outln!("{arg} sets {}\n", knob.path);
            }
            outln!(
                "{}\n  {}\n  accepts: {}",
                knob.path,
                knob.doc,
                knob.kind.describe()
            );
            outln!("  default: {}", knob.default.to_json());
            let users: Vec<String> = emitter_for(&knob.path)
                .iter()
                .map(|e| format!("{} — {}", e.name, e.typst))
                .collect();
            if !users.is_empty() {
                outln!("  drives:  {}", users.join("\n           "));
            }
            ExitCode::SUCCESS
        }
    }
}

fn resolve(style: &StyleOptions, matches: &ArgMatches) -> Result<Style, StyleError> {
    let config = Path::new(CONFIG_FILE);
    let file: Option<PathBuf> = style
        .style
        .clone()
        .or_else(|| config.exists().then(|| config.to_path_buf()));
    let mut sets: Vec<SetItem> = shorthand_assignments(&shorthand_values(matches))
        .into_iter()
        .map(SetItem::from)
        .collect();
    sets.extend(style.set.iter().cloned().map(SetItem::from));
    resolve_style(ResolveOptions {
        preset: style.preset.as_deref(),
        file: file.as_deref(),
        sets,
    })
}

fn style_command(opts: &StyleCommandOptions, matches: &ArgMatches) -> ExitCode {
    match resolve(&opts.style, matches) {
        Ok(style) => {
            outln!("{}", style.to_yaml());
            ExitCode::SUCCESS
        }
        Err(err) => user_error(err),
    }
}

struct Group<'a> {
    name: String,
    docs: Vec<&'a Doc>,
    volume: Option<Meta>,
}

fn entry_name(doc: &Doc) -> String {
    if doc.meta.origin.edition.is_empty() {
        doc.meta.id.clone()
    } else {
        format!("{}-{}", doc.meta.id, doc.meta.origin.edition)
    }
}

fn group_entries<'a>(docs: &'a [Doc], merge: Option<&str>) -> Vec<Group<'a>> {
    match merge {
        Some(_) if docs.is_empty() => Vec::new(),
        Some(name) => {
            let docs: Vec<&Doc> = docs.iter().collect();
            vec![Group {
                name: name.to_owned(),
                volume: Some(volume_meta(&docs, name)),
                docs,
            }]
        }
        None => docs
            .iter()
            .map(|doc| Group {
                name: entry_name(doc),
                docs: vec![doc],
                volume: None,
            })
            .collect(),
    }
}

struct RenderCtx<'a> {
    style: &'a Style,
    fonts: &'a Fonts,
    sections: &'a BTreeSet<ContentKey>,
    library: &'a Library,
    cli: &'a BuildArgs,
    dump_typst: bool,
}

fn walk_entries(
    source: &mut Source<'_>,
    entries: &[String],
    edition: &str,
    depth: u32,
    opts: &WalkOptions<'_>,
    failures: &mut Vec<(String, String)>,
    deliver: &mut dyn FnMut(LoadedEntry) -> Result<(), String>,
) -> Result<usize, ExitCode> {
    let mut queue: std::collections::VecDeque<(String, u32)> =
        entries.iter().map(|e| (e.clone(), 0)).collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut attempted = 0usize;
    let mut delivered = 0usize;
    let quiet = opts.quiet;
    let batch = entries.len() > 1 || depth > 0;

    while let Some((entry, reached_at)) = queue.pop_front() {
        let target = match normalize_entry(&entry, edition) {
            Ok(target) => target,
            Err(err) => {
                if !batch {
                    return Err(usage_error(err));
                }
                failures.push((entry, err.to_string()));
                continue;
            }
        };
        if !seen.insert(target.url.clone()) {
            continue;
        }
        let name = if target.edition == "live" {
            target.slug.clone()
        } else {
            format!("{} ({})", target.slug, target.edition)
        };

        attempted += 1;
        let total = attempted + queue.len();
        let counter = if total > 1 {
            format!("[{attempted}/{total}] ")
        } else {
            String::new()
        };
        let deeper = if reached_at > 0 {
            format!(" (depth {reached_at})")
        } else {
            String::new()
        };
        if !quiet {
            let verb = opts.verb.word(source, &target);
            eprintln!("{counter}{verb} {name}{deeper}");
        }

        let mut log = |message: &str| {
            if !quiet {
                eprintln!("{message}");
            }
        };
        let loaded = match load_entry(
            source,
            &target,
            LoadOptions {
                content: opts.sections,
                log: &mut log,
            },
        ) {
            Ok(loaded) => loaded,
            Err(err) => {
                if !batch {
                    return Err(user_error(err));
                }
                failures.push((name.clone(), err.to_string()));
                if !quiet {
                    eprintln!("  {name} failed, continuing");
                }
                continue;
            }
        };

        if opts.images {
            let mut note = |message: &str| {
                if !quiet {
                    eprintln!("  {message}");
                }
            };
            let pass = fetch_figures(&loaded.doc, source, &mut note);
            if pass.fetched > 0 && !quiet {
                eprintln!("  {} figures fetched", pass.fetched);
            }
        }

        if reached_at < depth {
            for url in related_entry_urls(&loaded.doc) {
                queue.push_back((url, reached_at + 1));
            }
        }
        match deliver(loaded) {
            Ok(()) => delivered += 1,
            Err(message) => {
                if !batch {
                    return Err(user_error(message));
                }
                failures.push((name.clone(), message));
                if !quiet {
                    eprintln!("  {name} failed, continuing");
                }
            }
        }
    }
    Ok(delivered)
}

#[derive(Clone, Copy)]
enum Verb {
    Checking,
    Reading,
    Either,
}

impl Verb {
    fn word(self, source: &Source<'_>, target: &EntryTarget) -> &'static str {
        match self {
            Verb::Checking => "checking",
            Verb::Reading => "reading",
            Verb::Either => {
                if !source.refresh && source.library.has(&target.url).unwrap_or(false) {
                    "reading"
                } else {
                    "fetching"
                }
            }
        }
    }
}

struct WalkOptions<'a> {
    verb: Verb,
    sections: &'a BTreeSet<ContentKey>,
    images: bool,
    quiet: bool,
}

fn entry_word(count: usize) -> &'static str {
    if count == 1 { "entry" } else { "entries" }
}

fn report_failures(failures: &[(String, String)], built: usize) -> ExitCode {
    if failures.is_empty() {
        return ExitCode::SUCCESS;
    }
    let total = built + failures.len();
    eprintln!(
        "\n{} of {total} {} failed:",
        failures.len(),
        entry_word(total)
    );
    for (name, message) in failures {
        let detail: Vec<String> = message.lines().map(|line| format!("  {line}")).collect();
        eprintln!("\n{name}:\n{}", detail.join("\n"));
    }
    ExitCode::from(USER_ERROR)
}

fn build(cli: &BuildArgs, matches: &ArgMatches, dump_typst: bool) -> ExitCode {
    let quiet = cli.verbosity.quiet;
    let loud = cli.verbosity.verbose;
    let mut style = match resolve(&cli.style, matches) {
        Ok(style) => style,
        Err(err) => return user_error(err),
    };

    let entries = match entries_with_from(&cli.entries, &cli.network.from) {
        Ok(entries) => entries,
        Err(code) => return code,
    };
    if entries.is_empty() {
        return usage_error("no entry given");
    }
    let sections: BTreeSet<ContentKey> =
        parse_content_keys(&cli.content.sections).expect("validated by clap");

    if let Err(code) = targets_for(&entries, &cli.fetching.edition) {
        return code;
    }

    let notes_set = matches.get_one::<String>("notes").is_some()
        || cli
            .style
            .set
            .iter()
            .any(|s| s.trim().starts_with("notes.style="));
    if notes_set && !sections.contains(&ContentKey::Notes) && !quiet {
        eprintln!(
            "  --notes has no effect while --sections leaves out notes: the notes page is not fetched and the markers stay links"
        );
    }

    let library_dir = cli.fetching.library_dir();
    let library = Library::new(&library_dir);
    if let Err(err) = library.init() {
        return user_error(err);
    }
    if loud {
        eprintln!("library {}", show_path(&library_dir));
        eprintln!("style   {}", describe_style_source(&cli.style));
    }

    let fonts = {
        let mut discovered = Fonts::discover(&font_dirs(), true);
        if let Err(err) = load_font_files(&mut style, &mut discovered) {
            return user_error(err);
        }
        let problems = font_problems(&View::new(&style), &discovered);
        if !problems.is_empty() {
            return user_error(describe_missing_fonts(&problems));
        }
        if loud {
            eprintln!("fonts   {} families", discovered.families().len());
        }
        discovered
    };

    let out_dir = &cli.output.out;
    if !dump_typst && let Err(err) = std::fs::create_dir_all(out_dir) {
        return user_error(format!("cannot create {}: {err}", show_path(out_dir)));
    }

    let mut fetcher = (!dump_typst).then(|| cli.network.fetcher(&cli.verbosity));
    let mut source = Source {
        library: &library,
        fetcher: fetcher.as_mut(),
        refresh: cli.network.refresh && !dump_typst,
        check_updates: false,
    };
    let ctx = RenderCtx {
        style: &style,
        fonts: &fonts,
        sections: &sections,
        library: &library,
        cli,
        dump_typst,
    };
    let walk = WalkOptions {
        verb: if dump_typst {
            Verb::Reading
        } else {
            Verb::Either
        },
        sections: &sections,
        images: !cli.content.no_images && !dump_typst,
        quiet,
    };

    let started = std::time::Instant::now();
    let mut failures: Vec<(String, String)> = Vec::new();
    let built = if let Some(merge) = cli.output.merge.as_deref() {
        let mut docs: Vec<Doc> = Vec::new();
        let read = match walk_entries(
            &mut source,
            &entries,
            &cli.fetching.edition,
            cli.follow_related,
            &walk,
            &mut failures,
            &mut |loaded| {
                docs.push(loaded.doc);
                Ok(())
            },
        ) {
            Ok(read) => read,
            Err(code) => return code,
        };
        if loud {
            eprintln!("read    {read} in {:.2} s", started.elapsed().as_secs_f64());
        }
        for group in group_entries(&docs, Some(merge)) {
            if let Err(message) = render_group(&group, &ctx) {
                return user_error(message);
            }
        }
        read
    } else {
        let mut built = 0usize;
        let walked = walk_entries(
            &mut source,
            &entries,
            &cli.fetching.edition,
            cli.follow_related,
            &walk,
            &mut failures,
            &mut |loaded| {
                let group = Group {
                    name: entry_name(&loaded.doc),
                    docs: vec![&loaded.doc],
                    volume: None,
                };
                render_group(&group, &ctx)?;
                built += 1;
                Ok(())
            },
        );
        if let Err(code) = walked {
            return code;
        }
        if loud {
            eprintln!(
                "read    {built} in {:.2} s",
                started.elapsed().as_secs_f64()
            );
        }
        built
    };
    report_failures(&failures, built)
}

fn cover_template(style: &Style) -> Result<Option<String>, String> {
    let value = |path: &str| style.get(path).map(Value::js_string).unwrap_or_default();
    if value("cover.style") != "custom" {
        return Ok(None);
    }
    let path = value("cover.template");
    if path.is_empty() {
        return Ok(None);
    }
    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| format!("cover.template {path}: {e}"))
}

fn render_group(group: &Group, ctx: &RenderCtx<'_>) -> Result<(), String> {
    let cli = ctx.cli;
    let quiet = cli.verbosity.quiet;
    let loud = cli.verbosity.verbose;
    let out_dir = &cli.output.out;
    let mut warnings: Vec<String> = Vec::new();
    let images = if cli.content.no_images {
        Default::default()
    } else {
        let mut collect = |message: String| warnings.push(message);
        group
            .docs
            .iter()
            .flat_map(|doc| images::collect(doc, &|url| ctx.library.copies_of(url), &mut collect))
            .collect()
    };
    let cover_template = cover_template(ctx.style)?;
    let sections = key_names(ctx.sections);
    let options = EmitOptions {
        sections: &sections,
        images: &images,
        cover_template: cover_template.as_deref(),
        volume: group.volume.as_ref(),
        fonts: Some(ctx.fonts),
        ..Default::default()
    };

    if ctx.dump_typst {
        let rendered = emit_document(&group.docs, ctx.style, options);
        warnings.extend(rendered.warnings);
        out!("{}", rendered.source);
        say_all(&warnings, quiet);
        return Ok(());
    }

    if !cli.output.format.pdf {
        build_epub_file(
            group,
            ctx.style,
            &sections,
            &images,
            ctx.fonts,
            cli,
            &mut warnings,
        )?;
        say_all(&warnings, quiet);
        return Ok(());
    }

    let job = job_name_for(&group.name);
    let source_file = out_dir.join(format!("{job}.typ"));
    let keep = cli.output.keep_typst;

    if !quiet {
        eprintln!("  typesetting {}", file_name_for(&group.name));
    }
    let typeset_at = std::time::Instant::now();
    let outcome = compile_document(
        &group.docs,
        ctx.style,
        options,
        ctx.fonts,
        &format!("typesep {}", env!("CARGO_PKG_VERSION")),
        &mut |message| say(&message, quiet),
    );
    let rendered = outcome.rendered;
    warnings.extend(rendered.warnings.iter().cloned());
    warnings.extend(outcome.notes);
    if keep && let Err(err) = std::fs::write(&source_file, &rendered.source) {
        return Err(format!("cannot write {}: {err}", show_path(&source_file)));
    }
    let compiled = match outcome.result {
        Ok(compiled) => compiled,
        Err(err) => {
            let kept = keep || std::fs::write(&source_file, &rendered.source).is_ok();
            let mut parts = vec!["Typst failed".to_owned()];
            parts.extend(err.lines.iter().map(|l| format!("  {l}")));
            if kept {
                parts.push(format!("  the source is in {}", show_path(&source_file)));
            }
            return Err(parts.join("\n"));
        }
    };

    let file = output_file(&group.name, "pdf", out_dir);
    if let Err(err) = std::fs::write(&file, &compiled.pdf) {
        return Err(format!("cannot write {}: {err}", show_path(&file)));
    }
    outln!("{}", show_path(&file));
    warnings.extend(compiled.warnings.iter().cloned());
    warnings.extend(
        compiled
            .moved_notes
            .iter()
            .map(|note| match group.docs.get(note.entry) {
                Some(doc) if group.docs.len() > 1 => format!("{}: {note}", doc.meta.id),
                _ => note.to_string(),
            }),
    );
    if !quiet {
        eprintln!("  {} pages", compiled.pages);
    }
    if loud {
        eprintln!("  typeset in {:.2} s", typeset_at.elapsed().as_secs_f64());
    }
    if cli.output.format.epub {
        build_epub_file(
            group,
            ctx.style,
            &sections,
            &images,
            ctx.fonts,
            cli,
            &mut warnings,
        )?;
    }
    say_all(&warnings, quiet);
    if keep {
        say(&format!("source in {}", show_path(&source_file)), quiet);
    }
    Ok(())
}

fn fetch_command(opts: &FetchCommandArgs) -> ExitCode {
    let quiet = opts.verbosity.quiet;
    let entries = match entries_with_from(&opts.entries, &opts.network.from) {
        Ok(entries) => entries,
        Err(code) => return code,
    };
    if entries.is_empty() {
        return usage_error("no entry given");
    }
    let sections: BTreeSet<ContentKey> =
        parse_content_keys(&opts.content.sections).expect("validated by clap");
    if let Err(code) = targets_for(&entries, &opts.fetching.edition) {
        return code;
    }

    let library_dir = opts.fetching.library_dir();
    let library = Library::new(&library_dir);
    if let Err(err) = library.init() {
        return user_error(err);
    }
    if opts.verbosity.verbose {
        eprintln!("library {}", show_path(&library_dir));
    }

    let mut fetcher = opts.network.fetcher(&opts.verbosity);
    let mut source = Source::fetching(&library, &mut fetcher).with_refresh(opts.network.refresh);
    let walk = WalkOptions {
        verb: Verb::Either,
        sections: &sections,
        images: !opts.content.no_images,
        quiet,
    };
    let mut failures: Vec<(String, String)> = Vec::new();
    let held = match walk_entries(
        &mut source,
        &entries,
        &opts.fetching.edition,
        opts.follow_related,
        &walk,
        &mut failures,
        &mut |_| Ok(()),
    ) {
        Ok(held) => held,
        Err(code) => return code,
    };
    let requests = source.fetcher.as_ref().map_or(0, |f| f.requests());
    if !quiet {
        eprintln!(
            "{held} {} in {}, {requests} requests",
            entry_word(held),
            show_path(&library_dir)
        );
    }
    report_failures(&failures, held)
}

fn update_command(opts: &UpdateArgs) -> ExitCode {
    let quiet = opts.verbosity.quiet;
    let library_dir = opts.fetching.library_dir();
    let library = Library::new(&library_dir);
    if let Err(err) = library.init() {
        return user_error(err);
    }

    let mut entries = match entries_with_from(&opts.entries, &opts.network.from) {
        Ok(entries) => entries,
        Err(code) => return code,
    };
    if entries.is_empty() {
        let held = library.list_entries();
        let live: Vec<&typesep_net::library::LibraryEntry> =
            held.iter().filter(|e| e.edition == "live").collect();
        if live.len() < held.len() && !quiet {
            eprintln!(
                "  {} archived entries in the library are left alone; name them with --edition to check them",
                held.len() - live.len()
            );
        }
        if live.is_empty() {
            return usage_error("the library holds no entries yet");
        }
        if !quiet {
            eprintln!(
                "checking {} {} of the library",
                live.len(),
                entry_word(live.len())
            );
        }
        entries = live.iter().map(|e| e.slug.clone()).collect();
    }
    let sections: BTreeSet<ContentKey> =
        parse_content_keys(&opts.sections).expect("validated by clap");
    if let Err(code) = targets_for(&entries, &opts.fetching.edition) {
        return code;
    }

    let mut fetcher = opts.network.fetcher(&opts.verbosity);
    let mut source = Source::fetching(&library, &mut fetcher)
        .with_refresh(opts.network.refresh)
        .with_check_updates(true);
    let walk = WalkOptions {
        verb: Verb::Checking,
        sections: &sections,
        images: false,
        quiet,
    };
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut changed = 0usize;
    let checked = match walk_entries(
        &mut source,
        &entries,
        &opts.fetching.edition,
        0,
        &walk,
        &mut failures,
        &mut |loaded| {
            let status = UpdateStatus::of(&loaded);
            if status.changed() {
                changed += 1;
            }
            let revision = if loaded.doc.meta.origin.revision.is_empty() {
                "(no revision date)"
            } else {
                &loaded.doc.meta.origin.revision
            };
            outln!("{:<28} {revision:<26} {status}", loaded.doc.meta.id);
            Ok(())
        },
    ) {
        Ok(checked) => checked,
        Err(code) => return code,
    };
    if !quiet {
        eprintln!(
            "{changed} of {checked} {} changed{}",
            entry_word(checked),
            if changed > 0 {
                "; typeset them with: typesep build <entry>..."
            } else {
                ""
            }
        );
    }
    report_failures(&failures, checked)
}

fn build_epub_file(
    group: &Group,
    style: &typesep_style::Style,
    sections: &BTreeSet<String>,
    images: &std::collections::BTreeMap<String, typesep_typst::images::Image>,
    fonts: &Fonts,
    cli: &BuildArgs,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    if !cli.verbosity.quiet {
        eprintln!("  building {}.epub", file_name_for(&group.name));
    }
    let epub = match build_epub(
        &group.docs,
        style,
        EpubOptions {
            sections,
            images,
            volume: group.volume.as_ref(),
            fonts,
        },
    ) {
        Ok(epub) => epub,
        Err(err) => return Err(err.to_string()),
    };
    for warning in epub.warnings {
        if !warnings.contains(&warning) {
            warnings.push(warning);
        }
    }
    let file = output_file(&group.name, "epub", &cli.output.out);
    if let Err(err) = std::fs::write(&file, &epub.bytes) {
        return Err(format!("cannot write {}: {err}", show_path(&file)));
    }
    outln!("{}", show_path(&file));
    Ok(())
}

fn say(message: &str, quiet: bool) {
    if !quiet {
        eprintln!("  {message}");
    }
}

fn say_all(warnings: &[String], quiet: bool) {
    if quiet || warnings.is_empty() {
        return;
    }
    for message in warnings.iter().take(8) {
        eprintln!("  {message}");
    }
    if warnings.len() > 8 {
        eprintln!("  and {} more notes", warnings.len() - 8);
    }
}

fn read_entries(
    library: &Library,
    targets: &[EntryTarget],
    sections: &BTreeSet<ContentKey>,
    quiet: bool,
) -> Result<Vec<Doc>, ExitCode> {
    let mut docs = Vec::new();
    let mut source = Source::offline(library);
    for target in targets {
        if !quiet {
            eprintln!("reading {}", target.slug);
        }
        let mut log = |message: &str| {
            if !quiet {
                eprintln!("{message}");
            }
        };
        match load_entry(
            &mut source,
            target,
            LoadOptions {
                content: sections,
                log: &mut log,
            },
        ) {
            Ok(loaded) => docs.push(loaded.doc),
            Err(err) => return Err(user_error(err)),
        }
    }
    Ok(docs)
}

fn targets_for(entries: &[String], edition: &str) -> Result<Vec<EntryTarget>, ExitCode> {
    let mut targets = Vec::new();
    for entry in entries {
        match normalize_entry(entry, edition) {
            Ok(target) => targets.push(target),
            Err(err) => return Err(usage_error(err)),
        }
    }
    Ok(targets)
}

fn dump_model(opts: &DumpModelOptions) -> ExitCode {
    if opts.entries.is_empty() {
        return usage_error("no entry given");
    }
    let sections: BTreeSet<ContentKey> =
        parse_content_keys(&opts.sections).expect("validated by clap");
    let targets = match targets_for(&opts.entries, &opts.fetching.edition) {
        Ok(targets) => targets,
        Err(code) => return code,
    };
    let library = Library::new(opts.fetching.library_dir());
    if let Err(err) = library.init() {
        return user_error(err);
    }
    let docs = match read_entries(&library, &targets, &sections, opts.verbosity.quiet) {
        Ok(docs) => docs,
        Err(code) => return code,
    };
    for doc in &docs {
        match model_json(doc) {
            Ok(text) => out!("{text}"),
            Err(err) => return user_error(err),
        }
    }
    ExitCode::SUCCESS
}

fn describe_style_source(opts: &StyleOptions) -> String {
    let config = Path::new(CONFIG_FILE);
    let file = opts
        .style
        .clone()
        .or_else(|| config.exists().then(|| config.to_path_buf()));
    match (&opts.preset, file) {
        (Some(preset), Some(file)) => format!("preset {preset}, {}", show_path(&file)),
        (Some(preset), None) => format!("preset {preset}"),
        (None, Some(file)) => show_path(&file),
        (None, None) => "the default preset".to_owned(),
    }
}

fn completions(opts: &CompletionsOptions) -> ExitCode {
    let mut cmd = command();
    let name = cmd.get_name().to_owned();
    let mut buffer: Vec<u8> = Vec::new();
    clap_complete::generate(opts.shell, &mut cmd, name, &mut buffer);
    write_out(&String::from_utf8_lossy(&buffer));
    ExitCode::SUCCESS
}

fn man_page() -> ExitCode {
    let mut buffer: Vec<u8> = Vec::new();
    if let Err(err) = clap_mangen::Man::new(command()).render(&mut buffer) {
        return user_error(format!("cannot render the manual page: {err}"));
    }
    write_out(&String::from_utf8_lossy(&buffer));
    ExitCode::SUCCESS
}

fn command() -> clap::Command {
    with_shorthands(Cli::command())
        .mut_subcommand("build", with_shorthands)
        .mut_subcommand("style", with_shorthands)
        .mut_subcommand("dump", |dump| dump.mut_subcommand("typst", with_shorthands))
}

fn matches_for<'a>(matches: &'a ArgMatches, path: &[&str]) -> &'a ArgMatches {
    let mut found = matches;
    for name in path {
        found = found
            .subcommand_matches(name)
            .expect("the subcommand clap matched");
    }
    found
}

fn main() -> ExitCode {
    let matches = command().get_matches();
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(err) => err.exit(),
    };
    match &cli.command {
        None => build(&cli.build, &matches, false),
        Some(Command::Build(args)) => build(args, matches_for(&matches, &["build"]), false),
        Some(Command::Fetch(opts)) => fetch_command(opts),
        Some(Command::Update(opts)) => update_command(opts),
        Some(Command::Style(opts)) => style_command(opts, matches_for(&matches, &["style"])),
        Some(Command::Dump(DumpCommand::Model(opts))) => dump_model(opts),
        Some(Command::Dump(DumpCommand::Typst(opts))) => build(
            &opts.as_build(),
            matches_for(&matches, &["dump", "typst"]),
            true,
        ),
        Some(Command::Presets) => presets(),
        Some(Command::Eject(opts)) => eject(opts),
        Some(Command::Explain(opts)) => explain(opts),
        Some(Command::Fonts) => fonts_command(),
        Some(Command::List(opts)) => list(opts),
        Some(Command::Completions(opts)) => completions(opts),
        Some(Command::Man) => man_page(),
        Some(Command::Reference) => reference::write(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_is_well_formed() {
        command().debug_assert();
    }

    #[test]
    fn a_merge_of_nothing_is_no_group() {
        assert!(group_entries(&[], Some("vol")).is_empty());
    }

    fn style_with(sets: &[&str]) -> Style {
        resolve_style(ResolveOptions {
            preset: Some("halfletter"),
            file: None,
            sets: sets.iter().map(|s| (*s).into()).collect(),
        })
        .unwrap()
    }

    #[test]
    fn a_custom_cover_is_read_from_the_file_the_style_names() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cover.typ");
        std::fs::write(&file, "#align(center)[= My cover]\n").unwrap();
        let path = file.to_str().unwrap();

        let custom = style_with(&["cover.style=custom", &format!("cover.template={path}")]);
        assert_eq!(
            cover_template(&custom).unwrap().as_deref(),
            Some("#align(center)[= My cover]\n")
        );

        let unused = style_with(&[&format!("cover.template={path}")]);
        assert_eq!(cover_template(&unused).unwrap(), None);
        let empty = style_with(&["cover.style=custom"]);
        assert_eq!(cover_template(&empty).unwrap(), None);
    }

    #[test]
    fn the_progress_verb_says_whether_an_entry_is_read_or_fetched() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::new(dir.path());
        library.init().unwrap();
        let held = normalize_entry("turing-machine", "live").unwrap();
        library
            .write(&held.url, b"<html></html>", None, None)
            .unwrap();
        let absent = normalize_entry("no-such-entry", "live").unwrap();

        let source = Source::offline(&library);
        assert_eq!(Verb::Either.word(&source, &held), "reading");
        assert_eq!(Verb::Either.word(&source, &absent), "fetching");
        assert_eq!(Verb::Reading.word(&source, &absent), "reading");
        assert_eq!(Verb::Checking.word(&source, &held), "checking");

        let refreshed = Source::offline(&library).with_refresh(true);
        assert_eq!(Verb::Either.word(&refreshed, &held), "fetching");
    }

    #[test]
    fn a_custom_cover_that_is_not_there_fails_the_build() {
        let style = style_with(&["cover.style=custom", "cover.template=/no/such/cover.typ"]);
        let error = cover_template(&style).unwrap_err();
        assert!(error.contains("/no/such/cover.typ"), "{error}");
    }

    #[test]
    fn the_style_flags_are_where_a_style_is_resolved() {
        let cmd = command();
        let has_paper = |path: &[&str]| {
            let mut found = &cmd;
            for name in path {
                found = found
                    .get_subcommands()
                    .find(|c| c.get_name() == *name)
                    .unwrap_or_else(|| panic!("no {name} command"));
            }
            found.get_arguments().any(|a| a.get_id() == "paper")
        };
        assert!(has_paper(&[]), "the default build path");
        assert!(has_paper(&["build"]));
        assert!(has_paper(&["style"]));
        assert!(has_paper(&["dump", "typst"]));
        assert!(
            !has_paper(&["dump", "model"]),
            "the model does not depend on the style"
        );
        assert!(!has_paper(&["list"]));
    }

    #[test]
    fn a_format_is_one_name_or_a_list() {
        let pdf = Formats {
            pdf: true,
            epub: false,
        };
        let epub = Formats {
            pdf: false,
            epub: true,
        };
        let both = Formats {
            pdf: true,
            epub: true,
        };
        assert_eq!(parse_formats("pdf").unwrap(), pdf);
        assert_eq!(parse_formats("EPUB").unwrap(), epub);
        assert_eq!(parse_formats("pdf,epub").unwrap(), both);
        assert_eq!(parse_formats("epub, pdf").unwrap(), both);
        assert_eq!(parse_formats("pdf,pdf").unwrap(), pdf);
        assert!(parse_formats("both").is_err());
        assert!(parse_formats("").is_err());
        assert!(parse_formats("pdf,").is_err());
    }

    #[test]
    fn dump_model_refuses_style_flags() {
        let parsed = command().try_get_matches_from([
            "typesep",
            "dump",
            "model",
            "turing-machine",
            "--paper",
            "a4",
        ]);
        assert!(parsed.is_err());
    }

    #[test]
    fn the_fetching_flags_are_where_the_network_is() {
        let cmd = command();
        let has_refresh = |path: &[&str]| {
            let mut found = &cmd;
            for name in path {
                found = found
                    .get_subcommands()
                    .find(|c| c.get_name() == *name)
                    .unwrap_or_else(|| panic!("no {name} command"));
            }
            found.get_arguments().any(|a| a.get_id() == "refresh")
        };
        assert!(has_refresh(&[]), "the default build path");
        assert!(has_refresh(&["build"]));
        assert!(has_refresh(&["fetch"]));
        assert!(has_refresh(&["update"]));
        assert!(!has_refresh(&["dump", "model"]));
        assert!(!has_refresh(&["dump", "typst"]));
        assert!(!has_refresh(&["style"]));
        assert!(!has_refresh(&["list"]));
    }

    #[test]
    fn the_new_verbs_take_what_d22_reserved_for_them() {
        let parsed = command()
            .try_get_matches_from([
                "typesep",
                "fetch",
                "turing-machine",
                "--follow-related",
                "2",
                "--delay",
                "100",
                "--refresh",
            ])
            .expect("fetch takes the fetching flags");
        let fetch = parsed.subcommand_matches("fetch").unwrap();
        assert_eq!(fetch.get_one::<u32>("follow_related"), Some(&2));
        assert_eq!(fetch.get_one::<u64>("delay"), Some(&100));

        assert!(
            command()
                .try_get_matches_from(["typesep", "update"])
                .is_ok()
        );
        assert!(
            command()
                .try_get_matches_from(["typesep", "update", "--rebuild"])
                .is_err()
        );
        assert!(
            command()
                .try_get_matches_from(["typesep", "fetch", "x", "-o", "out"])
                .is_err()
        );
    }

    #[test]
    fn an_entry_list_skips_comments_and_blank_lines() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("entries.txt");
        std::fs::write(
            &file,
            "# a comment\nturing-machine\n\n  computability  # trailing note\nhttps://plato.stanford.edu/entries/x/\n",
        )
        .unwrap();
        let entries = entries_with_from(&["named".to_owned()], &[file]).unwrap();
        assert_eq!(
            entries,
            [
                "named",
                "turing-machine",
                "computability",
                "https://plato.stanford.edu/entries/x/"
            ]
        );

        let missing = entries_with_from(&[], &[PathBuf::from("no/such/file")]);
        assert!(missing.is_err());
    }

    #[test]
    fn ejected_presets_declare_no_base() {
        assert_eq!(
            ejected_text("# one\n# two\npage:\n  size: a4\n"),
            "# one\n# two\npreset: none\n\npage:\n  size: a4\n"
        );
        assert_eq!(ejected_text("page: {}\n"), "preset: none\n\npage: {}\n");
    }

    #[test]
    fn flags_map_to_style_paths() {
        assert_eq!(
            path_for_flag("--font").unwrap().as_deref(),
            Some("fonts.main.family")
        );
        assert_eq!(
            path_for_flag("margin").unwrap().as_deref(),
            Some("page.margins")
        );
        assert_eq!(path_for_flag("typography.leading").unwrap(), None);
        assert!(
            path_for_flag("--paper2")
                .unwrap_err()
                .starts_with("--paper2 is not a style flag\n  style flags: --paper, --margin")
        );
    }

    #[test]
    fn entries_that_fail_are_not_counted_as_walked() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::new(dir.path());
        library.init().unwrap();
        let mut source = Source::offline(&library);
        let sections = BTreeSet::new();
        let walk = WalkOptions {
            verb: Verb::Reading,
            sections: &sections,
            images: false,
            quiet: true,
        };
        let mut failures = Vec::new();
        let walked = walk_entries(
            &mut source,
            &["aaa-x".to_owned(), "bbb-y".to_owned()],
            "live",
            0,
            &walk,
            &mut failures,
            &mut |_| Ok(()),
        );
        assert_eq!(walked.ok(), Some(0));
        assert_eq!(failures.len(), 2);
    }

    #[test]
    fn each_edition_of_an_entry_is_walked_once() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::new(dir.path());
        library.init().unwrap();
        let mut source = Source::offline(&library);
        let sections = BTreeSet::new();
        let walk = WalkOptions {
            verb: Verb::Reading,
            sections: &sections,
            images: false,
            quiet: true,
        };
        let mut failures = Vec::new();
        let entries = [
            "kant".to_owned(),
            "https://plato.stanford.edu/archives/sum2020/entries/kant/".to_owned(),
            "https://plato.stanford.edu/entries/kant/".to_owned(),
        ];
        let walked = walk_entries(
            &mut source,
            &entries,
            "live",
            0,
            &walk,
            &mut failures,
            &mut |_| Ok(()),
        );
        assert_eq!(walked.ok(), Some(0));
        let names: Vec<&str> = failures.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["kant", "kant (sum2020)"]);
    }

    #[test]
    fn archived_editions_get_their_own_file_name() {
        let mut doc = Doc::default();
        doc.meta.id = "kant".into();
        assert_eq!(entry_name(&doc), "kant");
        doc.meta.origin.edition = "sum2020".into();
        assert_eq!(entry_name(&doc), "kant-sum2020");
    }
}
