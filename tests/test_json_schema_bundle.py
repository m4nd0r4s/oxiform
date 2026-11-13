import itertools
import json
from typing import Any, cast

import pytest
from schema_support import (
    COMPONENTS,
    Address,
    Category,
    Checker,
    Customer,
    Order,
    check_metaschema,
    check_schema,
    module_with,
    validator,
)

import oxiform as ox


def bundle_of(*targets: Any, **options: Any) -> ox.SchemaBundle:
    return ox.json_schema_bundle(list(targets), ref_template=COMPONENTS, **options)


def test_nested_overlapping_and_recursive_types_share_definitions() -> None:
    bundle = bundle_of(Order, Customer, Address, Category, list[Category])
    assert list(bundle.defs) == ["Address", "Category", "Color", "Customer", "Order"]
    assert bundle.schemas[0] == {"$ref": "#/components/schemas/Order"}
    assert bundle.schemas[4] == {
        "type": "array",
        "items": {"$ref": "#/components/schemas/Category"},
    }
    order = bundle.defs["Order"]["properties"]
    assert order["customer"] == {"$ref": "#/components/schemas/Customer"}
    assert order["shipping"] == {"$ref": "#/components/schemas/Address"}
    assert order["billing"]["anyOf"][0] == {"$ref": "#/components/schemas/Address"}
    category = bundle.defs["Category"]["properties"]
    assert category["children"]["items"] == {"$ref": "#/components/schemas/Category"}
    assert category["parent"]["anyOf"][0] == {"$ref": "#/components/schemas/Category"}
    check_metaschema(bundle)


def test_names_and_documents_do_not_depend_on_the_order_of_the_targets() -> None:
    targets = [Order, Customer, Address, Category]
    reference = bundle_of(*targets)
    for permutation in itertools.permutations(targets):
        bundle = bundle_of(*permutation)
        assert json.dumps(bundle.defs) == json.dumps(reference.defs)
        for target, schema in zip(bundle.targets, bundle.schemas, strict=True):
            assert reference.schema_for(target) == schema


@pytest.mark.parametrize(
    "template",
    [
        "#/components/schemas/{name}",
        "#/definitions/{name}",
        "https://example.com/schemas/{name}.json",
        "{name}",
    ],
)
def test_refs_follow_the_ref_template(template: str) -> None:
    bundle = ox.json_schema_bundle([Order], ref_template=template)
    assert bundle.schemas[0] == {"$ref": template.replace("{name}", "Order")}
    refs = [ref for ref in _refs(bundle.defs)]
    assert refs
    assert all(ref.startswith(template.split("{name}")[0]) for ref in refs)
    assert all(ref.endswith(template.split("{name}")[1]) for ref in refs)
    assert template.replace("{name}", "Address") in refs


def test_the_default_template_makes_a_self_contained_document() -> None:
    document = ox.json_schema(Order)
    assert document["$schema"] == "https://json-schema.org/draft/2020-12/schema"
    assert document["$ref"] == "#/$defs/Order"
    assert sorted(document["$defs"]) == ["Address", "Category", "Color", "Customer", "Order"]

    check_schema(document)
    assert Checker(document).is_valid(
        {
            "customer": {"id": 1, "name": "Ana", "favorite": "blue"},
            "shipping": {"street": "Rua", "postalCode": "1000-001"},
        }
    )


def test_a_type_that_is_not_a_model_is_written_in_place_with_its_definitions() -> None:
    document = ox.json_schema(dict[str, list[Address]])
    assert document["type"] == "object"
    assert document["additionalProperties"]["items"] == {"$ref": "#/$defs/Address"}
    assert list(document["$defs"]) == ["Address"]
    assert "$defs" not in ox.json_schema(int)


def test_same_named_classes_get_distinct_names_from_their_modules() -> None:
    source = "import oxiform as ox\n@ox.model\nclass User:\n    name: str\n"
    billing = module_with("shop.billing.models", source)
    auth = module_with("shop.auth.models", source)
    plain = module_with("shop.other", "import oxiform as ox\n@ox.model\nclass Item:\n    id: int\n")
    bundle = bundle_of(billing.User, auth.User, plain.Item)
    assert list(bundle.defs) == ["Item", "auth.models.User", "billing.models.User"]
    assert bundle.schemas[0] == {"$ref": "#/components/schemas/billing.models.User"}
    assert bundle.schemas[1] == {"$ref": "#/components/schemas/auth.models.User"}
    # The same names whatever the order, and the shortest tail that tells them apart.
    again = bundle_of(plain.Item, auth.User, billing.User)
    assert list(again.defs) == list(bundle.defs)
    other = module_with("billing.models", source)
    bundle = bundle_of(billing.User, other.User)
    assert sorted(bundle.defs) == ["billing.models.User", "shop.billing.models.User"]


