//! Format readers and writers. Every reader emits the events of `oxiform_core::events`, so the
//! executor in `oxiform-core` is shared by all formats.

pub mod json;
