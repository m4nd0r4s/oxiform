//! Plan nodes and the checks they carry.

use regex::Regex;
use rustc_hash::FxHashMap;

use super::{NodeId, StructId};
use crate::description::{Bound, TagMode};

/// One step of a plan.
#[derive(Debug)]
pub enum Node {
    Any,
    Bool,
    None,
    Integer(NumberChecks),
    Float(NumberChecks),
    String(StringChecks),
    List {
        items: NodeId,
        length: LengthRange,
    },
    Dict {
        values: NodeId,
        length: LengthRange,
    },
    Optional(NodeId),
    Struct(StructId),
    Union(Box<UnionPlan>),
    Tagged(Box<TaggedPlan>),
    /// The variants to try, in order, with their names.
    Untagged(Vec<(String, NodeId)>),
}

/// The variant read for each kind of value. A number is read as `integer` when it has no
/// fractional part and there is an `integer` variant, and as `float` otherwise.
#[derive(Debug, Default)]
pub struct UnionPlan {
    /// The union as written in Python (`int | str`), for messages.
    pub name: String,
    pub null: Option<NodeId>,
    pub bool: Option<NodeId>,
    pub integer: Option<NodeId>,
    pub float: Option<NodeId>,
    pub string: Option<NodeId>,
    pub sequence: Option<NodeId>,
    pub map: Option<NodeId>,
}

/// Structures told apart by a tag.
#[derive(Debug)]
pub struct TaggedPlan {
    /// The union as written in Python (`Card | Pix`), for messages.
    pub name: String,
    pub mode: TagMode,
    /// The structure of each tag value.
    pub variants: FxHashMap<String, StructId>,
    /// The tag values, in declaration order, for messages.
    pub tags: Vec<String>,
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
