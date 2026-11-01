//! Where the executor builds its output. The Python bridge creates Python objects; tests build
//! plain Rust values.

use crate::plan::{ChoiceId, ConversionId, StructId};
use crate::temporal::Temporal;

/// Builds the values the executor validated.
pub trait Sink {
    type Value;
    type Error;
    /// A map being built.
    type Map;

    fn none(&mut self) -> Result<Self::Value, Self::Error>;
    fn bool(&mut self, value: bool) -> Result<Self::Value, Self::Error>;
    fn integer(&mut self, value: i64) -> Result<Self::Value, Self::Error>;
    /// An integer outside the 64-bit range, as decimal digits with an optional sign.
    fn big_integer(&mut self, digits: &str) -> Result<Self::Value, Self::Error>;
    fn float(&mut self, value: f64) -> Result<Self::Value, Self::Error>;
    fn string(&mut self, value: &str) -> Result<Self::Value, Self::Error>;
    /// Bytes decoded from the input; a mutable `bytearray` in Python when `mutable`.
    fn bytes(&mut self, value: &[u8], mutable: bool) -> Result<Self::Value, Self::Error>;
    fn list(&mut self, items: Vec<Self::Value>) -> Result<Self::Value, Self::Error>;
    fn tuple(&mut self, items: Vec<Self::Value>) -> Result<Self::Value, Self::Error>;
    fn set(&mut self, items: Vec<Self::Value>, frozen: bool) -> Result<Self::Value, Self::Error>;
    /// The value at `index` of a set of choices.
    fn choice(&mut self, choice: ChoiceId, index: u32) -> Result<Self::Value, Self::Error>;
    fn temporal(&mut self, value: Temporal) -> Result<Self::Value, Self::Error>;
    /// The value of a conversion built from `raw`, the string or integer read, or `None` when
    /// the conversion rejects it.
    fn convert(
        &mut self,
        conversion: ConversionId,
        raw: Self::Value,
    ) -> Result<Option<Self::Value>, Self::Error>;
    fn map_new(&mut self) -> Result<Self::Map, Self::Error>;
    /// Inserts an entry. Returns `false`, leaving the map unchanged, when `key` is already in it.
    fn map_insert(
        &mut self,
        map: &mut Self::Map,
        key: &str,
        value: Self::Value,
    ) -> Result<bool, Self::Error>;
    fn map_finish(&mut self, map: Self::Map) -> Result<Self::Value, Self::Error>;
    /// An instance of a structure, taking the field values out of `fields`. `None` marks a
    /// field that was missing and takes its default.
    fn instance(
        &mut self,
        structure: StructId,
        fields: &mut [Option<Self::Value>],
    ) -> Result<Self::Value, Self::Error>;
}
