//! Collecting pairs into groups.

use std::borrow::Cow;
use std::collections::HashMap;

use oxiform_core::events::{ReaderError, ReaderErrorDetails, ReaderErrorKind};
use oxiform_core::limits::Limits;

use super::percent::{self, Fault};
use super::reader::{Group, PairsReader, Text};

/// Collects pairs and finishes into a reader.
///
/// The first pair that cannot be read (an invalid percent-escape, bytes that are not UTF-8, a
/// limit) ends the collection: the reader meets the fault where it stands in the input, after the
/// pairs before it.
pub struct PairsBuilder<'input> {
    max_bytes: usize,
    max_items: usize,
    max_str_len: usize,
    fold_case: bool,
    groups: Vec<Group<'input>>,
    /// The group of each name; a hash table with a random seed, since names come from clients.
    index: HashMap<String, usize>,
    pairs: usize,
    bytes: usize,
    failed: bool,
}

/// Where a pair is in its source, for the message of a fault.
#[derive(Clone, Copy)]
enum Place {
    /// A byte offset in a query string.
    Offset(usize),
    /// The 0-based number of a pair of a list.
    Pair(usize),
}

impl<'input> PairsBuilder<'input> {
    /// A builder that enforces `max_bytes` (the whole source), `max_items` (pairs) and
    /// `max_str_len` (every decoded name and value), and, with `fold_case`, groups names that
    /// differ only in ASCII case (header names).
    pub fn new(limits: &Limits, fold_case: bool) -> Self {
        Self {
            max_bytes: limits.max_bytes,
            max_items: limits.max_items,
            max_str_len: limits.max_str_len,
            fold_case,
            groups: Vec::new(),
            index: HashMap::new(),
            pairs: 0,
            bytes: 0,
            failed: false,
        }
    }

    /// Adds the pairs of a query string or an `application/x-www-form-urlencoded` body: `name=value`
    /// separated by `&`, with `%XX` and `+` decoded, empty segments skipped and a name without `=`
    /// holding the empty string.
    pub fn add_urlencoded(&mut self, input: &'input [u8]) {
        if self.failed {
            return;
        }
        if input.len() > self.max_bytes {
            let limit = self.max_bytes;
            let message = format!("the input exceeds the limit max_bytes={limit}");
            self.fail_name(fault(
                ReaderErrorKind::ByteLimit { limit },
                message,
                Place::Offset(0),
            ));
            return;
        }
        let mut offset = 0;
        for segment in input.split(|&byte| byte == b'&') {
            let start = offset;
            offset += segment.len() + 1;
            if segment.is_empty() {
                continue;
            }
            let (name, value, value_start) = match segment.iter().position(|&byte| byte == b'=') {
                Some(equals) => (
                    &segment[..equals],
                    &segment[equals + 1..],
                    start + equals + 1,
                ),
                None => (segment, &segment[segment.len()..], start + segment.len()),
            };
            let name = percent::decode(name).map_err(|f| fault_of(f, start));
            let value = percent::decode(value).map_err(|f| fault_of(f, value_start));
            if !self.add(name, value, segment.len()) {
                return;
            }
        }
    }

    /// Adds one pair of a list, such as a header or a cookie: UTF-8, without percent-decoding.
    pub fn add_pair(&mut self, name: &'input [u8], value: &'input [u8]) {
        if self.failed {
            return;
        }
        let place = Place::Pair(self.pairs);
        let name = percent::utf8(name).map_err(|f| fault_in_pair(f, place, "name"));
        let value = percent::utf8(value).map_err(|f| fault_in_pair(f, place, "value"));
        self.add(name, value, 0);
    }

