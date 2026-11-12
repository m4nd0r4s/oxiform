import threading
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


def test_none_turns_every_size_limit_off_except_the_depth_ceiling() -> None:
    none = ox.Limits.none()
    assert none.max_bytes > 2**60
    assert none.max_items > 2**60
    assert none.max_str_len > 2**60
    assert none.max_int_digits > 2**60
    assert none.max_depth == ox.Limits.MAX_DEPTH
    deep = b"[" * 400 + b"]" * 400
    with pytest.raises(ox.DecodeError):
        ox.json.decode(deep)
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


def test_the_depth_ceiling_is_reachable_and_safe_on_a_small_stack() -> None:
    depth = ox.Limits.MAX_DEPTH
    raw = b"[" * depth + b"]" * depth
    assert ox.json.decode(raw, limits=ox.Limits.none()) is not None
    failures: list[BaseException] = []
    previous = threading.stack_size(512 * 1024)
    try:

        def work() -> None:
            try:
                ox.json.decode(raw, limits=ox.Limits.none())
                with pytest.raises(ox.DecodeError):
                    ox.json.decode(b"[" * 100_000, limits=ox.Limits.none())
            except BaseException as error:  # noqa: BLE001
                failures.append(error)

        thread = threading.Thread(target=work)
        thread.start()
        thread.join()
    finally:
        threading.stack_size(previous)
    assert failures == []


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