def test_distinct_classes_with_the_same_module_and_name_are_numbered() -> None:
    first = module_with("twins", "import oxiform as ox\n@ox.model\nclass Twin:\n    a: int\n")
    second_source = "import oxiform as ox\n@ox.model\nclass Twin:\n    b: int\n"
    twin_a = first.Twin
    second = module_with("twins", second_source)
    bundle = bundle_of(twin_a, second.Twin)
    assert sorted(bundle.defs) == ["twins.Twin", "twins.Twin_2"]
    assert bundle.defs["twins.Twin"]["properties"] == {"a": {"type": "integer"}}
    assert bundle.defs["twins.Twin_2"]["properties"] == {"b": {"type": "integer"}}


def test_names_are_valid_component_names() -> None:
    namespace: dict[str, Any] = {}
    exec("import oxiform as ox\n@ox.model\nclass Café:\n    id: int\n", namespace)  # noqa: S102
    namespace["Café"].__module__ = "tests.test_json_schema_bundle"
    bundle = bundle_of(namespace["Café"])
    assert list(bundle.defs) == ["Caf_"]

    def local() -> type:
        @ox.model
        class Inner:
            id: int

        return Inner

    assert list(bundle_of(local()).defs) == ["Inner"]


def test_a_class_used_as_input_and_output_is_one_definition_when_both_are_equal() -> None:
    bundle = bundle_of(Tree, ox.SchemaTarget(Tree, mode="serialization"))
    assert list(bundle.defs) == ["Tree"]
    assert bundle.schemas[0] == bundle.schemas[1]
    # Address has a default, which only the input states.
    bundle = bundle_of(Address, ox.SchemaTarget(Address, mode="serialization"))
    assert list(bundle.defs) == ["Address-Input", "Address-Output"]


def test_input_and_output_definitions_differ_where_the_directions_differ() -> None:
    bundle = bundle_of(Customer, ox.SchemaTarget(Customer, mode="serialization"))
    assert sorted(bundle.defs) == [
        "Address-Input",
        "Address-Output",
        "Color",
        "Customer-Input",
        "Customer-Output",
    ]
    assert bundle.schemas[0] == {"$ref": "#/components/schemas/Customer-Input"}
    assert bundle.schemas[1] == {"$ref": "#/components/schemas/Customer-Output"}
    request = bundle.defs["Customer-Input"]
    response = bundle.defs["Customer-Output"]
    assert request["description"] == "Someone who buys."
    # Defaults: optional on input, always written on output (a skipped one aside).
    assert request["required"] == ["id", "name"]
    assert response["required"] == [
        "id",
        "name",
        "age",
        "addresses",
        "favorite",
        "score",
        "oldName",
    ]
    assert request["properties"]["favorite"] == {
        "$ref": "#/components/schemas/Color",
        "default": "red",
    }
    assert "default" not in response["properties"]["favorite"]
    assert "default" not in request["properties"]["addresses"]  # a factory is never run
    # Renames apply both ways; aliases are read only.
    assert {"id", "oldName"} <= set(response["properties"])
    assert request["properties"]["legacyName"] == {
        "type": "string",
        "deprecated": True,
    }
    assert "legacyName" not in response["properties"]
    # Fields that are read only or written only.
    assert "passwordHash" in request["properties"]
    assert "passwordHash" not in response["properties"]
    assert "score" not in request["properties"]
    assert response["properties"]["score"] == {"type": "number"}
    # skip_if_none: optional, and never null.
    assert "nickname" not in response["required"]
    assert response["properties"]["nickname"] == {"type": "string"}
    assert request["properties"]["nickname"]["anyOf"][1] == {"type": "null"}
    check_metaschema(bundle)


