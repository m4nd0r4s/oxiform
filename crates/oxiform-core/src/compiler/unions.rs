//! Unions: which variant reads each kind of value, and tags that tell structures apart.

use rustc_hash::FxHashMap;

use super::{Compiler, DeclarationError, declaration_error};
use crate::description::{ChoiceValue, ConversionInput, Description, TagMode, TypeDescription};
use crate::plan::{Node, NodeId, StructId, TaggedPlan, UnionPlan};

/// The kinds of value a union tells apart, with the Python name used in messages.
#[derive(Clone, Copy)]
enum Kind {
    Null,
    Bool,
    Integer,
    Float,
    String,
    Sequence,
}

impl Kind {
    fn input_name(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool => "a bool",
            Self::Integer => "an integer",
            Self::Float => "a number",
            Self::String => "a string",
            Self::Sequence => "a sequence",
        }
    }
}

impl Compiler<'_> {
    pub(super) fn compile_union(
        &mut self,
        variants: &[TypeDescription],
        location: &str,
    ) -> Result<Node, DeclarationError> {
        let mut plan = UnionPlan {
            name: union_name(variants, self.description),
            ..UnionPlan::default()
        };
        let mut names: FxHashMap<&'static str, String> = FxHashMap::default();
        let mut maps = Vec::new();
        for variant in variants {
            let name = type_name(variant, self.description);
            let kinds = match variant {
                TypeDescription::None => vec![Kind::Null],
                TypeDescription::Bool => vec![Kind::Bool],
                TypeDescription::Integer(_) => vec![Kind::Integer],
                TypeDescription::Float(_) => vec![Kind::Float],
                TypeDescription::String(_)
                | TypeDescription::Bytes { .. }
                | TypeDescription::Temporal(_) => vec![Kind::String],
                TypeDescription::Converted(index) => self.conversion_kinds(*index),
                TypeDescription::List { .. }
                | TypeDescription::Collection { .. }
                | TypeDescription::Tuple(_) => vec![Kind::Sequence],
                TypeDescription::Choice(index) => self.choice_kinds(*index),
                TypeDescription::Dict { .. }
                | TypeDescription::Struct(_)
                | TypeDescription::Tagged { .. } => {
                    maps.push(variant);
                    continue;
                }
                TypeDescription::Any
                | TypeDescription::Optional(_)
                | TypeDescription::Union(_)
                | TypeDescription::Untagged(_) => {
                    return Err(declaration_error(
                        location,
                        format!("{name} cannot be a variant of a union"),
                    ));
                }
            };
            for kind in &kinds {
                if let Some(previous) = names.insert(kind.input_name(), name.clone()) {
                    return Err(declaration_error(
                        location,
                        format!(
                            "{previous} and {name} are both read from {}; \
                             tell them apart with ox.Tagged or use ox.Untagged",
                            kind.input_name()
                        ),
                    ));
                }
            }
            let node = self.compile_type(variant, location)?;
            for kind in kinds {
                let slot = match kind {
                    Kind::Null => &mut plan.null,
                    Kind::Bool => &mut plan.bool,
                    Kind::Integer => &mut plan.integer,
                    Kind::Float => &mut plan.float,
                    Kind::String => &mut plan.string,
                    Kind::Sequence => &mut plan.sequence,
                };
                *slot = Some(node);
            }
        }
        plan.map = self.compile_map_variants(&maps, location)?;
        Ok(Node::Union(Box::new(plan)))
    }

    /// The kinds of value a `Literal` or `Enum` is read from.
    fn choice_kinds(&self, index: usize) -> Vec<Kind> {
        let Some(choice) = self.description.choices.get(index) else {
            return Vec::new();
        };
        let mut kinds = Vec::new();
        for value in &choice.values {
            let kind = match value {
                ChoiceValue::Null => Kind::Null,
                ChoiceValue::Bool(_) => Kind::Bool,
                ChoiceValue::Integer(_) => Kind::Integer,
                ChoiceValue::String(_) => Kind::String,
            };
            if !kinds
                .iter()
                .any(|known: &Kind| known.input_name() == kind.input_name())
            {
                kinds.push(kind);
            }
        }
        kinds
    }

    /// The kinds of value a conversion is read from.
    fn conversion_kinds(&self, index: usize) -> Vec<Kind> {
        match self.description.conversions.get(index).map(|c| c.input) {
            Some(ConversionInput::String) => vec![Kind::String],
            Some(ConversionInput::Integer) => vec![Kind::Integer],
            Some(ConversionInput::Decimal) => vec![Kind::Integer, Kind::Float, Kind::String],
            None => Vec::new(),
        }
    }

    /// The variant read from a map. Several structures that declare the same tag field are
    /// told apart by it, as with `ox.Tagged(field)`.
    fn compile_map_variants(
        &mut self,
        maps: &[&TypeDescription],
        location: &str,
    ) -> Result<Option<NodeId>, DeclarationError> {
        match maps {
            [] => Ok(None),
            [only] => self.compile_type(only, location).map(Some),
            [first, second, ..] => {
                let shared_tag = self.shared_tag_field(maps);
                let Some(tag) = shared_tag else {
                    return Err(declaration_error(
                        location,
                        format!(
                            "{} and {} are both read from a map; give the models the same \
                             tag_field, tell them apart with ox.Tagged or use ox.Untagged",
                            type_name(first, self.description),
                            type_name(second, self.description)
                        ),
                    ));
                };
                let variants: Vec<TypeDescription> = maps.iter().map(|&v| v.clone()).collect();
                let node = self.compile_tagged(&TagMode::Internal { tag }, &variants, location)?;
                Ok(Some(self.push(node)))
            }
        }
    }

    /// The tag field every variant declares, when they are all structures with the same one.
    fn shared_tag_field(&self, maps: &[&TypeDescription]) -> Option<String> {
        let mut shared: Option<&String> = None;
        for variant in maps {
            let TypeDescription::Struct(index) = variant else {
                return None;
            };
            let field = self.description.structs.get(*index)?.tag_field.as_ref()?;
            if shared.is_some_and(|shared| shared != field) {
                return None;
            }
            shared = Some(field);
        }
        shared.cloned()
    }

    pub(super) fn compile_tagged(
        &mut self,
        mode: &TagMode,
        variants: &[TypeDescription],
        location: &str,
    ) -> Result<Node, DeclarationError> {
        let mut by_tag = FxHashMap::default();
        let mut tags = Vec::with_capacity(variants.len());
        for variant in variants {
            let TypeDescription::Struct(index) = variant else {
                return Err(declaration_error(
                    location,
                    format!(
                        "only models can be variants of a tagged union, not {}",
                        type_name(variant, self.description)
                    ),
                ));
            };
            let Some(structure) = self.description.structs.get(*index) else {
                return Err(declaration_error(
                    location,
                    format!("unknown structure index {index}"),
                ));
            };
            if let TagMode::Internal { tag } = mode {
                if structure
                    .tag_field
                    .as_ref()
                    .is_some_and(|field| field != tag)
                {
                    return Err(declaration_error(
                        location,
                        format!(
                            "the union is tagged by {tag:?}, but {} declares the tag_field {:?}",
                            structure.name,
                            structure.tag_field.as_deref().unwrap_or_default()
                        ),
                    ));
                }
                let reads_tag = structure.fields.iter().any(|field| {
                    field.wire_name == *tag || field.aliases.iter().any(|alias| alias == tag)
                });
                if reads_tag {
                    return Err(declaration_error(
                        location,
                        format!(
                            "{} has a field read from the tag key {tag:?}",
                            structure.name
                        ),
                    ));
                }
            }
            let id = StructId(u32::try_from(*index).expect("fewer than 2^32 structures"));
            if let Some(previous) = by_tag.insert(structure.tag.clone(), id) {
                let previous = &self.description.structs[previous.index()].name;
                return Err(declaration_error(
                    location,
                    format!(
                        "{previous} and {} have the same tag {:?}",
                        structure.name, structure.tag
                    ),
                ));
            }
            tags.push(structure.tag.clone());
        }
        Ok(Node::Tagged(Box::new(TaggedPlan {
            name: union_name(variants, self.description),
            mode: mode.clone(),
            variants: by_tag,
            tags,
        })))
    }

    pub(super) fn compile_untagged(
        &mut self,
        variants: &[TypeDescription],
        location: &str,
    ) -> Result<Node, DeclarationError> {
        let compiled = variants
            .iter()
            .map(|variant| {
                let node = self.compile_type(variant, location)?;
                Ok((type_name(variant, self.description), node))
            })
            .collect::<Result<Vec<_>, DeclarationError>>()?;
        Ok(Node::Untagged(compiled))
    }
}

