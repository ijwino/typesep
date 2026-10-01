use typesep_model::Doc;
use typesep_model::walk::figure_urls;

use crate::fetch::{FetchOptions, FetchResult};
use crate::net::Net;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FigureFetch {
    pub present: Vec<String>,
    pub fetched: usize,
    pub missing: Vec<String>,
    pub failed: Vec<(String, String)>,
}

pub fn fetch_figures(doc: &Doc, net: &mut Net<'_>, log: &mut dyn FnMut(&str)) -> FigureFetch {
    let mut result = FigureFetch::default();
    for url in figure_urls(doc) {
        match net.library.has(&url) {
            Ok(true) => {
                result.present.push(url);
                continue;
            }
            Ok(false) => {}
            Err(err) => {
                log(&format!("skipping image {url}: {err}"));
                result.failed.push((url, err.to_string()));
                continue;
            }
        }
        if !net.refresh && net.library.is_missing(&url).unwrap_or(false) {
            log(&format!(
                "skipping image {url}: the server had no such file last time (--refresh asks again)"
            ));
            result.missing.push(url);
            continue;
        }
        let Some(fetcher) = net.fetcher.as_deref_mut() else {
            let reason = format!("{url} is not in the library");
            log(&format!("skipping image {url}: {reason}"));
            result.failed.push((url, reason));
            continue;
        };
        match fetcher.get(&url, FetchOptions::default()) {
            Ok(FetchResult::NotModified) => {}
            Ok(FetchResult::Fetched(res)) => {
                if let Err(err) = net.library.write(
                    &url,
                    &res.body,
                    res.etag.as_deref(),
                    res.last_modified.as_deref(),
                ) {
                    log(&format!("skipping image {url}: {err}"));
                    result.failed.push((url, err.to_string()));
                    continue;
                }
                result.present.push(url);
                result.fetched += 1;
            }
            Err(err) => {
                let reason = err.to_string();
                log(&format!("skipping image {url}: {reason}"));
                if err.is_gone() {
                    let _ = net.library.mark_missing(&url);
                    result.missing.push(url);
                } else {
                    result.failed.push((url, reason));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use typesep_model::{Block, Figure};

    fn figure(src: &str) -> Figure {
        Figure {
            src: src.to_owned(),
            alt: String::new(),
            caption: None,
            id: None,
            inline: None,
        }
    }

    #[test]
    fn without_a_fetcher_every_figure_is_counted_somewhere() {
        let temp = tempfile::tempdir().unwrap();
        let library = crate::library::Library::new(temp.path());
        library.init().unwrap();
        let tm = "https://plato.stanford.edu/entries/turing-machine/";
        let (held, gone, absent) = (
            format!("{tm}held.png"),
            format!("{tm}gone.png"),
            format!("{tm}absent.png"),
        );
        library.write(&held, b"\x89PNG", None, None).unwrap();
        library.mark_missing(&gone).unwrap();
        let mut doc = Doc::default();
        for src in [&held, &gone, &absent] {
            doc.body.push(Block::Figure(figure(src)));
        }

        let pass = fetch_figures(&doc, &mut Net::offline(&library), &mut |_| {});
        assert_eq!(pass.present, [held]);
        assert_eq!(pass.missing, [gone]);
        assert_eq!(
            pass.failed,
            [(absent.clone(), format!("{absent} is not in the library"))]
        );
        assert_eq!(pass.fetched, 0);
    }
}
