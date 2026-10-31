//! The oxiform engine: compiles a type description into an immutable plan and runs it against
//! the events of a format reader. Nothing here depends on Python.

pub mod compiler;
pub mod description;
pub mod events;
pub mod executor;
pub mod issues;
pub mod limits;
pub mod plan;
pub mod sink;
pub mod writer;
