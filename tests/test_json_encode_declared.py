"""``encode(obj, type=T)`` walks the declared type ``T``, not the class of ``obj`` (OX-2)."""

import abc
import dataclasses
import json
from collections.abc import Callable
from dataclasses import dataclass
from datetime import UTC, date, datetime
from decimal import Decimal
from enum import Enum, IntEnum
from types import SimpleNamespace
from typing import Annotated, Any, Literal, NamedTuple, TypedDict
from unittest.mock import Mock

import pytest

import oxiform as ox


@dataclass(slots=True)
class Public:
    id: int
    name: str


@dataclass(slots=True)
class Private(Public):
    password_hash: str = "hash"
    notes: list[str] = dataclasses.field(default_factory=list[str])


@dataclass
class PlainPublic:
    id: int
    name: str


@dataclass
class PlainPrivate(PlainPublic):
    secret: str = "s3cret"


@ox.model
class ModelPublic:
    id: int


@ox.model
class ModelPrivate(ModelPublic):
    secret: str = "s3cret"


class CompactPublic(ox.Model, frozen=True):
    id: int


class CompactPrivate(CompactPublic, frozen=True):
    secret: str = "s3cret"


@dataclass
class Wrapper:
    owner: Public
    others: list[Public]
    by_name: dict[str, Public]
    maybe: Public | None = None
    pair: tuple[Public, int] = (Public(0, ""), 0)
    many: tuple[Public, ...] = ()


@dataclass
class Tree:
    value: int
    children: "list[Tree]"


@dataclass
class SecretTree(Tree):
    secret: str = "x"


@ox.model(rename_all="camelCase", skip_if_none=True)
class Profile:
    user_id: int = ox.field(rename="id")
    display_name: str = ""
    nick_name: str | None = None
    token: str = ox.field(default="", skip_encode=True)
    score: float = ox.field(default=0.0, skip=True)
    flags: int = ox.field(default=0, skip_if_default=True)


@ox.model
class ProfileWithSecrets(Profile):
    ssn: str = "000"


@ox.model
class Meta:
    created_by: str
    version: int = 1


@ox.model
class MetaPlus(Meta):
    internal: str = "x"


@ox.model
class Document:
    title: str
    meta: Meta = ox.field(flatten=True)


@ox.model(tag_field="kind")
class Animal:
    name: str


@ox.model(tag="dog")
class Dog(Animal):
    breed: str = "lab"


@ox.model(tag="puppy")
class Puppy(Dog):
    age: int = 0


@ox.model(tag="hound")
class Hound(Puppy):
    loud: bool = True


@ox.model(tag="cat")
class Cat(Animal):
    indoor: bool = True


@ox.model
class Card:
    number: str


@ox.model
class Pix:
    key: str


@ox.model
class BigCard(Card):
    cvv: str = "123"


class Pair(TypedDict):
    id: int


class Point(NamedTuple):
    x: int
    y: int


def encoded(value: object, type: Any, **options: Any) -> Any:
    return json.loads(ox.json.encode(value, type=type, **options))


# --- the security property: fields outside the declared type are never written ------------


@pytest.mark.parametrize(
    ("value", "declared", "expected"),
    [
        (Private(1, "a"), Public, {"id": 1, "name": "a"}),
        (PlainPrivate(1, "a"), PlainPublic, {"id": 1, "name": "a"}),
        (ModelPrivate(1), ModelPublic, {"id": 1}),
        (CompactPrivate(1), CompactPublic, {"id": 1}),
    ],
)
def test_writes_only_the_fields_of_the_declared_model(
    value: object, declared: type, expected: object
) -> None:
    assert encoded(value, declared) == expected
    assert len(ox.json.encode(value)) > len(ox.json.encode(value, type=declared))


