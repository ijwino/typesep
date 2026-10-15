//! Where the Wittgenstein Project keeps its texts: the addresses of a text's page and of the
//! API that renders it, the titles the user names them by, and the places in the library
//! they are kept at.

use typesep_model::url::percent_decode;
use typesep_net::library::{FIT_MARK, MAX_NAME, Place, PlaceError, fit_name, is_safe_segment};
use url::Url;

/// The site, as every address the library keeps is written.
pub const SITE: &str = "https://www.wittgensteinproject.org";
const HOSTS: [&str; 2] = ["www.wittgensteinproject.org", "wittgensteinproject.org"];
const PAGES: &str = "/w/index.php";
const API: &str = "/w/api.php";
const IMAGES: &str = "/w/images/";
/// What the API is asked for a text: its rendered HTML, its revision, its title as the page
/// shows it and its page properties, after any redirect.
const PARSE_QUERY: &str = "action=parse&prop=text%7Crevid%7Cdisplaytitle%7Cproperties&redirects=1&formatversion=2&format=json";
/// The file a text's API answer is kept in, in its unit.
pub const PARSE_FILE: &str = "parse.json";
/// The units of the texts, `pages/<title>`, and of the images, `images/<a>/<ab>`.
pub const PAGES_DIR: &str = "pages";
const IMAGES_DIR: &str = "images";

/// What the titles of the project's tree views say, in the languages it has them in: views
/// it derives from a text, which typesep does not read.
const TREE_VIEWS: [&str; 6] = [
    "tree-like view",
    "baumstruktur",
    "arborescente",
    "ad albero",
    "en árbol",
    "em árvore",
];

/// Characters MediaWiki leaves as they are when it writes a title into a URL.
fn kept_in_url(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-_.~;:@$!*(),/".contains(c)
}

fn percent_encode(text: &str, keep: impl Fn(char) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if keep(c) {
            out.push(c);
        } else {
            let mut bytes = [0u8; 4];
            for byte in c.encode_utf8(&mut bytes).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TitleError {
    Empty,
    Invalid { title: String, character: char },
    TreeView(String),
    NotAText(String),
}

impl std::fmt::Display for TitleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TitleError::Empty => write!(f, "no Wittgenstein Project title given"),
            TitleError::Invalid { title, character } => write!(
                f,
                "\"{title}\" cannot be a Wittgenstein Project title: it holds \"{character}\""
            ),
            TitleError::TreeView(title) => write!(
                f,
                "\"{title}\" is a tree view the Wittgenstein Project derives from a text; name the text itself"
            ),
            TitleError::NotAText(url) => write!(f, "not a Wittgenstein Project text URL: {url}"),
        }
    }
}

/// A title as MediaWiki keys it: spaces as underscores, none at either end or twice, and the
/// first letter in upper case.
pub fn normalize_title(input: &str) -> Result<String, TitleError> {
    let mut words: Vec<&str> = input
        .split(|c: char| c == '_' || c.is_whitespace())
        .filter(|word| !word.is_empty())
        .collect();
    if words.is_empty() {
        return Err(TitleError::Empty);
    }
    if let Some(character) = words
        .iter()
        .flat_map(|word| word.chars())
        .find(|c| "#<>[]|{}".contains(*c) || c.is_control())
    {
        return Err(TitleError::Invalid {
            title: input.trim().to_owned(),
            character,
        });
    }
    let first = words.remove(0);
    let mut chars = first.chars();
    let head: String = chars
        .next()
        .map(char::to_uppercase)
        .into_iter()
        .flatten()
        .collect();
    let mut title = head + chars.as_str();
    for word in words {
        title.push('_');
        title.push_str(word);
    }
    let shown = title.replace('_', " ").to_lowercase();
    if TREE_VIEWS.iter().any(|tree| shown.contains(tree)) {
        return Err(TitleError::TreeView(title.replace('_', " ")));
    }
    Ok(title)
}

fn site_url(input: &str) -> Option<Url> {
    let url = Url::parse(input.trim()).ok()?;
    let ours = matches!(url.scheme(), "http" | "https")
        && url
            .host_str()
            .is_some_and(|host| HOSTS.iter().any(|ours| host.eq_ignore_ascii_case(ours)));
    ours.then_some(url)
}

/// Whether `input` is an address on the project's site.
pub fn is_site_url(input: &str) -> bool {
    let lower = input.trim().to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")) && site_url(input).is_some()
}

fn query_value(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.is_empty())
}

