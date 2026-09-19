use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

pub const HOST: &str = "https://plato.stanford.edu";
const META: &str = "meta.json";
const DERIVED: &str = ".derived";
const INDEX: &str = "index.html";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlaceError {
    #[error("{0} is not a URL")]
    NotAUrl(String),
    #[error("{0} is not on {HOST}; the library mirrors only that site")]
    OtherSite(String),
    #[error("{0} has a query string, which the library cannot store")]
    QueryString(String),
    #[error("{url}: \"{segment}\" cannot be a file name in the library")]
    UnsafeSegment { url: String, segment: String },
    #[error("{0}: {META} is the library's own file")]
    MetaName(String),
}

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error(transparent)]
    Place(#[from] PlaceError),
    #[error(
        "{dir} has the layout of an older typesep (pages/<key>.html)\n  no released version ever wrote it: name another directory, and `fetch` into it"
    )]
    OldLayout { dir: String },
    #[error("{url} is not an entry page")]
    NotAnEntryPage { url: String },
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryEntry {
    pub url: String,
    pub slug: String,
    pub edition: String,
    pub revision: String,
    pub title: String,
    pub fetched_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRecord {
    pub title: Option<String>,
    pub revision: String,
    pub previous_revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub unit: String,
    pub file: String,
}

impl Place {
    pub fn unit_url(&self) -> String {
        if self.unit.is_empty() {
            format!("{HOST}/")
        } else {
            format!("{HOST}/{}/", self.unit)
        }
    }
}

fn safe_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_~!$&'()+,;=@%-".contains(c)
}

fn is_safe_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    match chars.next() {
        Some(first) if safe_char(first) => chars.all(|c| c == '.' || safe_char(c)),
        _ => false,
    }
}

pub fn place_for(url: &str) -> Result<Place, PlaceError> {
    let parsed = Url::parse(url).map_err(|_| PlaceError::NotAUrl(url.to_owned()))?;
    if parsed.origin().ascii_serialization() != HOST {
        return Err(PlaceError::OtherSite(url.to_owned()));
    }
    if parsed.query().is_some_and(|q| !q.is_empty()) {
        return Err(PlaceError::QueryString(url.to_owned()));
    }
    let path = parsed.path();
    let pathname = if path.ends_with('/') {
        format!("{path}{INDEX}")
    } else {
        path.to_owned()
    };
    let segments: Vec<&str> = pathname
        .strip_prefix('/')
        .unwrap_or(&pathname)
        .split('/')
        .collect();
    for segment in &segments {
        if !is_safe_segment(segment) {
            return Err(PlaceError::UnsafeSegment {
                url: url.to_owned(),
                segment: (*segment).to_owned(),
            });
        }
    }
    if segments.last() == Some(&META) {
        return Err(PlaceError::MetaName(url.to_owned()));
    }
    let unit_len = match segments.as_slice() {
        ["entries", _, _, ..] => 2,
        ["archives", _, "entries", _, _, ..] => 4,
        _ => segments.len() - 1,
    };
    Ok(Place {
        unit: segments[..unit_len].join("/"),
        file: segments[unit_len..].join("/"),
    })
}

