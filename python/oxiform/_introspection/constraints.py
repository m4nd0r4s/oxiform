"""Collects the oxiform constraints of an ``Annotated`` type into constraint values."""

from collections.abc import Iterable

from oxiform._introspection.model import ConstraintValues
from oxiform.constraints import Ge, Le, Len, OneOf, Pattern
from oxiform.errors import SchemaError

__all__ = ["collect_constraints"]

_INT64_RANGE = range(-(2**63), 2**63)


def collect_constraints(metadata: Iterable[object], location: str) -> ConstraintValues:
    """Constraint values from ``Annotated`` metadata. Objects that are not oxiform constraints
    belong to other libraries and are ignored."""
    values: ConstraintValues = {}
    for item in metadata:
        match item:
            case Ge(limit):
                _set(values, "minimum", _bound(limit, "Ge", location), "Ge", location)
            case Le(limit):
                _set(values, "maximum", _bound(limit, "Le", location), "Le", location)
            case Len(minimum, maximum):
                _check_length(minimum, maximum, location)
                if minimum > 0:
                    _set(values, "min_length", minimum, "Len", location)
                if maximum is not None:
                    _set(values, "max_length", maximum, "Len", location)
            case Pattern(regex):
                if not isinstance(regex, str):  # pyright: ignore[reportUnnecessaryIsInstance]
                    raise SchemaError(f"{location}: Pattern takes a str, not {regex!r}")
                _set(values, "pattern", regex, "Pattern", location)
            case OneOf(choices):
                _set(values, "one_of", _literals(choices, location), "OneOf", location)
            case _:
                pass
    return values


def _set(values: ConstraintValues, key: str, value: object, name: str, location: str) -> None:
    if key in values:
        raise SchemaError(f"{location}: {name} is declared twice")
    values[key] = value


def _bound(limit: object, name: str, location: str) -> int | float:
    if isinstance(limit, bool) or not isinstance(limit, int | float):
        raise SchemaError(f"{location}: {name} takes an int or a float, not {limit!r}")
    if isinstance(limit, int) and limit not in _INT64_RANGE:
        raise SchemaError(f"{location}: {name} limit {limit} does not fit in 64 bits")
    return limit


def _check_length(minimum: object, maximum: object, location: str) -> None:
    for value in (minimum, maximum):
        if value is None:
            continue
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise SchemaError(f"{location}: Len takes non-negative ints, not {value!r}")


def _literals(choices: tuple[object, ...], location: str) -> list[int | str]:
    literals: list[int | str] = []
    for choice in choices:
        if isinstance(choice, bool) or not isinstance(choice, int | str):
            raise SchemaError(f"{location}: OneOf takes ints or strs, not {choice!r}")
        if isinstance(choice, int) and choice not in _INT64_RANGE:
            raise SchemaError(f"{location}: OneOf value {choice} does not fit in 64 bits")
        literals.append(choice)
    return literals