def test_aliases_are_required_by_any_of_their_names() -> None:
    @ox.model
    class Named:
        customer: str = ox.field(aliases=["client", "buyer"])

    bundle = ox.json_schema_bundle([Named])
    schema = bundle.defs["Named"]
    assert "required" not in schema
    assert schema["anyOf"] == [
        {"required": ["customer"]},
        {"required": ["client"]},
        {"required": ["buyer"]},
    ]
    check = validator(bundle)
    assert check.is_valid({"client": "x"})
    assert check.is_valid({"customer": "x"})
    assert not check.is_valid({})


def test_a_class_that_holds_a_changed_class_changes_with_it() -> None:
    bundle = bundle_of(Order, ox.SchemaTarget(Order, mode="serialization"))
    # Order's own fields are equal in both directions, but it holds classes that are not, so it
    # differs too. Color is equal in both and stays one definition.
    assert sorted(bundle.defs) == [
        "Address-Input",
        "Address-Output",
        "Category-Input",
        "Category-Output",
        "Color",
        "Customer-Input",
        "Customer-Output",
        "Order-Input",
        "Order-Output",
    ]
    assert (
        bundle.defs["Order-Input"]["properties"]["customer"]["$ref"]
        == "#/components/schemas/Customer-Input"
    )
    assert (
        bundle.defs["Order-Output"]["properties"]["customer"]["$ref"]
        == "#/components/schemas/Customer-Output"
    )
    check_metaschema(bundle)


@ox.model
class Tree:
    label: str
    children: "list[Tree]"


@ox.model
class Ring:
    label: str
    next: "Ring | None" = ox.field(default=None, skip_if_default=True)


def test_recursive_classes_are_one_definition_when_equal_and_two_when_not() -> None:
    both = bundle_of(Tree, ox.SchemaTarget(Tree, mode="serialization"))
    assert list(both.defs) == ["Tree"]
    assert both.defs["Tree"]["properties"]["children"]["items"] == {
        "$ref": "#/components/schemas/Tree"
    }
    # Ring differs by direction through its own field, and points at itself in both.
    ring = bundle_of(Ring, ox.SchemaTarget(Ring, mode="serialization"))
    assert sorted(ring.defs) == ["Ring-Input", "Ring-Output"]
    assert ring.defs["Ring-Input"]["properties"]["next"]["anyOf"][0] == {
        "$ref": "#/components/schemas/Ring-Input"
    }
    assert ring.defs["Ring-Output"]["properties"]["next"] == {
        "$ref": "#/components/schemas/Ring-Output"
    }
    assert "required" in ring.defs["Ring-Input"] and ring.defs["Ring-Output"]["required"] == [
        "label"
    ]


def test_each_target_takes_its_own_mode_and_coercion() -> None:
    @ox.model
    class Page:
        number: int = 1

    bundle = bundle_of(
        Page,
        ox.SchemaTarget(Page, coercion="lax"),
        ox.SchemaTarget(Page, mode="serialization"),
    )
    assert sorted(bundle.defs) == ["Page-Input", "Page-Input-Lax", "Page-Output"]
    assert [target.mode for target in bundle.targets] == ["validation"] * 2 + ["serialization"]
    assert [target.coercion for target in bundle.targets] == ["strict", "lax", "strict"]
    assert bundle.schema_for(Page) == bundle.schemas[0]
    assert bundle.schema_for(ox.SchemaTarget(Page, coercion="lax")) == bundle.schemas[1]
    with pytest.raises(KeyError, match="not in this bundle"):
        bundle.schema_for(int)


def test_an_empty_bundle_is_empty() -> None:
    bundle = ox.json_schema_bundle([])
    assert (bundle.defs, bundle.schemas, bundle.targets) == ({}, (), ())


