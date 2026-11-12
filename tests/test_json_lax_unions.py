"""Lax mode in unions: which variant reads a string (section 4.5 of the spec).

``Optional[X]`` is written out on purpose: it is a different declaration from ``X | None``.
"""

# ruff: noqa: UP045

import datetime
from dataclasses import dataclass
from typing import Annotated, Any, Literal, Optional

import pytest

import oxiform as ox


def lax(raw: bytes, kind: Any) -> Any:
    return ox.json.decode(raw, kind, mode="lax")


@pytest.mark.parametrize(
    "kind",
    [
        int | float | None,
        int | bool | None,
        float | None,
        bool | None,
        Optional[int | float],
        Optional[int | bool],
        Optional[int],
    ],
)
def test_an_empty_string_is_none_in_every_optional_number_union(kind: Any) -> None:
    assert lax(b'""', kind) is None
    assert lax(b"null", kind) is None
    with pytest.raises(ox.ValidationError):
        ox.json.decode(b'""', kind)


def test_optional_unions_still_read_their_other_values() -> None:
    assert lax(b'"4"', int | float | None) == 4
    assert lax(b'"4.5"', int | float | None) == 4.5
    assert lax(b'"yes"', int | bool | None) is True
    with pytest.raises(ox.ValidationError):
        lax(b'"x"', int | float | None)


def test_an_empty_string_stays_text_when_the_type_can_read_it() -> None:
    assert lax(b'""', str | None) == ""
    assert lax(b'""', Optional[Literal["", "a"]]) == ""
    assert lax(b'""', int | Literal["", "a"] | None) == ""


def test_an_empty_string_is_none_for_types_with_no_empty_form() -> None:
    assert lax(b'""', Optional[Literal["a", "b"]]) is None
    assert lax(b'""', int | Literal["auto"] | None) is None
    assert lax(b'""', Optional[int | Literal["auto"]]) is None
    assert lax(b'""', Optional[datetime.date]) is None
    assert lax(b'""', datetime.date | None) is None


WORKERS = int | Literal["auto"]


@pytest.mark.parametrize("kind", [WORKERS, Literal["auto"] | int])
def test_a_number_written_as_text_is_read_next_to_a_literal(kind: Any) -> None:
    assert lax(b'"4"', kind) == 4
    assert type(lax(b'"4"', kind)) is int
    assert lax(b'"-4.0"', kind) == -4
    assert lax(b"4", kind) == 4
    assert lax(b'"auto"', kind) == "auto"
    with pytest.raises(ox.ValidationError) as caught:
        lax(b'"4.5"', kind)
    assert [error["code"] for error in caught.value.errors()] == ["literal"]


def test_the_environment_variable_case_of_a_config() -> None:
    @dataclass(slots=True)
    class Server:
        workers: int | Literal["auto"] = "auto"
        timeout: Annotated[int, ox.Ge(2)] | None = None

    assert ox.json.decode(b'{"workers": "4", "timeout": ""}', Server, mode="lax") == Server(4, None)
    assert ox.json.decode(b'{"workers": "auto"}', Server, mode="lax") == Server("auto")
    with pytest.raises(ox.ValidationError):
        ox.json.decode(b'{"workers": "4"}', Server)


def test_a_literal_member_beats_a_coercion_that_reads_the_same_text() -> None:
    assert lax(b'"yes"', bool | Literal["yes"]) == "yes"
    assert lax(b'"on"', bool | Literal["yes"]) is True
    assert lax(b'"0"', bool | Literal["yes"]) is False
    assert lax(b'"1"', int | Literal["1"]) == "1"
    assert lax(b"1", int | Literal["1"]) == 1


def test_constraints_apply_to_the_number_read_from_text() -> None:
    with pytest.raises(ox.ValidationError) as caught:
        lax(b'"4"', Annotated[int, ox.Ge(10)] | Literal["auto"])
    assert [error["code"] for error in caught.value.errors()] == ["too_small"]


def test_optional_union_with_a_literal() -> None:
    assert lax(b'"4"', Optional[int | Literal["auto"]]) == 4
    assert lax(b'"auto"', Optional[int | Literal["auto"]]) == "auto"
    assert lax(b"null", Optional[int | Literal["auto"]]) is None
    assert lax(b'"yes"', bool | Literal["auto"]) is True


def test_text_that_has_the_form_of_a_date_is_a_date_and_other_text_is_a_number() -> None:
    kinds: list[Any] = [int | datetime.date, datetime.date | int, Optional[int | datetime.date]]
    for kind in kinds:
        assert lax(b'"2026-01-31"', kind) == datetime.date(2026, 1, 31)
        assert lax(b'"4"', kind) == 4
        with pytest.raises(ox.ValidationError) as caught:
            lax(b'"soon"', kind)
        assert [error["code"] for error in caught.value.errors()] == ["format"]
    stamp = lax(b'"2026-01-31T10:00:00Z"', int | datetime.datetime)
    assert isinstance(stamp, datetime.datetime)
    assert lax(b'"4"', int | datetime.datetime) == 4


def test_a_str_variant_still_takes_every_string() -> None:
    assert lax(b'"4"', int | str) == "4"
