import datetime
import enum
import sys
from collections.abc import Iterator
from dataclasses import dataclass
from decimal import Decimal
from typing import Any, Literal
from uuid import UUID

import pytest

import oxiform as ox


class Perm(enum.IntFlag):
    READ = 1
    WRITE = 2


def codes(error: ox.ValidationError) -> list[str]:
    return [item["code"] for item in error.errors()]


def test_limits_default_to_the_values_of_the_spec() -> None:
    limits = ox.Limits()
    assert limits.max_bytes == 64 * 2**20
    assert limits.max_depth == 128
    assert limits.max_items == 1_000_000
    assert limits.max_str_len == 16 * 2**20
    assert limits.max_int_digits == 4300


def test_limits_are_values() -> None:
    assert ox.Limits(max_depth=4) == ox.Limits(max_depth=4)
    assert ox.Limits(max_depth=4) != ox.Limits()
    assert hash(ox.Limits(max_depth=4)) == hash(ox.Limits(max_depth=4))
    assert "max_depth=4" in repr(ox.Limits(max_depth=4))
    with pytest.raises(TypeError):
        ox.Limits(1)  # type: ignore[misc]
    with pytest.raises(AttributeError):
        ox.Limits().max_depth = 1  # type: ignore[misc]


def test_none_turns_every_size_limit_off_except_the_depth_ceiling() -> None:
    none = ox.Limits.none()
    assert none.max_bytes > 2**60
    assert none.max_items > 2**60
    assert none.max_str_len > 2**60
    assert none.max_int_digits > 2**60
    assert none.max_depth == ox.Limits.MAX_DEPTH
    deep = b"[" * 20 + b"]" * 20
    with pytest.raises(ox.DecodeError):
        ox.json.decode(deep, limits=ox.Limits(max_depth=10))
    assert ox.json.decode(deep, limits=none) is not None


@pytest.mark.parametrize("depth", [ox.Limits.MAX_DEPTH + 1, 20_000, 100_000, 1_000_000])
@pytest.mark.parametrize("opening", [b"[", b'{"a":'])
def test_deep_nesting_is_a_limit_error_and_never_a_crash(depth: int, opening: bytes) -> None:
    closing = b"]" if opening == b"[" else b"}"
    raw = opening * depth + b"1" + closing * depth
    for limits in (ox.Limits.none(), ox.Limits(max_depth=2**64 - 1), ox.Limits(max_depth=10**6)):
        with pytest.raises(ox.DecodeError) as caught:
            ox.json.decode(raw, limits=limits)
        error = caught.value.errors()[0]
        assert error["code"] == "limit_exceeded"
        assert error["ctx"]["limit"] == "max_depth"
        assert error["ctx"]["value"] == ox.Limits.MAX_DEPTH


def test_a_depth_above_the_ceiling_is_the_ceiling() -> None:
    assert ox.Limits(max_depth=10_000).max_depth == ox.Limits.MAX_DEPTH
    assert ox.Limits(max_depth=ox.Limits.MAX_DEPTH).max_depth == ox.Limits.MAX_DEPTH
    assert ox.Limits(max_depth=7).max_depth == 7
    assert ox.Limits(max_depth=10_000) == ox.Limits(max_depth=ox.Limits.MAX_DEPTH)


@pytest.mark.parametrize(
    "name", ["max_bytes", "max_depth", "max_items", "max_str_len", "max_int_digits"]
)
@pytest.mark.parametrize("value", [-1, 2**64, -(2**70)])
def test_a_size_out_of_range_says_what_is_allowed(name: str, value: int) -> None:
    with pytest.raises(ValueError, match=rf"{name} must be a whole number from 0 to") as caught:
        ox.Limits(**{name: value})
    assert "ox.Limits.none()" in str(caught.value)
    assert str(value) in str(caught.value)


def test_a_size_of_the_wrong_type_is_a_type_error() -> None:
    with pytest.raises(TypeError):
        ox.Limits(max_depth="deep")  # type: ignore[arg-type]
    with pytest.raises(TypeError):
        ox.Limits(max_items=1.5)  # type: ignore[arg-type]


