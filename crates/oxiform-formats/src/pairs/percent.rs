//! Percent-decoding of the names and values of a query string or a form.

use std::borrow::Cow;

/// Why a component could not be decoded.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Fault {
    /// A `%` that is not followed by two hexadecimal digits, at this offset of the component.
    Escape { at: usize, text: String },
    /// The decoded bytes are not UTF-8.
    Utf8 { valid_up_to: usize },
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// UTF-8 text, borrowed when there is nothing to decode.
pub(super) fn utf8(raw: &[u8]) -> Result<Cow<'_, str>, Fault> {
    std::str::from_utf8(raw)
        .map(Cow::Borrowed)
        .map_err(|error| Fault::Utf8 {
            valid_up_to: error.valid_up_to(),
        })
}

/// A name or a value of a query string or a form: `%XX` is a byte and `+` is a space (`%2B` is a
/// plus sign), and the bytes must be UTF-8, strictly.
pub(super) fn decode(raw: &[u8]) -> Result<Cow<'_, str>, Fault> {
    if !raw.iter().any(|byte| matches!(byte, b'%' | b'+')) {
        return utf8(raw);
    }
    let mut bytes = Vec::with_capacity(raw.len());
    let mut position = 0;
    while let Some(&byte) = raw.get(position) {
        match byte {
            b'+' => {
                bytes.push(b' ');
                position += 1;
            }
            b'%' => {
                let high = raw.get(position + 1).copied().and_then(hex);
                let low = raw.get(position + 2).copied().and_then(hex);
                let (Some(high), Some(low)) = (high, low) else {
                    let end = raw.len().min(position + 3);
                    return Err(Fault::Escape {
                        at: position,
                        text: String::from_utf8_lossy(&raw[position..end]).into_owned(),
                    });
                };
                bytes.push(high << 4 | low);
                position += 3;
            }
            other => {
                bytes.push(other);
                position += 1;
            }
        }
    }
    String::from_utf8(bytes)
        .map(Cow::Owned)
        .map_err(|error| Fault::Utf8 {
            valid_up_to: error.utf8_error().valid_up_to(),
        })
}
