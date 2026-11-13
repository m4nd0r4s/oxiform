"""The embedding ABI, through the sample host (`examples/abi-host`): a PyO3 extension that links
`oxiform-abi` and none of the engine, and decodes and encodes through the capsule of oxiform.

Build it with `maturin develop` in `examples/abi-host` (CI does, and sets
OXIFORM_REQUIRE_ABI_HOST so that a missing build fails instead of skipping).
"""

import gc
import importlib
import os
import sys
from concurrent.futures import ThreadPoolExecutor
from typing import Annotated, Any, Literal

import pytest

import oxiform as ox
from oxiform import _oxiform  # pyright: ignore[reportPrivateUsage]
from oxiform.errors import ValidationError

try:
    host: Any = importlib.import_module("oxiform_abi_host")
except ImportError:  # pragma: no cover - depends on the build
    if os.environ.get("OXIFORM_REQUIRE_ABI_HOST"):
        raise
    pytest.skip("build examples/abi-host to run the ABI tests", allow_module_level=True)

OX_OK, OX_INVALID, OX_PYTHON_ERROR, OX_INVALID_ARGUMENT, OX_UNSUPPORTED, OX_BUFFER = (
    0,
    1,
    2,
    3,
    4,
    5,
)


@ox.model
class Address:
    street: Annotated[str, ox.Len(1, 20)]
    zip: Literal["1000", "2000", 3000, None]


@ox.model(tag_field="kind", tag="dog")
class Dog:
    name: str


@ox.model(tag_field="kind", tag="cat")
class Cat:
    lives: Annotated[int, ox.Ge(1)]


@ox.model
class User:
    id: int
    name: Annotated[str, ox.Len(1, 8)]
    age: Annotated[int, ox.Ge(0), ox.Le(150)] = 0
    addresses: list[Address] = ox.field(default_factory=list)
    pet: Dog | Cat | None = None
    tags: dict[str, int] = ox.field(default_factory=dict)


@ox.model
class Loose:
    name: str


@ox.model(unknown="forbid")
class Tight:
    name: str


@ox.model
class Base:
    id: int


@ox.model
class Derived(Base):
    secret: str = "hidden"


UNTAGGED = list[Annotated[Address | Loose, ox.Untagged()]]


def problem_errors(exc: BaseException) -> list[dict[str, Any]]:
    return exc.args[0]["errors"]


def python_error(data: bytes, target: Any, **options: Any) -> ValidationError:
    with pytest.raises(ValidationError) as raised:
        ox.json.decode(data, target, **options)
    return raised.value


def python_errors(data: bytes, target: Any, **options: Any) -> list[dict[str, Any]]:
    return [dict(error) for error in python_error(data, target, **options).errors()]


def host_problem(plan: Any, data: bytes, **options: Any) -> dict[str, Any]:
    with pytest.raises(host.HostProblem) as raised:
        plan.decode(data, **options)
    return raised.value.args[0]


# --- version and the capsule ------------------------------------------------------------------


def test_the_capsule_has_a_released_shaped_version() -> None:
    assert _oxiform.ABI_VERSION == (1, 0)
    assert _oxiform._ABI_MAJORS == (1,)  # pyright: ignore[reportPrivateUsage]
    assert type(_oxiform._ABI_V1).__name__ == "PyCapsule"  # pyright: ignore[reportPrivateUsage]
    assert host.abi_version() == (1, 0, _oxiform.__version__)


def test_a_host_built_for_the_same_or_an_older_minor_is_accepted() -> None:
    assert host.import_as(1, 0) == (1, 0)


@pytest.mark.parametrize("major", [0, 2, 7])
def test_a_host_built_for_another_major_refuses_with_what_to_install(major: int) -> None:
    with pytest.raises(ImportError) as raised:
        host.import_as(major, 0)
    message = str(raised.value)
    assert f"ABI major version {major}" in message
    assert "exports ABI major version(s) [1]" in message
    assert "install an oxiform release that exports ABI" in message


