"""``ox.field``: a dataclass field with the oxiform attributes of section 5.2."""

import dataclasses
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

from oxiform.constraints import Ge, Le, Len, Pattern

__all__ = ["METADATA_KEY", "FieldOptions", "field", "options_of"]

METADATA_KEY = "oxiform"
"""The key of the field options in ``dataclasses.Field.metadata``."""


@dataclass(frozen=True, slots=True)
class FieldOptions:
    """What ``ox.field`` adds to a dataclass field."""

    rename: str | None = None
    aliases: tuple[str, ...] = ()
    skip_encode: bool = False
    skip_decode: bool = False
    skip_if: Callable[[Any], object] | None = None
    skip_if_default: bool = False
    skip_if_none: bool = False
    flatten: bool = False
    constraints: tuple[object, ...] = ()
    """Constraints from the shortcuts (``ge=0`` is ``Ge(0)``), applied like ``Annotated``."""


_NO_OPTIONS = FieldOptions()


def options_of(field: dataclasses.Field[Any]) -> FieldOptions:
    """The oxiform options of a dataclass field, empty when it was not made by ``ox.field``."""
    options = field.metadata.get(METADATA_KEY, _NO_OPTIONS)
    return options if isinstance(options, FieldOptions) else _NO_OPTIONS


def field(
    *,
    default: Any = dataclasses.MISSING,
    default_factory: Any = dataclasses.MISSING,
    rename: str | None = None,
    aliases: tuple[str, ...] | list[str] = (),
    skip: bool = False,
    skip_encode: bool = False,
    skip_decode: bool = False,
    skip_if: Callable[[Any], object] | None = None,
    skip_if_default: bool = False,
    skip_if_none: bool = False,
    flatten: bool = False,
    ge: int | float | None = None,
    le: int | float | None = None,
    min_len: int | None = None,
    max_len: int | None = None,
    pattern: str | None = None,
) -> Any:
    """A model field with a default, a wire name and encode and decode rules.

    - ``rename`` is the key in the input and output; ``aliases`` are other keys accepted on
      read only.
    - ``skip`` leaves the field out of both directions, ``skip_encode`` and ``skip_decode`` out
      of one. A field skipped on read needs a default.
    - ``skip_if``, ``skip_if_default`` and ``skip_if_none`` leave the field out of the output
      when the predicate holds, when it equals its default, or when it is ``None``.
    - ``flatten`` writes and reads the fields of a nested model at the same level.
    - ``ge``, ``le``, ``min_len``, ``max_len`` and ``pattern`` are shortcuts for ``Ge``,
      ``Le``, ``Len`` and ``Pattern``.
    """
    constraints: list[object] = []
    if ge is not None:
        constraints.append(Ge(ge))
    if le is not None:
        constraints.append(Le(le))
    if min_len is not None or max_len is not None:
        constraints.append(Len(min_len or 0, max_len))
    if pattern is not None:
        constraints.append(Pattern(pattern))
    options = FieldOptions(
        rename=rename,
        aliases=tuple(aliases),
        skip_encode=skip or skip_encode,
        skip_decode=skip or skip_decode,
        skip_if=skip_if,
        skip_if_default=skip_if_default,
        skip_if_none=skip_if_none,
        flatten=flatten,
        constraints=tuple(constraints),
    )
    return dataclasses.field(
        default=default,
        default_factory=default_factory,
        metadata={METADATA_KEY: options},
    )