fn union_name(variants: &[TypeDescription], description: &Description) -> String {
    variants
        .iter()
        .map(|variant| type_name(variant, description))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// A type as written in Python, for messages.
fn type_name(description: &TypeDescription, whole: &Description) -> String {
    match description {
        TypeDescription::Any => "Any".into(),
        TypeDescription::Bool => "bool".into(),
        TypeDescription::None => "None".into(),
        TypeDescription::Integer(_) => "int".into(),
        TypeDescription::Float(_) => "float".into(),
        TypeDescription::String(_) => "str".into(),
        TypeDescription::Bytes { mutable: false, .. } => "bytes".into(),
        TypeDescription::Bytes { mutable: true, .. } => "bytearray".into(),
        TypeDescription::Temporal(kind) => kind.name().into(),
        TypeDescription::Converted(index) => whole
            .conversions
            .get(*index)
            .map_or_else(|| format!("conversion {index}"), |c| c.name.clone()),
        TypeDescription::List { .. } => "list".into(),
        TypeDescription::Collection { kind, .. } => kind.name().into(),
        TypeDescription::Tuple(_) => "tuple".into(),
        TypeDescription::Choice(index) => whole
            .choices
            .get(*index)
            .map_or_else(|| format!("choice {index}"), |c| c.name.clone()),
        TypeDescription::Dict { .. } => "dict".into(),
        TypeDescription::Optional(inner) => format!("{} | None", type_name(inner, whole)),
        TypeDescription::Struct(index) => whole
            .structs
            .get(*index)
            .map_or_else(|| format!("structure {index}"), |s| s.name.clone()),
        TypeDescription::Union(variants)
        | TypeDescription::Tagged { variants, .. }
        | TypeDescription::Untagged(variants) => union_name(variants, whole),
    }
}