def test_a_host_built_for_a_newer_minor_refuses_and_says_to_upgrade() -> None:
    with pytest.raises(
        ImportError, match="needs oxiform ABI 1.99 or newer.*exports ABI 1.0.*upgrade"
    ):
        host.import_as(1, 99)


def test_every_status_has_a_message() -> None:
    for status in range(0, 7):
        assert host.status_message(status)
    assert "upgrade oxiform" in host.status_message(99)
    assert "upgrade oxiform" in host.status_message(OX_UNSUPPORTED)


# --- plans ------------------------------------------------------------------------------------


def test_a_plan_of_a_host_is_the_one_in_the_cache_of_oxiform() -> None:
    @ox.model
    class Fresh:
        id: int

    stats = _oxiform._plan_stats  # pyright: ignore[reportPrivateUsage]
    # A model is compiled when it is declared; a type expression made of it is not.
    compiled, cached = stats()
    assert host.plan_for(Fresh).target is Fresh
    assert stats() == (compiled, cached)
    # `list[Fresh]` is compiled by the first to ask, the host here...
    first = host.plan_for(list[Fresh])
    assert stats() == (compiled + 1, cached + 1)
    # ...and neither the host again nor `ox.json.decode` compiles it a second time.
    second = host.plan_for(list[Fresh])
    assert ox.json.decode(b'[{"id": 1}]', list[Fresh])[0].id == 1
    assert stats() == (compiled + 1, cached + 1)
    # The other way round: Python compiles first, the host finds the plan.
    ox.json.decode(b'{"a": {"id": 1}}', dict[str, Fresh])
    assert stats() == (compiled + 2, cached + 2)
    host.plan_for(dict[str, Fresh])
    assert stats() == (compiled + 2, cached + 2)
    assert first.target == list[Fresh]
    assert first.clone_handle().address == first.address != second.address


def test_plans_of_type_expressions_work_and_are_cached_too() -> None:
    plan = host.plan_for(list[User])
    compiled, _ = _oxiform._plan_stats()  # pyright: ignore[reportPrivateUsage]
    assert plan.decode(b'[{"id": 1, "name": "a"}]')[0].id == 1
    assert ox.json.decode(b"[]", list[User]) == []
    assert _oxiform._plan_stats()[0] == compiled  # pyright: ignore[reportPrivateUsage]


def test_a_declaration_error_is_a_schema_error() -> None:
    with pytest.raises(ox.SchemaError, match="unsupported type"):
        host.plan_for(object())


def test_plan_for_null_is_an_invalid_argument() -> None:
    status, error = host.plan_for_null()
    assert status == OX_INVALID_ARGUMENT
    assert isinstance(error, ValueError)
    assert "NULL type" in str(error)


def test_handles_release_their_references() -> None:
    @ox.model
    class Counted:
        id: int

    ox.json.decode(b'{"id": 1}', Counted)
    gc.collect()
    baseline = sys.getrefcount(Counted)
    for _ in range(300):
        plan = host.plan_for(Counted)
        clone = plan.clone_handle()
        decoded = plan.decode(b'{"id": 1}')
        del plan, clone, decoded
    gc.collect()
    assert sys.getrefcount(Counted) == baseline


# --- decode options ---------------------------------------------------------------------------

VALID_USER = (
    b'{"id": 1, "name": "ann", "addresses": [{"street": "x", "zip": "1000"}],'
    b' "pet": {"kind": "cat", "lives": 9}}'
)


def test_a_valid_document_decodes_to_the_same_object_as_the_python_api() -> None:
    plan = host.plan_for(User)
    assert plan.decode(VALID_USER) == ox.json.decode(VALID_USER, User)
    assert plan.decode(b"  {" + VALID_USER[1:]).pet == Cat(lives=9)


def test_strict_and_lax_modes() -> None:
    plan = host.plan_for(User)
    text = b'{"id": "7", "name": "ann"}'
    assert host_problem(plan, text)["errors"][0]["code"] == "type"
    assert plan.decode(text, mode="lax").id == 7
    with pytest.raises(ValueError, match='mode must be "strict" or "lax"'):
        plan.decode(text, mode="loose")


