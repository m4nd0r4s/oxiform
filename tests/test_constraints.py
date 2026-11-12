"""Constraints compare and hash with the type of their values (porto-309.18)."""

from typing import Annotated, Literal

import pytest

import oxiform as ox
from oxiform._introspection import describe


def test_constraints_that_differ_only_in_value_type_are_different() -> None:
    assert ox.Ge(1) != ox.Ge(True)
    assert ox.Ge(1) != ox.Ge(1.0)
    assert ox.Le(1) != ox.Le(True)
    assert ox.Len(1, 1) != ox.Len(True, 1.0)  # pyright: ignore[reportArgumentType]
    assert ox.Len(0, None) != ox.Len(False, None)
    assert ox.OneOf(1, "a") != ox.OneOf(True, "a")
    assert ox.Pattern("a") != ox.Pattern("b")
    assert ox.Ge(1) != ox.Le(1)
    assert len({ox.Ge(1), ox.Ge(True), ox.Ge(1.0)}) == 3


def test_equal_constraints_stay_equal_and_hash_alike() -> None:
    for first, second in [
        (ox.Ge(1), ox.Ge(1)),
        (ox.Le(2.5), ox.Le(2.5)),
        (ox.Len(1, 3), ox.Len(1, 3)),
        (ox.OneOf(1, "a"), ox.OneOf(1, "a")),
        (ox.Pattern("^a$"), ox.Pattern("^a$")),
        (ox.Ge(float("nan")), ox.Ge(float("nan"))),
    ]:
        assert first == second
        assert hash(first) == hash(second)


def test_zero_and_negative_zero_are_different_limits() -> None:
    assert ox.Ge(0.0) != ox.Ge(-0.0)


def test_constraints_still_match_by_position() -> None:
    match ox.Len(2, 5):
        case ox.Len(minimum, maximum):
            assert (minimum, maximum) == (2, 5)
        case _:
            pytest.fail("Len did not match")


def test_annotated_aliases_are_not_shared_between_limits_of_different_types() -> None:
    # Every pair uses limits no other test has used, so typing's cache starts cold for it.
    int_first = Annotated[int, ox.Ge(9001)]
    bool_like = Annotated[int, ox.Ge(9001.0)]
    assert int_first is not bool_like
    float_first = Annotated[int, ox.Le(9002.0)]
    int_second = Annotated[int, ox.Le(9002)]
    assert float_first is not int_second


def test_a_bool_limit_is_an_error_in_either_order() -> None:
    """``Ge(True)`` once got the alias, and the compiled plan, of ``Ge(1)`` and raised nothing."""
    valid_first = Annotated[int, ox.Ge(7001)]
    ox.json.decode(b"7001", valid_first)
    with pytest.raises(ox.SchemaError, match="Ge takes an int or a float, not True"):
        ox.json.decode(b"1", Annotated[int, ox.Ge(True)])
    with pytest.raises(ox.SchemaError, match="Ge takes an int or a float, not True"):
        describe(Annotated[int, ox.Ge(True)])

    # The bool alias first, now with a limit that was not seen before.
    with pytest.raises(ox.SchemaError, match="Le takes an int or a float, not True"):
        ox.json.decode(b"1", Annotated[int, ox.Le(True)])
    assert ox.json.decode(b"1", Annotated[int, ox.Le(1)]) == 1
    with pytest.raises(ox.SchemaError, match="Le takes an int or a float, not True"):
        ox.json.decode(b"1", Annotated[int, ox.Le(True)])


def test_plans_are_cached_per_limit_type() -> None:
    as_int = Annotated[float, ox.Ge(11)]
    as_float = Annotated[float, ox.Ge(11.0)]
    with pytest.raises(ox.ValidationError) as first:
        ox.json.decode(b"1", as_int)
    with pytest.raises(ox.ValidationError) as second:
        ox.json.decode(b"1", as_float)
    assert "11" in str(first.value) and "11.0" not in str(first.value)
    assert "11.0" in str(second.value)
    # The other order, with other limits.
    as_float = Annotated[float, ox.Le(-12.0)]
    as_int = Annotated[float, ox.Le(-12)]
    with pytest.raises(ox.ValidationError) as third:
        ox.json.decode(b"1", as_float)
    with pytest.raises(ox.ValidationError) as fourth:
        ox.json.decode(b"1", as_int)
    assert "-12.0" in str(third.value)
    assert "-12.0" not in str(fourth.value) and "-12" in str(fourth.value)


def test_literals_that_compare_equal_keep_their_own_choices() -> None:
    from dataclasses import dataclass

    @dataclass
    class Flags:
        number: Literal[1]
        flag: Literal[True]

    assert ox.json.decode(b'{"number": 1, "flag": true}', Flags) == Flags(1, True)
    for wrong in (b'{"number": true, "flag": true}', b'{"number": 1, "flag": 1}'):
        with pytest.raises(ox.ValidationError):
            ox.json.decode(wrong, Flags)
