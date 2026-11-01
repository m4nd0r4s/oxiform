import json
import typing
import zoneinfo
from datetime import UTC, date, datetime, time, timedelta, timezone
from typing import Annotated, Any

import pytest

import oxiform as ox

LISBON = zoneinfo.ZoneInfo("Europe/Lisbon")


@ox.model
class Booking:
    starts_at: datetime
    day: date
    opens: time
    length: timedelta
    local: ox.NaiveDateTime | None = None


def decode(raw: str | bytes, target: Any) -> Any:
    return ox.json.decode(raw, target)


def encoded(value: object) -> object:
    return json.loads(ox.json.encode(value))


def errors(raw: str, target: Any) -> list[tuple[str, str]]:
    with pytest.raises(ox.ValidationError) as raised:
        decode(raw, target)
    return [(e["code"], e["message"]) for e in raised.value.errors()]


def test_reads_and_writes_a_model_with_time_fields() -> None:
    raw = (
        '{"starts_at": "2026-10-28T10:00:00+01:00", "day": "2026-10-28",'
        ' "opens": "09:30:00", "length": "PT1H30M", "local": "2026-10-28T10:00:00.25"}'
    )
    booking = decode(raw, Booking)
    assert booking == Booking(
        datetime(2026, 10, 28, 10, tzinfo=timezone(timedelta(hours=1))),
        date(2026, 10, 28),
        time(9, 30),
        timedelta(hours=1, minutes=30),
        datetime(2026, 10, 28, 10, 0, 0, 250_000),
    )
    assert encoded(booking) == {
        "starts_at": "2026-10-28T10:00:00+01:00",
        "day": "2026-10-28",
        "opens": "09:30:00",
        "length": "PT1H30M",
        "local": "2026-10-28T10:00:00.250000",
    }
    assert decode(ox.json.encode(booking), Booking) == booking


def test_reads_utc_as_the_utc_singleton() -> None:
    assert decode('"2026-01-01T00:00:00Z"', datetime).tzinfo is UTC


def test_writes_any_time_zone_by_its_offset() -> None:
    summer = datetime(2026, 7, 1, 12, tzinfo=LISBON)
    assert encoded(summer) == "2026-07-01T12:00:00+01:00"
    assert encoded(datetime(2026, 1, 1, tzinfo=UTC)) == "2026-01-01T00:00:00Z"
    assert encoded(time(8, 0, 0, 5, tzinfo=timezone(timedelta(hours=-3)))) == (
        "08:00:00.000005-03:00"
    )


@pytest.mark.parametrize(
    ("value", "text"),
    [
        (timedelta(), "P0D"),
        (timedelta(days=-1), "-P1D"),
        (timedelta(seconds=-1), "-PT1S"),
        (timedelta(days=2, microseconds=500), "P2DT0.0005S"),
        (timedelta(days=999_999_999, hours=23), "P999999999DT23H"),
    ],
)
def test_round_trips_durations(value: timedelta, text: str) -> None:
    assert encoded(value) == text
    assert decode(json.dumps(text), timedelta) == value


def test_requires_a_timezone_unless_naive() -> None:
    assert errors('"2026-10-28T10:00:00"', datetime) == [
        ("timezone", "a datetime needs a timezone; use ox.NaiveDateTime for one without")
    ]
    assert errors('"2026-10-28T10:00:00Z"', ox.NaiveDateTime) == [
        ("timezone", "expected a datetime without a timezone")
    ]


@pytest.mark.parametrize(
    ("target", "raw", "message"),
    [
        (date, '"2026-02-30"', "invalid date"),
        (time, '"25:00:00"', "invalid time"),
        (datetime, '"2026-10-28"', "invalid datetime"),
        (timedelta, '"P1M"', "invalid duration"),
    ],
)
def test_reports_invalid_text_as_format(target: Any, raw: str, message: str) -> None:
    assert errors(raw, target) == [("format", message)]


def test_reads_time_types_from_strings_only() -> None:
    assert errors("1700000000", datetime) == [("type", "expected datetime, received int")]


def test_tells_time_types_apart_from_other_kinds_in_a_union() -> None:
    assert decode("3", date | int) == 3
    with pytest.raises(ox.SchemaError):
        decode('"2026-01-01"', date | str)


def test_rejects_misplaced_markers_and_constraints() -> None:
    with pytest.raises(ox.SchemaError, match="ox.NaiveDateTime applies to datetime only"):
        decode("1", Annotated[int, typing.get_args(ox.NaiveDateTime)[1]])
    with pytest.raises(ox.SchemaError, match="constraints do not apply to date"):
        decode('"2026-01-01"', Annotated[date, ox.Ge(0)])
