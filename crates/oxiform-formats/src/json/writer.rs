//! Writes JSON: compact by default, or indented like `json.dumps(indent=n)`.

use oxiform_core::writer::{EventWriter, WriteError};
use smallvec::SmallVec;

/// Builds one JSON document in memory.
pub struct JsonWriter {
    output: Vec<u8>,
    indent: Option<usize>,
    /// For each open container, whether nothing was written in it yet.
    empty: SmallVec<[bool; 32]>,
    /// Whether the last call was a key, so the next value follows it directly.
    after_key: bool,
}

impl JsonWriter {
    /// A writer that indents nested values by `indent` spaces, or writes everything on one line.
    pub fn new(indent: Option<usize>) -> Self {
        Self {
            output: Vec::with_capacity(128),
            indent,
            empty: SmallVec::new(),
            after_key: false,
        }
    }

    /// The document written so far.
    pub fn into_bytes(self) -> Vec<u8> {
        self.output
    }

    /// Writes the separator and line break that go before an item.
    #[inline]
    fn before_item(&mut self) {
        if self.after_key {
            self.after_key = false;
            return;
        }
        if let Some(empty) = self.empty.last_mut() {
            if !*empty {
                self.output.push(b',');
            }
            *empty = false;
            self.new_line();
        }
    }

    fn new_line(&mut self) {
        if let Some(indent) = self.indent {
            self.output.push(b'\n');
            let width = indent * self.empty.len();
            self.output.resize(self.output.len() + width, b' ');
        }
    }

    fn open(&mut self, bracket: u8) {
        self.before_item();
        self.output.push(bracket);
        self.empty.push(true);
    }

    fn close(&mut self, bracket: u8) {
        let was_empty = self.empty.pop().unwrap_or(true);
        if !was_empty {
            self.new_line();
        }
        self.output.push(bracket);
    }

    fn write_string(&mut self, text: &str) {
        self.output.reserve(text.len() + 2);
        self.output.push(b'"');
        let bytes = text.as_bytes();
        let mut run_start = 0;
        for (index, &byte) in bytes.iter().enumerate() {
            let escape = ESCAPES[usize::from(byte)];
            if escape == 0 {
                continue;
            }
            self.output.extend_from_slice(&bytes[run_start..index]);
            run_start = index + 1;
            if escape == b'u' {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                self.output.extend_from_slice(b"\\u00");
                self.output.push(HEX[usize::from(byte >> 4)]);
                self.output.push(HEX[usize::from(byte & 0xf)]);
            } else {
                self.output.extend_from_slice(&[b'\\', escape]);
            }
        }
        self.output.extend_from_slice(&bytes[run_start..]);
        self.output.push(b'"');
    }
}

/// For each byte, the character after the backslash that escapes it, `u` for `\u00XX`, or 0
/// when the byte is written as is.
const ESCAPES: [u8; 256] = {
    let mut table = [0; 256];
    let mut byte = 0;
    while byte < 0x20 {
        table[byte] = b'u';
        byte += 1;
    }
    table[0x08] = b'b';
    table[0x09] = b't';
    table[0x0a] = b'n';
    table[0x0c] = b'f';
    table[0x0d] = b'r';
    table[b'"' as usize] = b'"';
    table[b'\\' as usize] = b'\\';
    table
};

impl EventWriter for JsonWriter {
    fn null(&mut self) {
        self.before_item();
        self.output.extend_from_slice(b"null");
    }

    fn bool(&mut self, value: bool) {
        self.before_item();
        self.output
            .extend_from_slice(if value { b"true" } else { b"false" });
    }

    fn integer(&mut self, value: i64) {
        self.before_item();
        self.output
            .extend_from_slice(itoa::Buffer::new().format(value).as_bytes());
    }

    fn big_integer(&mut self, digits: &str) -> Result<(), WriteError> {
        self.before_item();
        self.output.extend_from_slice(digits.as_bytes());
        Ok(())
    }

    fn float(&mut self, value: f64) -> Result<(), WriteError> {
        if !value.is_finite() {
            return Err(WriteError {
                message: format!("{value} is not valid JSON"),
            });
        }
        self.before_item();
        self.output
            .extend_from_slice(ryu::Buffer::new().format_finite(value).as_bytes());
        Ok(())
    }

    fn string(&mut self, value: &str) {
        self.before_item();
        self.write_string(value);
    }

    fn map_start(&mut self, _length: usize) {
        self.open(b'{');
    }

    fn key(&mut self, key: &str) {
        self.before_item();
        self.write_string(key);
        self.output.push(b':');
        if self.indent.is_some() {
            self.output.push(b' ');
        }
        self.after_key = true;
    }

    fn map_end(&mut self) {
        self.close(b'}');
    }

    fn sequence_start(&mut self, _length: usize) {
        self.open(b'[');
    }

    fn sequence_end(&mut self) {
        self.close(b']');
    }
}
