"""Pair sources (OX-3): query strings, forms, headers and cookies read as a model in lax mode."""

import enum
from dataclasses import dataclass, field
from typing import Annotated, Any
from uuid import UUID

import pytest

import oxiform as ox


class Color(enum.Enum):
    RED = "red"
    BLUE = "blue"


@dataclass(slots=True)
class Query:
    page: Annotated[int, ox.Ge(1)] = 1
    size: int | None = None
    q: str | None = None
    tags: list[str] = field(default_factory=list[str])
    ids: tuple[int, ...] = ()
    colors: frozenset[Color] = frozenset()
    active: bool = False


@dataclass(slots=True)
class Required:
    page: int
    q: str | None  # optional without a default: None when the name is absent


@ox.model
class Headers:
    request_id: UUID = ox.field(rename="X-Request-Id")
    accept: list[str] = ox.field(default_factory=list, rename="Accept")
    user_agent: str | None = ox.field(default=None, rename="User-Agent")


@ox.model
class Aliased:
    page_size: int = ox.field(default=10, rename="page-size", aliases=["limit"])


@ox.model
class Names:
    spaced: str = ox.field(default="", rename="a b")
    note: str = ""


@ox.model(unknown="forbid")
class Strict:
    page: int = 1


def errors(call: Any, *args: Any, **kwargs: Any) -> list[dict[str, Any]]:
    with pytest.raises(ox.ValidationError) as raised:
        call(*args, **kwargs)
    return [dict(error) for error in raised.value.errors()]


def codes(call: Any, *args: Any, **kwargs: Any) -> list[tuple[str, str]]:
    return [(error["code"], error["pointer"]) for error in errors(call, *args, **kwargs)]


# --- query strings and forms ------------------------------------------------------------------


def test_a_query_string_is_read_in_lax_mode() -> None:
    query = ox.urlencoded.decode(b"page=3&size=10&q=hello&active=yes", Query)
    assert query == Query(page=3, size=10, q="hello", active=True)
    assert type(query.page) is int


def test_the_data_may_be_bytes_str_or_any_buffer() -> None:
    expected = Query(page=2)
    assert ox.urlencoded.decode("page=2", Query) == expected
    assert ox.urlencoded.decode(b"page=2", Query) == expected
    assert ox.urlencoded.decode(bytearray(b"page=2"), Query) == expected
    assert ox.urlencoded.decode(memoryview(b"page=2"), Query) == expected


def test_a_repeated_name_is_a_list_a_tuple_or_a_set_in_order() -> None:
    query = ox.urlencoded.decode(
        "tags=b&ids=3&tags=a&colors=red&ids=1&colors=blue&colors=red&tags=c", Query
    )
    assert query.tags == ["b", "a", "c"]
    assert query.ids == (3, 1)
    assert query.colors == frozenset({Color.RED, Color.BLUE})


def test_a_single_value_is_a_list_of_one_and_an_absent_name_takes_the_default() -> None:
    query = ox.urlencoded.decode("tags=only", Query)
    assert query.tags == ["only"] and query.ids == () and query.page == 1


def test_a_repeated_name_on_a_scalar_field_is_a_duplicate_key() -> None:
    found = errors(ox.urlencoded.decode, "page=1&page=2", Query)
    assert [(e["code"], e["pointer"], e["path"], e["input"]) for e in found] == [
        ("duplicate_key", "/page", ["page"], "page")
    ]
    assert "declare it as a list" in found[0]["message"]
    assert codes(ox.urlencoded.decode, "q=a&q=b&q=c", Query) == [("duplicate_key", "/q")]


def test_an_optional_field_that_is_absent_is_none_with_or_without_a_default() -> None:
    assert ox.urlencoded.decode("page=1", Required) == Required(page=1, q=None)
    assert ox.urlencoded.decode("page=1", Query).size is None
    assert codes(ox.urlencoded.decode, "q=x", Required) == [("missing", "/page")]


def test_an_empty_value_is_none_for_optional_types_and_text_for_str() -> None:
    query = ox.urlencoded.decode("size=&q=&page=2", Query)
    assert query.size is None
    assert query.q == ""
    assert ox.urlencoded.decode("size", Query).size is None  # a name without "="


@pytest.mark.parametrize(
    ("text", "value"),
    [("1", True), ("true", True), ("ON", True), ("0", False), ("No", False), ("off", False)],
)
def test_the_lax_table_reads_bools(text: str, value: bool) -> None:
    assert ox.urlencoded.decode(f"active={text}", Query).active is value


