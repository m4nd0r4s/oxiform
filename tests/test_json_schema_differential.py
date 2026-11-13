"""The schema and the decoder agree: for many inputs, ``decode`` succeeds exactly when the
schema validates, in strict and in lax mode, except for the listed cases; and what ``encode``
writes always validates against the serialization schema."""

import datetime
import decimal
import enum
import json
from typing import Annotated, Any, Literal

import pytest
from schema_support import Checker, validator

import oxiform as ox

COMPONENTS = "#/components/schemas/{name}"


class Color(enum.Enum):
    RED = "red"
    BLUE = "blue"


class Level(enum.IntEnum):
    LOW = 1
    HIGH = 2


class Mixed(enum.Enum):
    TEXT = "text"
    NUMBER = 2


class Perm(enum.Flag):
    READ = 1
    WRITE = 2
    EXEC = 4


class Sparse(enum.Flag):
    A = 1
    B = 4


class Bits(enum.IntFlag):
    A = 1
    B = 2


@ox.model
class Point:
    x: int
    y: int = 0


@ox.model(unknown="forbid")
class Strict:
    name: str


@ox.model(tag_field="kind")
class Dog:
    name: str


@ox.model(tag_field="kind", tag="cat")
class Cat:
    lives: int


@ox.model
class Card:
    number: str


@ox.model
class Pix:
    key: str


SCALARS: list[str] = [
    "null",
    "true",
    "false",
    "0",
    "1",
    "-1",
    "2",
    "42",
    "-7",
    "10",
    "1.5",
    "1.0",
    "1e2",
    "2.0",
    '""',
    '"a"',
    '"red"',
    '"RED"',
    '"text"',
    '"42"',
    '"-7"',
    '"+2"',
    '"007"',
    '"002"',
    '"2.0"',
    '"4.5"',
    '"1e3"',
    '"1.5"',
    '".5"',
    '"inf"',
    '"nan"',
    '"true"',
    '"False"',
    '"yes"',
    '"No"',
    '"on"',
    '"OFF"',
    '"1"',
    '"0"',
    '" 1"',
    '"1.2.3"',
    "[]",
    "[1]",
    '["a"]',
    "{}",
    '{"a": 1}',
]

CONTAINERS: list[str] = [
    "null",
    "[]",
    "[1]",
    "[1, 1]",
    "[1, 2, 3]",
    '[1, "a"]',
    '["a", 1]',
    '["a"]',
    '["a", "b"]',
    "[[1]]",
    "[null]",
    "[true]",
    "{}",
    '{"a": 1}',
    '{"a": "b"}',
    '{"a": 1, "b": 2}',
    "1",
    '"a"',
    "true",
]

MODELS: list[str] = [
    "null",
    "{}",
    '{"x": 1}',
    '{"x": 1, "y": 2}',
    '{"x": "1"}',
    '{"x": 1, "y": "2"}',
    '{"x": 1, "extra": 1}',
    '{"name": "a"}',
    '{"name": "a", "extra": 1}',
    '{"kind": "Dog", "name": "a"}',
    '{"kind": "cat", "lives": 9}',
    '{"kind": "Dog", "lives": 9}',
    '{"kind": "bird", "name": "a"}',
    '{"name": "a", "lives": 1}',
    '{"type": "Card", "number": "1"}',
    '{"type": "Pix", "key": "k"}',
    '{"type": "Cash"}',
    '{"number": "1"}',
    '{"key": "k"}',
    '{"Card": {"number": "1"}}',
    '{"Pix": {"key": "k"}}',
    '{"Card": {"number": "1"}, "Pix": {"key": "k"}}',
    '{"type": "Card", "data": {"number": "1"}}',
    '{"type": "Card"}',
    "[]",
    '"a"',
    "1",
]

# Types whose text the schema describes with a ``format`` only, which a validator does not
# assert unless asked: the pool leaves out strings for them.
NON_STRINGS = [item for item in SCALARS if not item.startswith('"')]

BASE64: list[str] = ['"YQ=="', '"YR=="', '"aGVsbG8="', '"aGVsbG8"', '"AAAA"', '"YQ="', '"a"', '""']
DECIMALS: list[str] = [
    f'"{text}"'
    for text in (
        "1.5",
        "Infinity",
        "-inf",
        "NaN",
        "sNaN12",
        "1.",
        "+.5",
        "1_0",
        "0x1",
        "1E+2",
        ".",
        "e5",
        "1e",
        " 1",
        "nan1",
        "infinit",
    )
]
CLOCKS: list[str] = [
    f'"{text}"'
    for text in (
        "10:00:00",
        "10:00:00.5",
        "10:00:00Z",
        "10:00:00+01:00",
        "10:00",
        "10:00:00 ",
        "2026-01-31T10:00:00",
        "2026-01-31t10:00:00",
        "2026-01-31 10:00:00.123",
        "2026-01-31T10:00:00Z",
        "2026-01-31T10:00",
        "2026-01-31T10:00:00,5",
    )
]

