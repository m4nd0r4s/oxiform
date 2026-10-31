//! The calls every format writer accepts. The encoder walks a value once and makes these calls,
//! so one walk serves JSON, YAML, TOML and MessagePack.

use std::fmt;

/// A value the format cannot represent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteError {
    pub message: String,
}

impl fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for WriteError {}

/// Writes one document. Containers announce their length, which some formats need up front.
pub trait EventWriter {
    fn null(&mut self);
    fn bool(&mut self, value: bool);
    fn integer(&mut self, value: i64);
    /// An integer outside the 64-bit range, as decimal digits with an optional sign.
    fn big_integer(&mut self, digits: &str) -> Result<(), WriteError>;
    fn float(&mut self, value: f64) -> Result<(), WriteError>;
    fn string(&mut self, value: &str);
    fn map_start(&mut self, length: usize);
    /// A key; the next call writes its value.
    fn key(&mut self, key: &str);
    fn map_end(&mut self);
    fn sequence_start(&mut self, length: usize);
    fn sequence_end(&mut self);
}
