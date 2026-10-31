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
    name: str
    wire_name: str
    value_type: TypeDescription
    default: Default


@dataclass(frozen=True, slots=True)
class StructDescription:
    name: str
    cls: type
    fields: tuple[FieldDescription, ...]
    unknown_fields: Literal["ignore", "forbid"]
    has_post_init: bool


@dataclass(frozen=True, slots=True)
class Description:
    root: TypeDescription
    structs: tuple[StructDescription, ...]