/// The title of the text `url` is the page of, `/w/index.php/<title>` or
/// `/w/index.php?title=<title>`, without normalizing it.
fn title_in(url: &Url) -> Option<String> {
    let path = url.path();
    if let Some(rest) = path.strip_prefix(PAGES).and_then(|p| p.strip_prefix('/'))
        && !rest.is_empty()
    {
        return Some(percent_decode(rest));
    }
    if path == PAGES || path == API {
        return query_value(url, if path == API { "page" } else { "title" });
    }
    None
}

/// The title `input` names: a page address on the site, or a title as it is.
pub fn title_of(input: &str) -> Result<String, TitleError> {
    if is_site_url(input) {
        let url = site_url(input).expect("a site URL");
        let title = title_in(&url).ok_or_else(|| TitleError::NotAText(input.trim().to_owned()))?;
        return normalize_title(&title);
    }
    normalize_title(input)
}

/// The address of the page of the text titled `title`, as the site writes it.
pub fn page_url(title: &str) -> String {
    format!("{SITE}{PAGES}/{}", percent_encode(title, kept_in_url))
}

/// The address the API answers with the text titled `title`, rendered.
pub fn api_url(title: &str) -> String {
    format!(
        "{SITE}{API}?{PARSE_QUERY}&page={}",
        percent_encode(title, kept_in_url)
    )
}

/// The full-size image a MediaWiki thumbnail address is made from:
/// `/w/images/thumb/b/b5/X.png/120px-X.png` is `/w/images/b/b5/X.png`. Any other address is
/// as it is, made absolute on the site.
pub fn full_size(src: &str) -> String {
    let Some(mut url) = site_url(src) else {
        return src.to_owned();
    };
    let path = url.path().to_owned();
    if let Some(rest) = path.strip_prefix("/w/images/thumb/") {
        let segments: Vec<&str> = rest.split('/').collect();
        if segments.len() >= 4 {
            url.set_path(&format!("{IMAGES}{}", segments[..3].join("/")));
        }
    }
    let _ = url.set_host(Some(HOSTS[0]));
    let _ = url.set_scheme("https");
    url.to_string()
}

/// A title written so it can name a directory, and be read back: letters, digits, `-`, `_`,
/// `(`, `)` and `,` as they are, every other character as `%` and the hex of its bytes. A
/// title that would be written longer than a name can be, as one in a script beyond Latin
/// soon is, is shortened as `fit_name` shortens it, and cannot be read back.
pub fn unit_name(title: &str) -> String {
    let name = percent_encode(title, |c| c.is_ascii_alphanumeric() || "-_(),".contains(c));
    fit_name(&name, MAX_NAME).into_owned()
}

/// The title a unit's name was written from; none for a name shortened to fit, whose title
/// is the one its `meta.json` records the address of.
pub fn title_of_unit(name: &str) -> Option<String> {
    (!name.contains(FIT_MARK)).then(|| percent_decode(name))
}

/// Where the library keeps what `url` names: a text's API answer, whichever address of the
/// text names it, at `pages/<title>/parse.json`, and an image at `images/<a>/<ab>/<file>`, its
/// path on the site.
pub fn place_for(url: &str) -> Result<Place, PlaceError> {
    let parsed = Url::parse(url).map_err(|_| PlaceError::NotAUrl(url.to_owned()))?;
    if site_url(url).is_none() {
        return Err(PlaceError::OtherSite {
            url: url.to_owned(),
            site: SITE,
        });
    }
    if let Some(rest) = parsed.path().strip_prefix(IMAGES) {
        let segments: Vec<&str> = rest.split('/').collect();
        if let Some(bad) = segments.iter().find(|segment| !is_safe_segment(segment)) {
            return Err(PlaceError::UnsafeSegment {
                url: url.to_owned(),
                segment: (*bad).to_owned(),
            });
        }
        let (file, dirs) = segments.split_last().expect("a split has a part");
        if *file == typesep_net::library::META {
            return Err(PlaceError::MetaName(url.to_owned()));
        }
        let file = fit_name(file, MAX_NAME);
        let unit = std::iter::once(IMAGES_DIR)
            .chain(dirs.iter().copied())
            .collect::<Vec<_>>()
            .join("/");
        return Ok(Place {
            unit_url: format!(
                "{SITE}{IMAGES}{}",
                dirs.iter().map(|d| format!("{d}/")).collect::<String>()
            ),
            unit,
            file: file.into_owned(),
        });
    }
    let title = title_in(&parsed)
        .ok_or_else(|| PlaceError::Unkept {
            url: url.to_owned(),
            kept: "the pages of texts and the images",
        })
        .and_then(|title| {
            normalize_title(&title).map_err(|_| PlaceError::UnsafeSegment {
                url: url.to_owned(),
                segment: title,
            })
        })?;
    Ok(Place {
        unit: format!("{PAGES_DIR}/{}", unit_name(&title)),
        file: PARSE_FILE.to_owned(),
        unit_url: page_url(&title),
    })
}

