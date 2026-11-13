"""Where the schema and the engine could disagree: regex dialects, flags, sets, the shape of
the arguments, the public types, and bundles written from several threads at once."""

import enum
import json
from concurrent.futures import ThreadPoolExecutor
from threading import Barrier
from typing import Annotated, Any

import pytest
from schema_support import Customer, Order, check_metaschema, validator

import oxiform as ox
from oxiform._json_schema.regex import portable_pattern


@pytest.mark.parametrize(
    ("source", "expected"),
    [
        (r"^\d+$", "^[0-9]+$"),
        (r"^[\d-]{2}$", "^[0-9-]{2}$"),
        (r"\w+@\w+\.com", r"[A-Za-z0-9_]+@[A-Za-z0-9_]+\.com"),
        (r"[^\w]", "[^A-Za-z0-9_]"),
        (r"\D\W", "[^0-9][^A-Za-z0-9_]"),
        (r"(?P<year>\d{4})", "(?<year>[0-9]{4})"),
        (r"(?<year>\d{4})", "(?<year>[0-9]{4})"),
        (r"\Aab\z", "^ab$"),
        (r"^[a-z]{2,3}(?:x|y)*$", r"^[a-z]{2,3}(?:x|y)*$"),
        (r"a\.b\\c\(", r"a\.b\\c\("),
        (r"\s\S\b", r"\s\S\b"),
        ("[é]", "[é]"),
    ],
)
def test_patterns_are_written_as_ecma_262_where_they_mean_the_same(
    source: str, expected: str
) -> None:
    assert portable_pattern(source, "root") == expected


@pytest.mark.parametrize(
    ("source", "what"),
    [
        ("(?i)abc", "the group syntax '(?i)'"),
        ("(?s:.)", "the group syntax"),
        (r"\pL+", "the Unicode class"),
        (r"\p{Greek}", "the Unicode class"),
        (r"\PL", "the Unicode class"),
        (r"[[:alpha:]]", "a nested class or a POSIX class"),
        (r"[a-z&&[^b]]", "the class operation"),
        (r"[a[b]]", "a nested class or a POSIX class"),
        (r"[a-z&&b]", "the class operation"),
        (r"[a-z--b]", "the class operation"),
        (r"[\D]", r"\D inside a class"),
        (r"\U0010FFFF", r"the escape \U"),
        (r"\x{41}", r"the escape \x"),
        (r"[]a]", "a ] first in a class"),
    ],
)
def test_patterns_with_no_ecma_form_are_declaration_errors_that_say_what_to_write(
    source: str, what: str
) -> None:
    @ox.model
    class Holder:
        value: Annotated[str, ox.Pattern(source)]

    with pytest.raises(ox.SchemaError) as raised:
        ox.json_schema(Holder)
    message = str(raised.value)
    assert "Holder.value: Pattern " in message and what in message
    assert "so that the schema and the decoder agree" in message
    assert "write " in message


def test_the_decoder_still_reads_what_the_schema_cannot_say() -> None:
    # Rust reads \d in Unicode; the schema describes ASCII digits, as ECMA 262 does.
    arabic = '"٣"'
    target = Annotated[str, ox.Pattern(r"^\d$")]
    assert ox.json.decode(arabic, target) == "٣"
    check = validator(ox.json_schema_bundle([target]))
    assert check.is_valid("3") and not check.is_valid("٣")


@pytest.mark.parametrize("text", ["ab_1-23", "A-00", "-", "a-1", "ab-123", "a b-12", "é-12", ""])
def test_translated_patterns_agree_with_the_decoder_on_ascii_text(text: str) -> None:
    target = Annotated[str, ox.Pattern(r"^\w+-\d{2}$")]
    decoded = True
    try:
        ox.json.decode(json.dumps(text), target)
    except ox.ValidationError:
        decoded = False
    check = validator(ox.json_schema_bundle([target]))
    if text.isascii():
        assert check.is_valid(text) is decoded
    else:
        assert decoded and not check.is_valid(text)  # Unicode letters: the stated difference


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


Wide = enum.Flag("Wide", {f"B{i}": 1 << i for i in range(12)})


