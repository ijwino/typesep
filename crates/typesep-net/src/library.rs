use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::source::Source;

pub const META: &str = "meta.json";
const DERIVED: &str = ".derived";

/// What a library written before each source had its own directory holds at its top, one name
/// at least: those were the SEP's units and the record of its root. A migration moves them
/// last, so that one cut short still shows.
const LEGACY_MARKS: [&str; 3] = ["archives", "entries", META];

/// The units whose `meta.json` such a library wrote, by their paths from its top, `*` standing
/// for any name: its root's, and each entry's, live and archived. A mark makes a directory
/// such a library only with one of these beside it.
const LEGACY_UNITS: [&str; 3] = ["", "entries/*", "archives/*/entries/*"];

/// The file at a library's top that one migration at a time holds locked. It is a dot file, so
/// a migration leaves it where it is and nothing that reads the library lists it.
const MIGRATION_LOCK: &str = ".migrate.lock";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlaceError {
    #[error("{0} is not a URL")]
    NotAUrl(String),
    #[error("{url} is not on {site}; the library mirrors only that site")]
    OtherSite { url: String, site: &'static str },
    #[error("{0} has a query string, which the library cannot store")]
    QueryString(String),
    #[error("{url}: \"{segment}\" cannot be a file name in the library")]
    UnsafeSegment { url: String, segment: String },
    #[error("{0}: {META} is the library's own file")]
    MetaName(String),
    /// An address on the site that is not of what the library keeps of it: `kept` says what
    /// that is.
    #[error("{url} is not among what the library keeps of its site: {kept}")]
    Unkept { url: String, kept: &'static str },
}

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error(transparent)]
    Place(#[from] PlaceError),
    #[error(
        "{dir} has the layout of an older typesep (pages/<key>.html)\n  no released version ever wrote it: name another directory, and `fetch` into it"
    )]
    OldLayout { dir: String },
    #[error(
        "{dir} holds an older library and a newer one, and both have {names}\n  typesep moves an older library into {dir}/{key} by itself; move one of each pair out of the way, then run it again"
    )]
    BothLayouts {
        dir: String,
        key: String,
        names: String,
    },
    #[error("{path} is where the library keeps a source's files, but it is not a directory")]
    NotADirectory { path: PathBuf },
    /// A work's record asked for at a URL that is not the page of a work: `page` says what
    /// the source's works are, as "an entry page".
    #[error("{url} is not {page}")]
    NotAWorkPage { url: String, page: &'static str },
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitMeta {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_revision: Option<String>,
    #[serde(default)]
    pub files: BTreeMap<String, FileMeta>,
}

/// What the library records of a work it has parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkRecord {
    pub title: Option<String>,
    pub revision: String,
    pub previous_revision: Option<String>,
}

/// Where a source keeps what a URL names: a file of a unit, the directory that holds the files
/// of one page with their `meta.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The unit's path under the source's directory, `/`-separated; empty for its top.
    pub unit: String,
    /// The file's path in the unit, `/`-separated.
    pub file: String,
    /// The URL the unit's `meta.json` records.
    pub unit_url: String,
}

fn safe_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_~!$&'()+,;=@%-".contains(c)
}

/// Whether `segment` can be a file or directory name in the library as it is: never empty,
/// never starting with a dot, never a separator.
pub fn is_safe_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    match chars.next() {
        Some(first) if safe_char(first) => chars.all(|c| c == '.' || safe_char(c)),
        _ => false,
    }
}

pub fn now_iso8601() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.millisecond(),
    )
}

pub fn default_library_dir() -> PathBuf {
    library_dir_from(|name| std::env::var_os(name).map(PathBuf::from))
}

fn library_dir_from(env: impl Fn(&str) -> Option<PathBuf>) -> PathBuf {
    let named = |name: &str| env(name).filter(|dir| !dir.as_os_str().is_empty());
    if let Some(dir) = named("TYPESEP_LIBRARY") {
        return dir;
    }
    let data = named("XDG_DATA_HOME")
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    data.join("typesep").join("library")
}

fn io_error(path: &Path, source: io::Error) -> LibraryError {
    LibraryError::Io {
        path: path.to_owned(),
        source,
    }
}

fn write_atomically(target: &Path, bytes: &[u8]) -> Result<(), LibraryError> {
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = target.with_file_name(format!("{name}.{}.tmp", std::process::id()));
    let written = fs::write(&tmp, bytes)
        .map_err(|e| io_error(&tmp, e))
        .and_then(|()| fs::rename(&tmp, target).map_err(|e| io_error(target, e)));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

/// What `Library::migrate_legacy` moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migrated {
    /// The directory the files went into.
    pub into: PathBuf,
    /// The names moved, in the order they were.
    pub moved: Vec<String>,
}

