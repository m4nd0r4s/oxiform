"""Constraints declared with ``Annotated``: ``Annotated[int, ox.Ge(0)]``."""

from dataclasses import dataclass
from typing import cast

__all__ = ["Ge", "Le", "Len", "OneOf", "Pattern"]


def _exact(value: object) -> tuple[type, object]:
    """``value`` with its type, so that ``1``, ``1.0`` and ``True`` stay different.

    Floats go by their ``repr``, which also tells ``0.0`` from ``-0.0`` and makes ``nan`` equal
    to itself.
    """
    return (float, repr(value)) if type(value) is float else (type(value), value)


class _Constraint:
    """Equality and hashing that include the type of every value.

    The dataclass default compares the fields as a tuple, where ``Ge(1) == Ge(True)``. ``typing``
    caches ``Annotated[int, Ge(1)]`` by equality of its metadata, so ``Annotated[int, Ge(True)]``
    would get the alias of the first (and with it the plan compiled for it, and no error for the
    ``bool`` limit). The cache keys of oxiform itself have the same need.
    """

    __slots__ = ()

    def _identity(self) -> tuple[object, ...]:
        raise NotImplementedError

    def __eq__(self, other: object) -> bool:
        if type(other) is not type(self):
            return NotImplemented
        return self._identity() == cast("_Constraint", other)._identity()

    def __hash__(self) -> int:
        return hash((type(self), self._identity()))


@dataclass(frozen=True, slots=True, eq=False)
class Ge(_Constraint):
    """The value is greater than or equal to ``limit``."""

    limit: int | float

    def _identity(self) -> tuple[object, ...]:
        return (_exact(self.limit),)


@dataclass(frozen=True, slots=True, eq=False)
class Le(_Constraint):
    """The value is less than or equal to ``limit``."""

    limit: int | float

    def _identity(self) -> tuple[object, ...]:
        return (_exact(self.limit),)


@dataclass(frozen=True, slots=True, eq=False)
class Len(_Constraint):
    """The number of characters or items is between ``min`` and ``max``, inclusive."""

    min: int = 0
    max: int | None = None

    def _identity(self) -> tuple[object, ...]:
        return (_exact(self.min), _exact(self.max))


@dataclass(frozen=True, slots=True, eq=False)
class Pattern(_Constraint):
    """The text contains a match of ``regex`` (anchor with ``^...$`` for the whole text).

    The syntax is that of the Rust ``regex`` crate: no lookaround and no backreferences, so
    matching never backtracks.
    """

    regex: str

    def _identity(self) -> tuple[object, ...]:
        return (_exact(self.regex),)


@dataclass(frozen=True, slots=True, init=False, eq=False)
class OneOf(_Constraint):
    """The value is one of ``values``."""

    values: tuple[int | str, ...]

    def __init__(self, *values: int | str) -> None:
        object.__setattr__(self, "values", values)

    def _identity(self) -> tuple[object, ...]:
        return tuple(_exact(value) for value in self.values)
