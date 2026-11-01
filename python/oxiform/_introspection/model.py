"""The shape of a type description, as handed to the Rust compiler.

Types are tuples whose first item is the kind, so the Rust side reads them without attribute
lookups:

- ``("any",)``, ``("bool",)``, ``("none",)``
- ``("int", constraints)``, ``("float", constraints)``, ``("str", constraints)``
- ``("bytes", mutable, constraints)``: ``bytes``, or ``bytearray`` when ``mutable``
- ``("converted", index)``, an index into ``Description.conversions``
- ``("list", items, constraints)``, ``("dict", values, constraints)``
- ``("collection", kind, items, constraints)``: a ``"tuple"`` of any length, a ``"set"`` or a
  ``"frozenset"``
- ``("tuple", items)``, one type per position
- ``("choice", index)``, an index into ``Description.choices``
- ``("optional", inner)``
- ``("struct", index)``, an index into ``Description.structs``
- ``("union", variants)``, chosen by the kind of value
- ``("tagged", mode, tag, content, variants)``, chosen by a tag; ``mode`` is ``"internal"``,
  ``"adjacent"`` (with ``content``) or ``"external"`` (without ``tag``)
- ``("untagged", variants)``, trying each in order
"""

from collections.abc import Callable
from dataclasses import dataclass
from typing import Any, Literal, TypeAlias

__all__ = [
    "ChoiceDescription",
    "ConstraintValues",
    "ConversionDescription",
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
    | tuple[Literal["bytes"], bool, ConstraintValues]
    | tuple[Literal["converted"], int]
    | tuple[Literal["list", "dict"], "TypeDescription", ConstraintValues]
    | tuple[
        Literal["collection"],
        Literal["tuple", "set", "frozenset"],
        "TypeDescription",
        ConstraintValues,
    ]
    | tuple[Literal["tuple"], tuple["TypeDescription", ...]]
    | tuple[Literal["choice"], int]
    | tuple[Literal["optional"], "TypeDescription"]
    | tuple[Literal["struct"], int]
    | tuple[Literal["union", "untagged"], tuple["TypeDescription", ...]]
    | tuple[
        Literal["tagged"],
        Literal["internal", "adjacent", "external"],
        str | None,
        str | None,
        tuple["TypeDescription", ...],
    ]
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
    tag: str
    """The value that identifies the structure in a tagged union."""
    tag_field: str | None
    """The key that holds the tag, when the model declares one."""


@dataclass(frozen=True, slots=True)
class ChoiceDescription:
    """The values a ``Literal`` or ``Enum`` allows."""

    name: str
    values: tuple[int | str | bool | None, ...]
    """Each value as it appears in the input."""
    objects: tuple[object, ...]
    """The Python object each value becomes: the literal itself or the enum member."""
    is_enum: bool


@dataclass(frozen=True, slots=True)
class ConversionDescription:
    """A type built by calling ``convert`` with the string or number read. ``ValueError`` or
    ``KeyError`` from the call rejects the value."""

    name: str
    format: str
    """The format reported when a value is rejected."""
    input: Literal["str", "int", "decimal"]
    """What is read: a string, an integer, or a number (or a string holding one) as text."""
    convert: Callable[[Any], object]


@dataclass(frozen=True, slots=True)
class Description:
    root: TypeDescription
    structs: tuple[StructDescription, ...]
    choices: tuple[ChoiceDescription, ...]
    conversions: tuple[ConversionDescription, ...]