def test_default_encode_still_walks_the_class_of_the_value() -> None:
    value = Private(1, "a")
    assert json.loads(ox.json.encode(value)) == {
        "id": 1,
        "name": "a",
        "password_hash": "hash",
        "notes": [],
    }
    assert ox.json.encode(value, type=None) == ox.json.encode(value)
    assert ox.json.encode(value, type=Private) == ox.json.encode(value)


def test_a_field_declared_as_the_base_holds_a_subclass_and_writes_only_the_base() -> None:
    wrapper = Wrapper(
        owner=Private(1, "a"),
        others=[Private(2, "b"), Public(3, "c")],
        by_name={"x": Private(4, "d")},
        maybe=Private(5, "e"),
        pair=(Private(6, "f"), 7),
        many=(Private(8, "g"),),
    )
    result = encoded(wrapper, Wrapper)
    assert result == {
        "owner": {"id": 1, "name": "a"},
        "others": [{"id": 2, "name": "b"}, {"id": 3, "name": "c"}],
        "by_name": {"x": {"id": 4, "name": "d"}},
        "maybe": {"id": 5, "name": "e"},
        "pair": [{"id": 6, "name": "f"}, 7],
        "many": [{"id": 8, "name": "g"}],
    }
    assert "password_hash" in ox.json.encode(wrapper).decode()
    assert "password_hash" not in ox.json.encode(wrapper, type=Wrapper).decode()


def test_writes_collections_of_a_declared_model() -> None:
    items = [Private(1, "a"), Private(2, "b")]
    expected = [{"id": 1, "name": "a"}, {"id": 2, "name": "b"}]
    assert encoded(items, list[Public]) == expected
    assert encoded(tuple(items), tuple[Public, ...]) == expected
    assert encoded(items, tuple[Public, Public]) == expected
    assert encoded({"a": items[0]}, dict[str, Public]) == {"a": expected[0]}
    assert encoded(items[0], Public | None) == expected[0]
    assert encoded(None, Public | None) is None
    frozen = (ModelPrivate(1),)
    assert encoded(frozen, tuple[ModelPublic, ...]) == [{"id": 1}]


def test_sets_are_written_sorted_with_their_declared_items() -> None:
    assert encoded({3, 1, 2}, set[int]) == [1, 2, 3]
    assert encoded(frozenset({"b", "a"}), frozenset[str]) == ["a", "b"]


def test_a_recursive_model_is_filtered_at_every_level() -> None:
    tree = SecretTree(1, [SecretTree(2, [])])
    assert encoded(tree, Tree) == {"value": 1, "children": [{"value": 2, "children": []}]}


def test_refuses_a_reference_cycle_under_a_declared_type() -> None:
    tree = Tree(1, [])
    tree.children.append(tree)
    with pytest.raises(ox.EncodeError, match="reference cycle"):
        ox.json.encode(tree, type=Tree)


# --- objects and mappings that are not instances of the declared model ---------------------


def test_reads_a_mapping_by_key_and_an_object_by_attribute() -> None:
    expected = {"id": 1, "name": "a"}
    assert encoded({"id": 1, "name": "a", "password_hash": "x"}, Public) == expected
    assert encoded(SimpleNamespace(id=1, name="a", password_hash="x"), Public) == expected
    assert encoded(PlainPrivate(1, "a"), Public) == expected  # another model, same fields
    assert encoded([{"id": 1, "name": "a", "extra": 1}], list[Public]) == [expected]


def test_mapping_keys_are_the_python_names_of_the_fields() -> None:
    mapping = {"user_id": 7, "display_name": "Ana", "nick_name": "A", "flags": 1}
    assert encoded(mapping, Profile) == {"id": 7, "displayName": "Ana", "nickName": "A", "flags": 1}
    with pytest.raises(ox.EncodeError, match='has no key "user_id"'):
        ox.json.encode({"id": 7, "displayName": "Ana"}, type=Profile)


