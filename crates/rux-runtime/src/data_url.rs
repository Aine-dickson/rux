//! `data:` URLs: an image written into the document itself,
//! `src="data:image/png;base64,iVBOR…"` or `url("data:image/svg+xml,%3Csvg…")`.
//!
//! RFC 2397: `data:[<media type>][;base64],<data>`. The media type is not
//! needed, since the picture's own bytes say what it is; the data is base64
//! when `;base64` says so and percent-encoded otherwise.

/// The bytes a `data:` URL holds, or `None` when `src` is not one.
pub fn bytes(src: &str) -> Option<Result<Vec<u8>, String>> {
    let rest = src.get(..5).filter(|s| s.eq_ignore_ascii_case("data:")).map(|_| &src[5..])?;
    let Some((head, data)) = rest.split_once(',') else {
        return Some(Err("a `data:` URL has a comma before its data, and this one has none".to_string()));
    };
    let base64 = head.rsplit(';').next().is_some_and(|p| p.trim().eq_ignore_ascii_case("base64"));
    Some(if base64 { decode_base64(&percent_decode(data)) } else { Ok(percent_decode(data)) })
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (b, ..) => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// Base64, standard or URL-safe, padded or not, with white space ignored, as
/// a browser reads it.
fn decode_base64(text: &[u8]) -> Result<Vec<u8>, String> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in text {
        if c.is_ascii_whitespace() || c == b'=' {
            continue;
        }
        let v = value(c).ok_or_else(|| format!("`{}` is not base64", c as char))?;
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_and_percent_encoded_data_are_read() {
        assert_eq!(bytes("data:image/png;base64,aGVsbG8="), Some(Ok(b"hello".to_vec())));
        assert_eq!(bytes("DATA:;base64,aGVsbG8"), Some(Ok(b"hello".to_vec())));
        assert_eq!(bytes("data:image/svg+xml,%3Csvg%3E"), Some(Ok(b"<svg>".to_vec())));
        assert_eq!(bytes("data:text/plain;charset=utf-8,a%20b"), Some(Ok(b"a b".to_vec())));
        assert!(matches!(bytes("data:image/png;base64,***"), Some(Err(_))));
        assert!(matches!(bytes("data:image/png;base64"), Some(Err(_))));
        assert_eq!(bytes("assets/a.png"), None);
    }
}
