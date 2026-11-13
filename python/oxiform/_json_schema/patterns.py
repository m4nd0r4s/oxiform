"""The text forms that lax mode reads and that JSON Schema has no ``format`` for, as the
``pattern`` of a string. They follow ECMA 262 (what JSON Schema asks of a ``pattern``): no
inline flags, so a word in any case is written with a class for each letter."""

__all__ = [
    "BASE64_TEXT",
    "BOOL_TEXT",
    "DECIMAL_TEXT",
    "FLOAT_TEXT",
    "INTEGER_TEXT",
    "NAIVE_DATETIME_TEXT",
    "TIME_TEXT",
    "bool_text",
    "integer_text",
    "texts_of",
]

INTEGER_TEXT = r"^[+-]?[0-9]+(\.0+)?$"
"""A whole number as text, with an optional fraction of zeros (section 4.5)."""

FLOAT_TEXT = r"^[+-]?([0-9]+(\.[0-9]*)?|\.[0-9]+)([eE][+-]?[0-9]+)?$"
"""A sign, digits with an optional fraction and an optional exponent."""

NAIVE_DATETIME_TEXT = r"^[0-9]{4}-[0-9]{2}-[0-9]{2}[Tt ][0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]+)?$"
"""A date and a time without an offset, which no JSON Schema ``format`` describes."""

TIME_TEXT = r"^[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]+)?(Z|[+-][0-9]{2}:[0-9]{2})?$"
"""A time with an optional offset. The ``time`` format requires one."""


DECIMAL_TEXT = (
    r"^[+-]?(([0-9]+(\.[0-9]*)?|\.[0-9]+)([eE][+-]?[0-9]+)?"
    r"|[Ii][Nn][Ff]([Ii][Nn][Ii][Tt][Yy])?|[sS]?[Nn][Aa][Nn][0-9]*)$"
)
"""A decimal as Python's ``Decimal`` reads it: digits, ``Infinity`` and ``NaN`` included."""

BASE64_TEXT = r"^([A-Za-z0-9+/]{4})*([A-Za-z0-9+/][AQgw]==|[A-Za-z0-9+/]{2}[AEIMQUYcgkosw048]=)?$"
"""Padded standard base64 whose unused trailing bits are zero, the only form that is read."""


def integer_text(value: int) -> str:
    """Text that lax mode reads as the integer ``value`` (the body of a pattern): an optional
    sign where it fits, leading zeros and a fraction of zeros."""
    if value == 0:
        return r"[+-]?0+(\.0+)?"
    sign = r"\+?" if value > 0 else "-"
    return sign + "0*" + str(abs(value)) + r"(\.0+)?"


def bool_text(value: bool) -> str:
    """The words that lax mode reads as ``value``, in any case (the body of a pattern)."""
    words = ("true", "yes", "on", "1") if value else ("false", "no", "off", "0")
    return "|".join(_any_case(word) for word in words)


def texts_of(values: "tuple[object, ...]") -> str | None:
    """A pattern for the text that lax mode reads as one of ``values``, ``None`` when no value
    is a number or a bool."""
    bodies: list[str] = []
    for value in values:
        if isinstance(value, bool):
            bodies.append(bool_text(value))
        elif isinstance(value, int):
            bodies.append(integer_text(value))
    return "^(?:" + "|".join(bodies) + ")$" if bodies else None


def _any_case(word: str) -> str:
    return "".join(f"[{c.upper()}{c.lower()}]" if c.isalpha() else c for c in word)


BOOL_WORDS = ("true", "yes", "on", "1", "false", "no", "off", "0")

BOOL_TEXT = "^(" + "|".join(_any_case(word) for word in BOOL_WORDS) + ")$"
"""The words lax mode reads as a ``bool``, in any case."""
