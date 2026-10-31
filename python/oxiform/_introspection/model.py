"""The shape of a type description, as handed to the Rust compiler.

Types are tuples whose first item is the kind, so the Rust side reads them without attribute
lookups:

- ``("any",)``, ``("bool",)``, ``("none",)``
- ``("int", constraints)``, ``("float", constraints)``, ``("str", constraints)``
- ``("list", items, constraints)``, ``("dict", values, constraints)``
- ``("optional", inner)``
- ``("struct", index)``, an index into ``Description.structs``
"""

from collections.abc import Callable
from dataclasses import dataclass
from typing import Literal, TypeAlias

__all__ = [
    "ConstraintValues",
    "Default",
    "Description",
    "FieldDescription",
    "Layout",
    "Member",
    "StructDescription",
    "TypeDescription",
]

ConstraintValues: TypeAlias = dict[str, object]
"""Constraint values by name: ``minimum``, ``maximum``, ``min_length``, ``max_length``,
``pattern``, ``one_of``."""

TypeDescription: TypeAlias = (
    tuple[Literal["any", "bool", "none"]]
    | tuple[Literal["int", "float", "str"], ConstraintValues]
    | tuple[Literal["list", "dict"], "TypeDescription", ConstraintValues]
    | tuple[Literal["optional"], "TypeDescription"]
    | tuple[Literal["struct"], int]
)

Default: TypeAlias = (
    tuple[Literal["value"], object] | tuple[Literal["factory"], Callable[[], object]] | None
)
"""What a missing field takes: a shared value, the result of a factory, or nothing (the field
is required)."""


@dataclass(frozen=True, slots=True)
class FieldDescription:
    """A field read from a key of the input."""

    name: str
    wire_name: str
    aliases: tuple[str, ...]
    value_type: TypeDescription
    default: Default


Member: TypeAlias = (
    tuple[Literal["field"], str, int]
    | tuple[Literal["default"], str, Default]
    | tuple[Literal["flatten"], str, "Layout"]
)
"""How one attribute of an instance gets its value: from the field at an index of
``StructDescription.fields``, from its default (a field that is not read), or from an instance
of a flattened model built from fields of the same level."""


@dataclass(frozen=True, slots=True)
class Layout:
    """How to build an instance of ``cls`` from the fields read."""

    cls: type
    members: tuple[Member, ...]
    has_post_init: bool


@dataclass(frozen=True, slots=True)
class StructDescription:
    name: str
    cls: type
    fields: tuple[FieldDescription, ...]
    """The keys read, flattened models included, in declaration order."""
    layout: Layout
    unknown_fields: Literal["ignore", "forbid"]


@dataclass(frozen=True, slots=True)
class Description:
    root: TypeDescription
    structs: tuple[StructDescription, ...]