def test_a_field_that_is_missing_names_the_field_and_the_type() -> None:
    with pytest.raises(ox.EncodeError) as raised:
        ox.json.encode({"id": 1}, type=Public)
    message = str(raised.value)
    assert 'cannot encode dict as Public: the mapping has no key "name"' in message
    assert "add it, or declare a type without that field" in message
    with pytest.raises(
        ox.EncodeError, match='SimpleNamespace as Public: the object has no attribute "name"'
    ):
        ox.json.encode(SimpleNamespace(id=1), type=Public)


def test_a_missing_nested_field_reports_where() -> None:
    wrapper: dict[str, Any] = {
        "owner": {"id": 1, "name": "a"},
        "others": [{"id": 2}],
        "by_name": {},
    }
    with pytest.raises(ox.EncodeError) as raised:
        ox.json.encode(wrapper, type=Wrapper)
    assert raised.value.pointer == "/others/0"
    assert str(raised.value).endswith("at /others/0")


def test_an_unset_slot_counts_as_missing() -> None:
    value = Public(1, "a")
    del value.name
    with pytest.raises(ox.EncodeError, match='has no attribute "name"'):
        ox.json.encode(value, type=Public)


@pytest.mark.parametrize("value", [None, 1, "text", 1.5, True, [1], (1,), {1}, b"x"])
def test_a_value_that_cannot_hold_fields_is_not_a_model(value: object) -> None:
    with pytest.raises(ox.EncodeError, match="cannot encode .* as Public: expected an instance"):
        ox.json.encode(value, type=Public)


def test_a_model_is_not_a_value_of_a_type_without_fields() -> None:
    for declared in (int, str, Decimal, date):
        with pytest.raises(ox.EncodeError, match=f"as {declared.__name__}: the value is not of"):
            ox.json.encode(Private(1, "a"), type=declared)
    with pytest.raises(ox.EncodeError, match="as int: the value is not of the declared type"):
        ox.json.encode({"id": 1}, type=int)
    with pytest.raises(ox.EncodeError, match="as a list"):
        ox.json.encode({"a": 1}, type=list[int])
    with pytest.raises(ox.EncodeError, match="as a dict"):
        ox.json.encode([1], type=dict[str, int])
    with pytest.raises(ox.EncodeError, match="a tuple"):
        ox.json.encode("ab", type=tuple[int, int])


def test_a_leaf_inside_a_model_is_guarded_too() -> None:
    @dataclass
    class Holder:
        number: int

    with pytest.raises(ox.EncodeError) as raised:
        ox.json.encode(Holder(Private(1, "a")), type=Holder)  # pyright: ignore[reportArgumentType]
    assert raised.value.pointer == "/number"


def test_a_tuple_of_the_wrong_length_is_refused() -> None:
    with pytest.raises(ox.EncodeError, match="3 items as a tuple of 2"):
        ox.json.encode((1, 2, 3), type=tuple[int, int])


def test_scalars_are_written_by_their_class() -> None:
    now = datetime(2026, 1, 2, 3, 4, 5, tzinfo=UTC)
    assert encoded(1, int) == 1
    assert encoded(True, bool) is True
    assert encoded(1, float) == 1
    assert encoded("a", str) == "a"
    assert encoded(Decimal("1.50"), Decimal) == "1.50"
    assert encoded(now, datetime) == "2026-01-02T03:04:05Z"
    assert encoded(date(2026, 1, 2), date) == "2026-01-02"
    assert encoded(b"ab", bytes) == "YWI="
    assert encoded(None, type(None)) is None
    assert encoded({"a": [1, 2.5]}, Any) == {"a": [1, 2.5]}
    assert encoded(Private(1, "a"), Any) == {
        "id": 1,
        "name": "a",
        "password_hash": "hash",
        "notes": [],
    }
    assert encoded(1, Annotated[int, ox.Ge(0)]) == 1


# --- unions: the variant is chosen by the class of the value -------------------------------