def test_max_errors_and_truncation() -> None:
    plan = host.plan_for(User)
    bad = b'{"id": "a", "name": "", "age": -1, "addresses": 3, "pet": 1, "tags": 4}'
    everything = host_problem(plan, bad)
    assert len(everything["errors"]) == 6 and not everything["truncated"]
    assert everything["errors"] == python_errors(bad, User)
    two = host_problem(plan, bad, max_errors=2)
    assert len(two["errors"]) == 2 and two["truncated"]
    assert two["errors"] == python_errors(bad, User, max_errors=2)
    one = host_problem(plan, bad, max_errors=1)
    assert len(one["errors"]) == 1 and one["truncated"]


@pytest.mark.parametrize(
    ("limits", "document", "code_ctx"),
    [
        ({"max_bytes": 10}, b'{"id": 1, "name": "ann"}', "max_bytes"),
        (
            {"max_depth": 2},
            b'{"id": 1, "name": "ann", "tags": {"a": 1}, "addresses": [[[]]]}',
            "max_depth",
        ),
        ({"max_items": 1}, b'{"id": 1, "name": "ann", "tags": {"a": 1, "b": 2}}', "max_items"),
        ({"max_str_len": 3}, b'{"id": 1, "name": "annabelle"}', "max_str_len"),
        ({"max_int_digits": 3}, b'{"id": 12345, "name": "ann"}', "max_int_digits"),
    ],
)
def test_every_limit_is_honoured_like_the_python_api(
    limits: dict[str, int], document: bytes, code_ctx: str
) -> None:
    plan = host.plan_for(User)
    problem = host_problem(plan, document, limits=limits)
    assert any(error["ctx"].get("limit") == code_ctx for error in problem["errors"]), problem
    assert problem["errors"] == python_errors(document, User, limits=ox.Limits(**limits))
    # A limit that was not asked for does not apply.
    assert plan.decode(VALID_USER).id == 1


def test_limits_come_from_oxiform_and_none_turns_them_off() -> None:
    default = host.limits("default")
    py_default = ox.Limits()
    assert default == {
        "max_bytes": py_default.max_bytes,
        "max_depth": py_default.max_depth,
        "max_items": py_default.max_items,
        "max_str_len": py_default.max_str_len,
        "max_int_digits": py_default.max_int_digits,
    }
    none = host.limits("none")
    assert none["max_bytes"] == 2**64 - 1 and none["max_depth"] == ox.Limits.MAX_DEPTH
    # max_depth is capped whatever is passed.
    deep = b"[" * 200 + b"]" * 200
    plan = host.plan_for(Any)
    problem = host_problem(plan, deep, limits={"max_depth": 10_000})
    assert problem["status"] == 413 and problem["errors"][0]["ctx"]["limit"] == "max_depth"


@pytest.mark.parametrize(
    ("target", "unknown", "accepted"),
    [
        (Loose, "model", True),
        (Loose, "ignore", True),
        (Loose, "forbid", False),
        (Tight, "model", False),
        (Tight, "ignore", True),
        (Tight, "forbid", False),
    ],
)
def test_the_per_call_unknown_policy_overrides_the_model(
    target: Any, unknown: str, accepted: bool
) -> None:
    plan = host.plan_for(target)
    data = b'{"name": "a", "extra": 1}'
    if accepted:
        assert plan.decode(data, unknown=unknown).name == "a"
    else:
        errors = host_problem(plan, data, unknown=unknown)["errors"]
        assert [(e["code"], e["pointer"]) for e in errors] == [("unknown_field", "/extra")]
    with pytest.raises(ValueError, match="unknown must be"):
        plan.decode(data, unknown="error")


