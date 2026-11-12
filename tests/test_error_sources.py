"""Source prefixes and combined reports (OX-4): one problem for the parts of a request."""

from dataclasses import dataclass
from http import HTTPStatus
from typing import Annotated, Any, Literal

import pytest

import oxiform as ox
from oxiform.errors import ErrorDetails


@dataclass(slots=True)
class Item:
    name: Annotated[str, ox.Len(min=1)]
    quantity: Annotated[int, ox.Ge(5)]


@dataclass(slots=True)
class Page:
    page: Annotated[int, ox.Ge(5)]


@dataclass(slots=True)
class Route:
    id: int


def fail(
    raw: bytes, target: Any, *, mode: Literal["strict", "lax"] = "strict", max_errors: int = 100
) -> ox.ValidationError:
    with pytest.raises(ox.ValidationError) as raised:
        ox.json.decode(raw, target, mode=mode, max_errors=max_errors)
    return raised.value


def request_errors() -> dict[str, ox.ValidationError]:
    return {
        "body": fail(b'{"name": "", "quantity": 0}', Item),
        "query": fail(b'{"page": "2"}', Page, mode="lax"),
        "path": fail(b'{"id": "abc"}', Route, mode="lax"),
    }


def test_three_sources_become_one_problem_with_prefixed_paths_and_pointers() -> None:
    combined = ox.ValidationError.combine(request_errors())
    assert [error["path"] for error in combined.errors()] == [
        ["body", "name"],
        ["body", "quantity"],
        ["query", "page"],
        ["path", "id"],
    ]
    problem = combined.to_problem(
        status=422, type="https://example.com/problems/validation", instance="/items/7"
    )
    assert problem == {
        "type": "https://example.com/problems/validation",
        "title": "Unprocessable Content",
        "status": 422,
        "detail": "4 errors validating request (json)",
        "instance": "/items/7",
        "errors": [
            {
                "pointer": "/body/name",
                "code": "too_short",
                "detail": "length 0 is less than the minimum 1",
            },
            {"pointer": "/body/quantity", "code": "too_small", "detail": "must be at least 5"},
            {"pointer": "/query/page", "code": "too_small", "detail": "must be at least 5"},
            {"pointer": "/path/id", "code": "type", "detail": problem["errors"][3]["detail"]},  # pyright: ignore[reportIndexIssue]
        ],
    }


def test_codes_messages_inputs_and_contexts_are_kept() -> None:
    parts = request_errors()
    combined = ox.ValidationError.combine(parts)
    original = [error for part in parts.values() for error in part.errors()]
    for before, after in zip(original, combined.errors(), strict=True):
        assert after["code"] == before["code"]
        assert after["message"] == before["message"]
        assert after["ctx"] == before["ctx"]
        assert after.get("input") == before.get("input")
        assert after["path"][1:] == before["path"]
    assert [error["code"] for error in combined.errors()] == [
        "too_short",
        "too_small",
        "too_small",
        "type",
    ]


def test_by_path_groups_the_combined_problems_by_prefixed_pointer() -> None:
    combined = ox.ValidationError.combine(request_errors())
    assert list(combined.by_path()) == ["/body/name", "/body/quantity", "/query/page", "/path/id"]


def test_the_parts_are_not_changed() -> None:
    parts = request_errors()
    ox.ValidationError.combine(parts)
    assert parts["body"].errors()[0]["pointer"] == "/name"
    assert parts["body"].errors()[0]["path"] == ["name"]


def test_with_source_prefixes_one_error_and_keeps_its_class_and_flags() -> None:
    error = fail(b'["a", "b", "c"]', list[int], max_errors=2)
    prefixed = error.with_source("query")
    assert [item["pointer"] for item in prefixed.errors()] == ["/query/0", "/query/1"]
    assert prefixed.truncated
    assert type(prefixed) is ox.ValidationError
    with pytest.raises(ox.DecodeError) as raised:
        ox.json.decode(b'{"page": ', Page)
    broken = raised.value.with_source("body")
    assert type(broken) is ox.DecodeError
    assert broken.errors()[0]["pointer"] == "/body/page"
    assert broken.errors()[0]["code"] == "syntax"


