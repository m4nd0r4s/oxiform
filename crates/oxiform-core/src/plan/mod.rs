//! The compiled plan: an immutable graph of nodes that the executor runs. Plans are shared
//! between threads without locks.

mod node;

use std::collections::HashMap;

pub use node::{LengthRange, Node, NumberChecks, StringChecks};

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
    field_indices: HashMap<String, usize>,
}

impl StructPlan {
    pub(crate) fn new(name: String, fields: Vec<FieldPlan>, unknown_fields: UnknownFields) -> Self {
        let field_indices = fields
            .iter()
            .enumerate()
            .map(|(index, field)| (field.wire_name.clone(), index))
            .collect();
        Self {
            name,
            fields,
            unknown_fields,
            field_indices,
        }
    }

    /// The index of the field read from `wire_name`.
    pub fn field_index(&self, wire_name: &str) -> Option<usize> {
        self.field_indices.get(wire_name).copied()
    }
}

/// A compiled field.
#[derive(Debug)]
pub struct FieldPlan {
    pub name: String,
    pub wire_name: String,
    pub value: NodeId,
    pub has_default: bool,
}
