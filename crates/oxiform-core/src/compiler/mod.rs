//! Checks a description and compiles it into a plan. Every mistake in a declaration is caught
//! here, before any data is read.

mod constraints;
mod unions;

use std::collections::HashSet;
use std::fmt;

use crate::description::{Description, StructDescription, TypeDescription};
use crate::plan::{FieldPlan, Node, NodeId, Plan, StructId, StructPlan};

/// A declaration that cannot be compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationError {
    /// Where the problem is: a structure and field (`User.age`) or `root`.
    pub location: String,
    pub message: String,
}

impl fmt::Display for DeclarationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.location, self.message)
    }
}

impl std::error::Error for DeclarationError {}

pub(crate) fn declaration_error(location: &str, message: impl Into<String>) -> DeclarationError {
    DeclarationError {
        location: location.to_owned(),
        message: message.into(),
    }
}

/// Compiles `description` into a plan.
pub fn compile(description: &Description) -> Result<Plan, DeclarationError> {
    let mut compiler = Compiler {
        description,
        nodes: Vec::new(),
    };
    let structs = description
        .structs
        .iter()
        .map(|struct_description| compiler.compile_struct(struct_description))
        .collect::<Result<Vec<_>, _>>()?;
    let root = compiler.compile_type(&description.root, "root")?;
    Ok(Plan {
        nodes: compiler.nodes,
        structs,
        root,
    })
}

struct Compiler<'description> {
    description: &'description Description,
    nodes: Vec<Node>,
}

impl Compiler<'_> {
    fn push(&mut self, node: Node) -> NodeId {
        let id = NodeId(u32::try_from(self.nodes.len()).expect("a plan has fewer than 2^32 nodes"));
        self.nodes.push(node);
        id
    }

    fn compile_struct(
        &mut self,
        description: &StructDescription,
    ) -> Result<StructPlan, DeclarationError> {
        let mut wire_names = HashSet::new();
        let mut fields = Vec::with_capacity(description.fields.len());
        for field in &description.fields {
            let location = format!("{}.{}", description.name, field.name);
            for key in std::iter::once(&field.wire_name).chain(&field.aliases) {
                if !wire_names.insert(key.as_str()) {
                    return Err(declaration_error(
                        &location,
                        format!("another field is already read from the key {key:?}"),
                    ));
                }
            }
            let value = self.compile_type(&field.value_type, &location)?;
            fields.push(FieldPlan {
                name: field.name.clone(),
                wire_name: field.wire_name.clone(),
                aliases: field.aliases.clone(),
                value,
                has_default: field.has_default,
            });
        }
        let tag = match &description.tag_field {
            Some(tag_field) if wire_names.contains(tag_field.as_str()) => {
                return Err(declaration_error(
                    &description.name,
                    format!("a field is read from the tag field {tag_field:?}"),
                ));
            }
            Some(tag_field) => Some((tag_field.clone(), description.tag.clone())),
            None => None,
        };
        Ok(StructPlan::new(
            description.name.clone(),
            fields,
            description.unknown_fields,
            tag,
        ))
    }

    fn compile_type(
        &mut self,
        description: &TypeDescription,
        location: &str,
    ) -> Result<NodeId, DeclarationError> {
        let node = match description {
            TypeDescription::Any => Node::Any,
            TypeDescription::Bool => Node::Bool,
            TypeDescription::None => Node::None,
            TypeDescription::Integer(declared) => {
                Node::Integer(constraints::number_checks(declared, "int", location)?)
            }
            TypeDescription::Float(declared) => {
                Node::Float(constraints::number_checks(declared, "float", location)?)
            }
            TypeDescription::String(declared) => {
                Node::String(constraints::string_checks(declared, location)?)
            }
            TypeDescription::List {
                items,
                constraints: declared,
            } => {
                let length = constraints::length_only(declared, "list", location)?;
                let items = self.compile_type(items, location)?;
                Node::List { items, length }
            }
            TypeDescription::Dict {
                values,
                constraints: declared,
            } => {
                let length = constraints::length_only(declared, "dict", location)?;
                let values = self.compile_type(values, location)?;
                Node::Dict { values, length }
            }
            TypeDescription::Optional(inner) => Node::Optional(self.compile_type(inner, location)?),
            TypeDescription::Struct(index) => {
                if *index >= self.description.structs.len() {
                    return Err(declaration_error(
                        location,
                        format!("unknown structure index {index}"),
                    ));
                }
                Node::Struct(StructId(
                    u32::try_from(*index).expect("fewer than 2^32 structures"),
                ))
            }
            TypeDescription::Union(variants) => self.compile_union(variants, location)?,
            TypeDescription::Tagged { mode, variants } => {
                self.compile_tagged(mode, variants, location)?
            }
            TypeDescription::Untagged(variants) => self.compile_untagged(variants, location)?,
        };
        Ok(self.push(node))
    }
}

#[cfg(test)]
mod tests;
