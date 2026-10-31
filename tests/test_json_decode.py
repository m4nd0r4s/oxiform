from dataclasses import dataclass, field
from typing import Annotated, Any

import pytest

import oxiform as ox


@dataclass(slots=True, frozen=True)
class Address:
    street: str
    postal_code: Annotated[str, ox.Pattern(r"^\d{4}-\d{3}$")]


@dataclass(slots=True)
class User:
    id: int
    name: Annotated[str, ox.Len(1, 80)]
    age: Annotated[int, ox.Ge(0), ox.Le(150)] | None = None
    addresses: list[Address] = field(default_factory=list[Address])
    level: Annotated[str, ox.OneOf("basic", "admin")] = "basic"


@dataclass
class Tree:
    value: int
    children: "list[Tree]"


def test_decodes_nested_dataclasses() -> None:
    user = ox.json.decode(
        b'{"id": 1, "name": "Rex", "addresses": [{"street": "A", "postal_code": "1000-001"}]}',
        User,
    )
    assert user == User(id=1, name="Rex", addresses=[Address("A", "1000-001")])


def test_fills_defaults_and_calls_factories_per_instance() -> None:
    first = ox.json.decode(b'{"id": 1, "name": "a"}', User)
    second = ox.json.decode(b'{"id": 2, "name": "b"}', User)
    assert (first.age, first.level, first.addresses) == (None, "basic", [])
    assert first.addresses is not second.addresses


def test_decodes_self_referencing_dataclasses() -> None:
    tree = ox.json.decode(b'{"value": 1, "children": [{"value": 2, "children": []}]}', Tree)
    assert tree == Tree(1, [Tree(2, [])])


def test_calls_post_init() -> None:
    @dataclass
    class Interval:
        start: int
        end: int
        length: int = field(init=False)

        def __post_init__(self) -> None:
            self.length = self.end - self.start

    assert ox.json.decode(b'{"start": 2, "end": 5}', Interval).length == 3


def test_accepts_every_input_kind() -> None:
    for data in (b"[1]", "[1]", bytearray(b"[1]"), memoryview(b"[1]")):
        assert ox.json.decode(data, list[int]) == [1]


def test_decodes_untyped_values() -> None:
    assert ox.json.decode(b'{"a": [1, 2.5, "x", null, true]}') == {"a": [1, 2.5, "x", None, True]}
    assert ox.json.decode(b"123456789012345678901234567890", Any) == 123456789012345678901234567890
    assert ox.json.decode('"ação 😀"', str) == "ação 😀"


def test_collects_every_error_with_its_path() -> None:
    with pytest.raises(ox.ValidationError) as raised:
        ox.json.decode(b'{"id": "x", "name": "", "addresses": [{"street": 1}]}', User)
    errors = raised.value.errors()
    assert [(error["pointer"], error["code"]) for error in errors] == [
        ("/id", "type"),
        ("/name", "too_short"),
        ("/addresses/0/street", "type"),
        ("/addresses/0/postal_code", "missing"),
    ]
    assert errors[0] == {
        "code": "type",
        "path": ["id"],
        "pointer": "/id",
        "message": "expected int, received str",
        "input": "x",
        "ctx": {"expected": "int", "received": "str"},
    }
    assert errors[1]["ctx"] == {"min": 1, "max": 80, "actual": 0}
    assert errors[2]["path"] == ["addresses", 0, "street"]


def test_formats_errors_for_people() -> None:
    with pytest.raises(ox.ValidationError) as raised:
        ox.json.decode(b'{"id": "x", "name": ""}', User)
    assert str(raised.value) == (
        "2 errors validating User (json):\n"
        "  /id    expected int, received str 'x'  [type]\n"
        "  /name  length 0 is less than the minimum 1  [too_short]"
    )


def test_groups_errors_by_pointer_and_as_problem_details() -> None:
    with pytest.raises(ox.ValidationError) as raised:
        ox.json.decode(b'{"id": 1, "name": "a", "age": -1, "level": "root"}', User)
    assert set(raised.value.by_path()) == {"/age", "/level"}
    problem = raised.value.to_problem()
    assert problem["status"] == 422
    assert problem["errors"] == [
        {"pointer": "/age", "code": "too_small", "detail": "must be at least 0"},
        {"pointer": "/level", "code": "literal", "detail": 'must be one of "basic", "admin"'},
    ]


def test_reports_malformed_input_as_a_decode_error() -> None:
    with pytest.raises(ox.DecodeError) as raised:
        ox.json.decode('{"id": 1,\n "name": }', User)
    (error,) = raised.value.errors()
    assert error["code"] == "syntax"
    assert error["pointer"] == "/name"
    assert error["ctx"] == {"line": 2, "column": 10, "format": "json"}
    assert isinstance(raised.value, ox.ValidationError)


def test_rejects_invalid_utf8() -> None:
    with pytest.raises(ox.DecodeError, match="invalid UTF-8 at byte 2"):
        ox.json.decode(b'"a\xff"', str)


def test_stops_at_max_errors() -> None:
    with pytest.raises(ox.ValidationError) as raised:
        ox.json.decode(b'["a", "b", "c"]', list[int], max_errors=2)
    assert len(raised.value.errors()) == 2
    assert raised.value.truncated


def test_raises_schema_errors_for_unsupported_types() -> None:
    with pytest.raises(ox.SchemaError, match="unsupported type"):
        ox.json.decode(b"1", complex)
    with pytest.raises(ox.SchemaError, match=r"root: invalid Pattern"):
        ox.json.decode(b'"a"', Annotated[str, ox.Pattern("(?=a)")])


def test_reuses_compiled_plans() -> None:
    for _ in range(3):
        assert ox.json.decode(b"[1, 2]", list[int]) == [1, 2]


def test_lets_exceptions_from_python_code_through() -> None:
    def explode() -> list[int]:
        raise RuntimeError("factory failed")

    @dataclass
    class Box:
        items: list[int] = field(default_factory=explode)

    with pytest.raises(RuntimeError, match="factory failed"):
        ox.json.decode(b"{}", Box)


def test_decodes_from_many_threads() -> None:
    from concurrent.futures import ThreadPoolExecutor

    payload = b'{"id": 1, "name": "Rex", "addresses": [{"street": "A", "postal_code": "1000-001"}]}'

    def decode_many(_: int) -> int:
        return sum(ox.json.decode(payload, User).id for _ in range(200))

    with ThreadPoolExecutor(max_workers=8) as pool:
        assert sum(pool.map(decode_many, range(8))) == 8 * 200


def test_reusable_decoders() -> None:
    decoder = ox.json.Decoder(list[int], max_errors=1)
    assert decoder.type == list[int]
    assert decoder.decode(b"[1, 2]") == [1, 2]
    with pytest.raises(ox.ValidationError) as raised:
        decoder.decode(b'["a", "b"]')
    assert len(raised.value.errors()) == 1
    assert ox.json.Decoder().decode(b'{"a": 1}') == {"a": 1}
