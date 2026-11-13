"""What each type becomes in JSON Schema, in each variant, and that real data fits it."""

import datetime
import decimal
import enum
import ipaddress
import json
import pathlib
import re
import uuid
import zoneinfo
from typing import Annotated, Any, Literal

import pytest
from schema_support import (
    COMPONENTS,
    Animal,
    Card,
    Cat,
    Checker,
    Color,
    Dog,
    Flat,
    Level,
    Payment,
    Pix,
    check_metaschema,
    validator,
)

import oxiform as ox
from oxiform._json_schema import patterns


def schema(target: Any, **options: Any) -> Any:
    return ox.json_schema_bundle([target], ref_template=COMPONENTS, **options).schemas[0]


def bundle(target: Any, **options: Any) -> ox.SchemaBundle:
    return ox.json_schema_bundle([target], ref_template=COMPONENTS, **options)


BYTES = {"type": "string", "contentEncoding": "base64", "pattern": patterns.BASE64_TEXT}


@pytest.mark.parametrize(
    ("target", "expected"),
    [
        (int, {"type": "integer"}),
        (float, {"type": "number"}),
        (bool, {"type": "boolean"}),
        (str, {"type": "string"}),
        (type(None), {"type": "null"}),
        (None, {"type": "null"}),
        (Any, {}),
        (bytes, BYTES),
        (bytearray, BYTES),
        (datetime.datetime, {"type": "string", "format": "date-time"}),
        (datetime.date, {"type": "string", "format": "date"}),
        (datetime.timedelta, {"type": "string", "format": "duration"}),
        (uuid.UUID, {"type": "string", "format": "uuid"}),
        (ipaddress.IPv4Address, {"type": "string", "format": "ipv4"}),
        (ipaddress.IPv6Address, {"type": "string", "format": "ipv6"}),
        (ipaddress.IPv4Network, {"type": "string", "format": "ipv4-network"}),
        (ipaddress.IPv6Interface, {"type": "string", "format": "ipv6-interface"}),
        (pathlib.Path, {"type": "string", "format": "path"}),
        (zoneinfo.ZoneInfo, {"type": "string", "format": "zoneinfo"}),
        (enum.IntFlag("Perm", "R W"), {"type": "integer"}),
        (tuple[()], {"type": "array", "maxItems": 0}),
        (list, {"type": "array", "items": {}}),
        (dict, {"type": "object", "additionalProperties": {}}),
        (tuple, {"type": "array", "items": {}}),
        (set, {"type": "array", "items": {}}),
        (list[int], {"type": "array", "items": {"type": "integer"}}),
        (frozenset[str], {"type": "array", "items": {"type": "string"}}),
        (tuple[int, ...], {"type": "array", "items": {"type": "integer"}}),
        (
            tuple[int, str],
            {
                "type": "array",
                "prefixItems": [{"type": "integer"}, {"type": "string"}],
                "minItems": 2,
                "maxItems": 2,
            },
        ),
        (
            dict[str, list[int]],
            {
                "type": "object",
                "additionalProperties": {"type": "array", "items": {"type": "integer"}},
            },
        ),
        (int | None, {"anyOf": [{"type": "integer"}, {"type": "null"}]}),
        (
            int | str | list[int] | None,
            {
                "anyOf": [
                    {"type": "integer"},
                    {"type": "string"},
                    {"type": "array", "items": {"type": "integer"}},
                    {"type": "null"},
                ]
            },
        ),
        (Literal["a"], {"type": "string", "const": "a"}),
        (Literal["a", "b"], {"type": "string", "enum": ["a", "b"]}),
        (Literal[1, 2], {"type": "integer", "enum": [1, 2]}),
        (Literal[True], {"type": "boolean", "const": True}),
        (Literal["a", 1, None], {"enum": ["a", 1, None]}),
        (Color, {"$ref": "#/components/schemas/Color"}),
    ],
)
def test_the_schema_of_each_type(target: Any, expected: dict[str, Any]) -> None:
    for mode in ("validation", "serialization"):
        written = schema(target, mode=mode)
        if mode == "serialization" and target in (set, frozenset[str]):
            expected = {**expected, "uniqueItems": True}
        assert written == expected
        check_metaschema(bundle(target, mode=mode))


def test_enums_are_definitions_of_their_values() -> None:
    defs = ox.json_schema_bundle([Color, Level], ref_template=COMPONENTS).defs
    assert defs == {
        "Color": {"type": "string", "enum": ["red", "blue"]},
        "Level": {"type": "integer", "enum": [1, 2]},
    }


