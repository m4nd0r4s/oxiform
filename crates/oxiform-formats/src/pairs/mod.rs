//! Sources of pairs: raw query strings, `application/x-www-form-urlencoded` bodies, and lists of
//! name/value pairs such as headers and cookies.
//!
//! A [`PairsBuilder`] collects the pairs, decodes them and groups the values by name; the
//! [`PairsReader`] it finishes into emits the events the executor reads (`execute_pairs`): one
//! map in which each name appears once, with its values as a sequence, in the order the names
//! first appeared.

mod builder;
mod percent;
mod reader;

pub use builder::PairsBuilder;
pub use reader::PairsReader;

#[cfg(test)]
mod tests;
