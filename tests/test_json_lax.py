import enum
from dataclasses import dataclass
from decimal import Decimal
from typing import Annotated, Literal
from uuid import UUID

import pytest

import oxiform as ox


class Level(enum.IntEnum):
    LOW = 1
    HIGH = 2


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