def test_decimal_reads_a_number_or_a_string_and_writes_a_string() -> None:
    text = {"type": "string", "format": "decimal", "pattern": patterns.DECIMAL_TEXT}
    assert schema(decimal.Decimal) == {"anyOf": [{"type": "number"}, text]}
    assert schema(decimal.Decimal, mode="serialization") == text
    check = Checker(schema(decimal.Decimal))
    assert check.is_valid("1.5") and check.is_valid(1.5) and not check.is_valid([])


def test_constraints_become_keywords() -> None:
    assert schema(Annotated[int, ox.Ge(0), ox.Le(150)]) == {
        "type": "integer",
        "minimum": 0,
        "maximum": 150,
    }
    assert schema(Annotated[float, ox.Ge(0.5)]) == {"type": "number", "minimum": 0.5}
    assert schema(Annotated[int, ox.OneOf(1, 2)]) == {"type": "integer", "enum": [1, 2]}
    assert schema(Annotated[str, ox.Len(1, 80), ox.Pattern("^a")]) == {
        "type": "string",
        "minLength": 1,
        "maxLength": 80,
        "pattern": "^a",
    }
    assert schema(Annotated[str, ox.OneOf("a", "b")]) == {"type": "string", "enum": ["a", "b"]}
    assert schema(Annotated[list[int], ox.Len(max=3)]) == {
        "type": "array",
        "items": {"type": "integer"},
        "maxItems": 3,
    }
    assert schema(Annotated[set[int], ox.Len(2)])["minItems"] == 2
    assert schema(Annotated[dict[str, int], ox.Len(1, 2)])["minProperties"] == 1
    assert schema(Annotated[dict[str, int], ox.Len(1, 2)])["maxProperties"] == 2
    assert schema(Annotated[int | None, ox.Ge(1)]) == {
        "anyOf": [{"type": "integer", "minimum": 1}, {"type": "null"}]
    }
    assert schema(Annotated[bytes, ox.Len(2)]) == BYTES


def test_field_shortcuts_are_constraints() -> None:
    @ox.model
    class Shortcuts:
        rating: float = ox.field(default=0.0, ge=0, le=5)
        code: str = ox.field(default="a", min_len=1, max_len=3, pattern="^[a-z]+$")

    props = ox.json_schema_bundle([Shortcuts]).defs["Shortcuts"]["properties"]
    assert props["rating"] == {"type": "number", "minimum": 0, "maximum": 5, "default": 0.0}
    assert props["code"]["minLength"] == 1 and props["code"]["pattern"] == "^[a-z]+$"


def test_dates_and_times_without_a_format_get_patterns() -> None:
    naive = schema(ox.NaiveDateTime)
    assert naive["type"] == "string" and "format" not in naive
    check = Checker(naive)
    assert check.is_valid("2026-01-31T10:00:00") and check.is_valid("2026-01-31 10:00:00.5")
    assert not check.is_valid("2026-01-31T10:00:00Z")
    clock = Checker(schema(datetime.time))
    assert clock.is_valid("10:00:00") and clock.is_valid("10:00:00+01:00")
    assert not clock.is_valid("10:00")


def test_lax_mode_adds_the_text_oxiform_reads() -> None:
    integer = Checker(schema(int, coercion="lax"))
    assert all(integer.is_valid(value) for value in (42, "42", "-7", "007", "42.0", 42.0))
    assert not any(integer.is_valid(value) for value in ("4.5", "1e3", "x", "", True, None))
    number = Checker(schema(float, coercion="lax"))
    assert all(number.is_valid(value) for value in (1.5, "1.5", "-2", ".5", "1e3", "2E-3"))
    assert not any(number.is_valid(value) for value in ("inf", "nan", "", "1.2.3", None))
    boolean = Checker(schema(bool, coercion="lax"))
    for word in ("true", "False", "YES", "no", "On", "OFF", "1", "0"):
        assert boolean.is_valid(word)
    assert boolean.is_valid(True) and not boolean.is_valid("maybe") and not boolean.is_valid(1)
    assert Checker(schema(Annotated[int, ox.Ge(5)], coercion="lax")).is_valid("1")
    assert not Checker(schema(Annotated[int, ox.Ge(5)], coercion="lax")).is_valid(1)