@pytest.mark.parametrize(
    ("call", "error", "message"),
    [
        (lambda: ox.json_schema_bundle(cast(Any, Address)), TypeError, "call ox.json_schema"),
        (lambda: ox.json_schema_bundle(cast(Any, "Address")), TypeError, "list of types"),
        (lambda: ox.json_schema_bundle(cast(Any, list[int])), TypeError, "list of types"),
        (lambda: ox.json_schema_bundle(cast(Any, 5)), TypeError, "list of types"),
        (lambda: ox.json_schema_bundle([], mode="input"), ValueError, '"validation"'),  # pyright: ignore[reportArgumentType]
        (lambda: ox.json_schema_bundle([], coercion="loose"), ValueError, '"strict" or "lax"'),  # pyright: ignore[reportArgumentType]
        (
            lambda: ox.json_schema_bundle([], mode="serialization", coercion="lax"),
            ValueError,
            'applies to mode="validation" only',
        ),
        (
            lambda: ox.json_schema_bundle([ox.SchemaTarget(int, "serialization", "lax")]),
            ValueError,
            'applies to mode="validation" only',
        ),
        (lambda: ox.json_schema_bundle([], ref_template="#/x"), ValueError, "exactly once"),
        (
            lambda: ox.json_schema_bundle([], ref_template="{name}{name}"),
            ValueError,
            "exactly once",
        ),
        (
            lambda: ox.json_schema_bundle([], ref_template=cast(Any, 5)),
            TypeError,
            "ref_template must be a str",
        ),
        (lambda: ox.json_schema(int, mode="x"), ValueError, '"validation"'),  # pyright: ignore[reportArgumentType]
    ],
)
def test_mistakes_say_what_to_pass(call: Any, error: type[Exception], message: str) -> None:
    with pytest.raises(error, match=message):
        call()


@pytest.mark.parametrize(
    ("target", "message"),
    [
        (object, "unsupported type"),
        (dict[int, str], "dict keys must be str"),
        (complex, "unsupported type"),
    ],
)
def test_types_oxiform_cannot_read_are_declaration_errors(target: Any, message: str) -> None:
    with pytest.raises(ox.SchemaError, match=message):
        ox.json_schema(target)


def test_misplaced_constraints_and_ambiguous_unions_are_declaration_errors() -> None:
    from typing import Annotated

    with pytest.raises(ox.SchemaError, match="do not apply"):
        ox.json_schema(Annotated[bool, ox.Ge(0)])
    with pytest.raises(ox.SchemaError, match="both read from"):
        ox.json_schema(int | Annotated[int, ox.Ge(0)])
    with pytest.raises(ox.SchemaError, match="only models"):
        ox.json_schema(Annotated[Address | int, ox.External()])
    with pytest.raises(ox.SchemaError, match="a model cannot flatten itself|Pattern"):
        ox.json_schema(Annotated[str, ox.Pattern("(?=x)")])


def test_a_class_not_defined_yet_says_so() -> None:
    namespace: dict[str, Any] = {"ox": ox}
    exec("@ox.model\nclass Early:\n    late: 'Late'\n", namespace)  # noqa: S102
    namespace["Early"].__module__ = "tests.schema_support"
    with pytest.raises(ox.SchemaError, match="cannot resolve an annotation"):
        ox.json_schema(namespace["Early"])


def test_the_same_bundle_is_written_twice() -> None:
    first = bundle_of(Order, ox.SchemaTarget(Order, mode="serialization"))
    second = bundle_of(Order, ox.SchemaTarget(Order, mode="serialization"))
    assert json.dumps(first.defs) == json.dumps(second.defs)
    first.defs["Order-Input"]["description"] = "changed"
    assert (
        "description"
        not in bundle_of(Order, ox.SchemaTarget(Order, mode="serialization")).defs["Order-Input"]
    )


def test_instances_validate_against_the_bundle() -> None:
    bundle = bundle_of(Order, ox.SchemaTarget(Order, mode="serialization"))
    request = validator(bundle, 0)
    response = validator(bundle, 1)
    body = {
        "customer": {"id": 1, "name": "Ana", "legacyName": "A", "favorite": "blue"},
        "shipping": {"street": "Rua", "postalCode": "1000-001"},
        "category": {"name": "root", "children": [{"name": "leaf"}]},
    }
    assert request.is_valid(body)
    order = ox.json.decode(json.dumps(body), Order)
    assert response.is_valid(json.loads(ox.json.encode(order)))
    assert not request.is_valid({**body, "shipping": {"street": "Rua", "postalCode": "x"}})
    assert not request.is_valid({"shipping": body["shipping"]})
    assert not response.is_valid({"customer": body["customer"]})


def _refs(value: object) -> list[str]:
    found: list[str] = []
    if isinstance(value, dict):
        node = cast("dict[str, Any]", value)
        if isinstance(node.get("$ref"), str):
            found.append(node["$ref"])
        for item in node.values():
            found.extend(_refs(item))
    elif isinstance(value, list):
        for item in cast("list[Any]", value):
            found.extend(_refs(item))
    return found
