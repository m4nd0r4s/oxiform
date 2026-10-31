//! JSON strings: borrowed from the input when there is no escape, decoded otherwise.

use std::borrow::Cow;

/// A string that could not be read: the offset of the problem and what is wrong.
pub(crate) type StringError = (usize, &'static str);

/// Reads the string whose opening quote is at `quote`. Returns the text and the offset just
/// after the closing quote.
pub(crate) fn read_string(input: &str, quote: usize) -> Result<(Cow<'_, str>, usize), StringError> {
    let bytes = input.as_bytes();
    let start = quote + 1;
    let mut position = start;
    loop {
        position = find_special(bytes, position);
        match bytes.get(position) {
            None => return Err((quote, "unterminated string")),
            Some(b'"') => return Ok((Cow::Borrowed(&input[start..position]), position + 1)),
            Some(b'\\') => break,
            Some(&byte) if byte < 0x20 => {
                return Err((position, "control characters must be escaped in strings"));
            }
            Some(_) => position += 1,
        }
    }
    let mut decoded = String::with_capacity(position - start + 16);
    decoded.push_str(&input[start..position]);
    read_escaped(input, quote, position, decoded)
}

/// Continues a string from its first backslash, decoding escapes into `decoded`.
fn read_escaped(
    input: &str,
    quote: usize,
    mut position: usize,
    mut decoded: String,
) -> Result<(Cow<'_, str>, usize), StringError> {
    let bytes = input.as_bytes();
    let mut run_start = position;
    loop {
        position = find_special(bytes, position);
        match bytes.get(position) {
            None => return Err((quote, "unterminated string")),
            Some(b'"') => {
                decoded.push_str(&input[run_start..position]);
                return Ok((Cow::Owned(decoded), position + 1));
            }
            Some(b'\\') => {
                decoded.push_str(&input[run_start..position]);
                position = decode_escape(bytes, position, &mut decoded)?;
                run_start = position;
            }
            Some(&byte) if byte < 0x20 => {
                return Err((position, "control characters must be escaped in strings"));
            }
            Some(_) => position += 1,
        }
    }
}

/// Decodes the escape at `backslash` and returns the offset after it.
fn decode_escape(
    bytes: &[u8],
    backslash: usize,
    decoded: &mut String,
) -> Result<usize, StringError> {
    let character = match bytes.get(backslash + 1) {
        Some(b'"') => '"',
        Some(b'\\') => '\\',
        Some(b'/') => '/',
        Some(b'b') => '\u{8}',
        Some(b'f') => '\u{c}',
        Some(b'n') => '\n',
        Some(b'r') => '\r',
        Some(b't') => '\t',
        Some(b'u') => return decode_unicode_escape(bytes, backslash, decoded),
        None => return Err((backslash, "unterminated string")),
        Some(_) => return Err((backslash, "invalid escape sequence")),
    };
    decoded.push(character);
    Ok(backslash + 2)
}

/// Decodes `\uXXXX`, joining a UTF-16 surrogate pair written as two escapes.
fn decode_unicode_escape(
    bytes: &[u8],
    backslash: usize,
    decoded: &mut String,
) -> Result<usize, StringError> {
    let first = read_hex4(bytes, backslash + 2)?;
    let after_first = backslash + 6;
    let code_point = match first {
        0xD800..=0xDBFF => {
            if bytes.get(after_first) != Some(&b'\\') || bytes.get(after_first + 1) != Some(&b'u') {
                return Err((backslash, "unpaired surrogate in \\u escape"));
            }
            let second = read_hex4(bytes, after_first + 2)?;
            if !(0xDC00..=0xDFFF).contains(&second) {
                return Err((backslash, "unpaired surrogate in \\u escape"));
            }
            let code_point = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
            decoded.push(char::from_u32(code_point).ok_or((backslash, "invalid \\u escape"))?);
            return Ok(after_first + 6);
        }
        0xDC00..=0xDFFF => return Err((backslash, "unpaired surrogate in \\u escape")),
        code_point => code_point,
    };
    decoded.push(char::from_u32(code_point).ok_or((backslash, "invalid \\u escape"))?);
    Ok(after_first)
}

fn read_hex4(bytes: &[u8], start: usize) -> Result<u32, StringError> {
    let mut value = 0;
    for offset in start..start + 4 {
        let digit = match bytes.get(offset) {
            Some(byte) => (*byte as char).to_digit(16),
            None => None,
        };
        value = value * 16 + digit.ok_or((start - 2, "expected 4 hex digits in \\u escape"))?;
    }
    Ok(value)
}

/// The offset of the first `"`, `\\` or control character at or after `position`, or the end
/// of the input. Checks eight bytes at a time.
fn find_special(bytes: &[u8], mut position: usize) -> usize {
    const ONES: u64 = 0x0101_0101_0101_0101;
    const HIGH_BITS: u64 = 0x8080_8080_8080_8080;
    while let Some(chunk) = bytes.get(position..position + 8) {
        let word = u64::from_le_bytes(chunk.try_into().expect("the chunk has eight bytes"));
        let quotes = word ^ (ONES * u64::from(b'"'));
        let backslashes = word ^ (ONES * u64::from(b'\\'));
        // A byte is flagged when it is zero (after the XOR) or below 0x20. Borrows can only
        // flag bytes after a real match, so the lowest flag is exact.
        let flags = (quotes.wrapping_sub(ONES) & !quotes)
            | (backslashes.wrapping_sub(ONES) & !backslashes)
            | (word.wrapping_sub(ONES * 0x20) & !word);
        let flags = flags & HIGH_BITS;
        if flags != 0 {
            return position + (flags.trailing_zeros() / 8) as usize;
        }
        position += 8;
    }
    while let Some(&byte) = bytes.get(position) {
        if byte == b'"' || byte == b'\\' || byte < 0x20 {
            break;
        }
        position += 1;
    }
    position
}