def test_a_union_picks_the_variant_of_the_class_of_the_value() -> None:
    declared = Annotated[Card | Pix, ox.Untagged()]
    assert encoded(Card("1"), declared) == {"number": "1"}
    assert encoded(Pix("k"), declared) == {"key": "k"}
    with pytest.raises(ox.SchemaError, match="both read from a map"):
        ox.json.encode(Card("1"), type=Card | Pix)  # decode cannot tell them apart either


def test_a_union_of_models_that_decode_by_tag_picks_by_class() -> None:
    declared = Dog | Cat
    assert encoded(Dog("rex"), declared) == {"kind": "dog", "name": "rex", "breed": "lab"}
    assert encoded(Cat("tom"), declared) == {"kind": "cat", "name": "tom", "indoor": True}


def test_a_subclass_in_a_union_is_written_as_the_first_variant_it_is_an_instance_of() -> None:
    puppy = Puppy("rex", age=1)
    hound = Hound("rex", age=2)
    assert encoded(hound, Dog | Cat) == {"kind": "dog", "name": "rex", "breed": "lab"}
    # The class of the value wins over declaration order.
    assert encoded(puppy, Annotated[Dog | Puppy, ox.Tagged("kind")]) == {
        "kind": "puppy",
        "name": "rex",
        "breed": "lab",
        "age": 1,
    }
    assert encoded(puppy, Annotated[Dog | Puppy, ox.Untagged()])["age"] == 1
    # With no exact class, declaration order decides: the fewer fields, the safer.
    assert encoded(hound, Annotated[Dog | Puppy, ox.Untagged()]) == {
        "kind": "dog",
        "name": "rex",
        "breed": "lab",
    }
    # (A union compares equal in any order, so the other order is a different alias only
    # under another marker.)
    assert encoded(hound, Annotated[Puppy | Dog, ox.External()])["puppy"]["age"] == 2


def test_a_union_of_kinds_picks_by_kind() -> None:
    declared = int | str | list[Public] | None
    assert encoded(1, declared) == 1
    assert encoded("a", declared) == "a"
    assert encoded(None, declared) is None
    assert encoded([Private(1, "a")], declared) == [{"id": 1, "name": "a"}]
    assert encoded(True, bool | int) is True
    assert encoded(2, float | str) == 2  # an int stands for a float when nothing is closer
    assert encoded(2, float | int) == 2


def test_a_union_with_a_model_and_a_dict() -> None:
    declared = Annotated[Public | dict[str, int], ox.Untagged()]
    assert encoded({"a": 1}, declared) == {"a": 1}
    assert encoded(Private(1, "a"), declared) == {"id": 1, "name": "a"}


def test_a_mapping_stands_for_the_first_model_it_has_every_field_of() -> None:
    declared = Annotated[Card | Pix, ox.Untagged()]
    assert encoded({"key": "k", "extra": 1}, declared) == {"key": "k"}
    assert encoded({"number": "1"}, declared) == {"number": "1"}
    assert encoded(SimpleNamespace(number="9", extra=1), declared) == {"number": "9"}


def test_a_value_that_is_none_of_the_variants_is_refused() -> None:
    for declared in (Annotated[Card | Pix, ox.Untagged()], int | str, list[int] | str):
        with pytest.raises(ox.EncodeError, match="none of the declared variants"):
            ox.json.encode(Private(1, "a"), type=declared)
    with pytest.raises(ox.EncodeError, match="none of the declared variants"):
        ox.json.encode(1.5, type=int | str)
    with pytest.raises(ox.EncodeError, match="none of the declared variants"):
        ox.json.encode({"other": 1}, type=Annotated[Card | Pix, ox.Untagged()])


