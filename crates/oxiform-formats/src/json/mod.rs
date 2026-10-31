//! JSON (RFC 8259).

mod location;
mod number;
mod reader;
mod string;

pub use reader::JsonReader;

#[cfg(test)]
mod tests;
