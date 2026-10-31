import json
from typing import Annotated, Any

import pytest

import oxiform as ox


@ox.model(tag_field="kind", rename_all="camelCase")
class Animal:
    name: str


@ox.model(tag="dog")
class Dog(Animal):
    breed_name: str


@ox.model
class Cat(Animal):
    indoor: bool


@ox.model
class Card:
    number: str


@ox.model
class Pix:
    key: str


@ox.model
class Payment:
    method: Annotated[Card | Pix, ox.Tagged("type")]
    history: list[Annotated[Card | Pix, ox.External()]] = ox.field(default_factory=list)
    backup: Annotated[Card | Pix | None, ox.Tagged("type", content="data")] = None


def decode(raw: bytes | str, target: Any) -> Any:
    return ox.json.decode(raw, target)


def encoded(value: object) -> object:
    return json.loads(ox.json.encode(value))


def test_chooses_by_the_kind_of_value() -> None:
    target = int | str | list[int] | None
    assert decode(b"1", target) == 1
    assert decode(b'"a"', target) == "a"
    assert decode(b"[1]", target) == [1]
    assert decode(b"null", target) is None
    assert decode(b"2", float | int) == 2 and type(decode(b"2", float | int)) is int
    assert decode(b"2.5", int | float) == 2.5
    with pytest.raises(ox.ValidationError) as raised:
        decode(b"true", target)
    assert raised.value.errors()[0]["message"] == "expected int | str | list | None, received bool"


def test_reads_and_writes_models_by_their_tag_field() -> None:
    pet = decode(b'{"kind": "dog", "name": "Rex", "breedName": "lab"}', Dog | Cat)
    assert pet == Dog("Rex", "lab")
    assert decode(b'{"name": "Tom", "indoor": true, "kind": "cat"}', Dog | Cat) == Cat("Tom", True)
    assert encoded(Dog("Rex", "lab")) == {"kind": "dog", "name": "Rex", "breedName": "lab"}
    assert ox.json.encode(Cat("Tom", True)).startswith(b'{"kind":"cat"')


def test_checks_the_tag_when_reading_a_concrete_model() -> None:
    assert decode(b'{"name": "Rex", "breedName": "lab"}', Dog) == Dog("Rex", "lab")
    with pytest.raises(ox.ValidationError) as raised:
        decode(b'{"kind": "cat", "name": "Rex", "breedName": "lab"}', Dog)
    assert raised.value.errors()[0]["code"] == "tag"
    assert raised.value.errors()[0]["ctx"] == {"allowed": ["dog"]}


def test_round_trips_each_tag_mode() -> None:
    payment = Payment(Card("4111"), [Pix("k"), Card("5")], Pix("p"))
    assert encoded(payment) == {
        "method": {"type": "Card", "number": "4111"},
        "history": [{"Pix": {"key": "k"}}, {"Card": {"number": "5"}}],
        "backup": {"type": "Pix", "data": {"key": "p"}},
    }
    assert decode(ox.json.encode(payment), Payment) == payment
    assert decode(b'{"method": {"number": "1", "type": "Card"}}', Payment) == Payment(Card("1"))


def test_reports_unknown_tags_with_the_allowed_values() -> None:
    with pytest.raises(ox.ValidationError) as raised:
        decode(b'{"method": {"type": "Cash"}}', Payment)
    error = raised.value.errors()[0]
    assert (error["code"], error["pointer"]) == ("tag", "/method/type")
    assert error["ctx"] == {"allowed": ["Card", "Pix"]}


def test_tries_untagged_variants_in_order() -> None:
    target = Annotated[Card | Pix, ox.Untagged()]
    assert decode(b'{"key": "k"}', target) == Pix("k")
    with pytest.raises(ox.ValidationError) as raised:
        decode(b'{"other": 1}', target)
    error = raised.value.errors()[0]
    assert error["code"] == "union"
    by_variant: Any = error["ctx"]["errors"]
    assert [issue["code"] for issue in by_variant["Card"]] == ["missing"]
    assert [issue["pointer"] for issue in by_variant["Pix"]] == ["/key"]


@pytest.mark.parametrize(
    ("target", "message"),
    [
        (Card | Pix, "root: Card and Pix are both read from a map"),
        (int | bool | Annotated[int, ox.Ge(0)], "root: int and int are both read from an integer"),
        (Annotated[Card | int, ox.External()], "root: only models can be variants"),
        (Annotated[Dog | Cat, ox.Tagged("type")], 'root: the union is tagged by "type", but Dog'),
    ],
)
def test_rejects_ambiguous_unions(target: object, message: str) -> None:
    with pytest.raises(ox.SchemaError) as raised:
        ox.warmup(target)
    assert str(raised.value).startswith(message)
