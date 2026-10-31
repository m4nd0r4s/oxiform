import json
import math
from dataclasses import dataclass, field
from enum import IntEnum, StrEnum

import pytest

import oxiform as ox


@dataclass(slots=True)
class Address:
    street: str
    postal_code: str


@dataclass
class User:
    id: int
    name: str
    tags: list[str] = field(default_factory=list[str])
    address: Address | None = None


class Point(ox.Model, frozen=True):
    x: int
    y: int = 0


class Level(IntEnum):
    BASIC = 1


class Color(StrEnum):
    RED = "red"


def test_writes_builtins_compactly() -> None:
    value: dict[str, object] = {
        "a": [1, -2.5, "x", None, True, False],
        "b": {},
        "c": [],
        "d": (1, 2),
    }
    assert ox.json.encode(value) == b'{"a":[1,-2.5,"x",null,true,false],"b":{},"c":[],"d":[1,2]}'


def test_agrees_with_the_standard_library() -> None:
    value: dict[str, object] = {
        "text": 'quote " backslash \\ newline \n tab \t control \x01 accent é emoji 😀',
        "floats": [0.1, 1e300, -0.0, 123456789.125, 5e-324],
        "big": 2**100,
        "negative_big": -(2**80),
        "int_key": {1: "one"},
    }
    assert json.loads(ox.json.encode(value)) == json.loads(json.dumps(value))


def test_keeps_floats_exact() -> None:
    for number in (0.1, 1 / 3, 2.5e-308, 1.7976931348623157e308, math.pi):
        assert ox.json.decode(ox.json.encode(number), float) == number


def test_writes_dataclasses_and_models_as_objects() -> None:
    user = User(1, "Rex", ["a"], Address("Main", "1000-001"))
    assert json.loads(ox.json.encode(user)) == {
        "id": 1,
        "name": "Rex",
        "tags": ["a"],
        "address": {"street": "Main", "postal_code": "1000-001"},
    }
    assert ox.json.encode(Point(1)) == b'{"x":1,"y":0}'


def test_round_trips_through_decode() -> None:
    user = User(1, "Rex", ["a", "b"], Address("Main", "1000-001"))
    assert ox.json.decode(ox.json.encode(user), User) == user
    assert ox.json.decode(ox.json.encode([Point(1, 2)]), list[Point]) == [Point(1, 2)]


def test_writes_int_and_str_enums_by_value() -> None:
    assert ox.json.encode([Level.BASIC, Color.RED]) == b'[1,"red"]'


def test_indents_and_sorts_like_the_standard_library() -> None:
    value: dict[str, object] = {"b": [1, {"d": None, "c": True}], "a": {}}
    expected = json.dumps(value, indent=2, sort_keys=True).encode()
    assert ox.json.encode(value, indent=2, sort_keys=True) == expected
    assert ox.json.Encoder(indent=2, sort_keys=True).encode(value) == expected
    assert ox.json.encode(Point(1, 2), sort_keys=True) == b'{"x":1,"y":2}'


def test_reports_what_cannot_be_encoded_and_where() -> None:
    with pytest.raises(ox.EncodeError) as raised:
        ox.json.encode({"items": [1, 2, complex(1, 2)]})
    assert str(raised.value) == "cannot encode an object of type complex at /items/2"
    assert raised.value.pointer == "/items/2"
    with pytest.raises(ox.EncodeError, match="NaN is not valid JSON at /0"):
        ox.json.encode([float("nan")])
    with pytest.raises(ox.EncodeError, match="dict keys must be str or int, not tuple"):
        ox.json.encode({(1, 2): "x"})


def test_refuses_reference_cycles() -> None:
    cycle: list[object] = []
    cycle.append(cycle)
    with pytest.raises(ox.EncodeError, match="reference cycle"):
        ox.json.encode(cycle)