def test_the_unknown_policy_reaches_nested_models() -> None:
    plan = host.plan_for(User)
    data = (
        b'{"id": 1, "name": "a", "addresses": [{"street": "s", "zip": null, "oops": 1}], "more": 2}'
    )
    errors = host_problem(plan, data, unknown="forbid")["errors"]
    assert sorted(e["pointer"] for e in errors) == ["/addresses/0/oops", "/more"]
    assert errors == python_errors(data, User, unknown="forbid")


# --- options structs the way a C host passes them ---------------------------------------------


def raw_decode(**options: Any) -> tuple[int, Any, Any, Any]:
    plan = host.plan_for(Loose)
    return plan.decode_raw(b'{"name": "x"}', **options)


def test_raw_options_with_every_field_default_work() -> None:
    status, value, problem, error = raw_decode()
    assert (status, value.name, problem, error) == (OX_OK, "x", None, None)


def test_options_of_a_later_minor_version_are_read_up_to_what_this_oxiform_knows() -> None:
    status, value, *_ = raw_decode(extra=64)
    assert (status, value.name) == (OX_OK, "x")
    status, value, *_ = raw_decode(extra=64, mode=1, unknown=2)
    assert status == OX_OK


def test_options_shorter_than_abi_1_0_are_refused() -> None:
    status, value, problem, error = raw_decode(struct_size=8)
    assert status == OX_INVALID_ARGUMENT and value is None and problem is None
    assert isinstance(error, ValueError) and "options.struct_size is 8" in str(error)
    assert "size_of the struct you were built against" in str(error)


def test_limits_shorter_than_abi_1_0_are_refused() -> None:
    status, _, _, error = raw_decode(limits_struct_size=4)
    assert status == OX_INVALID_ARGUMENT and "limits.struct_size is 4" in str(error)
    status, value, *_ = raw_decode(limits_struct_size=0x1000)
    assert status == OX_OK and value.name == "x"


@pytest.mark.parametrize(
    ("options", "status", "error_type", "fragment"),
    [
        ({"mode": 5}, OX_INVALID_ARGUMENT, ValueError, "OX_MODE_LAX"),
        ({"unknown": 5}, OX_INVALID_ARGUMENT, ValueError, "OX_UNKNOWN_FORBID"),
        ({"flags": 1}, OX_UNSUPPORTED, NotImplementedError, "upgrade oxiform"),
        ({"format": 2}, OX_UNSUPPORTED, NotImplementedError, "OX_FORMAT_JSON"),
        ({"format": 0}, OX_UNSUPPORTED, NotImplementedError, "upgrade oxiform"),
        ({"null_data": True}, OX_INVALID_ARGUMENT, ValueError, "NULL data pointer"),
    ],
)
def test_options_oxiform_cannot_honour_say_what_to_do(
    options: dict[str, Any], status: int, error_type: type[Exception], fragment: str
) -> None:
    got, value, problem, error = raw_decode(**options)
    assert got == status and value is None and problem is None
    assert type(error) is error_type
    assert fragment in str(error)


def test_a_context_is_reserved_and_never_silently_ignored() -> None:
    status, value, _, error = raw_decode(context={"request": object()})
    assert status == OX_UNSUPPORTED and value is None
    assert isinstance(error, NotImplementedError) and "pass NULL" in str(error)


def test_decode_with_no_plan_is_an_invalid_argument() -> None:
    status, error = host.decode_null_plan(b"{}")
    assert status == OX_INVALID_ARGUMENT and "NULL plan" in str(error)


def test_empty_input_is_a_syntax_decode_error() -> None:
    plan = host.plan_for(User)
    problem = host_problem(plan, b"")
    assert problem["status"] == 400
    assert problem["errors"] == python_errors(b"", User)
    assert problem["errors"][0]["code"] == "syntax"
    assert problem["errors"][0]["ctx"]["format"] == "json"


def test_invalid_utf8_is_a_syntax_error_with_the_position() -> None:
    plan = host.plan_for(User)
    problem = host_problem(plan, b'{"id": 1, "name": "\xff"}')
    assert problem["errors"] == python_errors(b'{"id": 1, "name": "\xff"}', User)
    assert problem["status"] == 400