def test_an_error_at_the_root_of_a_source_points_at_the_source() -> None:
    error = fail(b'"x"', int)
    assert error.errors()[0]["pointer"] == ""
    assert error.with_source("header").errors()[0]["pointer"] == "/header"


def test_a_prefix_can_be_added_twice_outermost_first() -> None:
    error = fail(b'["a"]', list[int])
    assert error.with_source("name").with_source("outer").errors()[0]["pointer"] == "/outer/name/0"


def test_slashes_and_tildes_are_escaped_in_the_pointer_and_not_in_the_path() -> None:
    error = fail(b'{"a/b": "x", "c~d": "y"}', dict[str, int])
    (first, second) = error.with_source("x/y~z").errors()
    assert first["path"] == ["x/y~z", "a/b"]
    assert first["pointer"] == "/x~1y~0z/a~1b"
    assert second["path"] == ["x/y~z", "c~d"]
    assert second["pointer"] == "/x~1y~0z/c~0d"
    # `~01` is the key "~1", not the key "/".
    tilde_one = fail(b'{"~1": "x"}', dict[str, int]).with_source("body").errors()[0]
    assert tilde_one["pointer"] == "/body/~01"


def test_max_errors_keeps_the_first_problems_across_the_parts_and_says_it_stopped() -> None:
    combined = ox.ValidationError.combine(request_errors(), max_errors=3)
    assert [error["pointer"] for error in combined.errors()] == [
        "/body/name",
        "/body/quantity",
        "/query/page",
    ]
    assert combined.truncated
    assert (
        combined.to_problem()["detail"]
        == "3 errors validating request (json) (stopped at the maximum)"
    )


def test_max_errors_equal_to_the_total_is_not_truncated() -> None:
    combined = ox.ValidationError.combine(request_errors(), max_errors=4)
    assert len(combined.errors()) == 4
    assert not combined.truncated


def test_max_errors_of_one_is_the_first_problem_of_the_first_part() -> None:
    combined = ox.ValidationError.combine(request_errors(), max_errors=1)
    assert [error["pointer"] for error in combined.errors()] == ["/body/name"]
    assert combined.truncated


def test_a_part_that_stopped_early_makes_the_combination_truncated() -> None:
    parts = {
        "body": fail(b'["a", "b", "c"]', list[int], max_errors=2),
        "query": fail(b'{"page": "2"}', Page, mode="lax"),
    }
    combined = ox.ValidationError.combine(parts, max_errors=100)
    assert len(combined.errors()) == 3
    assert combined.truncated


def test_max_errors_is_checked_like_the_decode_calls_check_it() -> None:
    for value in (0, -1, 2**64):
        with pytest.raises(ValueError, match=r"max_errors must be "):
            ox.ValidationError.combine(request_errors(), max_errors=value)
    with pytest.raises(ValueError, match=r"at least 1.*max_errors=1"):
        ox.ValidationError.combine(request_errors(), max_errors=0)
    for value in (1.5, True, False):
        with pytest.raises(TypeError, match=r"max_errors must be a whole number"):
            ox.ValidationError.combine(request_errors(), max_errors=value)  # pyright: ignore[reportArgumentType]


def test_pairs_may_repeat_a_source_and_keep_their_order() -> None:
    first = fail(b'{"page": "2"}', Page, mode="lax")
    second = fail(b'{"id": "abc"}', Route, mode="lax")
    combined = ox.ValidationError.combine(
        [("header", first), ("cookie", second), ("header", second)]
    )
    assert [error["pointer"] for error in combined.errors()] == [
        "/header/page",
        "/cookie/id",
        "/header/id",
    ]


def test_the_combination_is_a_decode_error_only_when_every_part_is_one() -> None:
    def unreadable(raw: bytes) -> ox.DecodeError:
        with pytest.raises(ox.DecodeError) as raised:
            ox.json.decode(raw, Page)
        return raised.value

    both = ox.ValidationError.combine({"body": unreadable(b"{"), "query": unreadable(b"[")})
    assert type(both) is ox.DecodeError
    mixed = ox.ValidationError.combine(
        {"body": unreadable(b"{"), "query": fail(b'{"page": "2"}', Page, mode="lax")}
    )
    assert type(mixed) is ox.ValidationError
    assert [error["code"] for error in mixed.errors()] == ["syntax", "too_small"]
    assert isinstance(mixed, ox.ValidationError)


