"""How to write the fields of a dataclass, read once per class by the encoder."""

import dataclasses
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

from oxiform._introspection.fields import checked_options, default_of, wire_name
from oxiform._introspection.model import Default
from oxiform._model_options import model_options

__all__ = ["EncodedField", "encoded_fields"]


@dataclass(frozen=True, slots=True)
class EncodedField:
    name: str
    wire_name: str
    flatten: bool
    """Write the fields of the value at this level instead of under a key."""
    omit_if_none: bool
    omit_if_default: Default
    """Leave the field out when it equals this default."""
    omit_if: Callable[[Any], object] | None
    """Leave the field out when this returns true for the value."""


def encoded_fields(cls: type) -> tuple[EncodedField, ...]:
    """The fields of the dataclass ``cls`` that are written, in declaration order."""
    model = model_options(cls)
    encoded: list[EncodedField] = []
    for field in dataclasses.fields(cls):
        options = checked_options(field, f"{cls.__qualname__}.{field.name}")
        if options.skip_encode:
            continue
        default = default_of(field)
        omit_if_default = options.skip_if_default or model.skip_if_default
        encoded.append(
            EncodedField(
                name=field.name,
                wire_name=wire_name(field.name, options, model),
                flatten=options.flatten,
                omit_if_none=options.skip_if_none or model.skip_if_none,
                omit_if_default=default if omit_if_default else None,
                omit_if=options.skip_if,
            )
        )
    return tuple(encoded)