def test_tagged_unions_write_the_tag_of_the_variant_in_the_declared_mode() -> None:
    internal = Annotated[Card | Pix, ox.Tagged("type")]
    assert encoded(BigCard("1"), internal) == {"type": "Card", "number": "1"}
    assert encoded(Pix("k"), internal) == {"type": "Pix", "key": "k"}
    adjacent = Annotated[Card | Pix, ox.Tagged("type", content="data")]
    assert encoded(BigCard("1"), adjacent) == {"type": "Card", "data": {"number": "1"}}
    external = Annotated[Card | Pix | None, ox.External()]
    assert encoded(BigCard("1"), external) == {"Card": {"number": "1"}}
    assert encoded(None, external) is None
    untagged = Annotated[Card | Pix, ox.Untagged()]
    assert encoded(BigCard("1"), untagged) == {"number": "1"}


def test_tagged_unions_inside_models_filter_their_variants() -> None:
    @ox.model
    class Payment:
        method: Annotated[Card | Pix, ox.Tagged("type")]
        history: list[Annotated[Card | Pix, ox.External()]]

    payment = Payment(BigCard("1"), [BigCard("2"), Pix("k")])
    assert encoded(payment, Payment) == {
        "method": {"type": "Card", "number": "1"},
        "history": [{"Card": {"number": "2"}}, {"Pix": {"key": "k"}}],
    }


def test_the_tag_written_is_the_one_of_the_declared_class() -> None:
    assert encoded(Puppy("rex"), Animal) == {"kind": "Animal", "name": "rex"}
    assert encoded(Puppy("rex"), Dog) == {"kind": "dog", "name": "rex", "breed": "lab"}


# --- field attributes and model options ----------------------------------------------------


def test_applies_wire_names_omissions_and_skips_of_the_declared_model() -> None:
    profile = ProfileWithSecrets(user_id=1, display_name="Ana", token="t", flags=0)
    assert encoded(profile, Profile) == {"id": 1, "displayName": "Ana"}
    # The same model written as itself, to see what the declared type removed.
    assert encoded(profile, ProfileWithSecrets) == {"id": 1, "displayName": "Ana", "ssn": "000"}
    assert encoded(ProfileWithSecrets(1, "Ana", "n", flags=3), Profile) == {
        "id": 1,
        "displayName": "Ana",
        "nickName": "n",
        "flags": 3,
    }


def test_omissions_apply_to_mappings_and_objects_too() -> None:
    assert encoded({"user_id": 1, "display_name": "x", "nick_name": None, "flags": 0}, Profile) == {
        "id": 1,
        "displayName": "x",
    }


def test_flattens_the_declared_model_of_a_flattened_field() -> None:
    document = Document("t", MetaPlus("me", internal="secret"))
    assert encoded(document, Document) == {"title": "t", "created_by": "me", "version": 1}
    assert json.loads(ox.json.encode(document)) == {
        "title": "t",
        "created_by": "me",
        "version": 1,
        "internal": "secret",
    }
    mapping = {"title": "t", "meta": {"created_by": "me", "version": 3, "internal": "x"}}
    assert encoded(mapping, Document) == {"title": "t", "created_by": "me", "version": 3}


def test_a_mapping_flattened_field_needs_its_fields() -> None:
    with pytest.raises(ox.EncodeError, match='as Meta: the mapping has no key "created_by"'):
        ox.json.encode({"title": "t", "meta": {}}, type=Document)
    with pytest.raises(ox.EncodeError, match="as Meta: expected an instance"):
        ox.json.encode({"title": "t", "meta": None}, type=Document)


def test_skip_encode_fields_stay_out_for_subclasses_and_mappings() -> None:
    mapping = {"user_id": 1, "display_name": "", "nick_name": None, "flags": 0, "token": "x"}
    assert encoded(mapping, Profile) == {"id": 1, "displayName": ""}
    assert b"token" not in ox.json.encode(ProfileWithSecrets(1, token="x"), type=Profile)


# --- format options, the encoder and caching -----------------------------------------------


