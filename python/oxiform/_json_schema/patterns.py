"""The text forms that lax mode reads and that JSON Schema has no ``format`` for, as the
``pattern`` of a string. They follow ECMA 262 (what JSON Schema asks of a ``pattern``): no
inline flags, so a word in any case is written with a class for each letter."""

__all__ = [
    "BOOL_TEXT",
    "FLOAT_TEXT",
    "INTEGER_TEXT",
    "NAIVE_DATETIME_TEXT",
    "TIME_TEXT",
]

INTEGER_TEXT = r"^[+-]?[0-9]+(\.0+)?$"
"""A whole number as text, with an optional fraction of zeros (section 4.5)."""

FLOAT_TEXT = r"^[+-]?([0-9]+(\.[0-9]*)?|\.[0-9]+)([eE][+-]?[0-9]+)?$"
"""A sign, digits with an optional fraction and an optional exponent."""

NAIVE_DATETIME_TEXT = r"^[0-9]{4}-[0-9]{2}-[0-9]{2}[T ][0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]+)?$"
"""A date and a time without an offset, which no JSON Schema ``format`` describes."""

TIME_TEXT = r"^[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]+)?(Z|[+-][0-9]{2}:[0-9]{2})?$"
"""A time with an optional offset. The ``time`` format requires one."""


def _any_case(word: str) -> str:
    return "".join(f"[{c.upper()}{c.lower()}]" if c.isalpha() else c for c in word)


BOOL_WORDS = ("true", "yes", "on", "1", "false", "no", "off", "0")

BOOL_TEXT = "^(" + "|".join(_any_case(word) for word in BOOL_WORDS) + ")$"
"""The words lax mode reads as a ``bool``, in any case."""
