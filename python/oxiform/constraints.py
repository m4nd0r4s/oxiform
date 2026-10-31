"""Constraints declared with ``Annotated``: ``Annotated[int, ox.Ge(0)]``."""

from dataclasses import dataclass

__all__ = ["Ge", "Le", "Len", "OneOf", "Pattern"]


@dataclass(frozen=True, slots=True)
class Ge:
    """The value is greater than or equal to ``limit``."""

    limit: int | float


@dataclass(frozen=True, slots=True)
class Le:
    """The value is less than or equal to ``limit``."""

    limit: int | float


@dataclass(frozen=True, slots=True)
class Len:
    """The number of characters or items is between ``min`` and ``max``, inclusive."""

    min: int = 0
    max: int | None = None


@dataclass(frozen=True, slots=True)
class Pattern:
    """The text contains a match of ``regex`` (anchor with ``^...$`` for the whole text).

    The syntax is that of the Rust ``regex`` crate: no lookaround and no backreferences, so
    matching never backtracks.
    """

    regex: str


@dataclass(frozen=True, slots=True, init=False)
class OneOf:
    """The value is one of ``values``."""

    values: tuple[int | str, ...]

    def __init__(self, *values: int | str) -> None:
        object.__setattr__(self, "values", values)