def test_a_python_exception_of_the_sink_comes_through() -> None:
    def explode() -> list[int]:
        raise RuntimeError("factory failed")

    @ox.model
    class Exploding:
        id: int = ox.field(default_factory=explode)  # pyright: ignore[reportAssignmentType]

    plan = host.plan_for(Exploding)
    with pytest.raises(RuntimeError, match="factory failed"):
        plan.decode(b"{}")


# --- issues ------------------------------------------------------------------------------------

ISSUE_CASES = [
    b'{"id": "x", "name": "", "age": 200}',
    b'{"id": 1, "name": "a", "addresses": [{"street": "", "zip": "9"},'
    b' {"street": "ok", "zip": 3001}]}',
    b'{"id": 1, "name": "a", "pet": {"kind": "bird"}}',
    b'{"id": 1, "name": "a", "pet": {"lives": 0}}',
    b'{"id": 1, "name": "a", "pet": 5}',
    b'{"id": 1, "name": "toolongname"}',
    b'{"id": 1, "name": "a", "tags": {"a": "b"}, "addresses": [1, null]}',
    b'{"id": 1, "id": 2, "name": "a"}',
    b'{"id": 1, "name": }',
    b"[1, 2",
    b"{}",
]


@pytest.mark.parametrize("document", ISSUE_CASES)
def test_issues_round_trip_exactly_like_the_python_errors(document: bytes) -> None:
    problem = host_problem(host.plan_for(User), document)
    assert problem["errors"] == python_errors(document, User)
    assert problem["type"] == "about:blank"
    assert problem["status"] in (400, 422)
    assert problem["detail"] == f"{len(problem['errors'])} error(s)"


def test_union_variants_and_literal_values_survive_in_the_context() -> None:
    document = (
        b'{"id": 1, "name": "a", "pet": {"kind": "bird"},'
        b' "addresses": [{"street": "s", "zip": "x"}]}'
    )
    errors = host_problem(host.plan_for(User), document)["errors"]
    by_code = {error["code"]: error for error in errors}
    assert by_code["literal"]["ctx"]["allowed"] == ["1000", "2000", 3000, None]
    assert by_code["tag"]["ctx"]["allowed"] == ["dog", "cat"]
    untagged = host.plan_for(UNTAGGED)
    problem = host_problem(untagged, b'[{"street": "", "zip": 1}]')
    union = problem["errors"][0]
    assert union["code"] == "union" and set(union["ctx"]["errors"]) == {"Address", "Loose"}
    assert problem["errors"] == python_errors(b'[{"street": "", "zip": 1}]', UNTAGGED)


def test_a_decode_that_stopped_reading_is_a_decode_report() -> None:
    plan = host.plan_for(User)
    assert host_problem(plan, b'{"id": ')["status"] == 400
    assert host_problem(plan, b'{"id": "x"}')["status"] == 422
    assert host_problem(plan, b"{}" + b" " * 100, limits={"max_bytes": 5})["status"] == 413


# --- sources and combined reports --------------------------------------------------------------


def test_a_report_is_rewritten_in_a_source() -> None:
    plan = host.plan_for(User)
    with pytest.raises(host.HostProblem) as raised:
        plan.decode(b'{"id": "x", "pet": {"kind": "bird"}}', source="query")
    with pytest.raises(ValidationError) as expected:
        ox.json.decode(b'{"id": "x", "pet": {"kind": "bird"}}', User)
    assert problem_errors(raised.value) == expected.value.with_source("query").errors()
    pointers = [error["pointer"] for error in problem_errors(raised.value)]
    assert all(pointer.startswith("/query/") for pointer in pointers)


def test_a_source_that_is_not_valid_is_refused() -> None:
    with pytest.raises(ValueError, match="argument is wrong"):
        host.plan_for(User).decode(b"{}", source="")


