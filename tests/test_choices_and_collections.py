import enum
import json
from typing import Annotated, Any, Literal

import pytest

import oxiform as ox


class Color(enum.Enum):
    RED = "red"
    GREEN = "green"
    CRIMSON = "red"  # an alias of RED


class Level(enum.IntEnum):
    LOW = 1
    HIGH = 2


class Size(enum.StrEnum):
    SMALL = "s"


@ox.model
class Shirt:
    color: Color
    level: Level = Level.LOW
    size: Literal["s", "m"] | None = None
    tags: frozenset[str] = frozenset()
    point: tuple[int, int] = (0, 0)


def decode(raw: bytes | str, target: Any) -> Any:
    return ox.json.decode(raw, target)


def first_error(raw: bytes | str, target: Any) -> Any:
    with pytest.raises(ox.ValidationError) as raised:
        decode(raw, target)
    return raised.value.errors()[0]


def test_reads_enum_members_by_value() -> None:
    assert decode(b'"red"', Color) is Color.RED
    assert decode(b"2", Level) is Level.HIGH
    assert decode(b'"s"', Size) is Size.SMALL
    error = first_error(b'"blue"', Color)
    assert (error["code"], error["ctx"]) == ("enum", {"allowed": ["red", "green"]})
    assert first_error(b'"2"', Level)["code"] == "enum"


def test_reads_literals_of_any_kind() -> None:
    target = Literal["a", 1, True, None]
    assert decode(b'"a"', target) == "a"
    assert decode(b"1", target) == 1
    assert decode(b"true", target) is True
    assert decode(b"null", target) is None
    error = first_error(b"false", target)
    assert error["message"] == "expected one of 'a', 1, True, None"
    assert error["ctx"] == {"allowed": ["a", 1, True, None]}
    assert decode(b'"red"', Literal[Color.RED]) is Color.RED


def test_reads_literals_and_enums_inside_unions() -> None:
    target = Literal["auto"] | int
    assert decode(b'"auto"', target) == "auto"
    assert decode(b"3", target) == 3
    assert first_error(b'"manual"', target)["code"] == "literal"


def test_reads_tuples_and_sets() -> None:
    assert decode(b"[1, 2, 3]", tuple[int, ...]) == (1, 2, 3)
    assert decode(b'[1, "a"]', tuple[int, str]) == (1, "a")
    assert decode(b"[1, 2, 1]", set[int]) == {1, 2}
    assert decode(b'["a"]', frozenset[str]) == frozenset({"a"})
    assert decode(b"[1, [2]]", tuple) == (1, [2])
    error = first_error(b"[1]", tuple[int, str])
    assert (error["code"], error["ctx"]["actual"]) == ("too_short", 1)
    assert first_error(b"[1, 2, 3]", tuple[int, int])["code"] == "too_long"
    assert first_error(b'[1, "x"]', tuple[int, int])["pointer"] == "/1"
    assert first_error(b"[1, 2, 3]", Annotated[set[int], ox.Len(max=2)])["code"] == "too_long"


def test_round_trips_a_model() -> None:
    shirt = Shirt(Color.GREEN, Level.HIGH, "m", frozenset({"b", "a"}), (1, 2))
    assert json.loads(ox.json.encode(shirt)) == {
        "color": "green",
        "level": 2,
        "size": "m",
        "tags": ["a", "b"],
        "point": [1, 2],
    }
    assert decode(ox.json.encode(shirt), Shirt) == shirt


def test_writes_sets_sorted_when_possible() -> None:
    assert ox.json.encode({3, 1, 2}) == b"[1,2,3]"
    assert json.loads(ox.json.encode({1, "a"})) in ([1, "a"], ["a", 1])


@pytest.mark.parametrize(
    ("target", "message"),
    [
        (Literal[1.5], "root: only None, bool, int and str values can be read, not 1.5"),
        (Annotated[Color, ox.Len(1)], "root: constraints do not apply to Color"),
        (set[list[int]], "root: set items must be hashable"),
        (Annotated[tuple[int, int], ox.Len(2)], "root: constraints do not apply to a fixed tuple"),
        (Literal["a"] | str, "root: Literal['a'] and str are both read from a string"),
    ],
)
def test_rejects_declarations_it_cannot_read(target: Any, message: str) -> None:
    with pytest.raises(ox.SchemaError) as raised:
        ox.warmup(target)
    assert str(raised.value).startswith(message)