def test_lax_mode_reads_choices_from_text_and_empty_as_none() -> None:
    assert Checker(schema(Literal[1, 2], coercion="lax")).is_valid("2")
    assert not Checker(schema(Literal[1, 2])).is_valid("2")
    levels = bundle(Level, coercion="lax")
    assert Checker({**levels.schemas[0], "components": {"schemas": levels.defs}}).is_valid("1")
    optional = Checker(schema(int | None, coercion="lax"))
    assert optional.is_valid("") and optional.is_valid(None) and optional.is_valid("3")
    assert not Checker(schema(int | None)).is_valid("")
    # A type that keeps "" does not read it as None.
    assert schema(str | None, coercion="lax") == {"anyOf": [{"type": "string"}, {"type": "null"}]}
    assert schema(Literal["", "a"] | None, coercion="lax")["anyOf"][-1] == {"type": "null"}
    assert schema(Annotated[int | str | None, ox.Untagged()], coercion="lax")["anyOf"][-1] == {
        "type": "null"
    }
    assert {"const": ""} in schema(Annotated[int | bool | None, ox.Untagged()], coercion="lax")[
        "anyOf"
    ]


def test_lax_mode_changes_a_models_definition() -> None:
    @ox.model
    class Query:
        page: int
        verbose: bool = False

    strict = ox.json_schema_bundle([Query]).defs["Query"]
    lax = ox.json_schema_bundle([Query], coercion="lax").defs["Query"]
    assert strict["properties"]["page"] == {"type": "integer"}
    assert lax["properties"]["page"]["anyOf"][1]["type"] == "string"
    check = Checker(lax)
    assert check.is_valid({"page": "3", "verbose": "yes"})
    assert not Checker(strict).is_valid({"page": "3"})


def test_internal_tags_by_tag_field_make_a_discriminated_union() -> None:
    found = bundle(Dog | Cat | None, mode="serialization")
    union = found.schemas[0]["anyOf"][0]
    assert union["discriminator"] == {
        "propertyName": "kind",
        "mapping": {"dog": "#/components/schemas/Dog", "cat": "#/components/schemas/Cat"},
    }
    # A plain $ref (with the tag required beside it), as the discriminator of OpenAPI wants.
    assert union["oneOf"][0] == {"$ref": "#/components/schemas/Dog", "required": ["kind"]}
    check = validator(found)
    assert check.is_valid({"kind": "dog", "name": "Rex", "breed": "lab"})
    assert not check.is_valid({"name": "Rex", "breed": "lab"})
    assert not check.is_valid({"kind": "cat", "name": "Rex", "breed": "lab"})
    assert check.is_valid(None)


def test_a_concrete_model_reads_its_tag_when_given_and_requires_it_on_output() -> None:
    request = ox.json_schema_bundle([Dog], ref_template=COMPONENTS).defs["Dog"]
    response = ox.json_schema_bundle([Dog], mode="serialization").defs["Dog"]
    assert "kind" not in request["required"] and "kind" in response["required"]
    assert request["properties"]["kind"] == response["properties"]["kind"]
    assert request["properties"]["kind"] == {"type": "string", "const": "dog"}
    assert not Checker(request).is_valid({"kind": "cat", "name": "a", "breed": "b"})
    assert Animal.__name__ == "Animal"


def test_every_tagged_mode_validates_what_decode_accepts() -> None:
    found = bundle(Payment)
    check = validator(found)
    body = {
        "internal": {"type": "Card", "number": "1"},
        "adjacent": {"type": "Pix", "data": {"key": "k"}},
        "external": {"Card": {"number": "1"}},
        "untagged": {"key": "k"},
        "pet": {"kind": "cat", "name": "Tom", "indoor": True},
        "maybe": {"type": "Pix", "key": "k"},
    }
    ox.json.decode(json.dumps(body), Payment)
    assert check.is_valid(body)
    cases: list[tuple[str, dict[str, Any]]] = [
        ("internal", {"type": "Cash", "number": "1"}),
        ("internal", {"type": "Card", "number": "1", "extra": 1}),
        ("internal", {"number": "1"}),
        ("adjacent", {"type": "Pix", "key": "k"}),
        ("external", {"Card": {"number": "1"}, "Pix": {"key": "k"}}),
        ("external", {"Cash": {}}),
        ("untagged", {"other": 1}),
    ]
    for field, broken in cases:
        assert not check.is_valid({**body, field: broken}), (field, broken)


