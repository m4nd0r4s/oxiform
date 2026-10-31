from dataclasses import dataclass, field
from typing import Annotated, Any, Optional

import pytest

import oxiform as ox
from oxiform._introspection import describe


@dataclass(slots=True)
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
class Node:
    value: int
    children: "list[Node]"


def test_describes_scalars() -> None:
    assert describe(int).root == ("int", {})
    assert describe(float).root == ("float", {})
    assert describe(str).root == ("str", {})
    assert describe(bool).root == ("bool",)
    assert describe(None).root == ("none",)
    assert describe(Any).root == ("any",)


def test_describes_containers_and_optionals() -> None:
    assert describe(list[int]).root == ("list", ("int", {}), {})
    assert describe(dict[str, float]).root == ("dict", ("float", {}), {})
    assert describe(list).root == ("list", ("any",), {})
    assert describe(int | None).root == ("optional", ("int", {}))
    assert describe(Optional[str]).root == ("optional", ("str", {}))  # noqa: UP045


def test_maps_constraints_to_values() -> None:
    annotated = Annotated[int, ox.Ge(0), ox.Le(10.5), ox.OneOf(1, 2)]
    assert describe(annotated).root == ("int", {"minimum": 0, "maximum": 10.5, "one_of": [1, 2]})
    assert describe(Annotated[str, ox.Len(2)]).root == ("str", {"min_length": 2})
    assert describe(Annotated[list[int], ox.Len(max=3)]).root == (
        "list",
        ("int", {}),
        {"max_length": 3},
    )


def test_moves_constraints_on_an_optional_to_the_inner_type() -> None:
    assert describe(Annotated[int | None, ox.Ge(1)]).root == ("optional", ("int", {"minimum": 1}))


def test_ignores_metadata_of_other_libraries() -> None:
    assert describe(Annotated[int, "a note", ox.Ge(0)]).root == ("int", {"minimum": 0})


def test_describes_dataclasses_with_defaults() -> None:
    description = describe(User)
    assert description.root == ("struct", 0)
    user, address = description.structs
    assert user.cls is User
    assert [field.name for field in user.fields] == ["id", "name", "age", "addresses", "level"]
    assert user.fields[1].value_type == ("str", {"min_length": 1, "max_length": 80})
    assert user.fields[2].value_type == ("optional", ("int", {"minimum": 0, "maximum": 150}))
    assert user.fields[3].value_type == ("list", ("struct", 1), {})
    assert user.fields[0].default is None
    assert user.fields[2].default == ("value", None)
    assert user.fields[3].default is not None and user.fields[3].default[0] == "factory"
    assert address.fields[1].value_type == ("str", {"pattern": r"^\d{4}-\d{3}$"})


def test_describes_self_referencing_dataclasses() -> None:
    description = describe(Node)
    assert len(description.structs) == 1
    assert description.structs[0].fields[1].value_type == ("list", ("struct", 0), {})


def test_skips_fields_outside_init() -> None:
    @dataclass
    class Counter:
        start: int
        total: int = field(init=False, default=0)

    assert [field.name for field in describe(Counter).structs[0].fields] == ["start"]


@pytest.mark.parametrize(
    ("annotation", "message"),
    [
        (complex, "root: unsupported type <class 'complex'>"),
        (int | str, "root: unions other than T | None are not supported yet"),
        (dict[int, str], "root: dict keys must be str, not <class 'int'>"),
        (Annotated[bool, ox.Ge(0)], "root: constraints do not apply to bool"),
        (Annotated[int, ox.Ge(0), ox.Ge(1)], "root: Ge is declared twice"),
        (Annotated[int, ox.Ge(True)], "root: Ge takes an int or a float, not True"),
        (Annotated[int, ox.Le(2**70)], f"root: Le limit {2**70} does not fit in 64 bits"),
        (Annotated[str, ox.Len(-1)], "root: Len takes non-negative ints, not -1"),
        (
            Annotated[str, ox.OneOf("a", 1.5)],  # pyright: ignore[reportArgumentType]
            "root: OneOf takes ints or strs, not 1.5",
        ),
    ],
)
def test_rejects_declarations_it_cannot_read(annotation: object, message: str) -> None:
    with pytest.raises(ox.SchemaError) as raised:
        describe(annotation)
    assert str(raised.value) == message


def test_reports_unresolvable_forward_references() -> None:
    @dataclass
    class Broken:
        other: "Missing"  # type: ignore[name-defined] # noqa: F821  # pyright: ignore[reportUndefinedVariable]

    with pytest.raises(ox.SchemaError, match="Broken: cannot resolve an annotation"):
        describe(Broken)