def request_parts() -> list[tuple[str, Any, bytes, str]]:
    return [
        ("path", host.plan_for(Base), b'{"id": "7"}', "lax"),
        ("query", host.plan_for(Loose), b'{"name": 3}', "strict"),
        ("body", host.plan_for(User), b'{"id": "x", "name": ""}', "strict"),
    ]


def test_the_parts_of_a_request_are_combined_in_the_order_given() -> None:
    parts = request_parts()
    with pytest.raises(host.HostProblem) as raised:
        host.decode_request(parts)
    errors = ValidationError.combine(
        {
            "query": python_error(b'{"name": 3}', Loose),
            "body": python_error(b'{"id": "x", "name": ""}', User),
        }
    ).errors()
    expected = [
        {**error, "path": ["query", *error["path"]], "pointer": "/query" + error["pointer"]}
        for error in python_errors(b'{"name": 3}', Loose)
    ] + [
        {**error, "path": ["body", *error["path"]], "pointer": "/body" + error["pointer"]}
        for error in python_errors(b'{"id": "x", "name": ""}', User)
    ]
    assert problem_errors(raised.value) == errors == expected
    assert raised.value.args[0]["status"] == 422


def test_a_combined_report_keeps_the_budget_and_says_it_was_cut() -> None:
    with pytest.raises(host.HostProblem) as raised:
        host.decode_request(request_parts(), max_errors=2)
    problem = raised.value.args[0]
    assert len(problem["errors"]) == 2 and problem["truncated"]
    assert problem["errors"][0]["pointer"].startswith("/query")


def test_a_combine_of_unreadable_parts_only_is_a_decode_report() -> None:
    parts = [("body", host.plan_for(User), b'{"id": ', "strict")]
    with pytest.raises(host.HostProblem) as raised:
        host.decode_request(parts)
    assert raised.value.args[0]["status"] == 400
    mixed = [("query", host.plan_for(Loose), b'{"name": 3}', "strict"), *parts]
    with pytest.raises(host.HostProblem) as raised:
        host.decode_request(mixed)
    assert raised.value.args[0]["status"] == 422
    assert [e["pointer"] for e in raised.value.args[0]["errors"]] == ["/query/name", "/body/id"]


def test_a_request_without_problems_returns_the_values() -> None:
    parts = [
        ("path", host.plan_for(Base), b'{"id": "7"}', "lax"),
        ("body", host.plan_for(Loose), b'{"name": "n"}', "strict"),
    ]
    values = host.decode_request(parts)
    assert values["path"].id == 7 and values["body"].name == "n"


# --- encode ------------------------------------------------------------------------------------


def test_encode_against_a_declared_type_never_writes_fields_outside_it() -> None:
    derived = Derived(id=1, secret="s3cret")
    declared = host.encode(derived, host.plan_for(Base))
    assert declared == ox.json.encode(derived, type=Base) == b'{"id":1}'
    assert host.encode(derived) == ox.json.encode(derived) == b'{"id":1,"secret":"s3cret"}'


def test_encode_options_are_those_of_the_python_api() -> None:
    value = {"b": 1, "a": [1, 2]}
    assert host.encode(value, sort_keys=True) == ox.json.encode(value, sort_keys=True)
    assert host.encode(value, indent=2) == ox.json.encode(value, indent=2)
    assert host.encode(value, sort_keys=True, indent=0) == ox.json.encode(
        value, sort_keys=True, indent=0
    )


def test_encode_appends_after_what_the_host_buffer_holds() -> None:
    assert host.encode({"a": 1}, prefix=b"HTTP/1.1 200\r\n\r\n") == b'HTTP/1.1 200\r\n\r\n{"a":1}'


def test_a_buffer_that_grows_is_grown_by_the_host_and_holds_the_whole_output() -> None:
    payload = [{"k": "v" * 50, "n": n} for n in range(500)]
    status, written, calls, error = host.encode_raw(
        payload, buffer="grow", capacity=1, prefix=b"=>"
    )
    assert (status, error) == (OX_OK, None)
    assert written == b"=>" + ox.json.encode(payload)
    assert calls == 1
    status, written, calls, _ = host.encode_raw(payload, buffer="grow", capacity=10**6)
    assert (status, calls) == (OX_OK, 0) and written == ox.json.encode(payload)


