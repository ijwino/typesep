use crate::fetch::{FetchError, FetchOptions, FetchResult, Fetcher};
use crate::library::{FileMeta, LibraryError, PlaceError, Store};

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error(transparent)]
    Place(#[from] PlaceError),
    #[error(transparent)]
    Fetch(#[from] FetchError),
    #[error("{url} is not in the library")]
    NotHeld { url: String },
    /// What the library lacks, with the command that fetches it, as its source puts it.
    #[error("{url} is not in the library at {dir}\n  fetch it first: {fetch}")]
    NotInLibrary {
        url: String,
        dir: String,
        fetch: String,
    },
    #[error("HTTP 404 for {url} (remembered{when}; --refresh asks the server again)")]
    Remembered { url: String, when: String },
    #[error("server answered 304 for {url} but the library has no copy")]
    NotModifiedWithoutCopy { url: String },
}

pub struct LoadedPage {
    pub text: String,
    pub meta: Option<FileMeta>,
    pub from_cache: bool,
    pub revalidated: bool,
}

/// Where a source's loader gets its files: its store in the library, and the network when the
/// command may fetch.
pub struct Net<'a> {
    pub store: Store<'a>,
    pub fetcher: Option<&'a mut Fetcher>,
    pub refresh: bool,
    pub check_updates: bool,
}

impl<'a> Net<'a> {
    pub fn offline(store: Store<'a>) -> Self {
        Self {
            store,
            fetcher: None,
            refresh: false,
            check_updates: false,
        }
    }

    pub fn fetching(store: Store<'a>, fetcher: &'a mut Fetcher) -> Self {
        Self {
            store,
            fetcher: Some(fetcher),
            refresh: false,
            check_updates: false,
        }
    }

    pub fn with_refresh(mut self, refresh: bool) -> Self {
        self.refresh = refresh;
        self
    }

    pub fn with_check_updates(mut self, check: bool) -> Self {
        self.check_updates = check;
        self
    }

    pub fn writes(&self) -> bool {
        self.fetcher.is_some()
    }

    pub fn page(&mut self, url: &str, refresh: bool) -> Result<LoadedPage, LoadError> {
        let meta = self.store.read_meta(url)?;
        let mut cached = self.store.read_body(url)?;

        if !refresh && let Some(text) = cached.take() {
            return Ok(LoadedPage {
                text,
                meta,
                from_cache: true,
                revalidated: false,
            });
        }
        if cached.is_none() && meta.as_ref().is_some_and(|m| m.missing) && !refresh {
            let when = meta
                .as_ref()
                .and_then(|m| m.fetched_at.as_deref())
                .map(|stamp| format!(" on {}", stamp.chars().take(10).collect::<String>()))
                .unwrap_or_default();
            return Err(LoadError::Remembered {
                url: url.to_owned(),
                when,
            });
        }

        let Some(fetcher) = self.fetcher.as_deref_mut() else {
            return Err(LoadError::NotHeld {
                url: url.to_owned(),
            });
        };
        let conditional = if refresh {
            FetchOptions {
                etag: meta.as_ref().and_then(|m| m.etag.as_deref()),
                last_modified: meta.as_ref().and_then(|m| m.last_modified.as_deref()),
            }
        } else {
            FetchOptions::default()
        };
        let answer = fetcher.get(url, conditional);
        let fetched = match answer {
            Ok(result) => result,
            Err(err) => {
                if err.is_gone() {
                    self.store.mark_missing(url)?;
                }
                return Err(err.into());
            }
        };

        match fetched {
            FetchResult::NotModified => match cached {
                Some(text) => Ok(LoadedPage {
                    text,
                    meta,
                    from_cache: true,
                    revalidated: true,
                }),
                None => Err(LoadError::NotModifiedWithoutCopy {
                    url: url.to_owned(),
                }),
            },
            FetchResult::Fetched(res) => {
                self.store.write(
                    url,
                    &res.body,
                    res.etag.as_deref(),
                    res.last_modified.as_deref(),
                )?;
                Ok(LoadedPage {
                    text: String::from_utf8_lossy(&res.body).into_owned(),
                    meta: self.store.read_meta(url)?,
                    from_cache: false,
                    revalidated: false,
                })
            }
        }
    }
}