@pytest.mark.parametrize(
    ("query", "pointer"),
    [
        ("page=1.5", "/page"),
        ("page=abc", "/page"),
        ("page=1e3", "/page"),
        ("size=4.5", "/size"),
        ("active=maybe", "/active"),
        ("ids=1&ids=x", "/ids/1"),
        ("colors=green", "/colors/0"),
        ("page=%20", "/page"),
    ],
)
def test_text_that_is_not_the_type_is_a_problem_with_the_wire_name(
    query: str, pointer: str
) -> None:
    assert errors(ox.urlencoded.decode, query, Query)[0]["pointer"] == pointer


def test_the_problems_say_which_value_of_a_list_is_wrong() -> None:
    assert codes(ox.urlencoded.decode, "ids=1&ids=x&ids=3&ids=y", Query) == [
        ("type", "/ids/1"),
        ("type", "/ids/3"),
    ]


def test_leading_zeros_are_read_and_the_constraint_is_checked_after() -> None:
    assert ox.urlencoded.decode("page=007", Query).page == 7
    assert ox.urlencoded.decode("page=2.0", Query).page == 2
    assert codes(ox.urlencoded.decode, "page=0", Query) == [("too_small", "/page")]


# --- percent-decoding -------------------------------------------------------------------------


def test_a_plus_is_a_space_and_an_escaped_plus_or_space_is_what_it_says() -> None:
    assert ox.urlencoded.decode("q=a+b", Query).q == "a b"
    assert ox.urlencoded.decode("q=a%20b", Query).q == "a b"
    assert ox.urlencoded.decode("q=a%2Bb", Query).q == "a+b"
    assert ox.urlencoded.decode("q=%2B%2b+", Query).q == "++ "


def test_names_are_decoded_and_utf8_is_read() -> None:
    assert ox.urlencoded.decode("a+b=1&note=%C3%A9%E2%82%AC", Names) == Names(spaced="1", note="é€")
    assert ox.urlencoded.decode("a%20b=2", Names).spaced == "2"
    assert ox.urlencoded.decode("note=é", Names).note == "é"  # raw non-ASCII text is fine too


@pytest.mark.parametrize("bad", ["q=%zz", "q=%4", "q=%", "q=a%g1", "%zz=1"])
def test_a_malformed_percent_escape_is_a_syntax_problem_with_a_pointer(bad: str) -> None:
    with pytest.raises(ox.DecodeError) as raised:
        ox.urlencoded.decode(bad, Query)
    (error,) = raised.value.errors()
    assert error["code"] == "syntax"
    assert error["ctx"]["format"] == "urlencoded"
    assert "percent-escape" in error["message"] and "%25" in error["message"]
    assert error["pointer"] == ("" if bad.startswith("%zz") else "/q")
    assert str(dict(error["ctx"])["column"]).isdigit()


@pytest.mark.parametrize("bad", ["q=%ff", "q=%C3", "q=%C3%28", "q=%80abc", "q=a%e2%82"])
def test_bytes_that_are_not_utf8_are_a_syntax_problem(bad: str) -> None:
    with pytest.raises(ox.DecodeError) as raised:
        ox.urlencoded.decode(bad, Query)
    (error,) = raised.value.errors()
    assert error["code"] == "syntax" and "UTF-8" in error["message"]


def test_raw_bytes_that_are_not_utf8_are_a_syntax_problem_too() -> None:
    with pytest.raises(ox.DecodeError):
        ox.urlencoded.decode(b"q=\xff", Query)


def test_an_unreadable_source_stops_at_the_fault_and_is_a_decode_error() -> None:
    with pytest.raises(ox.DecodeError) as raised:
        ox.urlencoded.decode("page=x&q=%zz", Query)
    # The problem found before the fault is kept; nothing after it is read.
    assert [(e["code"], e["pointer"]) for e in raised.value.errors()] == [
        ("type", "/page"),
        ("syntax", "/q"),
    ]


# --- aliases, unknown names and limits --------------------------------------------------------


def test_names_are_matched_by_wire_name_and_aliases() -> None:
    assert ox.urlencoded.decode("page-size=5", Aliased).page_size == 5
    assert ox.urlencoded.decode("limit=7", Aliased).page_size == 7
    assert ox.urlencoded.decode("page_size=7", Aliased).page_size == 10  # not the wire name
    assert codes(ox.urlencoded.decode, "limit=1&page-size=2", Aliased) == [
        ("duplicate_key", "/page-size")
    ]