def test_a_variant_that_forbids_unknown_keys_is_written_in_place_with_the_tag() -> None:
    payment = ox.json_schema_bundle([Payment]).defs["Payment"]["properties"]["internal"]
    card, pix = payment["oneOf"]
    assert card["additionalProperties"] is False
    assert card["properties"]["type"] == {"type": "string", "const": "Card"}
    assert card["required"] == ["number", "type"]
    assert pix["allOf"][0] == {"$ref": "#/$defs/Pix"}
    assert "discriminator" not in payment


def test_tagged_unions_write_what_encode_writes() -> None:
    found = bundle(Payment, mode="serialization")
    check = validator(found)
    value = Payment(Card("1"), Pix("k"), Card("2"), Pix("p"), Dog("Rex", "lab"), Pix("m"))
    assert check.is_valid(json.loads(ox.json.encode(value)))


def test_flattened_fields_are_properties_of_the_model() -> None:
    for mode in ("validation", "serialization"):
        found = bundle(Flat, mode=mode)
        flat = found.defs["Flat"]
        assert list(flat["properties"]) == ["id", "created_by"]
        assert flat["required"] == ["id", "created_by"]
    check = validator(bundle(Flat))
    assert check.is_valid({"id": 1, "created_by": "me"})
    assert not check.is_valid({"id": 1, "created_by": "me", "other": 1})
    assert list(bundle(Flat).defs) == ["Flat"]


def test_unknown_keys_are_forbidden_on_input_only() -> None:
    request = ox.json_schema_bundle([Card]).defs["Card"]
    response = ox.json_schema_bundle([Card], mode="serialization").defs["Card"]
    assert request["additionalProperties"] is False
    assert "additionalProperties" not in response


def test_skipped_and_omitted_fields_in_each_direction() -> None:
    @ox.model(skip_if_none=True)
    class Omitting:
        always: int
        maybe: int | None = None
        empty: list[int] = ox.field(default_factory=list[int], skip_if_default=True)
        odd: int = ox.field(default=0, skip_if=lambda value: value % 2 == 1)
        both: int = ox.field(default=0, skip=True)
        out: int = ox.field(default=0, skip_decode=True)
        into: int = ox.field(default=0, skip_encode=True)
        nothing: int | None = ox.field(default=None, skip_if_default=True)

    request = ox.json_schema_bundle([Omitting]).defs["Omitting"]
    response = ox.json_schema_bundle([Omitting], mode="serialization").defs["Omitting"]
    assert list(request["properties"]) == ["always", "maybe", "empty", "odd", "into", "nothing"]
    assert request["required"] == ["always"]
    assert list(response["properties"]) == [
        "always",
        "maybe",
        "empty",
        "odd",
        "out",
        "nothing",
    ]
    # Only what is always written is required, and a field left out when None is never null.
    assert response["required"] == ["always", "out"]
    assert response["properties"]["maybe"] == {"type": "integer"}
    assert response["properties"]["nothing"] == {"type": "integer"}
    assert request["properties"]["maybe"]["anyOf"][1] == {"type": "null"}


def test_defaults_are_json_or_left_out() -> None:
    import decimal as d

    @ox.model
    class Defaults:
        count: int = 3
        price: d.Decimal = d.Decimal("1.50")
        when: datetime.date = datetime.date(2026, 1, 31)
        tags: tuple[str, ...] = ("a",)
        nothing: int | None = None
        nan: float = float("nan")

    props = ox.json_schema_bundle([Defaults]).defs["Defaults"]["properties"]
    assert props["count"]["default"] == 3
    assert props["price"]["default"] == "1.50"
    assert props["when"]["default"] == "2026-01-31"
    assert props["tags"]["default"] == ["a"]
    assert props["nothing"]["default"] is None
    assert "default" not in props["nan"]


def test_a_dataclass_without_a_docstring_has_no_description() -> None:
    assert "description" not in ox.json_schema_bundle([Card]).defs["Card"]
    assert ox.json_schema_bundle([Cat]).defs["Cat"].get("description") is None


def test_encoded_values_fit_the_serialization_schema() -> None:
    value = Pix("k")
    found = bundle(Pix, mode="serialization")
    assert validator(found).is_valid(json.loads(ox.json.encode(value)))
    assert re.fullmatch(r"#/components/schemas/Pix", found.schemas[0]["$ref"])
