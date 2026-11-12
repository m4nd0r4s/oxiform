import enum
from dataclasses import dataclass
from decimal import Decimal
from typing import Annotated, Any, Literal
from uuid import UUID

import pytest

import oxiform as ox


class Level(enum.IntEnum):
    LOW = 1
    HIGH = 2


class Perm(enum.IntFlag):
    READ = 1
    WRITE = 2


@dataclass(slots=True)
class Settings:
    port: Annotated[int, ox.Ge(1), ox.Le(65535)]
    debug: bool
    ratio: float
    timeout: int | None
    price: Decimal
    level: Level
    mode: Literal[1, 2]
    name: str | None = None


ENV_LIKE = (
    b'{"port": "8080", "debug": "yes", "ratio": "0.5", "timeout": "", '
    b'"price": "9.90", "level": "2", "mode": "1", "name": ""}'
)


def test_lax_reads_text_as_the_declared_types() -> None:
    settings = ox.json.decode(ENV_LIKE, Settings, mode="lax")
    assert settings == Settings(
        port=8080,
        debug=True,
        ratio=0.5,
        timeout=None,
        price=Decimal("9.90"),
        level=Level.HIGH,
        mode=1,
        name="",
    )
    assert type(settings.port) is int
    assert type(settings.level) is Level


def test_strict_is_the_default_and_rejects_text() -> None:
    with pytest.raises(ox.ValidationError) as strict:
        ox.json.decode(ENV_LIKE, Settings)
    fields = {error["pointer"] for error in strict.value.errors()}
    assert fields == {"/port", "/debug", "/ratio", "/timeout", "/level", "/mode"}
    with pytest.raises(ox.ValidationError):
        ox.json.decode(ENV_LIKE, Settings, mode="strict")


def test_lax_still_validates_constraints_and_reports_every_error() -> None:
    raw = (
        b'{"port": "70000", "debug": "maybe", "ratio": "x", "timeout": "5", "price": "1", '
        b'"level": "3", "mode": "2"}'
    )
    with pytest.raises(ox.ValidationError) as caught:
        ox.json.decode(raw, Settings, mode="lax")
    summary = [(error["pointer"], error["code"]) for error in caught.value.errors()]
    assert summary == [
        ("/port", "too_large"),
        ("/debug", "type"),
        ("/ratio", "type"),
        ("/level", "enum"),
    ]
    assert caught.value.errors()[0].get("input") == "70000"


def test_lax_never_loses_information() -> None:
    with pytest.raises(ox.ValidationError):
        ox.json.decode(b'"4.5"', int, mode="lax")
    with pytest.raises(ox.ValidationError):
        ox.json.decode(b"true", int, mode="lax")
    assert ox.json.decode(b'"42.0"', int, mode="lax") == 42
    assert ox.json.decode(b"42.0", int, mode="lax") == 42
    assert ox.json.decode(b'"123456789012345678901234567890"', int, mode="lax") == (
        123456789012345678901234567890
    )
    assert ox.json.decode(b'"+123456789012345678901234567890"', int, mode="lax") == (
        123456789012345678901234567890
    )
    assert ox.json.decode(b'"-0007"', int, mode="lax") == -7


def test_lax_reads_other_converted_types_from_text() -> None:
    uuid = ox.json.decode(b'"a8098c1a-f86e-11da-bd1a-00112444be1e"', UUID, mode="lax")
    assert uuid == UUID("a8098c1a-f86e-11da-bd1a-00112444be1e")
    assert ox.json.decode(b'"1.50"', Decimal, mode="lax") == Decimal("1.50")
    assert ox.json.decode(b'"2"', Level, mode="lax") is Level.HIGH


def test_lax_works_in_collections_and_unions() -> None:
    assert ox.json.decode(b'["1", 2, "3"]', list[int], mode="lax") == [1, 2, 3]
    assert ox.json.decode(b'{"a": "1"}', dict[str, int], mode="lax") == {"a": 1}
    assert ox.json.decode(b'"7"', int | float, mode="lax") == 7
    assert ox.json.decode(b'"7.5"', int | float, mode="lax") == 7.5
    assert ox.json.decode(b'"7"', int | str, mode="lax") == "7"


def test_decoder_takes_the_same_options() -> None:
    decoder = ox.json.Decoder(list[int], mode="lax", limits=ox.Limits(max_items=2))
    assert decoder.decode(b'["1", "2"]') == [1, 2]
    with pytest.raises(ox.DecodeError):
        decoder.decode(b"[1, 2, 3]")


def test_an_unknown_mode_says_which_modes_exist() -> None:
    with pytest.raises(ValueError, match='mode must be "strict" or "lax"'):
        ox.json.decode(b"1", int, mode="loose")  # type: ignore[arg-type]
    with pytest.raises(ValueError, match='mode must be "strict" or "lax"'):
        ox.json.Decoder(int, mode="")  # type: ignore[arg-type]