def test_unknown_names_are_ignored_unless_forbidden_per_call() -> None:
    assert ox.urlencoded.decode("page=2&utm=x&utm=y", Query).page == 2
    found = errors(ox.urlencoded.decode, "page=2&utm=x&utm=y&zz=1", Query, unknown="forbid")
    assert [(e["code"], e["pointer"]) for e in found] == [
        ("unknown_field", "/utm"),
        ("unknown_field", "/zz"),
    ]
    assert ox.urlencoded.decode("utm=1", Query, unknown="ignore") == Query()


def test_a_model_that_forbids_unknown_names_is_honoured_and_ignore_overrides_it() -> None:
    assert codes(ox.urlencoded.decode, "other=1", Strict) == [("unknown_field", "/other")]
    assert ox.urlencoded.decode("other=1", Strict, unknown="ignore").page == 1


def test_max_errors_collects_that_many_problems() -> None:
    query = "page=x&size=y&active=z&ids=q"
    assert len(errors(ox.urlencoded.decode, query, Query)) == 4
    assert len(errors(ox.urlencoded.decode, query, Query, max_errors=2)) == 2


def test_limits_apply_to_the_pairs_the_strings_and_the_whole() -> None:
    with pytest.raises(ox.DecodeError, match="max_items") as items:
        ox.urlencoded.decode("a=1&b=2&c=3", Query, limits=ox.Limits(max_items=2))
    assert items.value.errors()[0]["ctx"]["limit"] == "max_items"
    with pytest.raises(ox.DecodeError, match="max_str_len") as text:
        ox.urlencoded.decode("q=abcdef", Query, limits=ox.Limits(max_str_len=5))
    assert text.value.errors()[0]["ctx"]["limit"] == "max_str_len"
    with pytest.raises(ox.DecodeError, match="max_bytes") as size:
        ox.urlencoded.decode("q=abcdef", Query, limits=ox.Limits(max_bytes=5))
    assert size.value.errors()[0]["ctx"]["limit"] == "max_bytes"
    assert ox.urlencoded.decode("q=abcde", Query, limits=ox.Limits(max_str_len=5)).q == "abcde"


def test_many_repeats_of_a_list_name_are_bounded_by_max_items() -> None:
    with pytest.raises(ox.DecodeError, match="max_items"):
        ox.urlencoded.decode("&".join(["tags=x"] * 50), Query, limits=ox.Limits(max_items=40))


def test_a_type_that_is_not_a_model_says_what_to_declare() -> None:
    with pytest.raises(TypeError, match="declare the parameters as a class"):
        ox.urlencoded.decode("a=1", dict[str, str])
    with pytest.raises(TypeError, match="ox.pairs.decode cannot read pairs as"):
        ox.pairs.decode([("a", "1")], int)


def test_arguments_are_checked_like_json_decode() -> None:
    with pytest.raises(ValueError, match="unknown must be"):
        ox.urlencoded.decode("a=1", Query, unknown="strict")  # pyright: ignore[reportArgumentType, reportCallIssue]
    with pytest.raises(ValueError, match="max_errors must be at least 1"):
        ox.urlencoded.decode("a=1", Query, max_errors=0)
    with pytest.raises(TypeError):
        ox.urlencoded.decode(12, Query)  # pyright: ignore[reportArgumentType, reportCallIssue]


# --- headers and cookies ----------------------------------------------------------------------

ID = "8f14e45f-ceea-467a-9a36-3a1f8a2d7a3b"


def test_header_names_are_case_insensitive_when_asked() -> None:
    headers = ox.pairs.decode(
        [("x-request-id", ID), ("ACCEPT", "text/html"), ("accept", "application/json")],
        Headers,
        case_insensitive=True,
    )
    assert headers == Headers(
        request_id=UUID(ID), accept=["text/html", "application/json"], user_agent=None
    )


def test_names_are_exact_without_case_insensitive() -> None:
    assert errors(ox.pairs.decode, [("x-request-id", ID)], Headers)[0]["code"] == "missing"
    assert ox.pairs.decode([("X-Request-Id", ID)], Headers).request_id == UUID(ID)


def test_two_spellings_of_a_scalar_header_are_a_duplicate_key() -> None:
    found = codes(
        ox.pairs.decode,
        [("X-Request-Id", ID), ("x-request-id", ID)],
        Headers,
        case_insensitive=True,
    )
    assert found == [("duplicate_key", "/X-Request-Id")]