    /// Adds one pair, returning whether the collection goes on. `extra` bytes are counted with
    /// the pair (the separators of a query string that the pair does not hold).
    fn add(&mut self, name: Text<'input>, value: Text<'input>, extra: usize) -> bool {
        self.pairs += 1;
        if self.pairs > self.max_items {
            let limit = self.max_items;
            let message =
                format!("the source has more than {limit} pairs: the limit max_items={limit}");
            let place = Place::Pair(self.pairs - 1);
            self.fail_name(fault(ReaderErrorKind::ItemLimit { limit }, message, place));
            return false;
        }
        let name = match name {
            Ok(name) => name,
            Err(error) => {
                self.fail_name(error);
                return false;
            }
        };
        let value_size = value.as_ref().map_or(0, |value| value.len());
        self.bytes = self.bytes.saturating_add(name.len() + value_size + extra);
        if self.bytes > self.max_bytes {
            let limit = self.max_bytes;
            let message = format!("the pairs exceed the limit max_bytes={limit}");
            let place = Place::Pair(self.pairs - 1);
            self.fail_name(fault(ReaderErrorKind::ByteLimit { limit }, message, place));
            return false;
        }
        if name.len() > self.max_str_len {
            let limit = self.max_str_len;
            let message = format!("a name exceeds the limit max_str_len={limit}");
            let place = Place::Pair(self.pairs - 1);
            self.fail_name(fault(
                ReaderErrorKind::StringLimit { limit },
                message,
                place,
            ));
            return false;
        }
        let value = match value {
            Ok(value) if value.len() > self.max_str_len => {
                let limit = self.max_str_len;
                let message = format!("a value exceeds the limit max_str_len={limit}");
                let place = Place::Pair(self.pairs - 1);
                Err(fault(
                    ReaderErrorKind::StringLimit { limit },
                    message,
                    place,
                ))
            }
            other => other,
        };
        let failed = value.is_err();
        let group = self.group_of(name);
        self.groups[group].values.push(value);
        self.failed = failed;
        !failed
    }

    /// The group of `name`, started when it is the first time.
    fn group_of(&mut self, name: Cow<'input, str>) -> usize {
        let key = if self.fold_case {
            Cow::Owned(name.to_ascii_lowercase())
        } else {
            Cow::Borrowed(&*name)
        };
        if let Some(&group) = self.index.get(&*key) {
            return group;
        }
        let group = self.groups.len();
        self.index.insert(key.into_owned(), group);
        self.groups.push(Group {
            name: Ok(name),
            values: Vec::new(),
        });
        group
    }

    /// A fault in a name: the reader meets it in place of the name.
    fn fail_name(&mut self, error: ReaderError) {
        self.failed = true;
        self.groups.push(Group {
            name: Err(error),
            values: Vec::new(),
        });
    }

    /// The reader of what was collected.
    pub fn finish(self) -> PairsReader<'input> {
        PairsReader::new(self.groups)
    }
}

fn fault(kind: ReaderErrorKind, message: String, place: Place) -> ReaderError {
    let (offset, line, column) = match place {
        Place::Offset(offset) => (offset, 1, offset + 1),
        Place::Pair(index) => (index, index + 1, 1),
    };
    ReaderError::new(ReaderErrorDetails {
        kind,
        message,
        offset,
        line,
        column,
    })
}

/// A fault of a component of a query string that starts at byte `start`.
fn fault_of(fault_found: Fault, start: usize) -> ReaderError {
    match fault_found {
        Fault::Escape { at, text } => fault(
            ReaderErrorKind::Syntax,
            format!(
                "invalid percent-escape {text:?}: a % must be followed by two hexadecimal digits \
                 (write a literal % as %25)"
            ),
            Place::Offset(start + at),
        ),
        Fault::Utf8 { valid_up_to } => fault(
            ReaderErrorKind::Syntax,
            format!(
                "the percent-decoded text is not valid UTF-8 (invalid byte {valid_up_to} of it): \
                 encode text as UTF-8 before percent-encoding it"
            ),
            Place::Offset(start),
        ),
    }
}

/// A fault of the name or the value of a pair of a list.
fn fault_in_pair(fault_found: Fault, place: Place, part: &str) -> ReaderError {
    let Fault::Utf8 { valid_up_to } = fault_found else {
        unreachable!("only the percent-decoding finds an invalid escape")
    };
    fault(
        ReaderErrorKind::Syntax,
        format!(
            "the {part} is not valid UTF-8 (invalid byte {valid_up_to}): pass text, or bytes that hold UTF-8"
        ),
        place,
    )
}
