"""The same three models declared for each library, with the same constraints."""

from collections.abc import Callable
from dataclasses import dataclass, field
from typing import Annotated, Any, Literal

import msgspec
import orjson
import pydantic

import oxiform as ox

__all__ = ["DECODERS"]

# --- oxiform: plain dataclasses ---------------------------------------------------------------


@dataclass(slots=True)
class OxSmall:
    id: int
    name: str
    email: str
    active: bool
    score: float


@dataclass(slots=True)
class OxMedium:
    id: Annotated[int, ox.Ge(0)]
    username: Annotated[str, ox.Len(3, 30), ox.Pattern(r"^[a-z_]+$")]
    full_name: Annotated[str, ox.Len(1, 100)]
    email: str
    age: Annotated[int, ox.Ge(0), ox.Le(150)]
    height: Annotated[float, ox.Ge(0)]
    weight: Annotated[float, ox.Ge(0)]
    active: bool
    verified: bool
    role: Annotated[str, ox.OneOf("basic", "admin")]
    country: Annotated[str, ox.Len(2, 2)]
    city: str
    postal_code: Annotated[str, ox.Pattern(r"^\d{4}-\d{3}$")]
    phone: str | None
    tags: Annotated[list[str], ox.Len(max=10)]
    followers: Annotated[int, ox.Ge(0)]
    following: Annotated[int, ox.Ge(0)]
    rating: Annotated[float, ox.Ge(0), ox.Le(5)]
    bio: Annotated[str, ox.Len(max=500)]
    website: str


@dataclass(slots=True)
class OxCustomer:
    id: int
    name: str
    email: str


@dataclass(slots=True)
class OxItem:
    sku: str
    quantity: Annotated[int, ox.Ge(1)]
    price: Annotated[float, ox.Ge(0)]
    tags: list[str]


@dataclass(slots=True)
class OxOrder:
    order_id: int
    customer: OxCustomer
    items: list[OxItem] = field(default_factory=list[OxItem])
    notes: str | None = None


# --- msgspec ----------------------------------------------------------------------------------


class MsSmall(msgspec.Struct):
    id: int
    name: str
    email: str
    active: bool
    score: float


class MsMedium(msgspec.Struct):
    id: Annotated[int, msgspec.Meta(ge=0)]
    username: Annotated[str, msgspec.Meta(min_length=3, max_length=30, pattern=r"^[a-z_]+$")]
    full_name: Annotated[str, msgspec.Meta(min_length=1, max_length=100)]
    email: str
    age: Annotated[int, msgspec.Meta(ge=0, le=150)]
    height: Annotated[float, msgspec.Meta(ge=0)]
    weight: Annotated[float, msgspec.Meta(ge=0)]
    active: bool
    verified: bool
    role: Literal["basic", "admin"]
    country: Annotated[str, msgspec.Meta(min_length=2, max_length=2)]
    city: str
    postal_code: Annotated[str, msgspec.Meta(pattern=r"^\d{4}-\d{3}$")]
    phone: str | None
    tags: Annotated[list[str], msgspec.Meta(max_length=10)]
    followers: Annotated[int, msgspec.Meta(ge=0)]
    following: Annotated[int, msgspec.Meta(ge=0)]
    rating: Annotated[float, msgspec.Meta(ge=0, le=5)]
    bio: Annotated[str, msgspec.Meta(max_length=500)]
    website: str


class MsCustomer(msgspec.Struct):
    id: int
    name: str
    email: str


class MsItem(msgspec.Struct):
    sku: str
    quantity: Annotated[int, msgspec.Meta(ge=1)]
    price: Annotated[float, msgspec.Meta(ge=0)]
    tags: list[str]


class MsOrder(msgspec.Struct):
    order_id: int
    customer: MsCustomer
    items: list[MsItem] = []
    notes: str | None = None


# --- pydantic ---------------------------------------------------------------------------------


class PySmall(pydantic.BaseModel):
    id: int
    name: str
    email: str
    active: bool
    score: float


class PyMedium(pydantic.BaseModel):
    id: Annotated[int, pydantic.Field(ge=0)]
    username: Annotated[str, pydantic.Field(min_length=3, max_length=30, pattern=r"^[a-z_]+$")]
    full_name: Annotated[str, pydantic.Field(min_length=1, max_length=100)]
    email: str
    age: Annotated[int, pydantic.Field(ge=0, le=150)]
    height: Annotated[float, pydantic.Field(ge=0)]
    weight: Annotated[float, pydantic.Field(ge=0)]
    active: bool
    verified: bool
    role: Literal["basic", "admin"]
    country: Annotated[str, pydantic.Field(min_length=2, max_length=2)]
    city: str
    postal_code: Annotated[str, pydantic.Field(pattern=r"^\d{4}-\d{3}$")]
    phone: str | None
    tags: Annotated[list[str], pydantic.Field(max_length=10)]
    followers: Annotated[int, pydantic.Field(ge=0)]
    following: Annotated[int, pydantic.Field(ge=0)]
    rating: Annotated[float, pydantic.Field(ge=0, le=5)]
    bio: Annotated[str, pydantic.Field(max_length=500)]
    website: str


class PyCustomer(pydantic.BaseModel):
    id: int
    name: str
    email: str


class PyItem(pydantic.BaseModel):
    sku: str
    quantity: Annotated[int, pydantic.Field(ge=1)]
    price: Annotated[float, pydantic.Field(ge=0)]
    tags: list[str]


class PyOrder(pydantic.BaseModel):
    order_id: int
    customer: PyCustomer
    items: list[PyItem] = []
    notes: str | None = None


def _msgspec(model: type) -> Callable[[bytes], object]:
    decoder: msgspec.json.Decoder[Any] = msgspec.json.Decoder(model)
    return decoder.decode


def _pydantic(model: type[pydantic.BaseModel]) -> Callable[[bytes], object]:
    return model.model_validate_json


def _oxiform(model: type) -> Callable[[bytes], object]:
    return ox.json.Decoder(model).decode


DECODERS: dict[str, dict[str, Callable[[bytes], object]]] = {
    "small": {
        "oxiform": _oxiform(OxSmall),
        "msgspec": _msgspec(MsSmall),
        "pydantic": _pydantic(PySmall),
    },
    "medium": {
        "oxiform": _oxiform(OxMedium),
        "msgspec": _msgspec(MsMedium),
        "pydantic": _pydantic(PyMedium),
    },
    "nested": {
        "oxiform": _oxiform(OxOrder),
        "msgspec": _msgspec(MsOrder),
        "pydantic": _pydantic(PyOrder),
    },
    "untyped": {
        "oxiform": ox.json.Decoder().decode,
        "msgspec": msgspec.json.Decoder().decode,
        "orjson": orjson.loads,
    },
}
