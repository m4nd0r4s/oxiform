"""``ox.Secret[T]``: a value that hides itself (spec section 6.6).

The engine reads ``Secret[T]`` as ``T`` and wraps the result in this class, and writes it as
``MASK`` in every format. Nothing but ``reveal()`` gives the value back.
"""

import hmac
from typing import Any, Final, Generic, NoReturn, TypeVar, cast

__all__ = ["MASK", "Secret"]

_T = TypeVar("_T")

MASK: Final = "********"
"""What a secret shows and what encode writes: one fixed text, so it says nothing of the value."""


class Secret(Generic[_T]):
    """A value that does not show itself in ``repr``, ``str``, logs, errors or encoded output.

    ``Secret(value).reveal()`` is the only way to read the value back. Two secrets are equal
    when their values are, a secret never equals a bare value, and ``bool(secret)`` is whether
    the value is truthy (an empty secret is false and nothing more about it is shown).
    """

    __slots__ = ("_value",)

    _value: _T

    def __init__(self, value: _T) -> None:
        if isinstance(value, Secret):
            raise TypeError(
                "a Secret cannot hold a Secret: it already hides its value; "
                "pass the plain value, or use the Secret you have"
            )
        object.__setattr__(self, "_value", value)

    def reveal(self) -> _T:
        """The real value. Call it at the moment of use, and never log what it returns."""
        return self._value

    def __setattr__(self, name: str, value: object) -> NoReturn:
        raise AttributeError("a Secret is immutable; make a new one with ox.Secret(value)")

    def __delattr__(self, name: str) -> NoReturn:
        raise AttributeError("a Secret is immutable; make a new one with ox.Secret(value)")

    def __repr__(self) -> str:
        return f"Secret({MASK!r})"

    def __str__(self) -> str:
        return MASK

    def __format__(self, format_spec: str) -> str:
        return MASK

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, Secret):
            return NotImplemented
        mine: object = self._value
        theirs: object = cast("Secret[object]", other)._value
        if isinstance(mine, str) and isinstance(theirs, str):
            return hmac.compare_digest(mine.encode(), theirs.encode())
        if isinstance(mine, bytes) and isinstance(theirs, bytes):
            return hmac.compare_digest(mine, theirs)
        return bool(mine == theirs)

    def __hash__(self) -> int:
        return hash((Secret, self._value))

    def __bool__(self) -> bool:
        return bool(self._value)

    def __reduce__(self) -> tuple[Any, tuple[Any, ...]]:
        # Handed to worker processes, a config must keep its value: pickling is not a log.
        return (Secret, (self._value,))

    def __copy__(self) -> "Secret[_T]":
        return self

    def __deepcopy__(self, memo: dict[int, Any]) -> "Secret[_T]":
        return Secret(self._value)
