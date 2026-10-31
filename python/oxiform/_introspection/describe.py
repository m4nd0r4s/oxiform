"""Builds the description of a type by walking its annotations."""

import dataclasses
import types
import typing
from typing import Annotated, Any, Union

from oxiform._introspection.constraints import collect_constraints
from oxiform._introspection.model import (
    ConstraintValues,
    Default,
    Description,
    FieldDescription,
    StructDescription,
    TypeDescription,
)
from oxiform.errors import SchemaError

__all__ = ["describe"]


def describe(target: object) -> Description:
    """The description of ``target`` and of every structure it reaches.

    Raises ``SchemaError`` for types oxiform cannot read yet and for misplaced constraints.
    """
    builder = _DescriptionBuilder()
    root = builder.type_of(target, "root", {})
    return Description(root, tuple(builder.finished_structs()))


class _DescriptionBuilder:
    def __init__(self) -> None:
        self._structs: list[StructDescription | None] = []
        self._indices: dict[type, int] = {}

    def finished_structs(self) -> list[StructDescription]:
        finished = [struct for struct in self._structs if struct is not None]
        assert len(finished) == len(self._structs)
        return finished

    def type_of(
        self, annotation: object, location: str, outer: ConstraintValues
    ) -> TypeDescription:
        annotation, constraints = _unwrap_annotated(annotation, location, outer)
        if annotation is Any:
            return self._without_constraints(("any",), "Any", constraints, location)
        if annotation is bool:
            return self._without_constraints(("bool",), "bool", constraints, location)
        if annotation is None or annotation is types.NoneType:
            return self._without_constraints(("none",), "None", constraints, location)
        if annotation is int:
            return ("int", constraints)
        if annotation is float:
            return ("float", constraints)
        if annotation is str:
            return ("str", constraints)
        if annotation is list:
            return ("list", ("any",), constraints)
        if annotation is dict:
            return ("dict", ("any",), constraints)
        origin = typing.get_origin(annotation)
        arguments = typing.get_args(annotation)
        if origin is Union or origin is types.UnionType:
            return self._optional(arguments, location, constraints)
        if origin is list:
            (items,) = arguments
            return ("list", self.type_of(items, location, {}), constraints)
        if origin is dict:
            keys, values = arguments
            if keys is not str:
                raise SchemaError(f"{location}: dict keys must be str, not {keys!r}")
            return ("dict", self.type_of(values, location, {}), constraints)
        if isinstance(annotation, type) and dataclasses.is_dataclass(annotation):
            index = self._struct_index(annotation)
            return self._without_constraints(
                ("struct", index), annotation.__qualname__, constraints, location
            )
        raise SchemaError(f"{location}: unsupported type {annotation!r}")

    def _optional(
        self, arguments: tuple[object, ...], location: str, constraints: ConstraintValues
    ) -> TypeDescription:
        present = [argument for argument in arguments if argument not in (None, types.NoneType)]
        if len(present) != 1 or len(present) == len(arguments):
            raise SchemaError(f"{location}: unions other than T | None are not supported yet")
        return ("optional", self.type_of(present[0], location, constraints))

    def _without_constraints(
        self,
        description: TypeDescription,
        type_name: str,
        constraints: ConstraintValues,
        location: str,
    ) -> TypeDescription:
        if constraints:
            raise SchemaError(f"{location}: constraints do not apply to {type_name}")
        return description

    def _struct_index(self, cls: type) -> int:
        if cls in self._indices:
            return self._indices[cls]
        index = len(self._structs)
        self._indices[cls] = index
        self._structs.append(None)
        self._structs[index] = self._struct(cls)
        return index

    def _struct(self, cls: type) -> StructDescription:
        name = cls.__qualname__
        try:
            hints = typing.get_type_hints(cls, include_extras=True)
        except NameError as error:
            raise SchemaError(f"{name}: cannot resolve an annotation: {error}") from error
        fields = tuple(
            FieldDescription(
                name=field.name,
                wire_name=field.name,
                value_type=self.type_of(hints[field.name], f"{name}.{field.name}", {}),
                default=_default_of(field),
            )
            for field in dataclasses.fields(cls)
            if field.init
        )
        return StructDescription(
            name=name,
            cls=cls,
            fields=fields,
            unknown_fields="ignore",
            has_post_init=hasattr(cls, "__post_init__"),
        )


def _unwrap_annotated(
    annotation: object, location: str, outer: ConstraintValues
) -> tuple[object, ConstraintValues]:
    """The annotated type and its constraints, merged with those of enclosing ``Annotated``."""
    constraints = dict(outer)
    while typing.get_origin(annotation) is Annotated:
        annotation, *metadata = typing.get_args(annotation)
        for key, value in collect_constraints(metadata, location).items():
            if key in constraints:
                raise SchemaError(f"{location}: the same constraint is declared twice")
            constraints[key] = value
    return annotation, constraints


def _default_of(field: dataclasses.Field[object]) -> Default:
    if field.default is not dataclasses.MISSING:
        return ("value", field.default)
    if field.default_factory is not dataclasses.MISSING:
        return ("factory", field.default_factory)
    return None