def test_pairs_are_read_as_they_are_without_percent_decoding() -> None:
    @dataclass(slots=True)
    class Cookies:
        theme: str = ""
        token: str = ""

    cookies = ox.pairs.decode([("theme", "dark%20mode+x"), ("token", "a=b")], Cookies)
    assert cookies == Cookies(theme="dark%20mode+x", token="a=b")


def test_pairs_take_bytes_a_mapping_or_any_iterable() -> None:
    expected = Query(page=4, tags=["a", "b"])
    assert ox.pairs.decode([(b"page", b"4"), ("tags", "a"), (b"tags", "b")], Query) == expected
    assert ox.pairs.decode(iter([("page", "4"), ("tags", "a"), ("tags", "b")]), Query) == expected
    assert ox.pairs.decode({"page": "4"}, Query) == Query(page=4)


def test_a_pair_that_is_not_text_is_a_type_error_that_says_what_to_pass() -> None:
    with pytest.raises(TypeError, match=r"iterable of \(name, value\) pairs of str or bytes"):
        ox.pairs.decode([("page", 4)], Query)  # pyright: ignore[reportArgumentType, reportCallIssue]
    with pytest.raises(TypeError, match="not a pair"):
        ox.pairs.decode(["page"], Query)  # pyright: ignore[reportArgumentType, reportCallIssue]
    with pytest.raises(TypeError, match="got int"):
        ox.pairs.decode(5, Query)  # pyright: ignore[reportArgumentType, reportCallIssue]


def test_a_value_that_is_not_utf8_is_a_syntax_problem_at_its_name() -> None:
    with pytest.raises(ox.DecodeError) as raised:
        ox.pairs.decode([("q", b"\xff")], Query)
    (error,) = raised.value.errors()
    assert (error["code"], error["pointer"], error["ctx"]["format"]) == ("syntax", "/q", "pairs")


def test_pairs_limits_and_unknown_are_the_same_as_for_a_query_string() -> None:
    with pytest.raises(ox.DecodeError, match="max_items"):
        ox.pairs.decode([("a", "1")] * 5, Query, limits=ox.Limits(max_items=3))
    assert codes(ox.pairs.decode, [("zz", "1")], Query, unknown="forbid") == [
        ("unknown_field", "/zz")
    ]


# --- sources combined with a body into one problem ---------------------------------------------


@dataclass(slots=True)
class Item:
    name: Annotated[str, ox.Len(min=1)]
    quantity: Annotated[int, ox.Ge(5)]


def raised(call: Any, *args: Any, **kwargs: Any) -> ox.ValidationError:
    with pytest.raises(ox.ValidationError) as caught:
        call(*args, **kwargs)
    return caught.value


def test_the_problems_of_a_request_are_combined_with_source_prefixes() -> None:
    combined = ox.ValidationError.combine(
        {
            "query": raised(ox.urlencoded.decode, "page=0&page=1", Query),
            "header": raised(ox.pairs.decode, [("X-Request-Id", "x")], Headers),
            "body": raised(ox.json.decode, b'{"name": "", "quantity": 0}', Item),
        }
    )
    assert [(e["code"], e["pointer"]) for e in combined.errors()] == [
        ("duplicate_key", "/query/page"),
        ("format", "/header/X-Request-Id"),
        ("too_short", "/body/name"),
        ("too_small", "/body/quantity"),
    ]
    assert combined.unreadable_sources() == []


def test_an_unreadable_query_makes_the_combination_unreadable_for_that_source() -> None:
    with pytest.raises(ox.DecodeError) as query:
        ox.urlencoded.decode("q=%zz", Query)
    sourced = query.value.with_source("query")
    assert [(e["code"], e["pointer"]) for e in sourced.errors()] == [("syntax", "/query/q")]
    combined = ox.ValidationError.combine({"query": query.value})
    assert combined.unreadable_sources() == ["query"]


# --- plans are shared with the JSON decoder ----------------------------------------------------


def test_the_plan_of_a_model_is_the_one_ox_json_uses() -> None:
    from oxiform import _oxiform

    stats = _oxiform._plan_stats  # pyright: ignore[reportPrivateUsage]
    ox.urlencoded.decode("page=2", Query)
    first = stats()
    ox.urlencoded.decode("page=3", Query)
    ox.pairs.decode([("page", "4")], Query)
    ox.json.decode(b'{"page": 5}', Query)
    assert stats() == first
