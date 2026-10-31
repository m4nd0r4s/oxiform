//! JSON (RFC 8259).

mod location;
mod number;
mod reader;
mod string;
mod writer;

pub use reader::JsonReader;
pub use writer::JsonWriter;

#[cfg(test)]
mod tests;
