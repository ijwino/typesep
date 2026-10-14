//! Percent-escapes in addresses: read back as the text they were written from, and shown as a
//! browser shows an address.

/// The byte the escape `%XX` at `at` stands for, if one is there.
fn escaped_byte(bytes: &[u8], at: usize) -> Option<u8> {
    let hex = |at: usize| bytes.get(at).and_then(|b| (*b as char).to_digit(16));
    match (bytes.get(at), hex(at + 1), hex(at + 2)) {
        (Some(b'%'), Some(high), Some(low)) => Some((high * 16 + low) as u8),
        _ => None,
    }
}

/// `text` with every escape as the byte it stands for, the bytes read as UTF-8, any that are
/// not as the replacement character.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match escaped_byte(bytes, i) {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Characters an address keeps escaped when it is shown, as browsers keep them: the controls,
/// the spaces of every width, the marks that steer the direction of text, and the format
/// characters that show nothing, so what is shown is what is there and reads in the order it
/// is written. The joiners stay out: Indic and Persian words are written with them.
fn shown_escaped(c: char) -> bool {
    c.is_control()
        || c.is_whitespace()
        || matches!(
            c,
            '\u{ad}'
                | '\u{61c}'
                | '\u{115f}'
                | '\u{1160}'
                | '\u{180e}'
                | '\u{200b}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{3164}'
                | '\u{feff}'
                | '\u{ffa0}'
                | '\u{fff9}'..='\u{fffb}'
        )
}

/// `url` as a browser shows it, as an IRI: each run of escapes that spells characters beyond
/// ASCII written as those characters. An escaped ASCII character (`%20`, `%2F`, `%25`), a byte
/// that is no part of a character, and a character [`shown_escaped`] keeps stay as they are
/// written, so the address shown is the address. For display only; a link goes to `url`.
pub fn readable(url: &str) -> String {
    let bytes = url.as_bytes();
    let mut out = String::with_capacity(url.len());
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let mut run = Vec::new();
        while let Some(byte) = escaped_byte(bytes, i) {
            run.push(byte);
            i += 3;
        }
        if run.is_empty() {
            let c = url[i..].chars().next().expect("a character");
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        // Each byte of the run is three characters of the address, `%XX`.
        let written = |from: usize, len: usize| &url[start + 3 * from..start + 3 * (from + len)];
        let mut at = 0;
        for chunk in run.utf8_chunks() {
            for c in chunk.valid().chars() {
                let len = c.len_utf8();
                if c.is_ascii() || shown_escaped(c) {
                    out.push_str(written(at, len));
                } else {
                    out.push(c);
                }
                at += len;
            }
            out.push_str(written(at, chunk.invalid().len()));
            at += chunk.invalid().len();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding_reads_every_escape() {
        assert_eq!(percent_decode("%C3%9Cber_Gewi%C3%9Fheit"), "Über_Gewißheit");
        assert_eq!(percent_decode("a%20b%2Fc%"), "a b/c%");
        assert_eq!(percent_decode("%FFx"), "\u{fffd}x");
    }

    #[test]
    fn an_address_shows_its_characters_beyond_ascii() {
        assert_eq!(
            readable(
                "https://www.wittgensteinproject.org/w/index.php/%D9%85%D8%AD%D8%A7%D8%B6%D8%B1%D8%A9_%D9%81%D9%8A_%D8%B9%D9%84%D9%85_%D8%A7%D9%84%D8%A3%D8%AE%D9%84%D8%A7%D9%82"
            ),
            "https://www.wittgensteinproject.org/w/index.php/محاضرة_في_علم_الأخلاق"
        );
        assert_eq!(
            readable(
                "https://www.wittgensteinproject.org/w/index.php/%E0%A4%9F%E0%A5%8D%E0%A4%B0%E0%A5%88%E0%A4%95%E0%A5%8D%E0%A4%9F%E0%A5%87%E0%A4%9F%E0%A4%B8_%E0%A4%B2%E0%A5%89%E0%A4%9C%E0%A4%BF%E0%A4%95%E0%A5%8B-%E0%A4%AB%E0%A4%BC%E0%A4%BF%E0%A4%B2%E0%A5%8B%E0%A4%B8%E0%A5%89%E0%A4%AB%E0%A4%BC%E0%A4%BF%E0%A4%95%E0%A4%B8"
            ),
            "https://www.wittgensteinproject.org/w/index.php/ट्रैक्टेटस_लॉजिको-फ़िलोसॉफ़िकस"
        );
        assert_eq!(
            readable("https://x.test/%C3%9Cber_Gewi%c3%9fheit_(%E2%80%9CEin%E2%80%9D)"),
            "https://x.test/Über_Gewißheit_(“Ein”)"
        );
    }

    #[test]
    fn an_address_keeps_what_would_change_it_escaped() {
        let plain = "https://plato.stanford.edu/entries/turing-machine/?q=a&b=c#s1";
        assert_eq!(readable(plain), plain);
        assert_eq!(
            readable("https://x.test/a%20b%2Fc%3Fd%25e%23f"),
            "https://x.test/a%20b%2Fc%3Fd%25e%23f"
        );
        // ASCII escapes inside a run part the characters around them.
        assert_eq!(readable("/%D9%85%2F%D9%85%20"), "/م%2Fم%20");
        // A lone lead byte, a lone continuation byte, a cut sequence, an overlong one.
        assert_eq!(readable("/%D9x%85%E0%A4"), "/%D9x%85%E0%A4");
        assert_eq!(readable("/%C0%AF%E2%88%A7"), "/%C0%AF∧");
        assert_eq!(readable("/%D9%8"), "/%D9%8");
        assert_eq!(readable("/%"), "/%");
    }

    #[test]
    fn an_address_keeps_invisible_and_steering_characters_escaped() {
        // Right-to-left override, no-break space, zero-width space, byte order mark, a C1
        // control: each kept, the letters around them shown.
        assert_eq!(
            readable("/%D9%85%E2%80%AEx%C2%A0%E2%80%8B%EF%BB%BF%C2%85"),
            "/م%E2%80%AEx%C2%A0%E2%80%8B%EF%BB%BF%C2%85"
        );
        // The joiners a Persian or Indic word is written with are shown.
        assert_eq!(
            readable("/%D9%85%E2%80%8C%D9%85%E2%80%8D"),
            "/م\u{200c}م\u{200d}"
        );
    }
}