@pytest.mark.parametrize(
    ("limits", "kind", "raw"),
    [
        (ox.Limits(max_bytes=5), int, b"1234567"),
        (ox.Limits(max_depth=2), Any, b"[[[1]]]"),
        (ox.Limits(max_items=2), list[int], b"[1, 2, 3]"),
        (ox.Limits(max_items=1), dict[str, int], b'{"a": 1, "b": 2}'),
        (ox.Limits(max_str_len=3), str, b'"abcd"'),
        (ox.Limits(max_int_digits=3), int, b"1234"),
    ],
)
def test_exceeding_a_limit_is_a_decode_error(limits: ox.Limits, kind: Any, raw: bytes) -> None:
    with pytest.raises(ox.DecodeError) as caught:
        ox.json.decode(raw, kind, limits=limits)
    assert codes(caught.value) == ["limit_exceeded"]
    assert caught.value.errors()[0]["ctx"]["limit"] in {
        "max_bytes",
        "max_depth",
        "max_items",
        "max_str_len",
        "max_int_digits",
    }


def test_the_limit_is_named_in_the_error() -> None:
    with pytest.raises(ox.DecodeError) as caught:
        ox.json.decode(b"[1, 2, 3]", list[int], limits=ox.Limits(max_items=2))
    assert caught.value.errors()[0]["ctx"]["limit"] == "max_items"


def test_limits_apply_to_the_text_form_of_input_too() -> None:
    with pytest.raises(ox.DecodeError):
        ox.json.decode('"abcd"', str, limits=ox.Limits(max_str_len=3))
    with pytest.raises(ox.DecodeError):
        ox.json.decode(bytearray(b"1234567"), int, limits=ox.Limits(max_bytes=5))


def test_max_errors_stops_collection() -> None:
    with pytest.raises(ox.ValidationError) as caught:
        ox.json.decode(b'["a", "b", "c"]', list[int], max_errors=2)
    assert len(caught.value.errors()) == 2
    assert caught.value.truncated


@pytest.fixture
def default_int_digits() -> Iterator[int]:
    previous = sys.get_int_max_str_digits()
    sys.set_int_max_str_digits(4300)
    yield 4300
    sys.set_int_max_str_digits(previous)


@pytest.mark.parametrize("mode", ["strict", "lax"])
def test_none_still_stops_at_the_digit_limit_of_python(mode: str, default_int_digits: int) -> None:
    digits = "1" * (default_int_digits + 1)
    raw = digits.encode() if mode == "strict" else f'"{digits}"'.encode()
    with pytest.raises(ox.DecodeError) as caught:
        ox.json.decode(raw, int, mode=mode, limits=ox.Limits.none())  # type: ignore[arg-type]
    error = caught.value.errors()[0]
    assert error["code"] == "limit_exceeded"
    assert error["ctx"] == {"limit": "max_int_digits", "value": default_int_digits}
    assert "sys.set_int_max_str_digits" in error["message"]
    assert ox.json.decode(
        raw[:-1] if mode == "strict" else raw[:-2] + b'"',
        int,
        mode=mode,  # type: ignore[arg-type]
        limits=ox.Limits.none(),
    ) == int("1" * default_int_digits)


def test_python_integers_that_are_converted_obey_the_same_limit(default_int_digits: int) -> None:
    with pytest.raises(ox.DecodeError):
        ox.json.decode(b"1" * (default_int_digits + 1), Perm, limits=ox.Limits.none())
    with pytest.raises(ox.DecodeError):
        ox.json.decode(b"[" + b"1" * (default_int_digits + 1) + b"]", Any, limits=ox.Limits.none())


def test_the_digit_limit_follows_what_python_is_set_to(default_int_digits: int) -> None:
    sys.set_int_max_str_digits(0)
    big = "9" * 10_000
    assert ox.json.decode(big.encode(), int, limits=ox.Limits.none()) == int(big)
    sys.set_int_max_str_digits(1000)
    with pytest.raises(ox.DecodeError) as caught:
        ox.json.decode(b"9" * 1001, int, limits=ox.Limits.none())
    assert caught.value.errors()[0]["ctx"]["value"] == 1000


def test_leading_zeros_are_not_digits(default_int_digits: int) -> None:
    limits = ox.Limits(max_int_digits=5)
    for text, value in [("000001", 1), ("-000012345", -12345), ("+00000", 0), ("0", 0)]:
        assert ox.json.decode(f'"{text}"'.encode(), int, mode="lax", limits=limits) == value
    with pytest.raises(ox.DecodeError):
        ox.json.decode(b'"123456"', int, mode="lax", limits=limits)
    with pytest.raises(ox.DecodeError):
        ox.json.decode(b'"-0123456"', int, mode="lax", limits=limits)
    # Python counts the zeros, so they are taken off before the number is built.
    padded = "0" * 10_000 + "7" * 30
    assert ox.json.decode(f'"{padded}"'.encode(), int, mode="lax") == int("7" * 30)
    assert ox.json.decode(f'"-{padded}"'.encode(), int, mode="lax") == -int("7" * 30)


