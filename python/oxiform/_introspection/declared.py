"""What encoding as a declared type reads: the shape of a type and the fields written for it.

``encode(obj, type=T)`` walks ``T``, not the class of ``obj``. This module describes ``T`` for
the Rust walker, in tuples whose first item is the kind:

- ``("any",)``: written by the class of the value
- ``("leaf", name, classes, excluded, lenient)``: a scalar, or any class that has no fields to
  filter. ``classes`` are what an ``isinstance`` check in a union looks for, ``excluded`` the
  classes that do not count (``bool`` for ``int``) and ``lenient`` those accepted as a last
  resort (``int`` for ``float``)
- ``("literal", name, values)``
- ``("optional", inner)``
- ``("list", items)``, ``("dict", values)``
- ``("collection", kind, items)``: ``"tuple"``, ``"set"`` or ``"frozenset"``
- ``("tuple", items)``: one type per position
- ``("struct", index, name)``: an index into ``DeclaredDescription.structs``
- ``("union", name, variants)``: the variant is chosen by the class of the value
- ``("tagged", name, mode, tag, content, variants)``: each variant is ``(type, tag)``, where
  ``tag`` is what the model writes in its tagged union and ``None`` when it writes none
- ``("untagged", name, variants)``
"""

import dataclasses
import datetime
import enum
import types
import typing
from dataclasses import dataclass
from typing import Annotated, Any, Literal, TypeAlias, cast

from oxiform._introspection.encoding import EncodedField, encoded_fields
from oxiform._introspection.fields import tag_of
from oxiform._introspection.unions import UnionMode, is_union, union_mode, without_none
from oxiform._model_options import model_options
from oxiform.unions import External, Tagged, Untagged

__all__ = ["DeclaredDescription", "DeclaredField", "DeclaredStruct", "declared_description"]

DeclaredType: TypeAlias = tuple[Any, ...]


@dataclass(frozen=True, slots=True)
class DeclaredField:
    """A field of the declared model, written under its wire name."""

    encoded: EncodedField
    value_type: DeclaredType
    """The declared type of the field. For a flattened field, the model that is flattened."""


@dataclass(frozen=True, slots=True)
class DeclaredStruct:
    name: str
    cls: type
    fields: tuple[DeclaredField, ...]


@dataclass(frozen=True, slots=True)
class DeclaredDescription:
    root: DeclaredType
    structs: tuple[DeclaredStruct, ...]


def declared_description(target: object) -> DeclaredDescription:
    """The description of ``target`` for encoding. ``target`` has already been read by
    ``describe``, so it is a declaration oxiform accepts."""
    builder = _Builder()
    root = builder.type_of(target)
    return DeclaredDescription(root, tuple(builder.finished()))


_BARE_COLLECTIONS: dict[object, Literal["tuple", "set", "frozenset"]] = {
    tuple: "tuple",
    set: "set",
    frozenset: "frozenset",
}


