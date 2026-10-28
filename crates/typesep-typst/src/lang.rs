//! Language tags as a document names them (`de`, `en-GB`, `sr-Latn-RS`), read into what Typst
//! and XHTML take, so that no tag reaches either unless it is one.

/// `tag` as a BCP 47 language tag, `-` between its subtags: a primary language of two or
/// three letters and subtags of one to eight letters and digits; or none when it is not one.
pub fn bcp47(tag: &str) -> Option<String> {
    let tag = tag.trim().replace('_', "-");
    let mut subtags = tag.split('-');
    let primary = subtags.next()?;
    if !(2..=3).contains(&primary.len()) || !primary.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    if !subtags
        .all(|sub| (1..=8).contains(&sub.len()) && sub.chars().all(|c| c.is_ascii_alphanumeric()))
    {
        return None;
    }
    Some(tag)
}

/// What Typst's `text(lang:, region:)` take for a tag: its language, lower case, and its
/// region when it names one, upper case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypstLang {
    pub lang: String,
    pub region: Option<String>,
}

/// The language and region of `tag` for Typst, or none when `tag` is not a language tag. A
/// script subtag (`Latn`) and those after the region are passed over: Typst has no use for
/// them.
pub fn typst_lang(tag: &str) -> Option<TypstLang> {
    let tag = bcp47(tag)?;
    let mut subtags = tag.split('-');
    let lang = subtags.next()?.to_ascii_lowercase();
    let mut region = None;
    for sub in subtags {
        match sub.len() {
            4 if sub.chars().all(|c| c.is_ascii_alphabetic()) => continue,
            2 if sub.chars().all(|c| c.is_ascii_alphabetic()) => {
                region = Some(sub.to_ascii_uppercase());
                break;
            }
            _ => break,
        }
    }
    Some(TypstLang { lang, region })
}

/// Whether Typst has hyphenation patterns for `lang`, a code `typst_lang` gave: it
/// hyphenates with those of the hypher crate, which knows languages by their two-letter code.
pub fn has_hyphenation(lang: &str) -> bool {
    let Ok(code) = <[u8; 2]>::try_from(lang.as_bytes()) else {
        return false;
    };
    hypher::Lang::from_iso(code).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_is_read_only_when_it_is_one() {
        assert_eq!(bcp47("de").as_deref(), Some("de"));
        assert_eq!(bcp47(" en_GB ").as_deref(), Some("en-GB"));
        assert_eq!(bcp47("sr-Latn-RS").as_deref(), Some("sr-Latn-RS"));
        for hostile in [
            "", "e", "english", "de\"", "de)", "x-\"", "de-", "ar rtl", "#set", "de-]", "ä",
        ] {
            assert_eq!(bcp47(hostile), None, "{hostile:?}");
        }
    }

    #[test]
    fn typst_takes_the_language_and_the_region() {
        let lang = |tag: &str| typst_lang(tag).map(|l| (l.lang, l.region));
        assert_eq!(lang("DE"), Some(("de".into(), None)));
        assert_eq!(lang("de-at"), Some(("de".into(), Some("AT".into()))));
        assert_eq!(lang("sr-Latn-RS"), Some(("sr".into(), Some("RS".into()))));
        assert_eq!(
            lang("es-419"),
            Some(("es".into(), None)),
            "Typst has no numeric regions"
        );
        assert_eq!(lang("ar\" + rtl"), None);
    }

    #[test]
    fn hyphenation_patterns_are_those_of_hypher() {
        for lang in ["en", "de", "it", "hi"] {
            assert!(has_hyphenation(lang), "{lang}");
        }
        for lang in ["ar", "he", "zh", "ja", "fas", ""] {
            assert!(!has_hyphenation(lang), "{lang}");
        }
    }
}
