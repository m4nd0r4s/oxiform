import enum
import ipaddress
import json
import pathlib
import uuid
import zoneinfo
from decimal import Decimal
from typing import Annotated, Any

import pytest

import oxiform as ox


class Permission(enum.Flag):
    READ = 1
    WRITE = 2


class Mode(enum.IntFlag):
    X = 1
    W = 2


@ox.model
class Payment:
    amount: Decimal
    id: uuid.UUID
    signature: bytes = b""
    buffer: bytearray = ox.field(default_factory=bytearray)


def decode(raw: str, target: Any) -> Any:
    return ox.json.decode(raw, target)


def errors(raw: str, target: Any) -> list[tuple[str, str, str]]:
    with pytest.raises(ox.ValidationError) as raised:
        decode(raw, target)
    return [(e["code"], e["pointer"], e["message"]) for e in raised.value.errors()]


def round_trip(value: object, target: Any) -> object:
    return decode(ox.json.encode(value).decode(), target)


def test_reads_and_writes_a_model_with_converted_fields() -> None:
    raw = '{"amount": "10.50", "id": "12345678-1234-5678-1234-567812345678", "signature": "aGk="}'
    payment = decode(raw, Payment)
    assert payment == Payment(
        Decimal("10.50"), uuid.UUID("12345678-1234-5678-1234-567812345678"), b"hi"
    )
    assert json.loads(ox.json.encode(payment)) == {
        "amount": "10.50",
        "id": "12345678-1234-5678-1234-567812345678",
        "signature": "aGk=",
        "buffer": "",
    }
    assert round_trip(payment, Payment) == payment


def test_reads_decimals_from_numbers_without_losing_precision() -> None:
    assert decode("0.1000000000000000000001", Decimal) == Decimal("0.1000000000000000000001")
    assert decode("12", Decimal) == Decimal(12)
    assert decode('"-Infinity"', Decimal) == Decimal("-Infinity")
    assert errors('"1_000"', Decimal) == [("format", "", "invalid Decimal")]
    assert errors("true", Decimal) == [("type", "", "expected Decimal, received bool")]


@pytest.mark.parametrize(
    ("target", "value"),
    [
        (pathlib.Path, pathlib.Path("/tmp/a b")),
        (pathlib.PurePosixPath, pathlib.PurePosixPath("etc/hosts")),
        (ipaddress.IPv4Address, ipaddress.IPv4Address("10.0.0.1")),
        (ipaddress.IPv6Address, ipaddress.IPv6Address("::1")),
        (ipaddress.IPv4Network, ipaddress.IPv4Network("10.0.0.0/8")),
        (ipaddress.IPv6Interface, ipaddress.IPv6Interface("fe80::1/64")),
        (zoneinfo.ZoneInfo, zoneinfo.ZoneInfo("Europe/Lisbon")),
        (bytearray, bytearray(b"\x00\xff")),
    ],
)
def test_round_trips_each_type(target: Any, value: object) -> None:
    result = round_trip(value, target)
    assert result == value
    assert type(result) is type(value)


@pytest.mark.parametrize(
    ("target", "raw", "message"),
    [
        (uuid.UUID, '"nope"', "invalid UUID"),
        (ipaddress.IPv4Address, '"300.0.0.1"', "invalid IPv4Address"),
        (ipaddress.IPv4Network, '"10.0.0.1/8"', "invalid IPv4Network"),
        (zoneinfo.ZoneInfo, '"Mars/Olympus"', "invalid ZoneInfo"),
        (bytes, '"a"', "invalid base64"),
    ],
)
def test_reports_values_that_do_not_convert(target: Any, raw: str, message: str) -> None:
    assert errors(raw, target) == [("format", "", message)]


def test_reads_flags_from_integers() -> None:
    assert decode("3", Permission) == Permission.READ | Permission.WRITE
    assert decode("0", Permission) == Permission(0)
    assert decode("8", Mode) == Mode(8)
    assert errors("4", Permission) == [("enum", "", "4 is not a valid Permission")]
    assert errors('"READ"', Permission) == [("type", "", "expected Permission, received str")]
    assert json.loads(ox.json.encode([Permission.READ | Permission.WRITE, Mode.W])) == [3, 2]


def test_reports_the_format_and_the_path() -> None:
    with pytest.raises(ox.ValidationError) as raised:
        decode('{"amount": "x", "id": "y"}', Payment)
    assert [(e["pointer"], e["ctx"]) for e in raised.value.errors()] == [
        ("/amount", {"format": "decimal"}),
        ("/id", {"format": "uuid"}),
    ]


def test_checks_the_length_of_bytes() -> None:
    assert errors('"aGk="', Annotated[bytes, ox.Len(max=1)]) == [
        ("too_long", "", "length 2 is more than the maximum 1")
    ]


def test_tells_conversions_apart_in_a_union() -> None:
    assert decode("1.5", Decimal | None) == Decimal("1.5")
    assert decode('"::1"', ipaddress.IPv6Address | int) == ipaddress.IPv6Address("::1")
    with pytest.raises(ox.SchemaError) as raised:
        decode("1", Decimal | int)
    assert str(raised.value).startswith("root: Decimal and int are both read from")


def test_rejects_constraints_on_conversions() -> None:
    with pytest.raises(ox.SchemaError) as raised:
        decode('"1"', Annotated[Decimal, ox.Ge(0)])
    assert str(raised.value) == "root: constraints do not apply to Decimal"


def test_refuses_to_write_a_zone_without_a_key() -> None:
    with open("/usr/share/zoneinfo/UTC", "rb") as file:
        zone = zoneinfo.ZoneInfo.from_file(file)
    with pytest.raises(ox.EncodeError, match="ZoneInfo without a key"):
        ox.json.encode(zone)


def test_sets_of_bytes_are_allowed_but_not_of_bytearray() -> None:
    assert decode('["aGk="]', frozenset[bytes]) == frozenset({b"hi"})
    with pytest.raises(ox.SchemaError):
        decode("[]", set[bytearray])  # type: ignore[type-var]