def test_a_flag_lists_the_integers_it_takes_and_an_intflag_takes_any() -> None:
    perm = ox.json_schema_bundle([Perm]).schemas[0]
    assert perm == {"type": "integer", "enum": [*range(-8, 8)]}
    sparse = ox.json_schema_bundle([Sparse]).schemas[0]
    assert sparse["enum"] == [-8, -7, -4, -3, 0, 1, 4, 5]
    assert ox.json_schema_bundle([Bits]).schemas[0] == {"type": "integer"}
    wide = ox.json_schema_bundle([Wide]).schemas[0]
    assert wide == {"type": "integer", "minimum": -4096, "maximum": 4095}
    lax = ox.json_schema_bundle([Perm], coercion="lax").schemas[0]
    assert lax["anyOf"][0]["enum"] == perm["enum"]
    check = validator(ox.json_schema_bundle([Perm], coercion="lax"))
    assert check.is_valid("007") and check.is_valid("-8") and not check.is_valid("8")


def test_a_set_may_repeat_on_input_and_never_repeats_on_output() -> None:
    assert ox.json.decode("[1, 1]", set[int]) == {1}
    request = ox.json_schema_bundle([set[int]]).schemas[0]
    response = ox.json_schema_bundle([set[int]], mode="serialization").schemas[0]
    assert "uniqueItems" not in request and response["uniqueItems"] is True
    assert validator(ox.json_schema_bundle([set[int]])).is_valid([1, 1])


def test_an_empty_tuple_is_a_valid_schema() -> None:
    bundle = ox.json_schema_bundle([tuple[()], list[tuple[()]]])
    check_metaschema(bundle)
    assert bundle.schemas[0] == {"type": "array", "maxItems": 0}


def test_variants_of_a_discriminated_union_are_plain_references() -> None:
    @ox.model(tag_field="kind")
    class Base:
        name: str

    @ox.model(tag="a")
    class A(Base):
        pass

    @ox.model(tag="b")
    class B(Base):
        pass

    union = ox.json_schema_bundle([Annotated[A | B, ox.Tagged("kind")]]).schemas[0]
    assert [variant["$ref"] for variant in union["oneOf"]] == ["#/$defs/A", "#/$defs/B"]
    assert all(variant["required"] == ["kind"] for variant in union["oneOf"])
    assert union["discriminator"]["mapping"] == {"a": "#/$defs/A", "b": "#/$defs/B"}


@pytest.mark.parametrize("targets", [{int, str}, frozenset({int}), {"a": int}])
def test_targets_must_have_an_order(targets: Any) -> None:
    with pytest.raises(TypeError, match="list or a tuple of targets") as raised:
        ox.json_schema_bundle(targets)
    assert "pass [type, ...]" in str(raised.value)


def test_a_tuple_or_a_generator_of_targets_is_fine() -> None:
    assert len(ox.json_schema_bundle((int, str)).schemas) == 2
    assert len(ox.json_schema_bundle(target for target in (int, str, bool)).schemas) == 3


def test_the_types_of_the_bundle_are_public() -> None:
    defs: dict[str, ox.JsonSchema] = ox.json_schema_bundle([Order]).defs
    mode: ox.SchemaMode = "serialization"
    coercion: ox.SchemaCoercion = "strict"
    assert ox.json_schema_bundle([Order], mode=mode, coercion=coercion).targets[0].mode == mode
    assert "Order" in defs


def test_bundles_written_from_several_threads_are_the_same() -> None:
    targets = [Order, Customer, ox.SchemaTarget(Order, mode="serialization")]
    expected = ox.json_schema_bundle(targets, ref_template="#/components/schemas/{name}")
    reference = json.dumps([expected.defs, expected.schemas])
    threads = 8
    barrier = Barrier(threads)

    def write(_: int) -> list[str]:
        barrier.wait()
        found: list[str] = []
        for _round in range(25):
            bundle = ox.json_schema_bundle(targets, ref_template="#/components/schemas/{name}")
            found.append(json.dumps([bundle.defs, bundle.schemas]))
            single = json.dumps(ox.json_schema(Order, mode="serialization"))
            assert "Order" in single
        return found

    with ThreadPoolExecutor(threads) as pool:
        results = [item for found in pool.map(write, range(threads)) for item in found]
    assert len(results) == threads * 25
    assert all(result == reference for result in results)
