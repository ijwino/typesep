//! Percent-escapes in addresses, read back as the text they were written from.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding_reads_every_escape() {
        assert_eq!(percent_decode("%C3%9Cber_Gewi%C3%9Fheit"), "Über_Gewißheit");
        assert_eq!(percent_decode("a%20b%2Fc%"), "a b/c%");
        assert_eq!(percent_decode("%FFx"), "\u{fffd}x");
    }
}
