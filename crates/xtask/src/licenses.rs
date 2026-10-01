use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const SHIPPED: &str = "typesep";

const TARGETS: [&str; 4] = [
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
];

const PREFERENCE: [&str; 14] = [
    "0BSD",
    "Unlicense",
    "CC0-1.0",
    "MIT",
    "ISC",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "Zlib",
    "BSL-1.0",
    "Unicode-3.0",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "MPL-2.0",
    "CDLA-Permissive-2.0",
];

const HOLDER_FREE: [&str; 7] = [
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "MPL-2.0",
    "CC0-1.0",
    "Unicode-3.0",
    "BSL-1.0",
    "CDLA-Permissive-2.0",
];

const SOURCE_DISCLOSURE: [&str; 1] = ["MPL-2.0"];

fn preference_index(license: &str) -> usize {
    PREFERENCE
        .iter()
        .position(|known| *known == license)
        .unwrap_or(usize::MAX)
}

fn holder_free(license: &str) -> bool {
    HOLDER_FREE.contains(&license)
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    license: Option<String>,
    license_file: Option<String>,
    #[serde(default)]
    authors: Vec<String>,
    repository: Option<String>,
    manifest_path: String,
    source: Option<String>,
}

#[derive(Deserialize)]
struct DepKind {
    kind: Option<String>,
}

#[derive(Deserialize)]
struct Dep {
    pkg: String,
    dep_kinds: Vec<DepKind>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    deps: Vec<Dep>,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    resolve: Resolve,
}

struct Crate {
    name: String,
    version: String,
    expression: String,
    chosen: Vec<String>,
    authors: Vec<String>,
    repository: Option<String>,
    dir: PathBuf,
}

impl Crate {
    fn label(&self) -> String {
        format!("{} {}", self.name, self.version)
    }

    fn source_url(&self) -> String {
        self.repository
            .clone()
            .unwrap_or_else(|| format!("https://crates.io/crates/{}/{}", self.name, self.version))
    }

    fn holders(&self) -> String {
        let names: Vec<String> = self
            .authors
            .iter()
            .map(|author| {
                let mut out = String::new();
                let mut depth = 0usize;
                for ch in author.chars() {
                    match ch {
                        '<' => depth += 1,
                        '>' => depth = depth.saturating_sub(1),
                        _ if depth == 0 => out.push(ch),
                        _ => {}
                    }
                }
                out.trim().to_owned()
            })
            .filter(|name| !name.is_empty())
            .collect();
        if names.is_empty() {
            format!("the {} authors", self.name)
        } else {
            names.join(", ")
        }
    }
}

fn metadata(manifest: &Path, target: &str) -> Result<Metadata, String> {
    let out = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--filter-platform",
            target,
        ])
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .map_err(|err| format!("cargo metadata: {err}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata --filter-platform {target}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|err| format!("cargo metadata: {err}"))
}

fn linked_into(meta: &Metadata) -> Result<HashSet<String>, String> {
    let nodes: HashMap<&str, &Node> = meta
        .resolve
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let by_id: HashMap<&str, &Package> = meta
        .packages
        .iter()
        .map(|pkg| (pkg.id.as_str(), pkg))
        .collect();
    let mut queue: Vec<String> = meta
        .workspace_members
        .iter()
        .filter(|id| {
            by_id
                .get(id.as_str())
                .is_some_and(|pkg| pkg.name == SHIPPED)
        })
        .cloned()
        .collect();
    if queue.is_empty() {
        return Err(format!(
            "no workspace member is named {SHIPPED}; the notice would describe nothing"
        ));
    }
    let mut seen: HashSet<String> = HashSet::new();
    while let Some(id) = queue.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(node) = nodes.get(id.as_str()) else {
            continue;
        };
        for dep in &node.deps {
            let runtime = dep
                .dep_kinds
                .iter()
                .any(|kind| kind.kind.as_deref() != Some("dev"));
            if runtime && !seen.contains(&dep.pkg) {
                queue.push(dep.pkg.clone());
            }
        }
    }
    Ok(seen)
}

fn resolve_expression(expression: &str, where_: &str) -> Result<Vec<String>, String> {
    let cleaned: String = expression
        .chars()
        .map(|ch| if ch == '(' || ch == ')' { ' ' } else { ch })
        .collect();
    let conjuncts: Vec<String> = split_keyword(&cleaned, "AND")
        .into_iter()
        .map(|part| part.trim().to_owned())
        .filter(|part| !part.is_empty())
        .collect();

    conjuncts
        .iter()
        .map(|conjunct| {
            let pieces = split_or(conjunct);
            let mut ranked: Vec<&str> = pieces
                .iter()
                .map(|part| part.trim())
                .filter(|part| PREFERENCE.contains(part))
                .collect();
            ranked.sort_by_key(|license| preference_index(license));
            ranked
                .first()
                .map(|first| (*first).to_owned())
                .ok_or_else(|| {
                    format!(
                        "{where_}: no known license in \"{conjunct}\". Add it to PREFERENCE in \
                     crates/xtask/src/licenses.rs after deciding whether typesep can ship \
                     under it."
                    )
                })
        })
        .collect()
}

