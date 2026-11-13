"""Helpers and models shared by the JSON Schema tests."""

import enum
import sys
import types
from collections.abc import Mapping
from typing import Annotated, Any, Literal

from jsonschema import Draft202012Validator

import oxiform as ox

COMPONENTS = "#/components/schemas/{name}"


class Color(enum.Enum):
    RED = "red"
    BLUE = "blue"


class Level(enum.IntEnum):
    LOW = 1
    HIGH = 2


@ox.model(rename_all="camelCase")
class Address:
    street: str
    postal_code: Annotated[str, ox.Pattern(r"^\d{4}-\d{3}$")]
    country: Literal["PT", "DE", "BR"] = "PT"


@ox.model(rename_all="camelCase")
class Customer:
    """Someone who buys."""

    customer_id: int = ox.field(rename="id")
    name: Annotated[str, ox.Len(1, 80)]
    nickname: str | None = ox.field(default=None, skip_if_none=True)
    age: Annotated[int, ox.Ge(0), ox.Le(150)] | None = None
    addresses: list[Address] = ox.field(default_factory=list[Address])
    favorite: Color = Color.RED
    password_hash: str = ox.field(default="", skip_encode=True)
    score: float = ox.field(default=0.0, skip_decode=True)
    old_name: str = ox.field(default="", aliases=["legacyName"])


@ox.model
class Category:
    name: str
    children: "list[Category]" = ox.field(default_factory=list["Category"])
    parent: "Category | None" = None


@ox.model
class Order:
    customer: Customer
    shipping: Address
    billing: Address | None = None
    category: Category | None = None


@ox.model(tag_field="kind")
class Animal:
    name: str


@ox.model(tag="dog")
class Dog(Animal):
    breed: str


@ox.model(tag="cat")
class Cat(Animal):
    indoor: bool


@ox.model(unknown="forbid")
class Card:
    number: str


@ox.model
class Pix:
    key: str


@ox.model
class Payment:
    internal: Annotated[Card | Pix, ox.Tagged("type")]
    adjacent: Annotated[Card | Pix, ox.Tagged("type", content="data")]
    external: Annotated[Card | Pix, ox.External()]
    untagged: Annotated[Card | Pix, ox.Untagged()]
    pet: Dog | Cat
    maybe: Annotated[Card | Pix | None, ox.Tagged("type")] = None


@ox.model
class Meta:
    created_by: str


@ox.model(unknown="forbid")
class Flat:
    id: int
    meta: Meta = ox.field(flatten=True)


class Checker:
    """A JSON Schema 2020-12 validator, typed: the schema is checked against the metaschema
    (offline, the one the package carries) when the checker is made."""

    def __init__(self, schema: Mapping[str, Any]) -> None:
        check_schema(schema)
        self._validator: Any = Draft202012Validator(schema)

    def is_valid(self, instance: object) -> bool:
        return bool(self._validator.is_valid(instance))


def check_schema(schema: Mapping[str, Any]) -> None:
    """Raises ``jsonschema.SchemaError`` unless ``schema`` is valid JSON Schema 2020-12."""
    checker: Any = Draft202012Validator
    checker.check_schema(schema)


def validator(bundle: ox.SchemaBundle, index: int = 0) -> Checker:
    """A checker for the schema of a target, with the bundle's definitions beside it."""
    document: dict[str, Any] = {
        **bundle.schemas[index],
        "components": {"schemas": bundle.defs},
        "$defs": bundle.defs,
    }
    return Checker(document)


def check_metaschema(bundle: ox.SchemaBundle) -> None:
    """Every schema of the bundle is valid JSON Schema 2020-12 by itself."""
    for schema in (*bundle.defs.values(), *bundle.schemas):
        check_schema(schema)


def module_with(name: str, source: str) -> types.ModuleType:
    """A module that is importable by name, so that its classes resolve their annotations."""
    module = types.ModuleType(name)
    sys.modules[name] = module
    exec(source, module.__dict__)  # noqa: S102
    return module