def test_nothing_to_combine_is_an_empty_validation_error() -> None:
    combined = ox.ValidationError.combine({})
    assert combined.errors() == []
    assert not combined.truncated


def test_the_issues_of_union_variants_in_a_context_move_with_their_parent() -> None:
    variant: ErrorDetails = {
        "code": "type",
        "path": ["v"],
        "pointer": "/v",
        "message": "m",
        "ctx": {},
    }
    union: ErrorDetails = {
        "code": "union",
        "path": ["v"],
        "pointer": "/v",
        "message": "m",
        "ctx": {"variants": {"int": [variant]}, "count": 1},  # pyright: ignore[reportAssignmentType]
    }
    moved = ox.ValidationError([union]).with_source("body").errors()[0]
    assert moved["ctx"]["count"] == 1
    assert moved["ctx"]["variants"] == {  # pyright: ignore[reportUnknownMemberType]
        "int": [
            {"code": "type", "path": ["body", "v"], "pointer": "/body/v", "message": "m", "ctx": {}}
        ]
    }


def test_the_message_of_a_combined_error_lists_every_prefixed_pointer() -> None:
    text = str(ox.ValidationError.combine(request_errors(), target="POST /items"))
    assert text.splitlines()[0] == "4 errors validating POST /items (json):"
    assert "  /body/name      length 0 is less than the minimum 1  [too_short]" in text
    assert "/path/id" in text


@pytest.mark.parametrize("source", ["", 1, None])
def test_a_source_must_be_a_non_empty_string(source: object) -> None:
    error = fail(b'"x"', int)
    with pytest.raises(ValueError, match=r"non-empty string such as"):
        error.with_source(source)  # pyright: ignore[reportArgumentType]
    with pytest.raises(ValueError, match=r"non-empty string such as"):
        ox.ValidationError.combine({source: error})  # pyright: ignore[reportArgumentType]


def test_a_part_must_be_a_validation_error() -> None:
    with pytest.raises(TypeError, match=r"'query'.*str.*pass the error a decode call raised"):
        ox.ValidationError.combine({"query": "nope"})  # pyright: ignore[reportArgumentType]


def test_to_problem_defaults_are_unchanged_and_instance_is_left_out() -> None:
    problem = fail(b'"x"', int).to_problem()
    assert problem["type"] == "about:blank"
    assert problem["title"] == "Unprocessable Content"
    assert problem["status"] == 422
    assert "instance" not in problem


def test_to_problem_takes_a_status_and_the_title_follows_it() -> None:
    error = fail(b'"x"', int)
    assert error.to_problem(status=400)["title"] == "Bad Request"
    assert error.to_problem(status=400)["status"] == 400
    assert error.to_problem(status=413, title="Too big")["title"] == "Too big"


@pytest.mark.parametrize("status", [200, 399, 600, 999, True, False, None, "422", 422.0])
@pytest.mark.parametrize("title", [None, "Custom title"])
def test_to_problem_refuses_a_status_that_is_not_an_error_status_whatever_the_title(
    status: object, title: str | None
) -> None:
    with pytest.raises(ValueError, match=r"status must be an HTTP error status from 400 to 599"):
        fail(b'"x"', int).to_problem(status=status, title=title)  # pyright: ignore[reportArgumentType]


def test_to_problem_turns_an_http_status_member_into_a_plain_int() -> None:
    problem = fail(b'"x"', int).to_problem(status=HTTPStatus.UNPROCESSABLE_ENTITY)
    assert problem["status"] == 422
    assert type(problem["status"]) is int
    assert problem["title"] == "Unprocessable Content"


@pytest.mark.parametrize(
    ("status", "title"),
    [
        (400, "Bad Request"),
        (402, "Payment Required"),
        (407, "Proxy Authentication Required"),
        (413, "Content Too Large"),
        (507, "Insufficient Storage"),
        (508, "Loop Detected"),
        (511, "Network Authentication Required"),
    ],
)
def test_to_problem_knows_the_registered_titles(status: int, title: str) -> None:
    assert fail(b'"x"', int).to_problem(status=status)["title"] == title


