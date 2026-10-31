"""How to write the fields of a dataclass, read once per class by the encoder."""

import dataclasses
import typing
from collections.abc import Callable
from dataclasses import dataclass
from typing import Annotated, Any, Literal, TypeAlias

from oxiform._introspection.fields import checked_options, default_of, tag_of, wire_name
from oxiform._introspection.model import Default
from oxiform._introspection.unions import is_union, union_mode, without_none
from oxiform._model_options import model_options
from oxiform.unions import External, Tagged

__all__ = ["EncodeHint", "EncodedField", "encoded_fields"]

EncodeHint: TypeAlias = (
    tuple[
        Literal["tagged"],
        Literal["internal", "adjacent", "external"],
        str | None,
        str | None,
        dict[type, str],
    ]
    | tuple[Literal["list", "dict"], "EncodeHint"]
)
"""What the declared type of a field adds to its runtime value: the tag of a tagged union,
by class, or a hint for the items of a list or the values of a dict."""


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
    constant: str | None = None
    """A fixed value written instead of an attribute: the model's tag."""
    hint: EncodeHint | None = None


def encoded_fields(cls: type) -> tuple[EncodedField, ...]:
    """The fields of the dataclass ``cls`` that are written, in declaration order, after its
    tag when the model declares a ``tag_field``."""
    model = model_options(cls)
    encoded: list[EncodedField] = []
    if model.tag_field is not None:
        encoded.append(
            EncodedField("", model.tag_field, False, False, None, None, constant=tag_of(cls))
        )
    hints = _type_hints(cls)
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
                hint=encode_hint(hints.get(field.name)),
            )
        )
    return tuple(encoded)


def encode_hint(annotation: object) -> EncodeHint | None:
    """The hint for a value declared as ``annotation``, or ``None`` when its runtime type says
    everything."""
    if typing.get_origin(annotation) is Annotated:
        inner, *metadata = typing.get_args(annotation)
        mode = union_mode(metadata, "")
        if isinstance(mode, Tagged | External) and is_union(inner):
            return _tagged_hint(mode, without_none(typing.get_args(inner)))
        return encode_hint(inner)
    if is_union(annotation):
        for argument in typing.get_args(annotation):
            hint = encode_hint(argument)
            if hint is not None:
                return hint
        return None
    origin = typing.get_origin(annotation)
    arguments = typing.get_args(annotation)
    if origin is list and arguments:
        inner_hint = encode_hint(arguments[0])
        return None if inner_hint is None else ("list", inner_hint)
    if origin is dict and len(arguments) == 2:
        inner_hint = encode_hint(arguments[1])
        return None if inner_hint is None else ("dict", inner_hint)
    return None


def _tagged_hint(mode: Tagged | External, variants: list[object]) -> EncodeHint | None:
    tags = {
        variant: tag_of(variant)
        for variant in variants
        if isinstance(variant, type) and dataclasses.is_dataclass(variant)
    }
    if isinstance(mode, External):
        return ("tagged", "external", None, None, tags)
    if mode.content is not None:
        return ("tagged", "adjacent", mode.field, mode.content, tags)
    # Models that declare this tag field already write it.
    tags = {
        variant: tag
        for variant, tag in tags.items()
        if model_options(variant).tag_field != mode.field
    }
    return ("tagged", "internal", mode.field, None, tags) if tags else None


def _type_hints(cls: type) -> dict[str, object]:
    try:
        return typing.get_type_hints(cls, include_extras=True)
    except NameError:
        return {}
