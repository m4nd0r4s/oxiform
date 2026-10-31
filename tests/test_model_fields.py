import copy
import dataclasses
import json
import sys
from typing import Any

import pytest

import oxiform as ox
from oxiform._naming import RenameRule, rename


@ox.model(rename_all="camelCase", kw_only=True)
class Invoice:
    invoice_id: int = ox.field(rename="id")
    customer: str = ox.field(aliases=["client"])
    postal_code: str = ""
    notes: str = ox.field(default="", skip_if_default=True)
    internal_score: float = ox.field(default=0.0, skip=True)
    password_hash: str = ox.field(default="", skip_encode=True)
    total: int = ox.field(default=0, skip_decode=True)


@ox.model
class Meta:
    created_by: str
    version: int = 1


@ox.model
class Document:
    title: str
    meta: Meta = ox.field(flatten=True)


def decode(raw: bytes | str, target: type[Any]) -> Any:
    return ox.json.decode(raw, target)


def encoded(value: object) -> object:
    return json.loads(ox.json.encode(value))


@pytest.mark.parametrize(
    ("rule", "expected"),
    [
        ("camelCase", "postalCodeV2"),
        ("PascalCase", "PostalCodeV2"),
        ("snake_case", "postal_code_v2"),
        ("kebab-case", "postal-code-v2"),
        ("SCREAMING_SNAKE_CASE", "POSTAL_CODE_V2"),
    ],
)
def test_renames_with_each_rule(rule: RenameRule, expected: str) -> None:
    assert rename("postal_code_v2", rule) == expected
    assert rename("postalCodeV2", rule) == expected


def test_models_are_slotted_dataclasses() -> None:
    invoice = Invoice(invoice_id=1, customer="Ana")
    assert dataclasses.is_dataclass(invoice)
    assert not hasattr(invoice, "__dict__")
    assert repr(invoice).startswith("Invoice(invoice_id=1, customer='Ana'")


def test_reads_and_writes_wire_names() -> None:
    invoice = decode(b'{"id": 7, "customer": "Ana", "postalCode": "1000"}', Invoice)
    assert (invoice.invoice_id, invoice.customer, invoice.postal_code) == (7, "Ana", "1000")
    assert encoded(invoice) == {"id": 7, "customer": "Ana", "postalCode": "1000", "total": 0}


def test_accepts_aliases_on_read_only() -> None:
    invoice = decode(b'{"id": 1, "client": "Rui"}', Invoice)
    assert invoice.customer == "Rui"
    assert "client" not in encoded(invoice)  # type: ignore[operator]
    with pytest.raises(ox.ValidationError) as raised:
        decode(b'{"id": 1, "customer": "a", "client": "b"}', Invoice)
    assert raised.value.errors()[0]["code"] == "duplicate_key"


def test_skips_fields_in_one_direction_or_both() -> None:
    raw = b'{"id": 1, "customer": "a", "internal_score": 9, "passwordHash": "h", "total": 5}'
    invoice = decode(raw, Invoice)
    assert invoice.internal_score == 0.0
    assert invoice.password_hash == "h"
    assert invoice.total == 0
    assert encoded(Invoice(invoice_id=1, customer="a", internal_score=3, password_hash="h")) == {
        "id": 1,
        "customer": "a",
        "postalCode": "",
        "total": 0,
    }


def test_omits_fields_on_write() -> None:
    @ox.model(skip_if_none=True)
    class Profile:
        name: str
        nickname: str | None = None
        tags: list[str] = ox.field(default_factory=list[str], skip_if_default=True)
        score: int = ox.field(default=0, skip_if=lambda score: score < 0)

    assert encoded(Profile("a")) == {"name": "a", "score": 0}
    assert encoded(Profile("a", "b", ["x"], -1)) == {"name": "a", "nickname": "b", "tags": ["x"]}
    assert encoded(Invoice(invoice_id=1, customer="a", notes="n"))["notes"] == "n"  # type: ignore[index]


def test_sorts_keys_by_wire_name() -> None:
    invoice = Invoice(invoice_id=1, customer="a", notes="n")
    assert ox.json.encode(invoice, sort_keys=True) == (
        b'{"customer":"a","id":1,"notes":"n","postalCode":"","total":0}'
    )


def test_flattens_a_model_in_both_directions() -> None:
    document = decode(b'{"title": "t", "created_by": "ana"}', Document)
    assert document == Document("t", Meta("ana"))
    assert encoded(document) == {"title": "t", "created_by": "ana", "version": 1}
    assert decode(ox.json.encode(document), Document) == document
    with pytest.raises(ox.ValidationError) as raised:
        decode(b'{"title": "t"}', Document)
    assert raised.value.errors()[0]["pointer"] == "/created_by"


