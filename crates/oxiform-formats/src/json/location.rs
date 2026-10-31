//! Line and column of a byte offset, computed only when an error is reported.

/// 1-based line and column (in characters) of `offset` in `input`.
pub(crate) fn line_and_column(input: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(input.len());
    let before = &input.as_bytes()[..offset];
    let line = before.iter().filter(|&&byte| byte == b'\n').count() + 1;
    let line_start = before
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(0, |index| index + 1);
    let column = input[line_start..floor_char_boundary(input, offset)]
        .chars()
        .count()
        + 1;
    (line, column)
}

fn floor_char_boundary(input: &str, mut offset: usize) -> usize {
    while !input.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}
