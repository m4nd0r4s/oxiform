"""Lax mode in unions: which variant reads a string (section 4.5 of the spec).

``Optional[X]`` is written out on purpose: it is a different declaration from ``X | None``.
"""

# ruff: noqa: UP007, UP045

import datetime
import enum
import ipaddress
import pathlib
import uuid
import zoneinfo
from dataclasses import dataclass
from decimal import Decimal
from typing import Annotated, Any, Literal, Optional, Union

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


class Perm(enum.IntFlag):
    READ = 1
    WRITE = 2


class Colour(enum.Enum):
    RED = "red"


class Level(enum.IntEnum):
    LOW = 1


PATHS: tuple[Any, ...] = (
    pathlib.Path,
    pathlib.PurePath,
    pathlib.PosixPath,
    pathlib.PurePosixPath,
    pathlib.PureWindowsPath,
)

NO_EMPTY_FORM: list[Any] = [
    int,
    float,
    bool,
    Decimal,
    datetime.date,
    datetime.datetime,
    datetime.time,
    datetime.timedelta,
    uuid.UUID,
    zoneinfo.ZoneInfo,
    *PATHS,
    ipaddress.IPv4Address,
    ipaddress.IPv6Address,
    ipaddress.IPv4Network,
    ipaddress.IPv6Network,
    ipaddress.IPv4Interface,
    ipaddress.IPv6Interface,
    Perm,
    Level,
    Colour,
    Literal["a", "b"],
    Literal[1, 2],
    list[int],
    dict[str, int],
]


@pytest.mark.parametrize("kind", NO_EMPTY_FORM, ids=repr)
def test_an_empty_string_is_none_for_every_type_with_no_empty_form(kind: Any) -> None:
    assert lax(b'""', Optional[kind]) is None
    assert lax(b'""', kind | None) is None
    assert lax(b'""', Union[kind, None]) is None
    assert lax(b"null", Optional[kind]) is None
    if kind not in PATHS:  # Path("") is a valid path: only lax mode reads it as unset
        with pytest.raises(ox.ValidationError):
            ox.json.decode(b'""', Optional[kind])


@pytest.mark.parametrize("kind", [kind for kind in NO_EMPTY_FORM if kind not in PATHS], ids=repr)
def test_without_none_an_empty_string_is_still_an_error(kind: Any) -> None:
    with pytest.raises(ox.ValidationError):
        lax(b'""', kind)


def test_an_unset_path_is_none_and_never_the_current_directory() -> None:
    assert lax(b'""', Optional[pathlib.Path]) is None
    assert lax(b'""', pathlib.Path | None) is None
    assert lax(b'"a/b"', pathlib.Path | None) == pathlib.Path("a/b")
    assert lax(b'"a/b"', Optional[pathlib.Path]) == pathlib.Path("a/b")
    assert lax(b'""', pathlib.Path) == pathlib.Path()  # asked for a Path, so a Path


def test_types_with_an_empty_form_keep_the_empty_string() -> None:
    assert lax(b'""', Optional[str]) == ""
    assert lax(b'""', str | None) == ""
    assert lax(b'""', Optional[bytes]) == b""
    assert lax(b'""', bytes | None) == b""
    assert lax(b'""', Optional[Literal["", "a"]]) == ""
    assert lax(b'""', Literal["", "a"] | None) == ""


def test_the_other_variants_of_such_a_union_still_read_their_text() -> None:
    address = ipaddress.IPv4Address | None
    assert lax(b'"10.0.0.1"', address) == ipaddress.IPv4Address("10.0.0.1")
    assert lax(b'""', address) is None
    with pytest.raises(ox.ValidationError) as caught:
        lax(b'"nope"', address)
    assert [error["code"] for error in caught.value.errors()] == ["format"]
    ident = uuid.UUID | int | None
    assert lax(b'"7"', ident) == 7
    assert lax(b'""', ident) is None
    assert lax(b'"a8098c1a-f86e-11da-bd1a-00112444be1e"', ident) == uuid.UUID(
        "a8098c1a-f86e-11da-bd1a-00112444be1e"
    )


def test_the_designed_ambiguities() -> None:
    assert lax(b'"1234"', int | bytes) == b"\xd7m\xf8"
    assert lax(b'"1"', int | bytes) == 1
    thirty_two = b'"' + b"1" * 32 + b'"'
    assert lax(thirty_two, int | uuid.UUID) == uuid.UUID("1" * 32)
    assert lax(b'"123"', int | uuid.UUID) == 123
    assert lax(b'"20240101"', datetime.date | int) == 20240101
    assert lax(b'"20240101"', datetime.datetime | int) == 20240101
    assert lax(b'"2024-01-01"', datetime.date | int) == datetime.date(2024, 1, 1)
