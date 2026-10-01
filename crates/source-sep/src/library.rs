use std::fs;

use typesep_net::library::{LibraryError, Place, PlaceError, Store, is_safe_segment};
use url::Url;

pub const HOST: &str = "https://plato.stanford.edu";
const INDEX: &str = "index.html";
const META: &str = typesep_net::library::META;

/// Where the library keeps what a URL on the site names: the path, as the site serves it, with
/// an entry's directory (live or archived) as its unit and `index.html` for a directory.
pub fn place_for(url: &str) -> Result<Place, PlaceError> {
    let parsed = Url::parse(url).map_err(|_| PlaceError::NotAUrl(url.to_owned()))?;
    if parsed.origin().ascii_serialization() != HOST {
        return Err(PlaceError::OtherSite {
            url: url.to_owned(),
            site: HOST,
        });
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
    let unit = segments[..unit_len].join("/");
    let unit_url = if unit.is_empty() {
        format!("{HOST}/")
    } else {
        format!("{HOST}/{unit}/")
    };
    Ok(Place {
        unit,
        file: segments[unit_len..].join("/"),
        unit_url,
    })
}

fn is_entry_unit(unit: &str) -> bool {
    match unit.split('/').collect::<Vec<_>>().as_slice() {
        ["entries", slug] => !slug.is_empty(),
        ["archives", edition, "entries", slug] => !edition.is_empty() && !slug.is_empty(),
        _ => false,
    }
}

/// Records the title and revision of the entry whose page `entry_url` is.
pub fn record_entry(
    store: &Store<'_>,
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
    store.record_work(entry_url, title, revision)
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

fn entries_under(store: &Store<'_>, prefix: &str, edition: &str, out: &mut Vec<LibraryEntry>) {
    let base = if prefix.is_empty() {
        store.dir().to_owned()
    } else {
        store.dir().join(prefix)
    };
    let Ok(dir) = fs::read_dir(base.join("entries")) else {
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
        let Some(meta) = store.unit_meta(&unit) else {
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

/// The entries the library holds, live and archived, by slug and then edition.
pub fn list_entries(store: &Store<'_>) -> Vec<LibraryEntry> {
    let mut out = Vec::new();
    entries_under(store, "", "live", &mut out);
    let archives = store.dir().join("archives");
    if let Ok(editions) = fs::read_dir(&archives) {
        for edition in editions
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
        {
            if !archives.join(&edition).join("entries").exists() {
                continue;
            }
            entries_under(store, &format!("archives/{edition}"), &edition, &mut out);
        }
    }
    out.sort_by(|a, b| a.slug.cmp(&b.slug).then_with(|| a.edition.cmp(&b.edition)));
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use typesep_net::library::{FileMeta, Library, WorkRecord};

    use super::*;
    use crate::Sep;

    const TM: &str = "https://plato.stanford.edu/entries/turing-machine/";

    fn place(unit: &str, file: &str) -> Place {
        Place {
            unit: unit.to_owned(),
            file: file.to_owned(),
            unit_url: if unit.is_empty() {
                format!("{HOST}/")
            } else {
                format!("{HOST}/{unit}/")
            },
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
            Err(PlaceError::OtherSite { .. })
        ));
        assert!(
            matches!(
                place_for("http://plato.stanford.edu/entries/x/"),
                Err(PlaceError::OtherSite { .. })
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

        assert_eq!(
            place_for(&format!("{HOST}/contents.html"))
                .unwrap()
                .unit_url,
            format!("{HOST}/")
        );
        assert_eq!(place_for(&format!("{TM}local.js")).unwrap().unit_url, TM);
    }

    #[test]
    fn the_write_side_stores_the_file_and_its_record() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        library.init().unwrap();
        let library = library.store(&Sep);

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
                .join("sep/entries/turing-machine/BNfiles/chunk-9-1")
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
    fn a_rewritten_file_is_replaced_whole_never_rewritten_in_place() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        library.init().unwrap();
        let library = library.store(&Sep);
        library.write(TM, b"<p>first</p>", None, None).unwrap();
        let held = temp.path().join("sep/entries/turing-machine/index.html");
        let link = temp.path().join("old.html");
        fs::hard_link(&held, &link).unwrap();

        library.write(TM, b"<p>second</p>", None, None).unwrap();
        assert_eq!(fs::read(&held).unwrap(), b"<p>second</p>");
        assert_eq!(
            fs::read(&link).unwrap(),
            b"<p>first</p>",
            "the old file is untouched until the new one takes its name"
        );
        assert!(
            !held
                .parent()
                .unwrap()
                .read_dir()
                .unwrap()
                .filter_map(Result::ok)
                .any(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        );
    }

    #[test]
    fn a_revision_that_moves_keeps_the_one_it_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        library.init().unwrap();
        let library = library.store(&Sep);
        library.write(TM, b"<html/>", None, None).unwrap();

        record_entry(&library, TM, "Turing Machines", "Wed May 21, 2025").unwrap();
        let record = library.work_record(TM).unwrap().expect("a record");
        assert_eq!(record.title.as_deref(), Some("Turing Machines"));
        assert_eq!(record.revision, "Wed May 21, 2025");
        assert_eq!(record.previous_revision, None);

        record_entry(&library, TM, "Turing Machines", "Wed May 21, 2025").unwrap();
        assert_eq!(
            library.work_record(TM).unwrap().unwrap().previous_revision,
            None
        );

        record_entry(&library, TM, "Turing Machines", "Thu Sep 10, 2026").unwrap();
        let record = library.work_record(TM).unwrap().unwrap();
        assert_eq!(record.revision, "Thu Sep 10, 2026");
        assert_eq!(
            record.previous_revision.as_deref(),
            Some("Wed May 21, 2025"),
            "what a refresh replaced"
        );

        let entries = list_entries(&library);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].slug, "turing-machine");
        assert_eq!(entries[0].edition, "live");

        let err = record_entry(&library, &format!("{TM}notes.html"), "x", "y").unwrap_err();
        assert!(matches!(err, LibraryError::NotAnEntryPage { .. }));
    }

    #[test]
    fn a_written_meta_json_is_the_file_d15_describes() {
        let temp = tempfile::tempdir().unwrap();
        let library = Library::new(temp.path());
        library.init().unwrap();
        let library = library.store(&Sep);
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
        record_entry(&library, TM, "Turing Machines", "Wed May 21, 2025").unwrap();

        let text =
            fs::read_to_string(temp.path().join("sep/entries/turing-machine/meta.json")).unwrap();
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
                .join("sep/entries/turing-machine")
                .read_dir()
                .unwrap()
                .filter_map(Result::ok)
                .any(|e| e.file_name().to_string_lossy().ends_with(".tmp")),
            "the temporary file is renamed, not left behind"
        );
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
        let dir = &temp.path().join("sep");
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

        let library = Library::new(temp.path());
        library.init().unwrap();
        let library = library.store(&Sep);
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

        assert_eq!(
            library.copies_of(&format!("{TM}S1.svg")),
            [
                dir.join("entries/turing-machine/S1.svg"),
                dir.join(".derived/entries/turing-machine/S1.svg.png")
            ]
        );
        assert!(library.copies_of(&format!("{TM}?x=1")).is_empty());

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
            library.work_record(TM).unwrap(),
            Some(WorkRecord {
                title: Some("Turing Machines".to_owned()),
                revision: "Wed May 21, 2025".to_owned(),
                previous_revision: None
            })
        );
        assert_eq!(
            library
                .work_record(&format!("{HOST}/entries/unparsed/"))
                .unwrap(),
            None,
            "a page that was never parsed has no revision"
        );
        assert_eq!(
            library
                .work_record(&format!("{HOST}/entries/nowhere/"))
                .unwrap(),
            None
        );

        let listed = list_entries(&library);
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
    fn a_corrupt_meta_json_reads_as_absent() {
        let temp = tempfile::tempdir().unwrap();
        let dir = &temp.path().join("sep");
        seed(dir, "entries/x", "{ not json", &[("index.html", "<html/>")]);
        let library = Library::new(temp.path());
        let library = library.store(&Sep);
        assert_eq!(library.unit_meta("entries/x"), None);
        assert_eq!(
            library
                .read_body(&format!("{HOST}/entries/x/"))
                .unwrap()
                .as_deref(),
            Some("<html/>"),
            "the file is still readable"
        );
        assert!(list_entries(&library).is_empty());

        library
            .write(&format!("{HOST}/entries/x/"), b"<html/>", None, None)
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("entries/x/meta.json.corrupt")).unwrap(),
            "{ not json",
            "a write moves the unreadable record aside rather than lose it"
        );
        assert!(library.unit_meta("entries/x").is_some());
    }
}
