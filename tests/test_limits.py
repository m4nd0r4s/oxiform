from typing import Any

import pytest

import oxiform as ox


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


def test_none_turns_every_size_limit_off() -> None:
    none = ox.Limits.none()
    assert none.max_bytes > 2**60
    assert none.max_depth > 2**60
    deep = b"[" * 1000 + b"]" * 1000
    with pytest.raises(ox.DecodeError):
        ox.json.decode(deep)
    assert ox.json.decode(deep, limits=none) is not None


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