fn split_keyword(text: &str, keyword: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if word.eq_ignore_ascii_case(keyword) {
            parts.push(std::mem::take(&mut current));
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
    }
    parts.push(current);
    parts
}

fn split_or(text: &str) -> Vec<String> {
    split_keyword(text, "OR")
        .into_iter()
        .flat_map(|part| {
            part.split('/')
                .map(|piece| piece.trim().to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
        .lines()
        .map(|line| line.trim_end_matches([' ', '\t']))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn names_license(license: &str, name: &str) -> Option<bool> {
    let lower = name.to_ascii_lowercase();
    let has = |needle: &str| lower.contains(needle);
    Some(match license {
        "MIT" => has("mit"),
        "Apache-2.0" | "Apache-2.0 WITH LLVM-exception" => has("apache"),
        "MPL-2.0" => has("mpl"),
        "Unicode-3.0" => has("unicode"),
        "BSL-1.0" => has("boost") || has("bsl"),
        "Zlib" => has("zlib"),
        "ISC" => has("isc"),
        "0BSD" => has("0bsd") || has("zero"),
        "BSD-2-Clause" | "BSD-3-Clause" => lower.match_indices("bsd").any(|(at, _)| {
            let rest = &lower[at..];
            !rest.contains("0bsd") && !rest.contains("boring")
        }),
        "Unlicense" => has("unlicense"),
        "CC0-1.0" => has("cc0"),
        "CDLA-Permissive-2.0" => has("cdla"),
        _ => return None,
    })
}

fn license_files(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            lower.starts_with("license")
                || lower.starts_with("licence")
                || lower.starts_with("copying")
        })
        .collect();
    names.sort();
    names
}

fn read(dir: &Path, name: &str) -> Option<String> {
    let text = normalize(&fs::read_to_string(dir.join(name)).ok()?);
    (!text.is_empty()).then_some(text)
}

fn license_text_from(krate: &Crate, license: &str) -> Option<String> {
    let names = license_files(&krate.dir);
    if names.is_empty() {
        return None;
    }
    let named = names
        .iter()
        .find(|name| names_license(license, name).unwrap_or(false));
    if let Some(name) = named {
        return read(&krate.dir, name);
    }
    if krate.chosen.len() == 1 && names.len() == 1 {
        return read(&krate.dir, &names[0]);
    }
    None
}

fn all_license_text(krate: &Crate) -> Option<String> {
    let parts: Vec<String> = license_files(&krate.dir)
        .iter()
        .filter_map(|name| {
            let text = read(&krate.dir, name)?;
            Some(format!(
                "{name}\n{}\n\n{text}",
                "-".repeat(name.chars().count())
            ))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn notice_text(krate: &Crate) -> Option<String> {
    if !krate.chosen.iter().any(|l| l.starts_with("Apache-2.0")) {
        return None;
    }
    let mut names: Vec<String> = fs::read_dir(&krate.dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.to_ascii_lowercase().starts_with("notice"))
        .collect();
    names.sort();
    read(&krate.dir, names.first()?)
}

const MIT_TEMPLATE: &str = r#"MIT License

Copyright (c) {holders}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE."#;

const ZERO_BSD_TEMPLATE: &str = r#"BSD Zero Clause License

Copyright (c) {holders}

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY
AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR
OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THIS SOFTWARE."#;

fn template(license: &str) -> Option<&'static str> {
    match license {
        "MIT" => Some(MIT_TEMPLATE),
        "0BSD" => Some(ZERO_BSD_TEMPLATE),
        _ => None,
    }
}

fn resolve_text(
    krate: &Crate,
    license: &str,
    shared: &HashMap<String, String>,
) -> Result<String, String> {
    if let Some(own) = license_text_from(krate, license) {
        return Ok(own);
    }
    if holder_free(license)
        && let Some(borrowed) = shared.get(license)
    {
        return Ok(borrowed.clone());
    }
    if let Some(template) = template(license) {
        return Ok(template.replace("{holders}", &krate.holders()));
    }
    if let Some(everything) = all_license_text(krate) {
        return Ok(everything);
    }
    Err(format!(
        "{}: no text for {license}. The crate ships no license file, no other crate in the tree \
         carries that license, and crates/xtask/src/licenses.rs has no template for it. Add one, \
         or add the file name to names_license if the crate does ship it.",
        krate.label()
    ))
}

fn shared_texts(crates: &[Crate]) -> HashMap<String, String> {
    let mut shared = HashMap::new();
    for krate in crates {
        for license in &krate.chosen {
            if !holder_free(license) || shared.contains_key(license) {
                continue;
            }
            if let Some(text) = license_text_from(krate, license) {
                shared.insert(license.clone(), text);
            }
        }
    }
    shared
}

struct Section<'a> {
    license: String,
    text: String,
    users: Vec<&'a Crate>,
}

fn render(crates: &[Crate], targets: &[&str]) -> Result<String, String> {
    let shared = shared_texts(crates);

    let mut sections: Vec<Section> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for krate in crates {
        for license in &krate.chosen {
            let text = resolve_text(krate, license, &shared)?;
            let key = (license.clone(), text.clone());
            match index.get(&key) {
                Some(at) => sections[*at].users.push(krate),
                None => {
                    index.insert(key, sections.len());
                    sections.push(Section {
                        license: license.clone(),
                        text,
                        users: vec![krate],
                    });
                }
            }
        }
    }

    let mut by_license: Vec<&Section> = sections.iter().collect();
    by_license.sort_by(|a, b| {
        preference_index(&a.license)
            .cmp(&preference_index(&b.license))
            .then(b.users.len().cmp(&a.users.len()))
            .then(a.users[0].name.cmp(&b.users[0].name))
    });

    let rule = "=".repeat(78);
    let thin = "-".repeat(78);
    let mut out: Vec<String> = vec![
        "typesep: third-party licenses".to_owned(),
        rule.clone(),
        String::new(),
        "This file is generated by `cargo run -p xtask -- licenses`. Do not edit it by hand."
            .to_owned(),
        String::new(),
    ];
    out.push(
        "The typesep executable is statically linked, so the code of the crates listed".to_owned(),
    );
    out.push(
        "below is compiled into it and their licenses come with it. Two notices are published"
            .to_owned(),
    );
    out.push("beside this one and are not repeated here:".to_owned());
    out.push(String::new());
    out.push("  LICENSE                typesep\u{2019}s own terms (MIT).".to_owned());
    out.push(
        "  GUST-FONT-LICENSE.txt  the thirteen TeX Gyre faces compiled into the binary.".to_owned(),
    );
    out.push(String::new());
    if targets.len() == 1 {
        out.push(format!(
            "Covers the dependencies of the {} binary alone.",
            targets[0]
        ));
    } else {
        out.push("Covers the union of the dependencies of every released platform:".to_owned());
        for target in targets {
            out.push(format!("  {target}"));
        }
    }
    out.push(String::new());
    out.push(
        "Where a crate is offered under a choice of licenses, this file records the one".to_owned(),
    );
    out.push(
        "typesep takes it under; the full expression is shown beside each crate below.".to_owned(),
    );
    out.push(String::new());
    out.push(format!(
        "{} crates, {} distinct license texts.",
        crates.len(),
        by_license.len()
    ));
    out.push(String::new());

    let disclose: Vec<&Crate> = crates
        .iter()
        .filter(|krate| {
            krate
                .chosen
                .iter()
                .any(|id| SOURCE_DISCLOSURE.contains(&id.as_str()))
        })
        .collect();
    if !disclose.is_empty() {
        out.push(rule.clone());
        out.push("Source availability".to_owned());
        out.push(rule.clone());
        out.push(String::new());
        out.push(
            "The Mozilla Public License 2.0 (section 3.2) asks that anyone who receives the"
                .to_owned(),
        );
        out.push(
            "binary be told where to obtain the source of the files it covers. Unmodified"
                .to_owned(),
        );
        out.push("copies of these crates, at the exact versions compiled in, are at:".to_owned());
        out.push(String::new());
        for krate in &disclose {
            out.push(format!("  {}", krate.label()));
            out.push(format!("      {}", krate.source_url()));
            out.push(format!(
                "      https://crates.io/crates/{}/{}",
                krate.name, krate.version
            ));
        }
        out.push(String::new());
    }

    let notices: Vec<(&Crate, String)> = crates
        .iter()
        .filter_map(|krate| notice_text(krate).map(|text| (krate, text)))
        .collect();
    if !notices.is_empty() {
        out.push(rule.clone());
        out.push("NOTICE files".to_owned());
        out.push(rule.clone());
        out.push(String::new());
        out.push(
            "The Apache License 2.0 (section 4(d)) asks that the NOTICE these crates ship be"
                .to_owned(),
        );
        out.push("carried in any distribution of a work derived from them.".to_owned());
        out.push(String::new());
        for (krate, text) in &notices {
            out.push(thin.clone());
            out.push(krate.label());
            out.push(thin.clone());
            out.push(String::new());
            out.push(text.clone());
            out.push(String::new());
        }
    }

    out.push(rule.clone());
    out.push("Crates".to_owned());
    out.push(rule.clone());
    out.push(String::new());
    let width = crates
        .iter()
        .map(|krate| krate.label().chars().count())
        .max()
        .unwrap_or(0);
    for krate in crates {
        out.push(format!(
            "  {:width$}  {}",
            krate.label(),
            krate.expression,
            width = width
        ));
    }
    out.push(String::new());

    for section in by_license {
        out.push(rule.clone());
        out.push(section.license.clone());
        out.push(rule.clone());
        out.push(String::new());
        out.push(format!(
            "Used by {} crate{}:",
            section.users.len(),
            if section.users.len() == 1 { "" } else { "s" }
        ));
        for krate in &section.users {
            out.push(format!("  {}", krate.label()));
        }
        out.push(String::new());
        out.push(thin.clone());
        out.push(String::new());
        out.push(section.text.clone());
        out.push(String::new());
    }

    Ok(format!("{}\n", out.join("\n")))
}

fn collect(manifest: &Path, targets: &[&str]) -> Result<Vec<Crate>, String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut crates: Vec<Crate> = Vec::new();
    for target in targets {
        let meta = metadata(manifest, target)?;
        let linked = linked_into(&meta)?;
        for pkg in &meta.packages {
            if pkg.source.is_none() {
                continue;
            }
            if !linked.contains(&pkg.id) {
                continue;
            }
            let key = format!("{} {}", pkg.name, pkg.version);
            if !seen.insert(key.clone()) {
                continue;
            }
            let Some(expression) = pkg.license.clone() else {
                let ships = pkg
                    .license_file
                    .as_ref()
                    .map(|file| format!(" (it ships {file}; read it and decide)"))
                    .unwrap_or_default();
                return Err(format!("{key}: no license field in its manifest{ships}."));
            };
            crates.push(Crate {
                name: pkg.name.clone(),
                version: pkg.version.clone(),
                chosen: resolve_expression(&expression, &key)?,
                expression,
                authors: pkg.authors.clone(),
                repository: pkg.repository.clone(),
                dir: Path::new(&pkg.manifest_path)
                    .parent()
                    .unwrap_or(Path::new("."))
                    .to_path_buf(),
            });
        }
    }
    crates.sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));
    Ok(crates)
}