def test_sorts_keys_and_indents_with_a_declared_type() -> None:
    @ox.model
    class Unsorted:
        zebra: int
        apple: int

    assert ox.json.encode(Unsorted(1, 2), type=Unsorted, sort_keys=True) == b'{"apple":2,"zebra":1}'
    assert ox.json.encode(Unsorted(1, 2), type=Unsorted) == b'{"zebra":1,"apple":2}'
    assert ox.json.encode(Unsorted(1, 2), type=Unsorted, indent=2) == (
        b'{\n  "zebra": 1,\n  "apple": 2\n}'
    )
    assert encoded(
        {"b": Private(1, "x"), "a": Private(2, "y")}, dict[str, Public], sort_keys=True
    ) == {"a": {"id": 2, "name": "y"}, "b": {"id": 1, "name": "x"}}
    assert (
        ox.json.encode(
            {"b": Private(1, "x"), "a": Private(2, "y")}, type=dict[str, Public], sort_keys=True
        )
        == b'{"a":{"id":2,"name":"y"},"b":{"id":1,"name":"x"}}'
    )


def test_the_encoder_takes_a_default_type_and_a_type_per_call() -> None:
    encoder = ox.json.Encoder(type=Public)
    assert encoder.type is Public
    assert encoder.encode(Private(1, "a")) == b'{"id":1,"name":"a"}'
    assert encoder.encode(Private(1, "a"), type=Private) == ox.json.encode(Private(1, "a"))
    plain = ox.json.Encoder(sort_keys=True)
    assert plain.type is None
    assert plain.encode(Private(1, "a")) == ox.json.encode(Private(1, "a"), sort_keys=True)
    assert plain.encode(Private(1, "a"), type=Public) == b'{"id":1,"name":"a"}'
    with pytest.raises(ox.SchemaError):
        ox.json.Encoder(type=complex)


def test_the_plan_of_a_type_is_reused_and_keeps_its_own_options() -> None:
    for _ in range(3):
        assert ox.json.encode(Private(1, "a"), type=Public) == b'{"id":1,"name":"a"}'
        assert ox.json.encode(Private(1, "a")) != ox.json.encode(Private(1, "a"), type=Public)
    unhashable = Annotated[list[int], ox.Len(max=3), ["unhashable", "note"]]
    assert ox.json.encode([1, 2], type=unhashable) == b"[1,2]"


def test_declared_encode_round_trips_through_decode() -> None:
    wrapper = Wrapper(Public(1, "a"), [Public(2, "b")], {"x": Public(3, "c")}, Public(4, "d"))
    raw = ox.json.encode(wrapper, type=Wrapper)
    assert ox.json.decode(raw, Wrapper) == wrapper
    document = Document("t", Meta("me", 2))
    assert ox.json.decode(ox.json.encode(document, type=Document), Document) == document
    profile = Profile(user_id=1, display_name="Ana", nick_name="n", flags=3)
    assert ox.json.decode(ox.json.encode(profile, type=Profile), Profile) == profile


# --- declarations ---------------------------------------------------------------------------


def test_the_declared_type_must_be_a_type_oxiform_accepts() -> None:
    for declared in (
        complex,
        dict[int, str],
        Annotated[int, ox.Ge(True)],
        Annotated[int | str, ox.Ge(0)],
    ):
        with pytest.raises(ox.SchemaError):
            ox.json.encode(1, type=declared)


def test_typeddict_and_namedtuple_are_not_declarable_yet() -> None:
    # Section 3.5 lists them, but ``describe`` has no reader for either, so neither decodes.
    with pytest.raises(ox.SchemaError, match="unsupported type"):
        ox.json.encode({"id": 1}, type=Pair)
    with pytest.raises(ox.SchemaError, match="unsupported type"):
        ox.json.encode(Point(1, 2), type=Point)
    with pytest.raises(ox.SchemaError, match="unsupported type"):
        ox.json.decode(b'{"id": 1}', Pair)


# --- objects that only claim to be instances never reach the slot reader --------------------


@dataclass(slots=True)
class SlottedDS:
    a: int
    b: str


@dataclass
class PlainDS:
    a: int
    b: str