Case = tuple[str, Any, list[str]]

CASES: list[Case] = [
    ("int", int, SCALARS),
    ("float", float, SCALARS),
    ("bool", bool, SCALARS),
    ("str", str, SCALARS),
    ("None", type(None), SCALARS),
    ("Any", Any, SCALARS),
    ("bytes", bytes, SCALARS),
    ("Decimal", decimal.Decimal, SCALARS),
    ("Decimal text", decimal.Decimal, DECIMALS),
    ("bytes base64", bytes, BASE64),
    ("time", datetime.time, NON_STRINGS + CLOCKS[:6]),
    ("naive text", ox.NaiveDateTime, CLOCKS[6:]),
    ("Ge/Le", Annotated[int, ox.Ge(2), ox.Le(10)], SCALARS),
    ("float Ge", Annotated[float, ox.Ge(1.5)], SCALARS),
    ("OneOf int", Annotated[int, ox.OneOf(1, 42, -7)], SCALARS),
    ("OneOf str", Annotated[str, ox.OneOf("a", "red")], SCALARS),
    ("Len str", Annotated[str, ox.Len(1, 3)], SCALARS),
    ("Pattern", Annotated[str, ox.Pattern(r"^\d+$")], SCALARS),
    ("Literal str", Literal["a", "red"], SCALARS),
    ("Literal one", Literal["a"], SCALARS),
    ("Literal int", Literal[1, 2, 42], SCALARS),
    ("Literal neg", Literal[-7, 0], SCALARS),
    ("Literal bool", Literal[True], SCALARS),
    ("Literal int bool", Literal[1, True], SCALARS),
    ("Literal mixed", Literal["a", 2, None], SCALARS),
    ("Enum str", Color, SCALARS),
    ("IntEnum", Level, SCALARS),
    ("Enum mixed", Mixed, SCALARS),
    ("Flag", Perm, SCALARS + ["3", "5", "6", "7", "8", "-8", "-9"]),
    ("sparse Flag", Sparse, SCALARS + ["5", "4", "-4", "-3", "-8", "-5"]),
    ("IntFlag", Bits, SCALARS + ["3", "8", "-1", "100"]),
    ("Optional int", int | None, SCALARS),
    ("Optional str", str | None, SCALARS),
    ("Optional Literal", Literal["a"] | None, SCALARS),
    ("Optional bool", bool | None, SCALARS),
    ("Optional Literal empty", Literal["", "a"] | None, SCALARS),
    ("int | str", int | str, SCALARS),
    ("int | str | None", int | str | None, SCALARS),
    ("int | float", int | float, SCALARS),
    ("int | bool", int | bool, SCALARS),
    ("int | Literal", int | Literal["auto"], SCALARS),
    ("float | list", float | list[int], SCALARS + CONTAINERS),
    ("date", datetime.date, NON_STRINGS),
    ("datetime", datetime.datetime, NON_STRINGS),
    ("timedelta", datetime.timedelta, NON_STRINGS),
    ("naive", ox.NaiveDateTime, NON_STRINGS + ['"2026-01-31T10:00:00"', '"2026-01-31t10:00:00"']),
    ("list[int]", list[int], CONTAINERS),
    ("list[str]", list[str], CONTAINERS),
    ("list", list, CONTAINERS),
    ("Len list", Annotated[list[int], ox.Len(1, 2)], CONTAINERS),
    ("tuple[int, ...]", tuple[int, ...], CONTAINERS),
    ("tuple[int, str]", tuple[int, str], CONTAINERS),
    ("tuple[()]", tuple[()], CONTAINERS),
    ("set[int]", set[int], CONTAINERS),
    ("frozenset[str]", frozenset[str], CONTAINERS),
    ("dict[str, int]", dict[str, int], CONTAINERS),
    ("dict", dict, CONTAINERS),
    ("Len dict", Annotated[dict[str, int], ox.Len(1, 1)], CONTAINERS),
    ("list[int | None]", list[int | None], CONTAINERS),
    ("Optional list", list[int] | None, CONTAINERS),
    ("model", Point, MODELS),
    ("forbid", Strict, MODELS),
    ("Optional model", Point | None, MODELS),
    ("list[model]", list[Point], CONTAINERS + ['[{"x": 1}]', '[{"y": 1}]', '[{"x": 1}, {}]']),
    ("tag_field union", Dog | Cat, MODELS),
    ("Tagged", Annotated[Card | Pix, ox.Tagged("type")], MODELS),
    ("Tagged adjacent", Annotated[Card | Pix, ox.Tagged("type", content="data")], MODELS),
    ("External", Annotated[Card | Pix, ox.External()], MODELS),
    ("Untagged", Annotated[Card | Pix, ox.Untagged()], MODELS),
    ("Optional Tagged", Annotated[Card | Pix | None, ox.Tagged("type")], MODELS),
]

