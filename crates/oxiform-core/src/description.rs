//! The description of a type that the Python side produces from annotations. The compiler
//! checks it and turns it into a [`Plan`](crate::plan::Plan).

/// A type to read, with the constraints declared on it.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeDescription {
    Any,
    Bool,
    None,
    Integer(Constraints),
    Float(Constraints),
    String(Constraints),
    List {
        items: Box<TypeDescription>,
        constraints: Constraints,
    },
    /// A map with string keys.
    Dict {
        values: Box<TypeDescription>,
        constraints: Constraints,
    },
    /// `T | None`.
    Optional(Box<TypeDescription>),
    /// A structure, by its index in [`Description::structs`]. Indices let structures refer to
    /// themselves.
    Struct(usize),
    /// One of several types, chosen by the kind of value in the input (null, bool, number,
    /// string, sequence or map). Structures that share a tag field are told apart by it.
    Union(Vec<TypeDescription>),
    /// One of several structures, chosen by their tag.
    Tagged {
        mode: TagMode,
        variants: Vec<TypeDescription>,
    },
    /// One of several types, trying each in order.
    Untagged(Vec<TypeDescription>),
}

/// Where the tag of a tagged union is in the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagMode {
    /// A key of the structure itself: `{"type": "card", ...}`.
    Internal { tag: String },
    /// A key next to another that holds the value: `{"type": "card", "data": {...}}`.
    Adjacent { tag: String, content: String },
    /// The only key of a map, holding the value: `{"card": {...}}`.
    External,
}

/// Constraints of one value. Which ones apply depends on the type.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Constraints {
    /// `Ge`: the value is at least this.
    pub minimum: Option<Bound>,
    /// `Le`: the value is at most this.
    pub maximum: Option<Bound>,
    /// `Len`: minimum number of characters or items.
    pub min_length: Option<usize>,
    /// `Len`: maximum number of characters or items.
    pub max_length: Option<usize>,
    /// `Pattern`: a regular expression the text must contain a match of.
    pub pattern: Option<String>,
    /// `OneOf`: the only values allowed.
    pub one_of: Option<Vec<Literal>>,
}

impl Constraints {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A numeric limit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bound {
    Integer(i64),
    Float(f64),
}

/// A constant value, for `OneOf`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Literal {
    Integer(i64),
    String(String),
}

/// What to do with keys that match no field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UnknownFields {
    #[default]
    Ignore,
    Forbid,
}

/// A structure: named fields read from a map.
#[derive(Debug, Clone, PartialEq)]
pub struct StructDescription {
    pub name: String,
    pub fields: Vec<FieldDescription>,
    pub unknown_fields: UnknownFields,
    /// The value that identifies this structure in a tagged union.
    pub tag: String,
    /// The key that holds the tag, when the structure declares one: present in the input, it
    /// must hold `tag`.
    pub tag_field: Option<String>,
}

/// One field of a structure.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDescription {
    /// The Python attribute name.
    pub name: String,
    /// The key in the input.
    pub wire_name: String,
    /// Other keys the field is also read from.
    pub aliases: Vec<String>,
    pub value_type: TypeDescription,
    /// Whether a missing key takes a default instead of being an error.
    pub has_default: bool,
}

/// A complete description: the root type and every structure it reaches.
#[derive(Debug, Clone, PartialEq)]
pub struct Description {
    pub root: TypeDescription,
    pub structs: Vec<StructDescription>,
}