@dataclass(slots=True)
class Small:
    n: int = 0


@pytest.mark.parametrize(
    ("kind", "raw", "mode"),
    [
        (str, b'"abcd"', "strict"),
        (int, b'"0000042"', "lax"),
        (int, b'"42.0000"', "lax"),
        (float, b'"1.2345"', "lax"),
        (bool, b'"false"', "lax"),
        (int | float | None, b'"1.2345"', "lax"),
        (Perm, b'"0001"', "lax"),
        (Literal["a", "b"], b'"abcd"', "strict"),
        (Literal[1, 2], b'"0001"', "lax"),
        (datetime.datetime, b'"2026-01-01T00:00:00Z"', "strict"),
        (datetime.date, b'"2026-01-01"', "strict"),
        (datetime.timedelta, b'"P1DT1H"', "strict"),
        (UUID, b'"a8098c1a-f86e-11da-bd1a-00112444be1e"', "strict"),
        (Decimal, b'"1.2345"', "strict"),
        (bytes, b'"QUJDREVGRw=="', "strict"),
        (list[str], b'["a", "abcd"]', "strict"),
        (dict[str, int], b'{"abcd": 1}', "strict"),
        (Small, b'{"abcd": 1}', "strict"),
        (Small, b'{"n": 1, "ignored": "a long string"}', "strict"),
        (Any, b'{"a": ["abcd"]}', "strict"),
        (int | Literal["auto"], b'"abcd"', "lax"),
    ],
)
def test_max_str_len_applies_to_every_string_that_is_read(kind: Any, raw: bytes, mode: str) -> None:
    limits = ox.Limits(max_str_len=3, max_int_digits=100)
    with pytest.raises(ox.DecodeError) as caught:
        ox.json.decode(raw, kind, mode=mode, limits=limits)  # type: ignore[arg-type]
    error = caught.value.errors()[0]
    assert error["code"] == "limit_exceeded"
    assert (error["ctx"]["limit"], error["ctx"]["value"]) == ("max_str_len", 3)


def test_max_str_len_allows_a_string_of_exactly_that_length() -> None:
    limits = ox.Limits(max_str_len=3)
    assert ox.json.decode(b'"abc"', str, limits=limits) == "abc"
    assert ox.json.decode(b'"042"', int, mode="lax", limits=limits) == 42
    assert ox.json.decode(b'"1.5"', float, mode="lax", limits=limits) == 1.5
    assert ox.json.decode(b'{"abc": 1}', dict[str, int], limits=limits) == {"abc": 1}
    # The limit counts bytes of UTF-8, as the spec says.
    with pytest.raises(ox.DecodeError):
        ox.json.decode('"éé"'.encode(), str, limits=limits)


@pytest.mark.parametrize("value", [-1, 2**64, -(2**70)])
def test_max_errors_out_of_range_says_what_is_allowed(value: int) -> None:
    with pytest.raises(ValueError, match=r"max_errors must be a whole number from 1 to") as caught:
        ox.json.decode(b"[1]", list[int], max_errors=value)
    assert str(value) in str(caught.value)
    with pytest.raises(ValueError, match=r"max_errors must be a whole number from 1 to"):
        ox.json.Decoder(list[int], max_errors=value)


def test_max_errors_of_zero_is_refused_and_one_stops_at_the_first_error() -> None:
    with pytest.raises(ValueError, match=r"at least 1.*max_errors=1"):
        ox.json.decode(b"[1]", list[int], max_errors=0)
    with pytest.raises(ValueError, match=r"at least 1"):
        ox.json.Decoder(list[int], max_errors=0)
    with pytest.raises(ox.ValidationError) as caught:
        ox.json.decode(b'["a", "b"]', list[int], max_errors=1)
    assert len(caught.value.errors()) == 1
    assert caught.value.truncated


def test_max_errors_of_the_wrong_type_is_a_type_error() -> None:
    with pytest.raises(TypeError):
        ox.json.decode(b"[1]", list[int], max_errors="all")  # type: ignore[arg-type]


def test_max_errors_refuses_a_bool_instead_of_reading_it_as_one() -> None:
    for value in (True, False):
        with pytest.raises(TypeError, match=r"max_errors must be a whole number, not bool"):
            ox.json.decode(b"[1]", list[int], max_errors=value)  # pyright: ignore[reportArgumentType]
        with pytest.raises(TypeError, match=r"max_errors must be a whole number, not bool"):
            ox.json.Decoder(list[int], max_errors=value)  # pyright: ignore[reportArgumentType]