def test_a_fixed_buffer_is_left_alone_when_it_is_too_small() -> None:
    status, written, calls, error = host.encode_raw(
        {"a": 1}, buffer="fixed", capacity=4, prefix=b"ab"
    )
    assert status == OX_BUFFER and written == b"ab" and calls == 0
    assert isinstance(error, BufferError)
    assert "fixed at 4 bytes with 2 used" in str(error) and "reserve" in str(error)
    status, written, *_ = host.encode_raw({"a": 1}, buffer="fixed", capacity=2 + 7, prefix=b"ab")
    assert status == OX_OK and written == b'ab{"a":1}'


def test_a_reserve_that_fails_or_lies_is_a_buffer_error_and_nothing_is_written() -> None:
    status, written, calls, error = host.encode_raw(
        {"a": 1}, buffer="refuse", capacity=1, prefix=b"x"
    )
    assert (status, written, calls) == (OX_BUFFER, b"x", 1)
    assert isinstance(error, BufferError) and "reserve callback of the buffer failed" in str(error)
    status, written, calls, error = host.encode_raw({"a": 1}, buffer="lie", capacity=1, prefix=b"x")
    assert (status, written, calls) == (OX_BUFFER, b"x", 1)
    assert isinstance(error, BufferError) and "broke its contract" in str(error)


def test_an_object_that_cannot_be_written_leaves_the_buffer_as_it_was() -> None:
    status, written, calls, error = host.encode_raw(
        object(), buffer="grow", capacity=0, prefix=b"keep"
    )
    assert status == OX_PYTHON_ERROR and written == b"keep" and calls == 0
    assert isinstance(error, ox.EncodeError)
    with pytest.raises(ox.EncodeError):
        host.encode(object())


def test_encoding_as_a_declared_type_the_object_is_not_raises_an_encode_error() -> None:
    with pytest.raises(ox.EncodeError):
        host.encode(Loose("x"), host.plan_for(Base))


@pytest.mark.parametrize(
    ("options", "status", "error_type", "fragment"),
    [
        ({"flags": 0x80}, OX_UNSUPPORTED, NotImplementedError, "upgrade oxiform"),
        ({"struct_size": 4}, OX_INVALID_ARGUMENT, ValueError, "struct_size"),
        ({"format": 3}, OX_UNSUPPORTED, NotImplementedError, "OX_FORMAT_JSON"),
        ({"buffer_struct_size": 8}, OX_INVALID_ARGUMENT, ValueError, "buffer.struct_size is 8"),
    ],
)
def test_encode_arguments_oxiform_cannot_honour_say_what_to_do(
    options: dict[str, Any], status: int, error_type: type[Exception], fragment: str
) -> None:
    got, written, _, error = host.encode_raw({"a": 1}, prefix=b"p", **options)
    assert got == status and written == b"p"
    assert type(error) is error_type and fragment in str(error)


# --- threads -----------------------------------------------------------------------------------


def test_one_plan_serves_many_threads() -> None:
    plan = host.plan_for(User)

    def work(n: int) -> tuple[int, bytes]:
        total = 0
        for index in range(150):
            document = b'{"id": %d, "name": "t%d"}' % (index, n)
            total += plan.decode(document).id
            try:
                plan.decode(b'{"id": "x"}')
            except host.HostProblem as exc:
                total += len(exc.args[0]["errors"])
        return total, host.encode(plan.decode(b'{"id": 1, "name": "z"}'), plan)

    with ThreadPoolExecutor(max_workers=8) as pool:
        results = list(pool.map(work, range(8)))
    assert {total for total, _ in results} == {sum(range(150)) + 150 * 2}
    assert {encoded for _, encoded in results} == {
        ox.json.encode(ox.json.decode(b'{"id": 1, "name": "z"}', User))
    }