def errors_of(raw: bytes, kind: Any) -> list[dict[str, Any]]:
    with pytest.raises(ox.ValidationError) as caught:
        ox.json.decode(raw, kind, mode="lax")
    return [dict(error) for error in caught.value.errors()]


def test_an_error_after_coercion_shows_the_text_that_was_received() -> None:
    for raw, shown in [(b'"42.0"', "42.0"), (b'"+0042"', "+0042"), (b"42.0", "42.0")]:
        (error,) = errors_of(raw, Annotated[int, ox.Ge(100)])
        assert error["code"] == "too_small"
        assert error["input"] == shown
    (error,) = errors_of(b'"1e1"', Annotated[float, ox.Ge(100)])
    assert error["input"] == "1e1"
    (error,) = errors_of(b'"5"', Annotated[int, ox.OneOf(1, 2)])
    assert error["input"] == "5"
    (error,) = errors_of(b'"3.0"', Level)
    assert error["input"] == "3.0"


def test_a_long_received_text_is_cut_in_the_error() -> None:
    (error,) = errors_of(b'"' + b"9" * 500 + b'.0"', Annotated[int, ox.Le(10)])
    assert error["input"] == "9" * 80


@pytest.mark.parametrize(
    ("raw", "kind", "expected"),
    [
        (b'"abc"', int, 'expected int, received str (lax mode reads whole numbers, as 42, "42"'),
        (b'"4.5"', int, "not 4.5 or 1e3)"),
        (b"4.5", int, "expected int, received float (lax mode reads whole numbers"),
        (
            b'"abc"',
            float,
            'expected float, received str (lax mode reads numbers as text, such as "1.5"',
        ),
        (b'"inf"', float, 'not "inf" or "nan")'),
        (b'"maybe"', bool, 'expected bool, received str (lax mode reads the text "true", "false"'),
        (b"2", bool, "expected bool, received int (lax mode reads the text"),
        (b'"abc"', Perm, "expected Perm, received str (lax mode reads whole numbers"),
    ],
)
def test_a_lax_mismatch_says_what_text_is_accepted(raw: bytes, kind: Any, expected: str) -> None:
    (error,) = errors_of(raw, kind)
    assert error["code"] == "type"
    assert expected in error["message"]


def test_values_that_text_cannot_stand_for_get_no_hint() -> None:
    for raw, kind in [(b"[1]", int), (b"null", int), (b"{}", bool), (b"[]", float)]:
        (error,) = errors_of(raw, kind)
        assert "lax mode" not in error["message"]
    with pytest.raises(ox.ValidationError) as strict:
        ox.json.decode(b'"abc"', int)
    assert "lax mode" not in strict.value.errors()[0]["message"]


def test_a_literal_of_bools_reads_the_words_for_a_bool() -> None:
    assert ox.json.decode(b'"true"', Literal[True], mode="lax") is True
    assert ox.json.decode(b'"YES"', Literal[True], mode="lax") is True
    assert ox.json.decode(b'"off"', Literal[False], mode="lax") is False
    assert ox.json.decode(b'"0"', Literal[True, False], mode="lax") is False
    with pytest.raises(ox.ValidationError):
        ox.json.decode(b'"false"', Literal[True], mode="lax")
    with pytest.raises(ox.ValidationError):
        ox.json.decode(b'"true"', Literal[True])
    # A bool is never a number, nor a number a bool, in a Literal as anywhere.
    with pytest.raises(ox.ValidationError):
        ox.json.decode(b"1", Literal[True], mode="lax")


def test_an_exact_member_wins_and_then_the_integer_before_the_bool() -> None:
    assert ox.json.decode(b'"1"', Literal[1, "1"], mode="lax") == "1"
    assert ox.json.decode(b'"1"', Literal["1", 1], mode="lax") == "1"
    assert ox.json.decode(b"1", Literal[1, "1"], mode="lax") == 1
    assert ox.json.decode(b'"2"', Literal[1, "1", 2], mode="lax") == 2
    assert ox.json.decode(b'"yes"', Literal["yes", True], mode="lax") == "yes"
    assert ox.json.decode(b'"on"', Literal["yes", True], mode="lax") is True
    assert ox.json.decode(b'"1"', Literal[1, True], mode="lax") == 1
    assert type(ox.json.decode(b'"1"', Literal[1, True], mode="lax")) is int
    assert ox.json.decode(b'"1"', Literal[True, 2], mode="lax") is True


def test_flags_are_read_from_text_like_other_integer_types() -> None:
    assert ox.json.decode(b'"3"', Perm, mode="lax") == Perm.READ | Perm.WRITE
    assert ox.json.decode(b'"2.0"', Perm, mode="lax") is Perm.WRITE
    with pytest.raises(ox.ValidationError):
        ox.json.decode(b'"3"', Perm)
