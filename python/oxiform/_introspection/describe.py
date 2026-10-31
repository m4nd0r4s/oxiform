"""Builds the description of a type by walking its annotations."""

import dataclasses
import types
import typing
from typing import Annotated, Any

from oxiform._introspection.constraints import collect_constraints
from oxiform._introspection.fields import checked_options, default_of, tag_of, wire_name
from oxiform._introspection.model import (
    ConstraintValues,
    Description,
    FieldDescription,
    Layout,
    Member,
    StructDescription,
    TypeDescription,
)
from oxiform._introspection.unions import UnionMode, is_union, union_mode, without_none
from oxiform._model_options import model_options
from oxiform.errors import SchemaError
from oxiform.unions import External, Tagged, Untagged

__all__ = ["UnresolvedAnnotationError", "describe"]


class UnresolvedAnnotationError(SchemaError):
    """An annotation names something not defined yet. Models compile on first use instead."""


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
        annotation, constraints, mode = _unwrap_annotated(annotation, location, outer)
        if mode is not None:
            return self._marked_union(annotation, mode, location, constraints)
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
        if is_union(annotation):
            return self._union(arguments, location, constraints)
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

    def _union(
        self, arguments: tuple[object, ...], location: str, constraints: ConstraintValues
    ) -> TypeDescription:
        present = without_none(arguments)
        if len(present) == 1 and len(arguments) == 2:
            return ("optional", self.type_of(present[0], location, constraints))
        if constraints:
            raise SchemaError(f"{location}: constraints do not apply to a union")
        return ("union", tuple(self.type_of(argument, location, {}) for argument in arguments))

    def _marked_union(
        self,
        annotation: object,
        mode: UnionMode,
        location: str,
        constraints: ConstraintValues,
    ) -> TypeDescription:
        """A union with ``ox.Tagged``, ``ox.External`` or ``ox.Untagged``. ``None`` among the
        variants makes the whole union optional."""
        name = type(mode).__name__
        if not is_union(annotation):
            raise SchemaError(f"{location}: ox.{name} applies to a union, not {annotation!r}")
        if constraints:
            raise SchemaError(f"{location}: constraints do not apply to a union")
        arguments = typing.get_args(annotation)
        present = without_none(arguments)
        variants = tuple(self.type_of(argument, location, {}) for argument in present)
        described: TypeDescription
        match mode:
            case Tagged(field, None):
                described = ("tagged", "internal", field, None, variants)
            case Tagged(field, content):
                described = ("tagged", "adjacent", field, content, variants)
            case External():
                described = ("tagged", "external", None, None, variants)
            case Untagged():
                described = ("untagged", variants)
        return ("optional", described) if len(present) < len(arguments) else described

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
        fields: list[FieldDescription] = []
        layout = self._layout(cls, fields, ())
        return StructDescription(
            name=cls.__qualname__,
            cls=cls,
            fields=tuple(fields),
            layout=layout,
            unknown_fields=model_options(cls).unknown,
            tag=tag_of(cls),
            tag_field=model_options(cls).tag_field,
        )

    def _layout(
        self, cls: type, fields: list[FieldDescription], flattening: tuple[type, ...]
    ) -> Layout:
        """The layout of ``cls``, adding the fields it reads to ``fields``. ``flattening`` holds
        the classes whose fields are being read at this level, to catch cycles."""
        hints = _type_hints(cls)
        model = model_options(cls)
        members: list[Member] = []
        for field in dataclasses.fields(cls):
            location = f"{cls.__qualname__}.{field.name}"
            options = checked_options(field, location)
            default = default_of(field)
            if not field.init or options.skip_decode:
                if default is not None:
                    members.append(("default", field.name, default))
                continue
            annotation = hints[field.name]
            if options.flatten:
                inner = _flattened_class(annotation, location)
                if inner is cls or inner in flattening:
                    raise SchemaError(f"{location}: a model cannot flatten itself")
                inner_layout = self._layout(inner, fields, (*flattening, cls))
                members.append(("flatten", field.name, inner_layout))
                continue
            outer = collect_constraints(options.constraints, location)
            fields.append(
                FieldDescription(
                    name=field.name,
                    wire_name=wire_name(field.name, options, model),
                    aliases=options.aliases,
                    value_type=self.type_of(annotation, location, outer),
                    default=default,
                )
            )
            members.append(("field", field.name, len(fields) - 1))
        return Layout(cls, tuple(members), hasattr(cls, "__post_init__"))


def _type_hints(cls: type) -> dict[str, object]:
    try:
        return typing.get_type_hints(cls, include_extras=True)
    except NameError as error:
        message = f"{cls.__qualname__}: cannot resolve an annotation: {error}"
        raise UnresolvedAnnotationError(message) from error


def _flattened_class(annotation: object, location: str) -> type:
    if typing.get_origin(annotation) is Annotated:
        raise SchemaError(f"{location}: a flattened field cannot be constrained")
    if not isinstance(annotation, type) or not dataclasses.is_dataclass(annotation):
        raise SchemaError(f"{location}: only a model can be flattened, not {annotation!r}")
    return annotation


def _unwrap_annotated(
    annotation: object, location: str, outer: ConstraintValues
) -> tuple[object, ConstraintValues, UnionMode | None]:
    """The annotated type, its constraints merged with those of enclosing ``Annotated``, and how
    it is told apart when it is a union."""
    constraints = dict(outer)
    mode: UnionMode | None = None
    while typing.get_origin(annotation) is Annotated:
        annotation, *metadata = typing.get_args(annotation)
        found = union_mode(metadata, location)
        if found is not None:
            if mode is not None:
                raise SchemaError(f"{location}: a union is told apart in one way only")
            mode = found
        for key, value in collect_constraints(metadata, location).items():
            if key in constraints:
                raise SchemaError(f"{location}: the same constraint is declared twice")
            constraints[key] = value
    return annotation, constraints, mode
