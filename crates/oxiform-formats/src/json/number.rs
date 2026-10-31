//! JSON number grammar. The text is returned as written; the executor decides how to read it.

/// The end of the number that starts at `start`, or the offset of the first invalid byte.
#[inline]
pub(crate) fn scan_number(bytes: &[u8], start: usize) -> Result<usize, (usize, &'static str)> {
    let mut position = start;
    if bytes.get(position) == Some(&b'-') {
        position += 1;
    }
    match bytes.get(position) {
        Some(b'0') => {
            position += 1;
            if bytes.get(position).is_some_and(u8::is_ascii_digit) {
                return Err((position, "leading zeros are not allowed in numbers"));
            }
        }
        Some(b'1'..=b'9') => position = skip_digits(bytes, position),
        _ => return Err((position, "expected a digit")),
    }
    if bytes.get(position) == Some(&b'.') {
        position += 1;
        if !bytes.get(position).is_some_and(u8::is_ascii_digit) {
            return Err((position, "expected a digit after the decimal point"));
        }
        position = skip_digits(bytes, position);
    }
    if matches!(bytes.get(position), Some(b'e' | b'E')) {
        position += 1;
        if matches!(bytes.get(position), Some(b'+' | b'-')) {
            position += 1;
        }
        if !bytes.get(position).is_some_and(u8::is_ascii_digit) {
            return Err((position, "expected a digit in the exponent"));
        }
        position = skip_digits(bytes, position);
    }
    Ok(position)
}

#[inline]
fn skip_digits(bytes: &[u8], mut position: usize) -> usize {
    while bytes.get(position).is_some_and(u8::is_ascii_digit) {
        position += 1;
    }
    position
}