@pytest.mark.parametrize("name", ["type", "title", "instance"])
@pytest.mark.parametrize("value", ["", 1, b"x", ["a"]])
def test_to_problem_checks_type_title_and_instance_are_non_empty_strings(
    name: str, value: object
) -> None:
    arguments: dict[str, Any] = {name: value}
    with pytest.raises(ValueError, match=rf"{name} must be a non-empty string.*pass "):
        fail(b'"x"', int).to_problem(**arguments)


def test_to_problem_asks_for_a_title_when_it_has_none_for_the_status() -> None:
    with pytest.raises(ValueError, match=r"pass title="):
        fail(b'"x"', int).to_problem(status=499)
    assert fail(b'"x"', int).to_problem(status=499, title="Client Closed Request")["status"] == 499


@pytest.mark.parametrize("parts", ["ab", b"ab", 5, None])
def test_combine_refuses_parts_that_are_not_a_mapping_or_pairs(parts: object) -> None:
    with pytest.raises(TypeError, match=r"cannot read parts of type.*mapping of source to Validat"):
        ox.ValidationError.combine(parts)  # pyright: ignore[reportArgumentType]


@pytest.mark.parametrize("item", [("query",), ("a", "b", "c"), "ab", 7])
def test_combine_refuses_a_part_that_is_not_a_pair(item: object) -> None:
    with pytest.raises(TypeError, match=r"where a \(source, error\) pair belongs"):
        ox.ValidationError.combine([item])  # pyright: ignore[reportArgumentType]


def test_combine_refuses_a_bare_error_in_the_list() -> None:
    with pytest.raises(TypeError, match=r"where a \(source, error\) pair belongs"):
        ox.ValidationError.combine([fail(b'"x"', int)])  # pyright: ignore[reportArgumentType]


def test_combining_nothing_says_so_without_empty_parentheses() -> None:
    problem = ox.ValidationError.combine({}).to_problem()
    assert problem["detail"] == "0 errors validating request"
    assert problem["errors"] == []


def test_by_source_groups_the_combined_problems_by_their_source() -> None:
    grouped = ox.ValidationError.combine(request_errors()).by_source()
    assert list(grouped) == ["body", "query", "path"]
    assert [error["pointer"] for error in grouped["body"]] == ["/body/name", "/body/quantity"]
    assert [error["code"] for error in grouped["path"]] == ["type"]


def test_unreadable_sources_name_the_parts_that_could_not_be_read() -> None:
    with pytest.raises(ox.DecodeError) as syntax:
        ox.json.decode(b"{", Page)
    with pytest.raises(ox.DecodeError) as too_big:
        ox.json.decode(b'{"page": 1}', Page, limits=ox.Limits(max_bytes=3))
    readable = ox.ValidationError.combine(request_errors())
    assert readable.unreadable_sources() == []
    mixed = ox.ValidationError.combine(
        {
            "path": request_errors()["path"],
            "query": syntax.value,
            "body": too_big.value,
            "header": request_errors()["query"],
        }
    )
    assert mixed.unreadable_sources() == ["query", "body"]
    assert type(mixed) is ox.ValidationError


def test_the_budget_is_first_come_so_the_order_of_the_parts_is_the_priority() -> None:
    parts = request_errors()
    by_priority = ox.ValidationError.combine(
        [("path", parts["path"]), ("query", parts["query"]), ("body", parts["body"])], max_errors=2
    )
    assert [error["pointer"] for error in by_priority.errors()] == ["/path/id", "/query/page"]


def test_a_prefixed_problem_keeps_the_key_order_of_the_engine() -> None:
    original = fail(b'{"name": "", "quantity": 1}', Item).errors()[0]
    moved = ox.ValidationError.combine(
        {"body": fail(b'{"name": "", "quantity": 1}', Item)}
    ).errors()[0]
    assert list(moved) == list(original) == ["code", "path", "pointer", "message", "input", "ctx"]