@ox.model
class ModelDS:
    a: int
    b: str


class _ClaimsEverything(type):
    def __instancecheck__(cls, instance: object) -> bool:
        return True


@dataclass(slots=True)
class _WithInstanceCheck(metaclass=_ClaimsEverything):
    a: int
    b: str


@dataclass(slots=True)
class _AbstractDS(abc.ABC):
    a: int
    b: str


class _Impostor:
    """Not an instance of anything, with the attributes of a model."""

    def __init__(self) -> None:
        self.a = 1
        self.b = "x"


class _Proxy:
    """Reports another class through ``__class__``, as lazy-object proxies and mocks do."""

    def __init__(self, cls: type) -> None:
        self._cls = cls
        self.a = 1
        self.b = "x"

    @property
    def __class__(self) -> type:  # pyright: ignore[reportIncompatibleMethodOverride]
        return self._cls


_ABSTRACT_REGISTERED = _AbstractDS.register(_Impostor)
_ANY_CLASS = [SlottedDS, PlainDS, ModelDS, _WithInstanceCheck, _AbstractDS]


@pytest.mark.parametrize("declared", _ANY_CLASS)
def test_an_object_that_claims_to_be_an_instance_is_read_by_attribute(declared: type) -> None:
    assert encoded(_Proxy(declared), declared) == {"a": 1, "b": "x"}
    assert encoded(_Impostor(), declared) == {"a": 1, "b": "x"}
    assert encoded([_Proxy(declared)], list[declared]) == [{"a": 1, "b": "x"}]  # type: ignore[valid-type]


@pytest.mark.parametrize("declared", [SlottedDS, PlainDS, ModelDS])
def test_a_mock_with_a_spec_does_not_crash(declared: type) -> None:
    mock = Mock(spec=declared)
    assert isinstance(mock, declared)  # what Mock claims
    with pytest.raises(ox.EncodeError, match="cannot encode Mock as"):
        ox.json.encode(mock, type=declared)
    mock.a, mock.b = 1, "x"
    assert encoded(mock, declared) == {"a": 1, "b": "x"}
    assert encoded({"inner": mock}, dict[str, declared]) == {"inner": {"a": 1, "b": "x"}}  # type: ignore[valid-type]


def test_an_unset_slot_of_a_real_instance_is_missing_and_a_subclass_is_read_by_slot() -> None:
    @dataclass(slots=True)
    class Sub(SlottedDS):
        c: int = 3

    assert encoded(Sub(1, "x"), SlottedDS) == {"a": 1, "b": "x"}
    value = Sub(1, "x")
    del value.a
    with pytest.raises(ox.EncodeError, match='has no attribute "a"'):
        ox.json.encode(value, type=SlottedDS)


def test_a_mapping_stands_for_the_variant_that_has_the_fields_without_running_getattr() -> None:
    class CatchAll:
        def __getattr__(self, name: str) -> object:
            return 1

    with pytest.raises(ox.EncodeError, match="none of the declared variants"):
        ox.json.encode(CatchAll(), type=Annotated[Card | Pix, ox.Untagged()])
    holder = SimpleNamespace(key="k")
    assert encoded(holder, Annotated[Card | Pix, ox.Untagged()]) == {"key": "k"}


# --- scalars are checked against the declared type ------------------------------------------


class Status(Enum):
    ON = "on"


class Level(IntEnum):
    LOW = 1


@pytest.mark.parametrize(
    ("value", "declared"),
    [
        ("SECRET", Literal["a", "b"]),
        (1, Literal[True]),
        (True, Literal[1]),
        (Level.LOW, Literal[1]),
        (1, type(None)),
        (1, bool),
        (0, bool),
        (True, float),
        (True, int),
        (1.5, int),
        ("1", int),
        (b"x", str),
        ("x", bytes),
        (1, str),
        (Decimal(1), float),
        (1, Decimal),
        ("on", Status),
        (1, Status),
        (datetime(2026, 1, 2, tzinfo=UTC), date),
        (None, int),
        (None, str),
    ],
)
def test_a_scalar_must_be_of_the_declared_type(value: object, declared: Any) -> None:
    with pytest.raises(ox.EncodeError) as raised:
        ox.json.encode(value, type=declared)
    assert "the value is not of the declared type" in str(raised.value)
    assert "SECRET" not in str(raised.value)  # the message names the type, never the value