/// `text` as a shell reads it back: quoted when it holds anything but letters, digits and
/// `-_./:`.
pub fn shell_word(text: &str) -> String {
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:".contains(c))
    {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "Logisch-philosophische_Abhandlung";

    #[test]
    fn a_title_is_keyed_as_mediawiki_keys_it() {
        assert_eq!(
            normalize_title("Logisch-philosophische Abhandlung").unwrap(),
            TEXT
        );
        assert_eq!(
            normalize_title("  logisch-philosophische__Abhandlung ").unwrap(),
            TEXT
        );
        assert_eq!(normalize_title("über Gewißheit").unwrap(), "Über_Gewißheit");
        assert_eq!(normalize_title(" _ "), Err(TitleError::Empty));
        assert!(matches!(
            normalize_title("A [b]"),
            Err(TitleError::Invalid { character: '[', .. })
        ));
        for tree in [
            "Logisch-philosophische Abhandlung (Darstellung in Baumstruktur)",
            "Tractatus Logico-Philosophicus (static tree-like view)",
            "Tractatus logico-philosophicus (version arborescente interactive)",
            "Tractatus logico-philosophicus (visualizzazione ad albero)",
            "Tratado lógico-filosófico (presentación en árbol)",
            "Tractatus Logico-Philosophicus (visualização em árvore)",
        ] {
            let err = normalize_title(tree).unwrap_err();
            assert!(matches!(err, TitleError::TreeView(_)), "{tree}");
            assert!(err.to_string().ends_with("name the text itself"), "{err}");
        }
    }

    #[test]
    fn a_page_address_names_its_title() {
        for input in [
            "https://www.wittgensteinproject.org/w/index.php/Logisch-philosophische_Abhandlung",
            "http://wittgensteinproject.org/w/index.php/Logisch-philosophische_Abhandlung#2.0121",
            "https://www.wittgensteinproject.org/w/index.php?title=Logisch-philosophische_Abhandlung&action=info",
            "https://WWW.wittgensteinproject.org/w/index.php?title=Logisch-philosophische+Abhandlung",
            "Logisch-philosophische Abhandlung",
        ] {
            assert_eq!(title_of(input).unwrap(), TEXT, "{input}");
        }
        assert_eq!(
            title_of("https://www.wittgensteinproject.org/w/index.php/%C3%9Cber_Gewi%C3%9Fheit")
                .unwrap(),
            "Über_Gewißheit"
        );
        assert_eq!(
            title_of("https://www.wittgensteinproject.org/w/index.php/Main_Page").unwrap(),
            "Main_Page"
        );
        assert_eq!(
            title_of("https://www.wittgensteinproject.org/").unwrap_err(),
            TitleError::NotAText("https://www.wittgensteinproject.org/".into())
        );
        assert!(is_site_url(
            " HTTPS://wittgensteinproject.org/w/index.php/X"
        ));
        assert!(!is_site_url(
            "https://wittgensteinproject.org.example.com/w/index.php/X"
        ));
        assert!(!is_site_url("https://plato.stanford.edu/entries/x/"));
        assert!(!is_site_url("Tractatus"));
    }

    #[test]
    fn a_title_is_written_into_its_addresses_as_the_site_writes_it() {
        assert_eq!(
            page_url("Tractatus_Logico-Philosophicus_(English)"),
            "https://www.wittgensteinproject.org/w/index.php/Tractatus_Logico-Philosophicus_(English)"
        );
        assert_eq!(
            page_url("Über_Gewißheit"),
            "https://www.wittgensteinproject.org/w/index.php/%C3%9Cber_Gewi%C3%9Fheit"
        );
        assert_eq!(
            api_url(TEXT),
            "https://www.wittgensteinproject.org/w/api.php?action=parse&prop=text%7Crevid%7Cdisplaytitle%7Cproperties&redirects=1&formatversion=2&format=json&page=Logisch-philosophische_Abhandlung"
        );
        assert_eq!(
            title_of(&api_url("Über_Gewißheit")).unwrap(),
            "Über_Gewißheit"
        );
    }

    #[test]
    fn a_thumbnail_is_traded_for_the_image_it_was_made_from() {
        assert_eq!(
            full_size(
                "https://www.wittgensteinproject.org/w/images/thumb/b/b5/PU_48-1.png/120px-PU_48-1.png"
            ),
            "https://www.wittgensteinproject.org/w/images/b/b5/PU_48-1.png"
        );
        assert_eq!(
            full_size(
                "https://wittgensteinproject.org/w/images/thumb/e/e7/154%2C25r.png/550px-154%2C25r.png"
            ),
            "https://www.wittgensteinproject.org/w/images/e/e7/154%2C25r.png"
        );
        assert_eq!(
            full_size("https://www.wittgensteinproject.org/w/images/8/80/Zettel_249.gif"),
            "https://www.wittgensteinproject.org/w/images/8/80/Zettel_249.gif"
        );
        assert_eq!(
            full_size("https://example.org/x.png"),
            "https://example.org/x.png"
        );
    }

    #[test]
    fn a_text_and_its_images_have_their_places_in_the_library() {
        let page = Place {
            unit: "pages/Logisch-philosophische_Abhandlung".into(),
            file: "parse.json".into(),
            unit_url: page_url(TEXT),
        };
        assert_eq!(place_for(&page_url(TEXT)).unwrap(), page);
        assert_eq!(
            place_for(&api_url(TEXT)).unwrap(),
            page,
            "the API answer is the text's file"
        );
        assert_eq!(
            place_for("https://www.wittgensteinproject.org/w/index.php?title=Logisch-philosophische%20Abhandlung")
                .unwrap(),
            page
        );
        let place = place_for(&page_url("Bemerkungen_über_Frazers_“The_Golden_Bough”")).unwrap();
        assert_eq!(
            place.unit,
            "pages/Bemerkungen_%C3%BCber_Frazers_%E2%80%9CThe_Golden_Bough%E2%80%9D"
        );
        assert!(place.unit.split('/').all(is_safe_segment));
        assert_eq!(
            title_of_unit(&unit_name("Notes_Dictated_to_G.E._Moore_in_Norway")).unwrap(),
            "Notes_Dictated_to_G.E._Moore_in_Norway",
            "a unit name reads back as its title"
        );
        assert_eq!(unit_name(".hidden%"), "%2Ehidden%25");

        assert_eq!(
            place_for("https://www.wittgensteinproject.org/w/images/e/e7/154%2C25r.png").unwrap(),
            Place {
                unit: "images/e/e7".into(),
                file: "154%2C25r.png".into(),
                unit_url: "https://www.wittgensteinproject.org/w/images/e/e7/".into(),
            }
        );
        assert!(matches!(
            place_for("https://plato.stanford.edu/entries/x/"),
            Err(PlaceError::OtherSite { .. })
        ));
        assert!(matches!(
            place_for("https://www.wittgensteinproject.org/w/skins/x.css"),
            Err(PlaceError::Unkept { .. })
        ));
        assert!(matches!(
            place_for("https://www.wittgensteinproject.org/w/images/a/.x"),
            Err(PlaceError::UnsafeSegment { .. })
        ));
    }

    #[test]
    fn a_title_or_an_image_too_long_to_name_a_file_as_it_is_is_shortened_to_fit() {
        // 75 bytes, written in 219, as the Hindi Tractatus's 86 are in 254: it fits as it is,
        // and reads back.
        let fits = "तार्किक_दर्शन_तार्किक_दर्शन";
        let name = unit_name(fits);
        assert_eq!(name.len(), 219);
        assert_eq!(title_of_unit(&name).as_deref(), Some(fits));

        let longer = format!("{fits}_तार्किक");
        let name = unit_name(&longer);
        assert!(name.len() <= MAX_NAME, "{}", name.len());
        assert!(is_safe_segment(&name), "{name}");
        assert!(name.starts_with(&unit_name(fits)[..200]), "{name}");
        assert_eq!(
            title_of_unit(&name),
            None,
            "a shortened name is not read back"
        );
        assert_ne!(unit_name(&format!("{longer}_2")), name);
        let place = place_for(&page_url(&longer)).unwrap();
        assert_eq!(place.unit, format!("pages/{name}"));
        assert_eq!(
            title_of(&place.unit_url).unwrap(),
            longer,
            "its address names it"
        );

        let image = format!(
            "https://www.wittgensteinproject.org/w/images/e/e7/{}.png",
            "%E0%A4%A4".repeat(40)
        );
        let place = place_for(&image).unwrap();
        assert!(place.file.len() <= MAX_NAME, "{}", place.file);
        assert!(place.file.ends_with(".png") && is_safe_segment(&place.file));
        assert_eq!(
            place_for(&image).unwrap(),
            place,
            "the same place every time"
        );
    }

    #[test]
    fn a_word_is_quoted_for_the_shell_when_it_needs_to_be() {
        assert_eq!(shell_word(TEXT), TEXT);
        assert_eq!(
            shell_word("Tractatus_Logico-Philosophicus_(English)"),
            "'Tractatus_Logico-Philosophicus_(English)'"
        );
        assert_eq!(shell_word("Wittgenstein's"), "'Wittgenstein'\\''s'");
    }
}
