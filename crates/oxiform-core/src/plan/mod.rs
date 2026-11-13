//! The compiled plan: an immutable graph of nodes that the executor runs. Plans are shared
//! between threads without locks.

mod node;

use rustc_hash::FxHashMap;

pub use node::{ChoicePlan, LengthRange, Node, NumberChecks, StringChecks, TaggedPlan, UnionPlan};

use crate::description::{ConversionDescription, UnknownFields};

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

/// The index of a set of choices in its plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChoiceId(pub(crate) u32);

impl ChoiceId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// The index of a conversion in its plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConversionId(pub(crate) u32);

impl ConversionId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A compiled type.
#[derive(Debug)]
pub struct Plan {
    pub(crate) nodes: Vec<Node>,
    pub(crate) structs: Vec<StructPlan>,
    pub(crate) choices: Vec<ChoicePlan>,
    pub(crate) conversions: Vec<ConversionDescription>,
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

    pub fn choice_plan(&self, id: ChoiceId) -> &ChoicePlan {
        &self.choices[id.index()]
    }

    pub fn conversion(&self, id: ConversionId) -> &ConversionDescription {
        &self.conversions[id.index()]
    }

    /// Whether a value read as `node` can be `None`: `Any`, `None`, `T | None`, and a union or
    /// an untagged union with a variant that is.
    pub fn accepts_none(&self, node: NodeId) -> bool {
        match self.node(node) {
            Node::Any | Node::None | Node::Optional { .. } => true,
            Node::Union(union) => union.null.is_some(),
            Node::Untagged(variants) => variants.iter().any(|(_, node)| self.accepts_none(*node)),
            _ => false,
        }
    }

    /// Whether an instance of `structure` holds a `Secret` anywhere inside: in a field, in a
    /// model nested in a field, in an item of a collection. It is what decides whether a
    /// message made from the instance may be shown.
    pub fn holds_secret(&self, structure: StructId) -> bool {
        let mut visited = vec![false; self.structs.len()];
        self.struct_holds_secret(structure, &mut visited)
    }

    /// Like [`holds_secret`](Self::holds_secret) for a value read as `node`: it is, or holds, a
    /// `Secret`.
    pub fn node_holds_a_secret(&self, node: NodeId) -> bool {
        let mut visited = vec![false; self.structs.len()];
        self.node_holds_secret(node, &mut visited)
    }

    fn struct_holds_secret(&self, structure: StructId, visited: &mut [bool]) -> bool {
        if std::mem::replace(&mut visited[structure.index()], true) {
            return false;
        }
        self.struct_plan(structure)
            .fields
            .iter()
            .any(|field| self.node_holds_secret(field.value, visited))
    }

    fn node_holds_secret(&self, node: NodeId, visited: &mut [bool]) -> bool {
        match self.node(node) {
            Node::Secret(_) => true,
            Node::List { items, .. } => self.node_holds_secret(*items, visited),
            Node::Dict { values, .. } => self.node_holds_secret(*values, visited),
            Node::Optional { inner, .. } => self.node_holds_secret(*inner, visited),
            Node::Tuple(items) => items
                .iter()
                .any(|item| self.node_holds_secret(*item, visited)),
            Node::Struct(id) => self.struct_holds_secret(*id, visited),
            Node::Union(union) => [
                union.null,
                union.bool,
                union.integer,
                union.float,
                union.string,
                union.sequence,
                union.map,
            ]
            .into_iter()
            .flatten()
            .any(|variant| self.node_holds_secret(variant, visited)),
            Node::Tagged(tagged) => tagged
                .variants
                .values()
                .any(|id| self.struct_holds_secret(*id, visited)),
            Node::Untagged(variants) => variants
                .iter()
                .any(|(_, node)| self.node_holds_secret(*node, visited)),
            Node::Any
            | Node::Bool
            | Node::None
            | Node::Integer(_)
            | Node::Float(_)
            | Node::String(_)
            | Node::Bytes { .. }
            | Node::Temporal(_)
            | Node::Converted(_)
            | Node::Choice(_) => false,
        }
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
    /// Whether the sink checks the value of at least one field ([`FieldPlan::validated`]).
    /// Structures that have none skip the step altogether.
    pub field_validators: bool,
    /// Whether the sink checks each instance after it is built.
    pub model_validators: bool,
    field_indices: FxHashMap<String, usize>,
    /// The same names with ASCII letters in lower case, for sources that do not tell case apart.
    folded_indices: FxHashMap<String, usize>,
}

impl StructPlan {
    pub(crate) fn new(
        name: String,
        fields: Vec<FieldPlan>,
        unknown_fields: UnknownFields,
        tag: Option<(String, String)>,
        model_validators: bool,
    ) -> Self {
        let field_validators = fields.iter().any(|field| field.validated);
        let field_indices = fields
            .iter()
            .enumerate()
            .flat_map(|(index, field)| {
                std::iter::once(&field.wire_name)
                    .chain(&field.aliases)
                    .map(move |key| (key.clone(), index))
            })
            .collect();
        let folded_indices = fields
            .iter()
            .enumerate()
            .flat_map(|(index, field)| {
                std::iter::once(&field.wire_name)
                    .chain(&field.aliases)
                    .map(move |key| (key.to_ascii_lowercase(), index))
            })
            .collect();
        Self {
            name,
            fields,
            unknown_fields,
            tag,
            field_validators,
            model_validators,
            field_indices,
            folded_indices,
        }
    }

    /// The index of the field read from `wire_name`, or from one of its aliases.
    pub fn field_index(&self, wire_name: &str) -> Option<usize> {
        self.field_indices.get(wire_name).copied()
    }

    /// Like [`field_index`](Self::field_index), without regard to ASCII case (header names).
    pub fn field_index_folded(&self, name: &str) -> Option<usize> {
        if name.bytes().any(|byte| byte.is_ascii_uppercase()) {
            self.folded_indices.get(&name.to_ascii_lowercase()).copied()
        } else {
            self.folded_indices.get(name).copied()
        }
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
    /// The sink checks the value once it is read, after the whole map (`@ox.validator`).
    pub validated: bool,
}