@pytest.mark.parametrize(
    ("value", "declared", "expected"),
    [
        ("a", Literal["a", "b"], "a"),
        (1, Literal[1, "x"], 1),
        (True, Literal[True], True),
        (None, Literal[None], None),
        (Status.ON, Literal[Status.ON], "on"),
        (None, type(None), None),
        (True, bool, True),
        (1, int, 1),
        (Level.LOW, int, 1),
        (Level.LOW, Level, 1),
        (1.5, float, 1.5),
        (2, float, 2),  # an int stands for a float, a bool never does
        ("x", str, "x"),
        (Status.ON, Status, "on"),
        (b"x", bytes, "eA=="),
        (bytearray(b"x"), bytes, "eA=="),
        (Decimal("1.5"), Decimal, "1.5"),
        (date(2026, 1, 2), date, "2026-01-02"),
    ],
)
def test_a_scalar_of_the_declared_type_is_written_by_its_class(
    value: object, declared: Any, expected: object
) -> None:
    assert encoded(value, declared) == expected


def test_scalars_are_checked_inside_models_and_collections() -> None:
    @dataclass
    class Result:
        status: Literal["ok", "error"]
        count: int
        ratio: float
        tags: list[str]

    assert encoded(Result("ok", 1, 2, ["a"]), Result) == {
        "status": "ok",
        "count": 1,
        "ratio": 2,
        "tags": ["a"],
    }
    for bad in (
        Result("SECRET", 1, 1.0, []),  # pyright: ignore[reportArgumentType]
        Result("ok", True, 1.0, []),
        Result("ok", 1, True, []),
        Result("ok", 1, 1.0, [b"x"]),  # pyright: ignore[reportArgumentType]
    ):
        with pytest.raises(ox.EncodeError) as raised:
            ox.json.encode(bad, type=Result)
        assert raised.value.pointer in ("/status", "/count", "/ratio", "/tags/0")
    assert encoded({"status": "ok", "count": 1, "ratio": 1.0, "tags": []}, Result)


def test_a_field_whose_annotation_oxiform_cannot_read_only_takes_plain_values() -> None:
    @ox.model
    class Loose:
        a: int
        callback: Callable[[int], int] | None = ox.field(default=None, skip_decode=True)

    # Fields that oxiform cannot read as a type keep the guard of a type without fields.
    assert encoded(Loose(1, None), Loose) == {"a": 1, "callback": None}


# --- the two escapes: Any and bare containers -----------------------------------------------


def test_any_and_bare_containers_write_by_the_class_of_the_value() -> None:
    secret = Private(1, "a")
    full: dict[str, object] = {"id": 1, "name": "a", "password_hash": "hash", "notes": []}
    assert encoded(secret, Any) == full
    assert encoded([secret], list) == [full]
    assert encoded({"k": secret}, dict) == {"k": full}
    assert encoded((secret,), tuple) == [full]
    assert encoded({1}, set) == [1]
    assert encoded([secret], list[Any]) == [full]
    # A declared item type filters.
    assert encoded([secret], list[Public]) == [{"id": 1, "name": "a"}]


def test_type_none_means_no_declared_type_and_none_is_spelled_type_of_none() -> None:
    secret = Private(1, "a")
    assert ox.json.encode(secret, type=None) == ox.json.encode(secret)
    assert ox.json.encode(None, type=type(None)) == b"null"
    with pytest.raises(ox.EncodeError):
        ox.json.encode(1, type=type(None))