# Where the schema is looser than the decoder, and why. A JSON Schema ``integer`` is any number
# without a fraction, so it cannot refuse the forms below that the decoder refuses (a strict
# ``int`` is written without a point or an exponent; lax mode also refuses an exponent).
INTEGER_FORMS = {"1.0", "1e2", "2.0"}

# Lax mode reads text that a pattern can describe but whose number a limit cannot bound: the
# ``minimum`` and ``maximum`` apply to the numeric branch only.
LAX_TEXT_LIMITS = {("Ge/Le", raw, "lax") for raw in ('"42"', '"-7"', '"1"', '"0"')} | {
    ("float Ge", raw, "lax") for raw in ('"-7"', '".5"', '"1"', '"0"')
}


def excused(name: str, raw: str, mode: str, decoded: bool, validated: bool) -> bool:
    if validated and not decoded:
        return raw in INTEGER_FORMS or (name, raw, mode) in LAX_TEXT_LIMITS
    return False


def decodes(raw: str, target: Any, mode: Literal["strict", "lax"]) -> bool:
    try:
        ox.json.decode(raw, target, mode=mode)
    except ox.ValidationError:
        return False
    return True


def check_case(
    name: str, target: Any, inputs: list[str], mode: Literal["strict", "lax"]
) -> list[str]:
    coercion = "lax" if mode == "lax" else "strict"
    bundle = ox.json_schema_bundle([target], ref_template=COMPONENTS, coercion=coercion)
    check = validator(bundle)
    differences: list[str] = []
    for raw in inputs:
        expected = decodes(raw, target, mode)
        actual = check.is_valid(json.loads(raw))
        if expected != actual and not excused(name, raw, mode, expected, actual):
            differences.append(f"{name} [{mode}] {raw}: decode {expected}, schema {actual}")
    return differences


@pytest.mark.parametrize("mode", ["strict", "lax"])
@pytest.mark.parametrize(("name", "target", "inputs"), CASES, ids=[case[0] for case in CASES])
def test_the_schema_accepts_what_decode_accepts(
    name: str, target: Any, inputs: list[str], mode: Literal["strict", "lax"]
) -> None:
    assert check_case(name, target, inputs, mode) == []


ENCODED: list[tuple[Any, list[Any]]] = [
    (int, [0, -5, 2**70]),
    (float, [1.5, 3, -0.0]),
    (bool, [True, False]),
    (str, ["", "a"]),
    (bytes, [b"", b"a", b"hello"]),
    (decimal.Decimal, [decimal.Decimal("1.50"), decimal.Decimal("-0.5E+3")]),
    (Literal["a", "b"], ["a", "b"]),
    (Literal[1, 2], [1, 2]),
    (Color, [Color.RED, Color.BLUE]),
    (Level, [Level.LOW, Level.HIGH]),
    (Mixed, [Mixed.TEXT, Mixed.NUMBER]),
    (Perm, [Perm.READ, Perm.READ | Perm.EXEC]),
    (Bits, [Bits.A | Bits.B]),
    (int | None, [None, 3]),
    (int | str | None, [None, 3, "x"]),
    (list[int], [[], [1, 2]]),
    (tuple[int, ...], [(), (1, 2)]),
    (tuple[int, str], [(1, "a")]),
    (tuple[()], [()]),
    (set[int], [set(), {3, 1, 2}]),
    (frozenset[str], [frozenset({"b", "a"})]),
    (dict[str, int], [{}, {"a": 1}]),
    (datetime.datetime, [datetime.datetime(2026, 1, 31, 10, tzinfo=datetime.UTC)]),
    (ox.NaiveDateTime, [datetime.datetime(2026, 1, 31, 10, 0, 0, 5)]),
    (datetime.date, [datetime.date(2026, 1, 31)]),
    (Point, [Point(1, 2)]),
    (Point | None, [None, Point(1)]),
    (list[Point], [[Point(1), Point(2, 3)]]),
    (Dog | Cat, [Dog("a"), Cat(9)]),
    (Annotated[Card | Pix, ox.Tagged("type")], [Card("1"), Pix("k")]),
    (Annotated[Card | Pix, ox.Tagged("type", content="data")], [Card("1"), Pix("k")]),
    (Annotated[Card | Pix, ox.External()], [Card("1"), Pix("k")]),
    (Annotated[Card | Pix, ox.Untagged()], [Card("1"), Pix("k")]),
]


@pytest.mark.parametrize(("target", "values"), ENCODED, ids=[repr(target) for target, _ in ENCODED])
def test_what_encode_writes_validates_against_the_serialization_schema(
    target: Any, values: list[Any]
) -> None:
    bundle = ox.json_schema_bundle([target], ref_template=COMPONENTS, mode="serialization")
    check: Checker = validator(bundle)
    for value in values:
        written = json.loads(ox.json.encode(value, type=target))
        assert check.is_valid(written), (value, written)
