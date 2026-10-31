"""What every direction needs from a dataclass field: its options, wire name and default."""

import dataclasses
from typing import Any

from oxiform._fields import FieldOptions, options_of
from oxiform._introspection.model import Default
from oxiform._model_options import OPTIONS_ATTRIBUTE, ModelOptions, model_options
from oxiform._naming import rename
from oxiform.errors import SchemaError

__all__ = ["checked_options", "default_of", "tag_of", "wire_name"]


def checked_options(field: dataclasses.Field[Any], location: str) -> FieldOptions:
    """The oxiform options of ``field``, refusing combinations that cannot work."""
    options = options_of(field)
    has_default = default_of(field) is not None
    if options.skip_decode and not has_default:
        raise SchemaError(f"{location}: a field skipped on read needs a default")
    if options.skip_if_default and not has_default:
        raise SchemaError(f"{location}: skip_if_default needs a default")
    if options.skip_if is not None and not callable(options.skip_if):
        raise SchemaError(f"{location}: skip_if takes a callable, not {options.skip_if!r}")
    for alias in options.aliases:
        if not isinstance(alias, str):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise SchemaError(f"{location}: aliases are strs, not {alias!r}")
    if options.rename is not None and not isinstance(options.rename, str):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise SchemaError(f"{location}: rename takes a str, not {options.rename!r}")
    if options.flatten:
        if options.rename is not None or options.aliases:
            raise SchemaError(f"{location}: a flattened field has no key of its own")
        if options.skip_encode or options.skip_decode or options.constraints:
            raise SchemaError(f"{location}: a flattened field cannot be skipped or constrained")
    return options


def wire_name(name: str, options: FieldOptions, model: ModelOptions) -> str:
    """The key of a field: ``rename`` wins over the model's ``rename_all``."""
    if options.rename is not None:
        return options.rename
    if model.rename_all is not None:
        return rename(name, model.rename_all)
    return name


def default_of(field: dataclasses.Field[Any]) -> Default:
    if field.default is not dataclasses.MISSING:
        return ("value", field.default)
    if field.default_factory is not dataclasses.MISSING:
        return ("factory", field.default_factory)
    return None


def tag_of(cls: type) -> str:
    """The tag of ``cls`` in a tagged union: its own ``tag``, or the class name written with
    the model's ``rename_all``. A tag is never inherited."""
    own = cls.__dict__.get(OPTIONS_ATTRIBUTE)
    if isinstance(own, ModelOptions) and own.tag is not None:
        return own.tag
    rule = model_options(cls).rename_all
    return rename(cls.__name__, rule) if rule is not None else cls.__name__
