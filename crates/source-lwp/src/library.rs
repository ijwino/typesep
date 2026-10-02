use std::fs;

use typesep_net::library::Store;
use typesep_net::source::Held;

use crate::address::{PAGES_DIR, PARSE_FILE, title_of_unit};

/// The texts the library holds, by title: those a load has recorded, whose API answer is
/// there.
pub fn list_texts(store: &Store<'_>) -> Vec<Held> {
    let Ok(dir) = fs::read_dir(store.dir().join(PAGES_DIR)) else {
        return Vec::new();
    };
    let mut out: Vec<Held> = dir
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let meta = store.unit_meta(&format!("{PAGES_DIR}/{name}"))?;
            let revision = meta.revision?;
            let file = meta.files.get(PARSE_FILE).filter(|file| !file.missing)?;
            Some(Held {
                id: title_of_unit(&name),
                edition: String::new(),
                url: meta.url,
                revision,
                title: meta.title.unwrap_or_default(),
                fetched_at: file.fetched_at.clone(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}