fn validate(manifest: &Path) -> Result<(), String> {
    let mut most = 0usize;
    for target in TARGETS {
        let found = collect(manifest, &[target])?;
        most = most.max(found.len());
        println!(
            "{target}: {} crates, every license expression resolved.",
            found.len()
        );
    }
    println!("Third-party licenses validate; the largest platform has {most} crates.");
    Ok(())
}

fn write(manifest: &Path, output: &Path, targets: &[&str], label: &str) -> Result<(), String> {
    let crates = collect(manifest, targets)?;
    let absent: Vec<&Crate> = crates.iter().filter(|krate| !krate.dir.exists()).collect();
    if let Some(first) = absent.first() {
        return Err(format!(
            "{} crates are not unpacked, so their license texts cannot be read; the notice would \
             be written from templates alone. Build first, then run this again. First: {}.",
            absent.len(),
            first.label()
        ));
    }
    let rendered = render(&crates, targets)?;
    fs::write(output, &rendered).map_err(|err| format!("{}: {err}", output.display()))?;
    println!(
        "THIRD-PARTY-LICENSES.txt ({label}): {} crates, {} lines.",
        crates.len(),
        rendered.split('\n').count()
    );
    Ok(())
}

pub fn run(
    root: &Path,
    target: Option<String>,
    validate_only: bool,
    out: Option<PathBuf>,
) -> std::process::ExitCode {
    let manifest = root.join("Cargo.toml");
    let output = out.unwrap_or_else(|| root.join("THIRD-PARTY-LICENSES.txt"));
    let result = if validate_only {
        validate(&manifest)
    } else if let Some(target) = target {
        write(&manifest, &output, &[&target], &target)
    } else {
        write(&manifest, &output, &TARGETS, "union of all four targets")
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask licenses: {message}");
            std::process::ExitCode::FAILURE
        }
    }
}
