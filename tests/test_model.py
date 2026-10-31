import copy
import dataclasses
import pickle
import sys
from typing import Annotated

import pytest

import oxiform as ox


class Point(ox.Model, frozen=True):
    x: int
    y: int = 0


class Labeled(Point, frozen=True):
    label: Annotated[str, ox.Len(1, 10)] = "origin"


class Box(ox.Model):
    corners: list[Point]
    name: str | None = None


@dataclasses.dataclass(slots=True, frozen=True)
class SlottedPoint:
    x: int
    y: int = 0


def test_subclasses_behave_like_dataclasses() -> None:
    point = Point(1, 2)
    assert point == Point(x=1, y=2)
    assert repr(point) == "Point(x=1, y=2)"
    assert Point.__match_args__ == ("x", "y")
    assert dataclasses.is_dataclass(point)
    with pytest.raises(dataclasses.FrozenInstanceError):
        point.x = 3  # type: ignore[misc]  # pyright: ignore[reportAttributeAccessIssue]


def test_instances_have_no_dict_and_the_size_of_a_slotted_dataclass() -> None:
    point = Point(1, 2)
    assert not hasattr(point, "__dict__")
    assert sys.getsizeof(point) <= sys.getsizeof(SlottedPoint(1, 2))


def test_inherits_fields() -> None:
    labeled = Labeled(1, 2, "a")
    assert (labeled.x, labeled.y, labeled.label) == (1, 2, "a")
    assert isinstance(labeled, Point)
    assert not hasattr(labeled, "__dict__")


def test_pickles_and_copies() -> None:
    box = Box([Point(1, 2)], "b")
    assert pickle.loads(pickle.dumps(box)) == box
    assert copy.deepcopy(box) == box


def test_decodes_from_json() -> None:
    box = ox.json.decode(b'{"corners": [{"x": 1}, {"x": 2, "y": 3}]}', Box)
    assert box == Box([Point(1), Point(2, 3)])
    labeled = ox.json.decode(b'{"x": 1, "label": "home"}', Labeled)
    assert labeled == Labeled(1, 0, "home")
    with pytest.raises(ox.ValidationError) as raised:
        ox.json.decode(b'{"x": 1, "label": ""}', Labeled)
    assert raised.value.errors()[0]["code"] == "too_short"


def test_accepts_the_dataclass_options() -> None:
    class Ordered(ox.Model, order=True, kw_only=True):
        rank: int

    assert Ordered(rank=1) < Ordered(rank=2)
    with pytest.raises(TypeError):
        Ordered(1)  # type: ignore[misc]  # pyright: ignore[reportCallIssue]