def test_forbids_unknown_fields_and_subclasses_inherit_the_options() -> None:
    @ox.model(unknown="forbid", rename_all="kebab-case")
    class Strict:
        max_size: int

    @ox.model
    class Child(Strict):
        min_size: int = 0

    with pytest.raises(ox.ValidationError) as raised:
        decode(b'{"max-size": 1, "extra": 2}', Child)
    assert raised.value.errors()[0]["code"] == "unknown_field"
    assert encoded(Child(2, 1)) == {"max-size": 2, "min-size": 1}


def test_model_base_class_takes_the_same_options() -> None:
    class Point(ox.Model, frozen=True, rename_all="PascalCase"):
        x_value: int
        y_value: int = ox.field(default=0, aliases=["Y"])

    assert decode(b'{"XValue": 1, "Y": 2}', Point) == Point(1, 2)
    assert encoded(Point(1)) == {"XValue": 1, "YValue": 0}


def test_field_shortcuts_add_constraints() -> None:
    @ox.model
    class Rated:
        rating: float = ox.field(default=0.0, ge=0, le=5)
        code: str = ox.field(default="AA", min_len=2, max_len=3, pattern=r"^[A-Z]+$")

    with pytest.raises(ox.ValidationError) as raised:
        decode(b'{"rating": 6, "code": "A"}', Rated)
    codes = [error["code"] for error in raised.value.errors()]
    assert codes == ["too_large", "too_short"]


def test_fields_outside_init_take_their_default() -> None:
    @ox.model
    class Counter:
        start: int
        total: int = ox.field(default=0)
        seen: int = dataclasses.field(init=False, default=3)

    assert decode(b'{"start": 1}', Counter).seen == 3


def test_replaces_fields() -> None:
    invoice = Invoice(invoice_id=1, customer="a")
    assert ox.replace(invoice, customer="b") == Invoice(invoice_id=1, customer="b")
    if sys.version_info >= (3, 13):
        assert copy.replace(invoice, invoice_id=2).invoice_id == 2  # type: ignore[attr-defined]


def test_compiles_forward_references_on_first_use() -> None:
    @ox.model
    class Tree:
        children: "list[Leaf]"

    @ox.model
    class Leaf:
        value: int

    globals()["Leaf"] = Leaf
    try:
        assert decode(b'{"children": [{"value": 1}]}', Tree) == Tree([Leaf(1)])
    finally:
        del globals()["Leaf"]


@pytest.mark.parametrize(
    ("declare", "message"),
    [
        (
            lambda: ox.model(type("A", (), {"__annotations__": {"a": int, "b": int}, "a": 1})),
            "A: non-default argument 'b' follows default argument",
        ),
        (
            lambda: ox.model(rename_all="camel")(type("B", (), {})),  # pyright: ignore[reportArgumentType]
            "B: rename_all must be one of camelCase",
        ),
        (
            lambda: ox.model(unknown="error")(type("C", (), {})),  # pyright: ignore[reportArgumentType]
            'C: unknown must be "ignore" or "forbid"',
        ),
        (
            lambda: ox.model(
                type("D", (), {"__annotations__": {"a": int}, "a": ox.field(skip=True)})
            ),
            "D.a: a field skipped on read needs a default",
        ),
        (
            lambda: ox.model(
                type(
                    "E",
                    (),
                    {"__annotations__": {"a": int}, "a": ox.field(skip_if_default=True)},
                )
            ),
            "E.a: skip_if_default needs a default",
        ),
        (
            lambda: ox.model(
                type("F", (), {"__annotations__": {"a": int}, "a": ox.field(flatten=True)})
            ),
            "F.a: only a model can be flattened",
        ),
        (
            lambda: ox.model(
                type(
                    "G",
                    (),
                    {
                        "__annotations__": {"created_by": str, "meta": Meta},
                        "meta": ox.field(flatten=True),
                    },
                )
            ),
            'G.created_by: another field is already read from the key "created_by"',
        ),
        (
            lambda: ox.model(
                type(
                    "H",
                    (),
                    {"__annotations__": {"a": int, "b": int}, "b": ox.field(aliases=["a"])},
                )
            ),
            'H.b: another field is already read from the key "a"',
        ),
    ],
)
def test_rejects_wrong_declarations_at_once(declare: Any, message: str) -> None:
    with pytest.raises(ox.SchemaError) as raised:
        declare()
    assert str(raised.value).startswith(message)