class _Builder:
    def __init__(self) -> None:
        self._structs: list[DeclaredStruct | None] = []
        self._indices: dict[type, int] = {}

    def finished(self) -> list[DeclaredStruct]:
        finished = [struct for struct in self._structs if struct is not None]
        assert len(finished) == len(self._structs)
        return finished

    def type_of(self, annotation: object) -> DeclaredType:
        mode: UnionMode | None = None
        while typing.get_origin(annotation) is Annotated:
            annotation, *metadata = typing.get_args(annotation)
            found = union_mode(metadata, "")
            mode = found if found is not None else mode
        if annotation is Any or annotation is object:
            return ("any",)
        if mode is not None and is_union(annotation):
            return self._marked_union(annotation, mode)
        if is_union(annotation):
            return self._union(typing.get_args(annotation))
        if annotation is None or annotation is types.NoneType:
            return ("leaf", "None", (types.NoneType,), (), ())
        if annotation is list:
            return ("list", ("any",))
        if annotation is dict:
            return ("dict", ("any",))
        if annotation in _BARE_COLLECTIONS:
            return ("collection", _BARE_COLLECTIONS[annotation], ("any",))
        origin = typing.get_origin(annotation)
        arguments = typing.get_args(annotation)
        if origin is list and arguments:
            return ("list", self.type_of(arguments[0]))
        if origin is dict and len(arguments) == 2:
            return ("dict", self.type_of(arguments[1]))
        if origin is tuple:
            return self._tuple(arguments)
        if origin in (set, frozenset) and arguments:
            kind: Literal["set", "frozenset"] = "set" if origin is set else "frozenset"
            return ("collection", kind, self.type_of(arguments[0]))
        if origin is Literal:
            name = f"Literal[{', '.join(repr(value) for value in arguments)}]"
            return ("literal", name, tuple(arguments))
        if isinstance(annotation, type):
            if dataclasses.is_dataclass(annotation):
                return ("struct", self._struct_index(annotation), annotation.__qualname__)
            return _leaf(cast("type", annotation))
        # Something oxiform cannot read as a type (a field that is neither read nor
        # validated): it has no fields to filter, so only a scalar may stand for it.
        return ("leaf", repr(annotation), (), (), ())

    def _union(self, arguments: tuple[object, ...]) -> DeclaredType:
        present = without_none(arguments)
        if len(present) == 1 and len(arguments) == 2:
            return ("optional", self.type_of(present[0]))
        variants = tuple(self.type_of(argument) for argument in arguments)
        return ("union", _union_name(variants), variants)

    def _marked_union(self, annotation: object, mode: UnionMode) -> DeclaredType:
        arguments = typing.get_args(annotation)
        present = without_none(arguments)
        variants = tuple(self.type_of(argument) for argument in present)
        described: DeclaredType
        if isinstance(mode, Untagged):
            described = ("untagged", _union_name(variants), variants)
        else:
            tagged = tuple(
                (variant, _variant_tag(argument, mode))
                for variant, argument in zip(variants, present, strict=True)
            )
            name = _union_name(variants)
            if isinstance(mode, External):
                described = ("tagged", name, "external", None, None, tagged)
            elif mode.content is not None:
                described = ("tagged", name, "adjacent", mode.field, mode.content, tagged)
            else:
                described = ("tagged", name, "internal", mode.field, None, tagged)
        return ("optional", described) if len(present) < len(arguments) else described

    def _tuple(self, arguments: tuple[object, ...]) -> DeclaredType:
        if len(arguments) == 2 and arguments[1] is Ellipsis:
            return ("collection", "tuple", self.type_of(arguments[0]))
        return ("tuple", tuple(self.type_of(item) for item in arguments))

    def _struct_index(self, cls: type) -> int:
        if cls in self._indices:
            return self._indices[cls]
        index = len(self._structs)
        self._indices[cls] = index
        self._structs.append(None)
        self._structs[index] = self._struct(cls)
        return index

    def _struct(self, cls: type) -> DeclaredStruct:
        hints = typing.get_type_hints(cls, include_extras=True)
        fields: list[DeclaredField] = []
        for encoded in encoded_fields(cls):
            if encoded.constant is not None:
                fields.append(DeclaredField(encoded, ("any",)))
                continue
            annotation = hints.get(encoded.name, Any)
            if encoded.flatten:
                inner = _unannotated(annotation)
                value_type: DeclaredType = (
                    ("struct", self._struct_index(inner), inner.__qualname__)
                    if isinstance(inner, type) and dataclasses.is_dataclass(inner)
                    else ("any",)
                )
            else:
                value_type = self.type_of(annotation)
            fields.append(DeclaredField(encoded, value_type))
        return DeclaredStruct(cls.__qualname__, cls, tuple(fields))


def _variant_tag(variant: object, mode: Tagged | External) -> str | None:
    """What a variant writes as its tag. A model that declares the tag field of an internal tag
    writes it itself, so it is given none here."""
    if not isinstance(variant, type) or not dataclasses.is_dataclass(variant):
        return None
    if isinstance(mode, Tagged) and mode.content is None:
        if model_options(variant).tag_field == mode.field:
            return None
    return tag_of(variant)


def _unannotated(annotation: object) -> object:
    while typing.get_origin(annotation) is Annotated:
        annotation = typing.get_args(annotation)[0]
    return annotation


def _leaf(cls: type) -> DeclaredType:
    """A class without fields to filter, as written by its runtime class."""
    if issubclass(cls, bool):
        return ("leaf", cls.__name__, (cls,), (), ())
    if issubclass(cls, int) and not issubclass(cls, enum.Enum):
        return ("leaf", cls.__name__, (cls,), (bool,), ())
    if issubclass(cls, float):
        return ("leaf", cls.__name__, (cls,), (), (int,))
    if cls is bytes or cls is bytearray:
        return ("leaf", cls.__name__, (bytes, bytearray), (), ())
    if cls is datetime.date:
        return ("leaf", "date", (cls,), (datetime.datetime,), ())
    return ("leaf", cls.__qualname__, (cls,), (), ())


def _union_name(variants: tuple[DeclaredType, ...]) -> str:
    return " | ".join(_name(variant) for variant in variants)


def _name(variant: DeclaredType) -> str:
    match variant:
        case ("leaf" | "literal", name, *_):
            return str(name)
        case ("struct", _, name):
            return str(name)
        case ("list", item):
            return f"list[{_name(item)}]"
        case ("dict", item):
            return f"dict[str, {_name(item)}]"
        case ("collection", kind, item):
            return f"{kind}[{_name(item)}]"
        case ("tuple", items):
            return f"tuple[{', '.join(_name(item) for item in items)}]"
        case ("optional", inner):
            return f"{_name(inner)} | None"
        case ("union" | "tagged" | "untagged", name, *_):
            return str(name)
        case _:
            return "Any"
