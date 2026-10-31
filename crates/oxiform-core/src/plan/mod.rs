//! The compiled plan: an immutable graph of nodes that the executor runs. Plans are shared
//! between threads without locks.

mod node;

use rustc_hash::FxHashMap;

pub use node::{LengthRange, Node, NumberChecks, StringChecks, TaggedPlan, UnionPlan};

use crate::description::UnknownFields;

/// The index of a node in its plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) u32);

/// The index of a structure in its plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructId(pub(crate) u32);

impl StructId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A compiled type.
#[derive(Debug)]
pub struct Plan {
    pub(crate) nodes: Vec<Node>,
    pub(crate) structs: Vec<StructPlan>,
    pub(crate) root: NodeId,
}

impl Plan {
    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    pub fn struct_plan(&self, id: StructId) -> &StructPlan {
        &self.structs[id.index()]
    }

    pub fn structs(&self) -> &[StructPlan] {
        &self.structs
    }
}

/// A compiled structure.
#[derive(Debug)]
pub struct StructPlan {
    pub name: String,
    pub fields: Vec<FieldPlan>,
    pub unknown_fields: UnknownFields,
    /// The key of the tag and the value it must hold, when the structure declares a tag field.
    pub tag: Option<(String, String)>,
    field_indices: FxHashMap<String, usize>,
}

impl StructPlan {
    pub(crate) fn new(
        name: String,
        fields: Vec<FieldPlan>,
        unknown_fields: UnknownFields,
        tag: Option<(String, String)>,
    ) -> Self {
        let field_indices = fields
            .iter()
            .enumerate()
            .flat_map(|(index, field)| {
                std::iter::once(&field.wire_name)
                    .chain(&field.aliases)
                    .map(move |key| (key.clone(), index))
            })
            .collect();
        Self {
            name,
            fields,
            unknown_fields,
            tag,
            field_indices,
        }
    }

    /// The index of the field read from `wire_name`, or from one of its aliases.
    pub fn field_index(&self, wire_name: &str) -> Option<usize> {
        self.field_indices.get(wire_name).copied()
    }

    /// Like [`field_index`](Self::field_index), checking `expected` first: keys usually come
    /// in declaration order, so this is one string comparison instead of a hash lookup.
    pub fn find_field(&self, wire_name: &str, expected: usize) -> Option<usize> {
        match self.fields.get(expected) {
            Some(field) if field.wire_name == wire_name => Some(expected),
            _ => self.field_index(wire_name),
        }
    }
}

/// A compiled field.
#[derive(Debug)]
pub struct FieldPlan {
    pub name: String,
    pub wire_name: String,
    pub aliases: Vec<String>,
    pub value: NodeId,
    pub has_default: bool,
}
