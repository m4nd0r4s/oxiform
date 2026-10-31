//! Plan nodes and the checks they carry.

use regex::Regex;

use super::{NodeId, StructId};
use crate::description::Bound;

/// One step of a plan.
#[derive(Debug)]
pub enum Node {
    Any,
    Bool,
    None,
    Integer(NumberChecks),
    Float(NumberChecks),
    String(StringChecks),
    List { items: NodeId, length: LengthRange },
    Dict { values: NodeId, length: LengthRange },
    Optional(NodeId),
    Struct(StructId),
}

/// Checks on a number.
#[derive(Debug, Default)]
pub struct NumberChecks {
    pub minimum: Option<Bound>,
    pub maximum: Option<Bound>,
    /// Allowed values, for integers.
    pub one_of: Option<Vec<i64>>,
}

/// Checks on a string.
#[derive(Debug, Default)]
pub struct StringChecks {
    /// Length in characters, as `len()` counts it in Python.
    pub length: LengthRange,
    pub pattern: Option<Regex>,
    pub one_of: Option<Vec<String>>,
}

/// An inclusive range of lengths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LengthRange {
    pub minimum: Option<usize>,
    pub maximum: Option<usize>,
}

impl LengthRange {
    pub fn is_unbounded(&self) -> bool {
        self.minimum.is_none() && self.maximum.is_none()
    }
}
