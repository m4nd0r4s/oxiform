"""Reads how a union is told apart from its ``Annotated`` metadata."""

import types
import typing
from collections.abc import Iterable
from typing import Union

from oxiform.errors import SchemaError
from oxiform.unions import External, Tagged, Untagged

__all__ = ["UnionMode", "is_union", "union_mode", "without_none"]

UnionMode = Tagged | External | Untagged


def union_mode(metadata: Iterable[object], location: str) -> UnionMode | None:
    """The ``ox.Tagged``, ``ox.External`` or ``ox.Untagged`` in ``metadata``, if any."""
    found: UnionMode | None = None
    for item in metadata:
        if isinstance(item, Tagged | External | Untagged):
            if found is not None:
                raise SchemaError(f"{location}: a union is told apart in one way only")
            found = item
    return found


def is_union(annotation: object) -> bool:
    origin = typing.get_origin(annotation)
    return origin is Union or origin is types.UnionType


def without_none(arguments: tuple[object, ...]) -> list[object]:
    return [argument for argument in arguments if argument not in (None, types.NoneType)]