/// The fetched files of every source: `<dir>/<source>/<unit>/<file>`, each unit with its
/// `meta.json`, and the copies converted from them under `<dir>/<source>/.derived/<unit>/`.
#[derive(Debug, Clone)]
pub struct Library {
    dir: PathBuf,
}

impl Library {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn at_default() -> Self {
        Self::new(default_library_dir())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn init(&self) -> Result<(), LibraryError> {
        if self.has_only_pages() {
            return Err(LibraryError::OldLayout {
                dir: self.dir.display().to_string(),
            });
        }
        fs::create_dir_all(&self.dir).map_err(|e| io_error(&self.dir, e))
    }

    /// Whether the directory is a `pages/` one and nothing else a library holds: neither the
    /// SEP's `entries/` of the layout before sources, nor a source's directory.
    fn has_only_pages(&self) -> bool {
        if !self.dir.join("pages").exists() || self.dir.join("entries").exists() {
            return false;
        }
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return false;
        };
        !entries.flatten().any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name != "pages" && !name.starts_with('.') && entry.path().is_dir()
        })
    }

    /// The files of `source`.
    pub fn store<'a>(&'a self, source: &'a dyn Source) -> Store<'a> {
        Store {
            library: self,
            source,
            dir: self.dir.join(source.key()),
        }
    }

    /// Moves a library written before each source had its own directory, which held the files
    /// of `old` (the SEP) at its top, into `<dir>/<source>`, `<source>` being its key, and says
    /// what it moved; a library in the present layout, or none at all, is left as it is.
    /// `sources` are the names of the sources' directories, which never move.
    ///
    /// A directory is taken for such a library only when a name at its top marks the old layout
    /// and a unit that layout kept records in its `meta.json` the address `old` places at that
    /// very unit, as only typesep writes it: a folder of the user's that happens to hold an
    /// `archives` or an `entries` is not touched, and no lock file is made in it.
    ///
    /// Nothing is copied or deleted: every top-level name but those and the dot files (save
    /// `.derived`) is renamed into `<dir>/<source>` as it is, one rename each, so each one
    /// either happened or did not. The names that mark the old layout go last, so a migration
    /// cut short leaves one at the top, and the next run moves what is left. A name that is at
    /// the top and in `<dir>/<source>` both is refused before anything moves: that is a
    /// library of each layout in one directory, not an interrupted migration.
    ///
    /// A typesep that finds the old layout holds `<dir>/.migrate.lock` locked while it looks
    /// again and moves, so of several run at once one migrates and the others find it done.
    /// Without the lock each would rename what it saw, and on Windows a rename of a name
    /// another has just moved can succeed as well, so both would say they moved it. The lock
    /// file stays: one deleted while another typesep waits on it would let a third lock a new
    /// one at the same time. The lock is a courtesy among typesep run at once, not a condition:
    /// where it cannot be had, the migration goes on without it (see `lock_for_migration`).
    pub fn migrate_legacy(
        &self,
        old: &dyn Source,
        sources: &[&str],
    ) -> Result<Option<Migrated>, LibraryError> {
        if self.legacy_names(old)?.is_none() {
            return Ok(None);
        }
        let _lock = self.lock_for_migration()?;
        let Some(names) = self.legacy_names(old)? else {
            return Ok(None);
        };

        let source = old.key();
        let into = self.dir.join(source);
        match fs::symlink_metadata(&into) {
            Ok(meta) if !meta.is_dir() => return Err(LibraryError::NotADirectory { path: into }),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(&into, e)),
        }
        let mut moving: Vec<String> = names
            .into_iter()
            .filter(|name| !name.starts_with('.') || name == DERIVED)
            .filter(|name| name != source && !sources.contains(&name.as_str()))
            .collect();
        let last = |name: &String| LEGACY_MARKS.iter().position(|mark| mark == name);
        moving.sort_by(|a, b| last(a).cmp(&last(b)).then_with(|| a.cmp(b)));

        // The new place is looked at first: another typesep migrating at the same time, where
        // there is no lock, moves a name in one rename, so a name seen in both places is one
        // there twice.
        let both: Vec<&str> = moving
            .iter()
            .filter(|name| exists(&into.join(name)) && exists(&self.dir.join(name)))
            .map(String::as_str)
            .collect();
        if !both.is_empty() {
            return Err(self.both_layouts(source, &both));
        }

        fs::create_dir_all(&into).map_err(|e| io_error(&into, e))?;
        let mut moved = Vec::new();
        let mut synced = false;
        for name in &moving {
            if !synced && last(name).is_some() {
                // What went before is on disk before the marks go: a crash cannot leave a
                // migration that looks finished with files still at the top.
                sync_dir(&into);
                sync_dir(&self.dir);
                synced = true;
            }
            let (from, to) = (self.dir.join(name), into.join(name));
            if exists(&to) {
                if exists(&from) {
                    return Err(self.both_layouts(source, &[name]));
                }
                // Another typesep moved it in the meantime.
                continue;
            }
            match fs::rename(&from, &to) {
                Ok(()) => moved.push(name.clone()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_error(&from, e)),
            }
        }
        sync_dir(&self.dir);
        Ok((!moved.is_empty()).then_some(Migrated { into, moved }))
    }

    /// The names at the library's top when one of them marks the old layout and it holds a
    /// unit of `old` as that layout kept it; none when no name does, the directory is not such
    /// a library, or there is no library.
    fn legacy_names(&self, old: &dyn Source) -> Result<Option<Vec<String>>, LibraryError> {
        let names = match fs::read_dir(&self.dir) {
            Ok(entries) => entries
                .map(|entry| entry.map_err(|e| io_error(&self.dir, e)))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect::<Vec<String>>(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_error(&self.dir, e)),
        };
        let marked = names
            .iter()
            .any(|name| LEGACY_MARKS.contains(&name.as_str()));
        let ours = marked
            && LEGACY_UNITS.iter().any(|pattern| {
                let pattern: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
                self.has_unit_of(old, "", &pattern)
            });
        Ok(ours.then_some(names))
    }

    /// Whether a unit under `unit` (a path from the library's top) whose path goes on as
    /// `pattern` does holds a `meta.json` that records the address `old` places at that unit.
    fn has_unit_of(&self, old: &dyn Source, unit: &str, pattern: &[&str]) -> bool {
        let Some((next, rest)) = pattern.split_first() else {
            let Some(meta) = fs::read_to_string(self.dir.join(unit).join(META))
                .ok()
                .and_then(|text| serde_json::from_str::<UnitMeta>(&text).ok())
            else {
                return false;
            };
            return old
                .place(&meta.url)
                .is_ok_and(|place| place.unit == unit && place.unit_url == meta.url);
        };
        let join = |name: &str| {
            if unit.is_empty() {
                name.to_owned()
            } else {
                format!("{unit}/{name}")
            }
        };
        if *next != "*" {
            return self.has_unit_of(old, &join(next), rest);
        }
        let Ok(entries) = fs::read_dir(self.dir.join(unit)) else {
            return false;
        };
        entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| !name.starts_with('.'))
            .any(|name| self.has_unit_of(old, &join(&name), rest))
    }

    /// The migration lock, held until the file is dropped, at best. A file that cannot be
    /// locked for any reason (a file system that cannot lock, NFS without its lock daemon) is
    /// kept unlocked; none is made where the library's directory is read-only or not the
    /// user's to write in, and the rename that then cannot be made says why. Only another
    /// failure to open the file is an error.
    fn lock_for_migration(&self) -> Result<Option<fs::File>, LibraryError> {
        let path = self.dir.join(MIGRATION_LOCK);
        let file = match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
        {
            Ok(file) => file,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::ReadOnlyFilesystem | io::ErrorKind::PermissionDenied
                ) =>
            {
                return Ok(None);
            }
            Err(e) => return Err(io_error(&path, e)),
        };
        // A lock refused, for whatever reason, is gone without.
        let _ = file.lock();
        Ok(Some(file))
    }

    fn both_layouts(&self, source: &str, names: &[&str]) -> LibraryError {
        LibraryError::BothLayouts {
            dir: self.dir.display().to_string(),
            key: source.to_owned(),
            names: names.join(", "),
        }
    }
}

fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Asks the file system to keep the renames made in `dir` so far, where it can be asked.
fn sync_dir(dir: &Path) {
    if let Ok(handle) = fs::File::open(dir) {
        let _ = handle.sync_all();
    }
}

/// The files of one source in the library, where the source's `place` says each URL goes.
#[derive(Clone)]
pub struct Store<'a> {
    library: &'a Library,
    source: &'a dyn Source,
    dir: PathBuf,
}

impl<'a> Store<'a> {
    /// The library's own directory, the one the user names.
    pub fn library_dir(&self) -> &Path {
        self.library.dir()
    }

    /// The source's directory in it.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn place(&self, url: &str) -> Result<Place, PlaceError> {
        self.source.place(url)
    }

    fn unit_dir(&self, unit: &str) -> PathBuf {
        if unit.is_empty() {
            self.dir.clone()
        } else {
            self.dir.join(unit)
        }
    }

    pub fn file_path(&self, url: &str) -> Result<PathBuf, PlaceError> {
        let place = self.place(url)?;
        Ok(self.unit_dir(&place.unit).join(&place.file))
    }

    pub fn derived_path(&self, url: &str, suffix: &str) -> Result<PathBuf, PlaceError> {
        let place = self.place(url)?;
        let base = self.dir.join(DERIVED);
        let base = if place.unit.is_empty() {
            base
        } else {
            base.join(&place.unit)
        };
        Ok(base.join(format!("{}{suffix}", place.file)))
    }

    /// The files that may hold what `url` names, the best first: the file fetched from it, then
    /// a copy converted to PNG. Neither need exist.
    pub fn copies_of(&self, url: &str) -> Vec<PathBuf> {
        let Ok(path) = self.file_path(url) else {
            return Vec::new();
        };
        let mut out = vec![path];
        out.extend(self.derived_path(url, ".png").ok());
        out
    }

    pub fn unit_meta(&self, unit: &str) -> Option<UnitMeta> {
        let text = fs::read_to_string(self.unit_dir(unit).join(META)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn read_meta(&self, url: &str) -> Result<Option<FileMeta>, PlaceError> {
        let place = self.place(url)?;
        Ok(self
            .unit_meta(&place.unit)
            .and_then(|unit| unit.files.get(&place.file).cloned()))
    }

    pub fn read_bytes(&self, url: &str) -> Result<Option<Vec<u8>>, LibraryError> {
        let path = self.file_path(url)?;
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_error(&path, e)),
        }
    }

    pub fn read_body(&self, url: &str) -> Result<Option<String>, LibraryError> {
        Ok(self
            .read_bytes(url)?
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
    }

    pub fn has(&self, url: &str) -> Result<bool, PlaceError> {
        let path = self.file_path(url)?;
        Ok(fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false))
    }

    pub fn is_missing(&self, url: &str) -> Result<bool, PlaceError> {
        Ok(self.read_meta(url)?.is_some_and(|meta| meta.missing))
    }

    fn write_unit(&self, unit: &str, meta: &UnitMeta) -> Result<(), LibraryError> {
        let dir = self.unit_dir(unit);
        fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
        let text =
            serde_json::to_string_pretty(meta).expect("a UnitMeta is representable as JSON") + "\n";
        write_atomically(&dir.join(META), text.as_bytes())
    }

    fn unit_or_new(&self, place: &Place) -> Result<UnitMeta, LibraryError> {
        let path = self.unit_dir(&place.unit).join(META);
        match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(unit) => return Ok(unit),
                Err(_) => {
                    let aside = path.with_file_name(format!("{META}.corrupt"));
                    fs::rename(&path, &aside).map_err(|e| io_error(&aside, e))?;
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(&path, e)),
        }
        Ok(UnitMeta {
            url: place.unit_url.clone(),
            title: None,
            revision: None,
            previous_revision: None,
            files: BTreeMap::new(),
        })
    }

    pub fn put(&self, url: &str, body: Option<&[u8]>, meta: FileMeta) -> Result<(), LibraryError> {
        let place = self.place(url)?;
        if let Some(bytes) = body {
            let target = self.unit_dir(&place.unit).join(&place.file);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
            }
            write_atomically(&target, bytes)?;
        }
        let mut unit = self.unit_or_new(&place)?;
        unit.files.insert(place.file.clone(), meta);
        self.write_unit(&place.unit, &unit)
    }

    pub fn write(
        &self,
        url: &str,
        body: &[u8],
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<(), LibraryError> {
        self.put(
            url,
            Some(body),
            FileMeta {
                fetched_at: Some(now_iso8601()),
                etag: etag.map(str::to_owned),
                last_modified: last_modified.map(str::to_owned),
                missing: false,
            },
        )
    }

    pub fn mark_missing(&self, url: &str) -> Result<(), LibraryError> {
        self.put(
            url,
            None,
            FileMeta {
                fetched_at: Some(now_iso8601()),
                missing: true,
                ..Default::default()
            },
        )
    }

    /// Records the title and revision of the work whose page `url` is, in its unit's
    /// `meta.json`, keeping the revision it replaces.
    pub fn record_work(&self, url: &str, title: &str, revision: &str) -> Result<(), LibraryError> {
        let place = self.place(url)?;
        let mut unit = self.unit_or_new(&place)?;
        if unit
            .revision
            .as_deref()
            .is_some_and(|held| held != revision)
        {
            unit.previous_revision = unit.revision.clone();
        }
        unit.title = Some(title.to_owned());
        unit.revision = Some(revision.to_owned());
        self.write_unit(&place.unit, &unit)
    }

    pub fn work_record(&self, url: &str) -> Result<Option<WorkRecord>, PlaceError> {
        let place = self.place(url)?;
        Ok(self.unit_meta(&place.unit).and_then(|unit| {
            unit.revision.map(|revision| WorkRecord {
                title: unit.title,
                revision,
                previous_revision: unit.previous_revision,
            })
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TM: &str = "https://plato.stanford.edu/entries/turing-machine/";

    #[test]
    fn the_default_location_prefers_the_library_variable_then_the_cache_alias_then_the_data_directory()
     {
        let with = |vars: &[(&str, &str)]| {
            let vars: Vec<(String, String)> = vars
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect();
            library_dir_from(move |name| {
                vars.iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| PathBuf::from(v))
            })
        };
        assert_eq!(
            with(&[("XDG_DATA_HOME", "/data")]),
            PathBuf::from("/data/typesep/library")
        );
        assert_eq!(
            with(&[("XDG_DATA_HOME", "/data"), ("TYPESEP_LIBRARY", "/lib")]),
            PathBuf::from("/lib")
        );
        assert_eq!(
            with(&[("XDG_DATA_HOME", "/data"), ("TYPESEP_LIBRARY", "")]),
            PathBuf::from("/data/typesep/library"),
            "an empty variable is unset"
        );
    }

    #[test]
    fn meta_json_reads_every_field() {
        let text = r#"{
  "url": "https://plato.stanford.edu/entries/turing-machine/",
  "title": "Turing Machines",
  "revision": "Wed May 21, 2025",
  "files": {
    "index.html": { "fetchedAt": "2026-09-03T07:21:46.210Z" },
    "local.js": { "fetchedAt": "2026-09-03T07:23:31.651Z", "etag": "\"5ba95fb1-cc\"", "lastModified": "Mon, 24 Sep 2018 22:05:37 GMT" },
    "notes.html": { "fetchedAt": "2026-09-03T07:26:29.245Z" },
    "figdesc.html": { "fetchedAt": "2026-09-09T11:12:40.842Z", "missing": true }
  }
}"#;
        let meta: UnitMeta = serde_json::from_str(text).unwrap();
        assert_eq!(meta.url, TM);
        assert_eq!(meta.title.as_deref(), Some("Turing Machines"));
        assert_eq!(meta.previous_revision, None);
        assert_eq!(
            meta.files.keys().collect::<Vec<_>>(),
            ["figdesc.html", "index.html", "local.js", "notes.html"],
            "sorted"
        );
        assert_eq!(
            meta.files["local.js"].etag.as_deref(),
            Some("\"5ba95fb1-cc\"")
        );
        assert!(meta.files["figdesc.html"].missing);
        assert!(!meta.files["index.html"].missing);

        let written = serde_json::to_string_pretty(&meta).unwrap();
        assert_eq!(
            written.lines().take(4).collect::<Vec<_>>(),
            [
                "{",
                "  \"url\": \"https://plato.stanford.edu/entries/turing-machine/\",",
                "  \"title\": \"Turing Machines\",",
                "  \"revision\": \"Wed May 21, 2025\","
            ]
        );
        assert!(!written.contains("null"));
        assert_eq!(written.matches("\"missing\"").count(), 1);
        assert_eq!(serde_json::from_str::<UnitMeta>(&written).unwrap(), meta);
    }

    #[test]
    fn a_moment_is_stamped_as_javascript_stamps_it() {
        let stamp = now_iso8601();
        assert_eq!(stamp.len(), 24, "{stamp}");
        assert_eq!(&stamp[4..5], "-");
        assert_eq!(&stamp[10..11], "T");
        assert_eq!(&stamp[23..], "Z");
        assert!(stamp[..4].chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn the_old_layout_is_refused_and_init_makes_the_directory() {
        let old = tempfile::tempdir().unwrap();
        fs::create_dir_all(old.path().join("pages")).unwrap();
        let err = Library::new(old.path()).init().unwrap_err();
        assert!(matches!(err, LibraryError::OldLayout { .. }));
        assert!(err.to_string().contains("name another directory"));

        fs::create_dir_all(old.path().join("sep/entries")).unwrap();
        Library::new(old.path())
            .init()
            .expect("a pages directory beside a source's is not the old layout");

        let temp = tempfile::tempdir().unwrap();
        let fresh = temp.path().join("new");
        Library::new(&fresh).init().unwrap();
        assert!(fresh.is_dir(), "init creates the directory");
    }

    /// Writes `files` (path, contents) under `dir`, a path ending in `/` making a directory.
    fn seed(dir: &Path, files: &[(&str, &str)]) {
        for (name, body) in files {
            let path = dir.join(name.trim_end_matches('/'));
            if name.ends_with('/') {
                fs::create_dir_all(&path).unwrap();
            } else {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, body).unwrap();
            }
        }
    }

    /// Every file and directory under `dir`, with what each file holds.
    fn listing(dir: &Path) -> Vec<(String, Option<String>)> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_owned()];
        while let Some(at) = stack.pop() {
            for entry in fs::read_dir(&at).unwrap().map(Result::unwrap) {
                let path = entry.path();
                let name = path
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if path.is_dir() {
                    out.push((format!("{name}/"), None));
                    stack.push(path);
                } else {
                    out.push((name, Some(fs::read_to_string(&path).unwrap())));
                }
            }
        }
        out.sort();
        out
    }

    const SEP_SITE: &str = "https://plato.stanford.edu/";

    /// The SEP as the migration knows it: its key, and the units its addresses are placed at,
    /// a directory's address at the directory.
    struct OldSep;

    impl Source for OldSep {
        fn key(&self) -> &'static str {
            "sep"
        }
        fn claims(&self, input: &str) -> bool {
            input.starts_with(SEP_SITE)
        }
        fn resolve(
            &self,
            _: &str,
            _: Option<&str>,
        ) -> Result<crate::source::WorkRef, crate::source::ResolveError> {
            unreachable!("a migration resolves nothing")
        }
        fn place(&self, url: &str) -> Result<Place, PlaceError> {
            let path = url.strip_prefix(SEP_SITE).ok_or(PlaceError::OtherSite {
                url: url.to_owned(),
                site: SEP_SITE,
            })?;
            let (unit, file) = path.rsplit_once('/').unwrap_or(("", path));
            Ok(Place {
                unit: unit.to_owned(),
                file: if file.is_empty() { "index.html" } else { file }.to_owned(),
                unit_url: if unit.is_empty() {
                    SEP_SITE.to_owned()
                } else {
                    format!("{SEP_SITE}{unit}/")
                },
            })
        }
        fn load(
            &self,
            _: &crate::source::WorkRef,
            _: &mut crate::net::Net<'_>,
            _: crate::source::LoadOptions<'_>,
        ) -> Result<crate::source::Loaded, crate::net::LoadError> {
            unreachable!("a migration loads nothing")
        }
        fn sections(&self) -> &'static [crate::source::SectionSpec] {
            &[]
        }
        fn held(&self, _: &Store<'_>) -> Vec<crate::source::Held> {
            Vec::new()
        }
    }

    /// The `meta.json` typesep wrote for the SEP's unit at `unit`.
    fn sep_record(unit: &str) -> String {
        let url = if unit.is_empty() {
            SEP_SITE.to_owned()
        } else {
            format!("{SEP_SITE}{unit}/")
        };
        format!(r#"{{"url":"{url}","files":{{"index.html":{{}}}}}}"#)
    }

    /// `listing` with the empty lock file a typesep that found the old layout leaves at the top.
    fn with_the_lock(mut listing: Vec<(String, Option<String>)>) -> Vec<(String, Option<String>)> {
        listing.push((MIGRATION_LOCK.to_owned(), Some(String::new())));
        listing.sort();
        listing
    }

    const LEGACY: [(&str, &str); 10] = [
        (
            "meta.json",
            r#"{"url":"https://plato.stanford.edu/","files":{}}"#,
        ),
        ("contents.html", "<ol/>"),
        (
            "entries/turing-machine/meta.json",
            r#"{"url":"https://plato.stanford.edu/entries/turing-machine/","files":{}}"#,
        ),
        ("entries/turing-machine/index.html", "<h1>TM</h1>"),
        (
            "archives/sum2026/entries/kant/meta.json",
            r#"{"url":"https://plato.stanford.edu/archives/sum2026/entries/kant/","files":{}}"#,
        ),
        ("archives/sum2026/entries/kant/index.html", "<h1>Kant</h1>"),
        (".derived/entries/turing-machine/S1.svg.png", "png"),
        ("info/", ""),
        (".git/HEAD", "ref: refs/heads/main"),
        ("lwp/x/index.html", "<p/>"),
    ];

    fn moved_under_sep(files: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
        let stays = |name: &str| name.starts_with(".git/") || name.starts_with("lwp/");
        let mut want: Vec<(&str, &str)> = files.to_vec();
        want.retain(|(name, _)| !stays(name));
        let target = tempfile::tempdir().unwrap();
        seed(&target.path().join("sep"), &want);
        let kept: Vec<(&str, &str)> = files
            .iter()
            .copied()
            .filter(|(name, _)| stays(name))
            .collect();
        seed(target.path(), &kept);
        with_the_lock(listing(target.path()))
    }

    #[test]
    fn a_library_of_the_layout_before_sources_moves_into_the_seps_directory() {
        let temp = tempfile::tempdir().unwrap();
        seed(temp.path(), &LEGACY);
        let library = Library::new(temp.path());

        let migrated = library
            .migrate_legacy(&OldSep, &["sep", "lwp"])
            .unwrap()
            .expect("a migration");
        assert_eq!(migrated.into, temp.path().join("sep"));
        assert_eq!(
            migrated.moved,
            [
                ".derived",
                "contents.html",
                "info",
                "archives",
                "entries",
                "meta.json"
            ],
            "what marks the old layout goes last, the other sources' directories and the dot \
             files stay"
        );
        assert_eq!(listing(temp.path()), moved_under_sep(&LEGACY));

        let store_dir = library.dir().join("sep");
        assert!(
            store_dir
                .join("entries/turing-machine/index.html")
                .is_file()
        );
        assert_eq!(
            library.migrate_legacy(&OldSep, &["sep", "lwp"]).unwrap(),
            None,
            "once is enough"
        );
        assert_eq!(listing(temp.path()), moved_under_sep(&LEGACY));
    }

    #[test]
    fn a_library_in_the_present_layout_or_none_at_all_is_left_alone() {
        let temp = tempfile::tempdir().unwrap();
        let present = [
            ("sep/meta.json", "{}"),
            ("sep/entries/turing-machine/index.html", "<h1>TM</h1>"),
            ("notes.txt", "mine"),
        ];
        seed(temp.path(), &present);
        let before = listing(temp.path());
        let library = Library::new(temp.path());
        assert_eq!(library.migrate_legacy(&OldSep, &["sep"]).unwrap(), None);
        assert_eq!(listing(temp.path()), before);

        let empty = tempfile::tempdir().unwrap();
        assert_eq!(
            Library::new(empty.path())
                .migrate_legacy(&OldSep, &["sep"])
                .unwrap(),
            None
        );
        let absent = empty.path().join("nowhere");
        assert_eq!(
            Library::new(&absent)
                .migrate_legacy(&OldSep, &["sep"])
                .unwrap(),
            None
        );
        assert!(
            !absent.exists(),
            "nothing is created for a library that is not there"
        );
    }

    #[test]
    fn a_folder_with_the_names_of_the_old_layout_but_no_record_of_typesep_is_left_alone() {
        let temp = tempfile::tempdir().unwrap();
        seed(
            temp.path(),
            &[
                ("archives/2019/taxes.pdf", "%PDF"),
                ("entries/diary/2020.txt", "dear diary"),
                ("entries/diary/meta.json", r#"{"author":"me"}"#),
                (
                    "entries/recipes/meta.json",
                    r#"{"url":"https://example.org/entries/recipes/","files":{}}"#,
                ),
                (
                    "entries/kant/meta.json",
                    r#"{"url":"https://plato.stanford.edu/entries/hume/","files":{}}"#,
                ),
                ("meta.json", r#"{"url":"https://example.org/","files":{}}"#),
                ("notes.txt", "mine"),
                ("photos/", ""),
            ],
        );
        let before = listing(temp.path());
        assert_eq!(
            Library::new(temp.path())
                .migrate_legacy(&OldSep, &["sep", "lwp"])
                .unwrap(),
            None
        );
        assert_eq!(
            listing(temp.path()),
            before,
            "nothing moved, and no lock file made"
        );
    }

    #[test]
    fn a_library_of_the_old_layout_with_only_its_entries_moves() {
        let temp = tempfile::tempdir().unwrap();
        let files = [
            ("entries/kant/index.html", "<h1>Kant</h1>"),
            ("entries/kant/meta.json", &sep_record("entries/kant")),
            ("entries/hume/index.html", "<h1>Hume</h1>"),
        ];
        seed(temp.path(), &files);
        let migrated = Library::new(temp.path())
            .migrate_legacy(&OldSep, &["sep", "lwp"])
            .unwrap()
            .expect("a migration");
        assert_eq!(migrated.moved, ["entries"]);
        assert_eq!(listing(temp.path()), moved_under_sep(&files));
    }

    #[cfg(unix)]
    #[test]
    fn a_library_that_cannot_hold_the_lock_file_says_what_cannot_move_instead() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        seed(temp.path(), &LEGACY);
        fs::create_dir(temp.path().join("sep")).unwrap();
        let mode = |mode| fs::set_permissions(temp.path(), fs::Permissions::from_mode(mode));
        mode(0o555).unwrap();
        if fs::write(temp.path().join("probe"), "").is_ok() {
            // Whoever may write in any directory can make the lock file too.
            mode(0o755).unwrap();
            return;
        }
        let err = Library::new(temp.path())
            .migrate_legacy(&OldSep, &["sep", "lwp"])
            .unwrap_err();
        mode(0o755).unwrap();
        let LibraryError::Io { path, source } = &err else {
            panic!("{err}");
        };
        assert_eq!(source.kind(), io::ErrorKind::PermissionDenied, "{err}");
        assert_eq!(
            path,
            &temp.path().join(".derived"),
            "the first name to move"
        );
        assert!(!temp.path().join(MIGRATION_LOCK).exists());
    }

    #[test]
    fn the_lock_an_earlier_migration_left_is_no_mark_of_the_old_layout() {
        let temp = tempfile::tempdir().unwrap();
        seed(
            temp.path(),
            &[(MIGRATION_LOCK, ""), ("sep/meta.json", "{}"), ("info/", "")],
        );
        let before = listing(temp.path());
        assert_eq!(
            Library::new(temp.path())
                .migrate_legacy(&OldSep, &["sep"])
                .unwrap(),
            None
        );
        assert_eq!(listing(temp.path()), before);
    }

    #[test]
    fn a_migration_cut_short_is_finished_by_the_next_run() {
        // Each point a migration can stop at: after making the directory, after moving the
        // first names, after moving all but the last mark.
        for done in [0, 3, 5] {
            let temp = tempfile::tempdir().unwrap();
            seed(temp.path(), &LEGACY);
            let order = [
                ".derived",
                "contents.html",
                "info",
                "archives",
                "entries",
                "meta.json",
            ];
            fs::create_dir(temp.path().join("sep")).unwrap();
            for name in &order[..done] {
                fs::rename(temp.path().join(name), temp.path().join("sep").join(name)).unwrap();
            }

            let migrated = Library::new(temp.path())
                .migrate_legacy(&OldSep, &["sep", "lwp"])
                .unwrap()
                .expect("the rest is moved");
            assert_eq!(migrated.moved, order[done..], "after {done} moves");
            assert_eq!(listing(temp.path()), moved_under_sep(&LEGACY));
        }
    }

    #[test]
    fn typesep_run_many_times_at_once_migrates_once_without_error() {
        for _ in 0..20 {
            let temp = tempfile::tempdir().unwrap();
            seed(temp.path(), &LEGACY);
            let results: Vec<Result<Option<Migrated>, String>> = std::thread::scope(|scope| {
                let runs: Vec<_> = (0..8)
                    .map(|_| {
                        scope.spawn(|| {
                            Library::new(temp.path())
                                .migrate_legacy(&OldSep, &["sep", "lwp"])
                                .map_err(|e| e.to_string())
                        })
                    })
                    .collect();
                runs.into_iter().map(|run| run.join().unwrap()).collect()
            });
            let mut moved: Vec<String> = Vec::new();
            for result in results {
                moved.extend(result.unwrap().into_iter().flat_map(|m| m.moved));
            }
            moved.sort();
            assert_eq!(
                moved,
                [
                    ".derived",
                    "archives",
                    "contents.html",
                    "entries",
                    "info",
                    "meta.json"
                ],
                "each name moved by one of them"
            );
            assert_eq!(listing(temp.path()), moved_under_sep(&LEGACY));
        }
    }

    #[test]
    fn both_layouts_with_the_same_names_are_refused_and_nothing_moves() {
        let temp = tempfile::tempdir().unwrap();
        seed(
            temp.path(),
            &[
                ("entries/turing-machine/index.html", "<h1>old</h1>"),
                ("meta.json", &sep_record("")),
                ("contents.html", "<ol/>"),
                ("sep/entries/turing-machine/index.html", "<h1>new</h1>"),
                ("sep/meta.json", "{}"),
            ],
        );
        let before = listing(temp.path());
        let err = Library::new(temp.path())
            .migrate_legacy(&OldSep, &["sep"])
            .unwrap_err();
        assert!(matches!(err, LibraryError::BothLayouts { .. }));
        assert_eq!(
            err.to_string(),
            format!(
                "{0} holds an older library and a newer one, and both have entries, meta.json\n  typesep moves an older library into {0}/sep by itself; move one of each pair out of the way, then run it again",
                temp.path().display()
            )
        );
        assert_eq!(listing(temp.path()), with_the_lock(before), "nothing moved");

        let file = tempfile::tempdir().unwrap();
        seed(
            file.path(),
            &[
                ("entries/x/index.html", "<p/>"),
                ("entries/x/meta.json", &sep_record("entries/x")),
                ("sep", "a file"),
            ],
        );
        let before = listing(file.path());
        let err = Library::new(file.path())
            .migrate_legacy(&OldSep, &["sep"])
            .unwrap_err();
        assert!(matches!(err, LibraryError::NotADirectory { .. }), "{err}");
        assert_eq!(listing(file.path()), with_the_lock(before));
    }
}
