use std::collections::HashMap;
use std::path::{Path, PathBuf};

use typst::Feature;
use typst::Features;
use typst::Library;
use typst::LibraryExt;
use typst::World;
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;

use crate::fonts::Fonts;

pub const MAIN_PATH: &str = "/main.typ";
pub const MITEX_SCOPE_PATH: &str = "/mitex/specs/mod.typ";
pub const FIGURES_PREFIX: &str = "/figures";

pub const MITEX_FILES: [(&str, &str); 3] = [
    (
        "/mitex/specs/mod.typ",
        include_str!("../mitex/specs/mod.typ"),
    ),
    (
        "/mitex/specs/prelude.typ",
        include_str!("../mitex/specs/prelude.typ"),
    ),
    (
        "/mitex/specs/latex/standard.typ",
        include_str!("../mitex/specs/latex/standard.typ"),
    ),
];

pub fn file_id(path: &str) -> FileId {
    RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(path).expect("a virtual path the emitter chose"),
    )
    .intern()
}

pub struct TypesetWorld<'a> {
    library: LazyHash<Library>,
    fonts: &'a Fonts,
    main: FileId,
    sources: HashMap<FileId, Source>,
    files: HashMap<FileId, PathBuf>,
}

impl<'a> TypesetWorld<'a> {
    pub fn new(main: String, figures: &[(String, PathBuf)], fonts: &'a Fonts) -> Self {
        let main_id = file_id(MAIN_PATH);
        let mut sources = HashMap::new();
        sources.insert(main_id, Source::new(main_id, main));
        for (path, text) in MITEX_FILES {
            let id = file_id(path);
            sources.insert(id, Source::new(id, text.to_owned()));
        }
        let files = figures
            .iter()
            .map(|(path, file)| (file_id(path), file.clone()))
            .collect();
        TypesetWorld {
            library: LazyHash::new(Library::builder().build()),
            fonts,
            main: main_id,
            sources,
            files,
        }
    }

    pub fn for_html(mut self) -> Self {
        self.library = LazyHash::new(
            Library::builder()
                .with_features(Features::from_iter([Feature::Html]))
                .build(),
        );
        self
    }

    pub fn main_source(&self) -> &Source {
        &self.sources[&self.main]
    }
}

fn not_found(id: FileId) -> FileError {
    FileError::NotFound(PathBuf::from(id.get().vpath().get_without_slash()))
}

impl World for TypesetWorld<'_> {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.fonts.book()
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.sources.get(&id).cloned().ok_or_else(|| not_found(id))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let Some(source) = self.sources.get(&id) {
            return Ok(Bytes::from_string(source.text().to_owned()));
        }
        let path: &Path = self.files.get(&id).ok_or_else(|| not_found(id))?;
        std::fs::read(path)
            .map(Bytes::new)
            .map_err(|err| FileError::from_io(err, path))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        let mut now = time::OffsetDateTime::now_utc();
        if let Some(offset) = offset {
            let seconds = (offset.hours() * 3600.0).round() as i32;
            now = now.to_offset(time::UtcOffset::from_whole_seconds(seconds).ok()?);
        }
        Datetime::from_ymd(now.year(), now.month() as u8, now.day())
    }
}