fn is_entry_unit(unit: &str) -> bool {
    match unit.split('/').collect::<Vec<_>>().as_slice() {
        ["entries", slug] => !slug.is_empty(),
        ["archives", edition, "entries", slug] => !edition.is_empty() && !slug.is_empty(),
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
        if self.dir.join("pages").exists() && !self.dir.join("entries").exists() {
            return Err(LibraryError::OldLayout {
                dir: self.dir.display().to_string(),
            });
        }
        fs::create_dir_all(&self.dir).map_err(|e| io_error(&self.dir, e))
    }

    fn unit_dir(&self, unit: &str) -> PathBuf {
        if unit.is_empty() {
            self.dir.clone()
        } else {
            self.dir.join(unit)
        }
    }

    pub fn file_path(&self, url: &str) -> Result<PathBuf, PlaceError> {
        let place = place_for(url)?;
        Ok(self.unit_dir(&place.unit).join(&place.file))
    }

    pub fn derived_path(&self, url: &str, suffix: &str) -> Result<PathBuf, PlaceError> {
        let place = place_for(url)?;
        let base = self.dir.join(DERIVED);
        let base = if place.unit.is_empty() {
            base
        } else {
            base.join(&place.unit)
        };
        Ok(base.join(format!("{}{suffix}", place.file)))
    }

    pub fn unit_meta(&self, unit: &str) -> Option<UnitMeta> {
        let text = fs::read_to_string(self.unit_dir(unit).join(META)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn read_meta(&self, url: &str) -> Result<Option<FileMeta>, PlaceError> {
        let place = place_for(url)?;
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
        let target = dir.join(META);
        let tmp = dir.join(format!("{META}.{}.tmp", std::process::id()));
        fs::write(&tmp, text).map_err(|e| io_error(&tmp, e))?;
        fs::rename(&tmp, &target).map_err(|e| io_error(&target, e))
    }

    fn unit_or_new(&self, place: &Place) -> UnitMeta {
        self.unit_meta(&place.unit).unwrap_or_else(|| UnitMeta {
            url: place.unit_url(),
            title: None,
            revision: None,
            previous_revision: None,
            files: BTreeMap::new(),
        })
    }

    pub fn put(&self, url: &str, body: Option<&[u8]>, meta: FileMeta) -> Result<(), LibraryError> {
        let place = place_for(url)?;
        if let Some(bytes) = body {
            let target = self.unit_dir(&place.unit).join(&place.file);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
            }
            fs::write(&target, bytes).map_err(|e| io_error(&target, e))?;
        }
        let mut unit = self.unit_or_new(&place);
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

    pub fn record_entry(
        &self,
        entry_url: &str,
        title: &str,
        revision: &str,
    ) -> Result<(), LibraryError> {
        let place = place_for(entry_url)?;
        if place.file != INDEX || !is_entry_unit(&place.unit) {
            return Err(LibraryError::NotAnEntryPage {
                url: entry_url.to_owned(),
            });
        }
        let mut unit = self.unit_or_new(&place);
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

    pub fn entry_meta(&self, entry_url: &str) -> Result<Option<EntryRecord>, PlaceError> {
        let place = place_for(entry_url)?;
        Ok(self.unit_meta(&place.unit).and_then(|unit| {
            unit.revision.map(|revision| EntryRecord {
                title: unit.title,
                revision,
                previous_revision: unit.previous_revision,
            })
        }))
    }

    fn entries_under(&self, prefix: &str, edition: &str, out: &mut Vec<LibraryEntry>) {
        let Ok(dir) = fs::read_dir(self.unit_dir(prefix).join("entries")) else {
            return;
        };
        for slug in dir
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
        {
            let unit = if prefix.is_empty() {
                format!("entries/{slug}")
            } else {
                format!("{prefix}/entries/{slug}")
            };
            let Some(meta) = self.unit_meta(&unit) else {
                continue;
            };
            let Some(revision) = meta.revision else {
                continue;
            };
            let Some(page) = meta.files.get(INDEX) else {
                continue;
            };
            if page.missing {
                continue;
            }
            out.push(LibraryEntry {
                url: meta.url,
                slug,
                edition: edition.to_owned(),
                revision,
                title: meta.title.unwrap_or_default(),
                fetched_at: page.fetched_at.clone(),
            });
        }
    }

    pub fn list_entries(&self) -> Vec<LibraryEntry> {
        let mut out = Vec::new();
        self.entries_under("", "live", &mut out);
        if let Ok(archives) = fs::read_dir(self.dir.join("archives")) {
            for edition in archives
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
            {
                if !self
                    .dir
                    .join("archives")
                    .join(&edition)
                    .join("entries")
                    .exists()
                {
                    continue;
                }
                self.entries_under(&format!("archives/{edition}"), &edition, &mut out);
            }
        }
        out.sort_by(|a, b| a.slug.cmp(&b.slug).then_with(|| a.edition.cmp(&b.edition)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TM: &str = "https://plato.stanford.edu/entries/turing-machine/";

    fn place(unit: &str, file: &str) -> Place {
        Place {
            unit: unit.to_owned(),
            file: file.to_owned(),
        }
    }

    #[test]
    fn a_url_is_placed_at_its_path() {
        assert_eq!(
            place_for(TM).unwrap(),
            place("entries/turing-machine", "index.html")
        );
        assert_eq!(
            place_for(&format!("{TM}local.js")).unwrap(),
            place("entries/turing-machine", "local.js")
        );
        assert_eq!(
            place_for(&format!("{TM}notes.html#note-1")).unwrap(),
            place("entries/turing-machine", "notes.html"),
            "the fragment is not part of the file"
        );
        assert_eq!(
            place_for(&format!(
                "{HOST}/entries/legal-probabilism/BNfiles/unnamed-chunk-9-1"
            ))
            .unwrap(),
            place("entries/legal-probabilism", "BNfiles/unnamed-chunk-9-1")
        );
        assert_eq!(
            place_for(&format!(
                "{HOST}/archives/sum2026/entries/turing-machine/S1.svg"
            ))
            .unwrap(),
            place("archives/sum2026/entries/turing-machine", "S1.svg")
        );
        assert_eq!(
            place_for(&format!("{HOST}/contents.html")).unwrap(),
            place("", "contents.html")
        );
        assert_eq!(
            place_for(&format!("{HOST}/archives/")).unwrap(),
            place("archives", "index.html")
        );
        assert_eq!(
            place_for(&format!("{HOST}/entries/frege-theorem/frege-R+xy.jpg")).unwrap(),
            place("entries/frege-theorem", "frege-R+xy.jpg")
        );
        assert_eq!(
            place_for(&format!("{HOST}/entries/x/../y/")).unwrap(),
            place("entries/y", "index.html"),
            "dot segments are resolved, never stored"
        );
        assert_eq!(
            place_for(&format!("{HOST}/entries/x")).unwrap(),
            place("entries", "x"),
            "without a trailing slash the last segment is a file"
        );

        assert_eq!(
            place_for(&format!("{TM}?x=1")),
            Err(PlaceError::QueryString(format!("{TM}?x=1")))
        );
        assert!(matches!(
            place_for("https://example.org/entries/x/"),
            Err(PlaceError::OtherSite(_))
        ));
        assert!(
            matches!(
                place_for("http://plato.stanford.edu/entries/x/"),
                Err(PlaceError::OtherSite(_))
            ),
            "the scheme is part of the origin"
        );
        assert_eq!(
            place_for(&format!("{HOST}/entries/x/.hidden")),
            Err(PlaceError::UnsafeSegment {
                url: format!("{HOST}/entries/x/.hidden"),
                segment: ".hidden".to_owned()
            })
        );
        assert!(
            matches!(
                place_for(&format!("{HOST}//entries/x/")),
                Err(PlaceError::UnsafeSegment { .. })
            ),
            "an empty segment is not a name"
        );
        assert!(matches!(
            place_for(&format!("{HOST}/entries/x/meta.json")),
            Err(PlaceError::MetaName(_))
        ));
        assert!(matches!(
            place_for("not a url"),
            Err(PlaceError::NotAUrl(_))
        ));

        assert_eq!(place("", "contents.html").unit_url(), format!("{HOST}/"));
        assert_eq!(place("entries/turing-machine", "index.html").unit_url(), TM);
    }

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
    fn the_write_side_stores_the_file_and_its_record() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        library.init().unwrap();

        library
            .write(TM, b"<h1>Turing Machines</h1>", Some("\"abc\""), None)
            .unwrap();
        assert_eq!(
            library.read_body(TM).unwrap().as_deref(),
            Some("<h1>Turing Machines</h1>")
        );
        assert!(library.has(TM).unwrap());
        let meta = library.read_meta(TM).unwrap().expect("a record");
        assert_eq!(meta.etag.as_deref(), Some("\"abc\""));
        assert_eq!(meta.last_modified, None);
        assert!(!meta.missing);
        let stamp = meta.fetched_at.expect("a fetchedAt");
        assert_eq!(stamp.len(), 24, "{stamp} is an ISO 8601 instant");
        assert!(stamp.ends_with('Z') && stamp.contains('T'));

        let figure = "https://plato.stanford.edu/entries/turing-machine/BNfiles/chunk-9-1";
        library.write(figure, b"\x89PNG", None, None).unwrap();
        assert!(
            temp.path()
                .join("entries/turing-machine/BNfiles/chunk-9-1")
                .is_file()
        );
        assert_eq!(
            library
                .unit_meta("entries/turing-machine")
                .unwrap()
                .files
                .keys()
                .collect::<Vec<_>>(),
            ["BNfiles/chunk-9-1", "index.html"],
            "one record per file, sorted"
        );

        let gone = "https://plato.stanford.edu/entries/turing-machine/figdesc.html";
        library.mark_missing(gone).unwrap();
        assert!(library.is_missing(gone).unwrap());
        assert!(!library.has(gone).unwrap());
        assert_eq!(library.read_body(gone).unwrap(), None);
        library.write(gone, b"<p>back</p>", None, None).unwrap();
        assert!(!library.is_missing(gone).unwrap());
    }

    #[test]
    fn a_revision_that_moves_keeps_the_one_it_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        library.init().unwrap();
        library.write(TM, b"<html/>", None, None).unwrap();

        library
            .record_entry(TM, "Turing Machines", "Wed May 21, 2025")
            .unwrap();
        let record = library.entry_meta(TM).unwrap().expect("a record");
        assert_eq!(record.title.as_deref(), Some("Turing Machines"));
        assert_eq!(record.revision, "Wed May 21, 2025");
        assert_eq!(record.previous_revision, None);

        library
            .record_entry(TM, "Turing Machines", "Wed May 21, 2025")
            .unwrap();
        assert_eq!(
            library.entry_meta(TM).unwrap().unwrap().previous_revision,
            None
        );

        library
            .record_entry(TM, "Turing Machines", "Thu Sep 10, 2026")
            .unwrap();
        let record = library.entry_meta(TM).unwrap().unwrap();
        assert_eq!(record.revision, "Thu Sep 10, 2026");
        assert_eq!(
            record.previous_revision.as_deref(),
            Some("Wed May 21, 2025"),
            "what a refresh replaced"
        );

        let entries = library.list_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].slug, "turing-machine");
        assert_eq!(entries[0].edition, "live");

        let err = library
            .record_entry(&format!("{TM}notes.html"), "x", "y")
            .unwrap_err();
        assert!(matches!(err, LibraryError::NotAnEntryPage { .. }));
    }

    #[test]
    fn a_written_meta_json_is_the_file_d15_describes() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        library.init().unwrap();
        library
            .put(
                TM,
                Some(b"<html/>"),
                FileMeta {
                    fetched_at: Some("2026-09-03T07:21:46.210Z".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        library
            .put(
                &format!("{TM}local.js"),
                Some(b"//"),
                FileMeta {
                    fetched_at: Some("2026-09-03T07:23:31.651Z".into()),
                    etag: Some("\"5ba95fb1-cc\"".into()),
                    last_modified: Some("Mon, 24 Sep 2018 22:05:37 GMT".into()),
                    missing: false,
                },
            )
            .unwrap();
        library
            .record_entry(TM, "Turing Machines", "Wed May 21, 2025")
            .unwrap();

        let text =
            fs::read_to_string(temp.path().join("entries/turing-machine/meta.json")).unwrap();
        assert_eq!(
            text,
            r#"{
  "url": "https://plato.stanford.edu/entries/turing-machine/",
  "title": "Turing Machines",
  "revision": "Wed May 21, 2025",
  "files": {
    "index.html": {
      "fetchedAt": "2026-09-03T07:21:46.210Z"
    },
    "local.js": {
      "fetchedAt": "2026-09-03T07:23:31.651Z",
      "etag": "\"5ba95fb1-cc\"",
      "lastModified": "Mon, 24 Sep 2018 22:05:37 GMT"
    }
  }
}
"#,
            "two-space indentation, the library's key order, sorted files, a trailing newline"
        );
        assert!(
            !temp
                .path()
                .join("entries/turing-machine")
                .read_dir()
                .unwrap()
                .filter_map(Result::ok)
                .any(|e| e.file_name().to_string_lossy().ends_with(".tmp")),
            "the temporary file is renamed, not left behind"
        );
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

    fn seed(dir: &Path, unit: &str, meta: &str, files: &[(&str, &str)]) {
        let unit_dir = if unit.is_empty() {
            dir.to_owned()
        } else {
            dir.join(unit)
        };
        fs::create_dir_all(&unit_dir).unwrap();
        fs::write(unit_dir.join(META), meta).unwrap();
        for (name, body) in files {
            let path = unit_dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
    }

    #[test]
    fn the_reader_finds_files_records_and_entries() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        seed(
            dir,
            "entries/turing-machine",
            r#"{"url":"https://plato.stanford.edu/entries/turing-machine/","title":"Turing Machines","revision":"Wed May 21, 2025",
                "files":{"index.html":{"fetchedAt":"2026-09-03T07:21:46.210Z","etag":"\"e1\""},"local.js":{"fetchedAt":"2026-09-03T07:23:31.651Z","missing":true},
                         "BNfiles/deep.png":{"fetchedAt":"2026-09-09T00:00:00.000Z"}}}"#,
            &[
                ("index.html", "<html/>"),
                ("BNfiles/deep.png", "\u{1}\u{2}"),
                ("empty.png", ""),
            ],
        );
        seed(
            dir,
            "entries/unparsed",
            r#"{"url":"https://plato.stanford.edu/entries/unparsed/","files":{"index.html":{"fetchedAt":"2026-09-03T00:00:00.000Z"}}}"#,
            &[("index.html", "<html/>")],
        );
        seed(
            dir,
            "entries/gone",
            r#"{"url":"https://plato.stanford.edu/entries/gone/","revision":"x","files":{"index.html":{"fetchedAt":"2026-09-03T00:00:00.000Z","missing":true}}}"#,
            &[],
        );
        seed(
            dir,
            "archives/sum2026/entries/turing-machine",
            r#"{"url":"https://plato.stanford.edu/archives/sum2026/entries/turing-machine/","title":"Turing Machines","revision":"Mon Sep 24, 2018","files":{"index.html":{"fetchedAt":"2026-09-04T00:00:00.000Z"}}}"#,
            &[("index.html", "<html/>")],
        );
        seed(
            dir,
            "",
            r#"{"url":"https://plato.stanford.edu/","files":{"contents.html":{"fetchedAt":"2026-09-10T12:24:38.287Z"}}}"#,
            &[("contents.html", "<ol/>")],
        );
        fs::create_dir_all(dir.join("archives/broken")).unwrap();

        let library = Library::new(dir);
        library.init().unwrap();
        assert_eq!(
            library.file_path(TM).unwrap(),
            dir.join("entries/turing-machine/index.html")
        );
        assert_eq!(
            library
                .derived_path(&format!("{TM}S1.svg"), ".pdf")
                .unwrap(),
            dir.join(".derived/entries/turing-machine/S1.svg.pdf")
        );
        assert_eq!(
            library
                .derived_path(&format!("{HOST}/contents.html"), ".x")
                .unwrap(),
            dir.join(".derived/contents.html.x")
        );

        assert_eq!(library.read_body(TM).unwrap().as_deref(), Some("<html/>"));
        assert_eq!(library.read_body(&format!("{TM}local.js")).unwrap(), None);
        assert_eq!(
            library
                .read_body(&format!("{HOST}/contents.html"))
                .unwrap()
                .as_deref(),
            Some("<ol/>")
        );
        assert_eq!(
            library
                .read_bytes(&format!("{TM}BNfiles/deep.png"))
                .unwrap(),
            Some(vec![1, 2])
        );
        assert!(library.has(&format!("{TM}BNfiles/deep.png")).unwrap());
        assert!(
            !library.has(&format!("{TM}empty.png")).unwrap(),
            "an empty file does not count"
        );
        assert!(!library.has(&format!("{TM}nowhere.png")).unwrap());
        assert!(library.is_missing(&format!("{TM}local.js")).unwrap());
        assert!(!library.is_missing(TM).unwrap());
        assert!(!library.is_missing(&format!("{TM}nowhere.png")).unwrap());
        assert_eq!(
            library.read_meta(TM).unwrap().unwrap().etag.as_deref(),
            Some("\"e1\"")
        );
        assert_eq!(
            library
                .read_meta(&format!("{HOST}/entries/nowhere/"))
                .unwrap(),
            None
        );

        assert_eq!(
            library.entry_meta(TM).unwrap(),
            Some(EntryRecord {
                title: Some("Turing Machines".to_owned()),
                revision: "Wed May 21, 2025".to_owned(),
                previous_revision: None
            })
        );
        assert_eq!(
            library
                .entry_meta(&format!("{HOST}/entries/unparsed/"))
                .unwrap(),
            None,
            "a page that was never parsed has no revision"
        );
        assert_eq!(
            library
                .entry_meta(&format!("{HOST}/entries/nowhere/"))
                .unwrap(),
            None
        );

        let listed = library.list_entries();
        let summary: Vec<(&str, &str, &str)> = listed
            .iter()
            .map(|e| (e.slug.as_str(), e.edition.as_str(), e.revision.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("turing-machine", "live", "Wed May 21, 2025"),
                ("turing-machine", "sum2026", "Mon Sep 24, 2018")
            ]
        );
        assert_eq!(
            listed[0].fetched_at.as_deref(),
            Some("2026-09-03T07:21:46.210Z")
        );
        assert_eq!(
            listed[1].url,
            format!("{HOST}/archives/sum2026/entries/turing-machine/")
        );
    }

    #[test]
    fn a_corrupt_meta_json_reads_as_absent_and_the_old_layout_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        seed(dir, "entries/x", "{ not json", &[("index.html", "<html/>")]);
        let library = Library::new(dir);
        assert_eq!(library.unit_meta("entries/x"), None);
        assert_eq!(
            library
                .read_body(&format!("{HOST}/entries/x/"))
                .unwrap()
                .as_deref(),
            Some("<html/>"),
            "the file is still readable"
        );
        assert!(library.list_entries().is_empty());

        let old = tempfile::tempdir().unwrap();
        fs::create_dir_all(old.path().join("pages")).unwrap();
        let err = Library::new(old.path()).init().unwrap_err();
        assert!(matches!(err, LibraryError::OldLayout { .. }));
        assert!(err.to_string().contains("name another directory"));

        let fresh = temp.path().join("new");
        Library::new(&fresh).init().unwrap();
        assert!(fresh.is_dir(), "init creates the directory");
    }
}
