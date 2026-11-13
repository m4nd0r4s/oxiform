"""Turns Python types into the description the Rust compiler reads."""

from oxiform._introspection.declared import (
    DeclaredDescription,
    DeclaredField,
    DeclaredStruct,
    declared_description,
)
from oxiform._introspection.describe import UnresolvedAnnotationError, describe
from oxiform._introspection.encoding import EncodedField, encoded_fields
from oxiform._introspection.model import (
    ConstraintValues,
    Default,
    Description,
    FieldDescription,
    Layout,
    Member,
    StructDescription,
    TypeDescription,
)

__all__ = [
    "DeclaredDescription",
    "DeclaredField",
    "DeclaredStruct",
    "ConstraintValues",
    "Default",
    "Description",
    "EncodedField",
    "FieldDescription",
    "Layout",
    "Member",
    "StructDescription",
    "TypeDescription",
    "UnresolvedAnnotationError",
    "declared_description",
    "describe",
    "encoded_fields",
]
