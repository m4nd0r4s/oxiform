"""``Literal`` and ``Enum``: the values allowed as they appear in the input, and the Python object
each one becomes."""

import enum
import typing

from oxiform._introspection.model import ChoiceDescription
from oxiform.errors import SchemaError

__all__ = ["enum_choice", "literal_choice"]

_INT64_RANGE = range(-(2**63), 2**63)


def literal_choice(annotation: object, location: str) -> ChoiceDescription:
    """The choice of a ``Literal``. Enum members are read from their value."""
    objects = typing.get_args(annotation)
    values = tuple(_wire_value(item, location) for item in objects)
    name = "Literal[" + ", ".join(repr(item) for item in objects) + "]"
    return ChoiceDescription(name=name, values=values, objects=objects, is_enum=False)


def enum_choice(cls: type[enum.Enum], location: str) -> ChoiceDescription:
    """The choice of an ``Enum``: its members, read from their values."""
    members = tuple(cls)
    values = tuple(_wire_value(member, location) for member in members)
    return ChoiceDescription(name=cls.__qualname__, values=values, objects=members, is_enum=True)


def _wire_value(item: object, location: str) -> int | str | bool | None:
    value = item.value if isinstance(item, enum.Enum) else item
    if value is None or isinstance(value, bool | str):
        return value
    if isinstance(value, int):
        if value not in _INT64_RANGE:
            raise SchemaError(f"{location}: the value {value} does not fit in 64 bits")
        return value
    raise SchemaError(f"{location}: only None, bool, int and str values can be read, not {item!r}")
