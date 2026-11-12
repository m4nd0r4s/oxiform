"""Deep nesting is a limit error and never a crash, whatever the type (section 7 of the spec).

Each model recurses through one way of nesting, and is read on a thread with the smallest stack
CPython gets on any platform we support (512 KiB, the macOS default). A crash would end the
test run, so these tests failing means the ceiling of ``Limits.MAX_DEPTH`` is too high.
"""

import threading
from collections.abc import Callable
from dataclasses import dataclass
from typing import Annotated, Any, Optional

import pytest

import oxiform as ox

STACK = 512 * 1024


@dataclass
class ViaOptional:
    child: Optional["ViaOptional"] = None


@dataclass
class ViaUnion:
    child: "ViaUnion | int | None" = None


@dataclass
class ViaUntagged:
    child: "Annotated[ViaUntagged | int, ox.Untagged()] | None" = None


@dataclass
class ViaList:
    kids: list["ViaList"]


@dataclass
class ViaTuple:
    kids: tuple["ViaTuple", ...]


@dataclass
class ViaDict:
    kids: dict[str, "ViaDict"]


def nested(prefix: str, leaf: str, suffix: str, levels: int) -> bytes:
    return (prefix * levels + leaf + suffix * levels).encode()


# kind, JSON that nests `levels` containers (the innermost is the leaf), JSON levels per step
SHAPES: list[tuple[type, Callable[[int], bytes], int]] = [
    (ViaOptional, lambda n: nested('{"child":', "null", "}", n), 1),
    (ViaUnion, lambda n: nested('{"child":', "null", "}", n), 1),
    (ViaUntagged, lambda n: nested('{"child":', "null", "}", n), 1),
    (ViaList, lambda n: nested('{"kids":[', '{"kids":[]}', "]}", n), 2),
    (ViaTuple, lambda n: nested('{"kids":[', '{"kids":[]}', "]}", n), 2),
    (ViaDict, lambda n: nested('{"kids":{"a":', '{"kids":{}}', "}}", n), 2),
]
SHAPE_IDS = [kind.__name__ for kind, _, _ in SHAPES]


def on_small_stack(work: Callable[[], None]) -> None:
    failures: list[BaseException] = []

    def run() -> None:
        try:
            work()
        except BaseException as error:  # noqa: BLE001
            failures.append(error)

    previous = threading.stack_size(STACK)
    try:
        thread = threading.Thread(target=run)
        thread.start()
        thread.join()
    finally:
        threading.stack_size(previous)
    assert failures == []


@pytest.mark.parametrize("mode", ["strict", "lax"])
@pytest.mark.parametrize(("kind", "build", "per_step"), SHAPES, ids=SHAPE_IDS)
def test_a_recursive_model_reads_to_the_ceiling_and_stops_there(
    kind: Any, build: Callable[[int], bytes], per_step: int, mode: str
) -> None:
    limits = ox.Limits.none()
    reachable = ox.Limits.MAX_DEPTH // per_step - 1

    def work() -> None:
        assert ox.json.decode(build(reachable), kind, mode=mode, limits=limits) is not None  # type: ignore[arg-type]
        for steps in (ox.Limits.MAX_DEPTH // per_step + 1, 1_000, 100_000):
            with pytest.raises(ox.DecodeError) as caught:
                ox.json.decode(build(steps), kind, mode=mode, limits=limits)  # type: ignore[arg-type]
            error = caught.value.errors()[0]
            assert error["code"] == "limit_exceeded"
            assert error["ctx"]["limit"] == "max_depth"

    on_small_stack(work)


@pytest.mark.parametrize("kind", [Any, list[Any], dict[str, Any]])
def test_untyped_data_stops_at_the_ceiling_on_a_small_stack(kind: Any) -> None:
    def work() -> None:
        for levels in (ox.Limits.MAX_DEPTH + 1, 100_000):
            with pytest.raises(ox.DecodeError):
                ox.json.decode(b"[" * levels, kind, limits=ox.Limits.none())
        exactly = b"[" * ox.Limits.MAX_DEPTH + b"]" * ox.Limits.MAX_DEPTH
        assert ox.json.decode(exactly, Any, limits=ox.Limits.none()) is not None

    on_small_stack(work)
