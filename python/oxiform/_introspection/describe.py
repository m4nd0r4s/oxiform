"""Builds the description of a type by walking its annotations."""

import dataclasses
import datetime
import difflib
import enum
import types
import typing
from collections.abc import Callable
from typing import Annotated, Any, Literal, TypeGuard

from oxiform._introspection.choices import enum_choice, literal_choice
from oxiform._introspection.constraints import collect_constraints
from oxiform._introspection.conversions import conversion_of, flag_conversion
from oxiform._introspection.fields import checked_options, default_of, tag_of, wire_name
from oxiform._introspection.model import (
    ChoiceDescription,
    ConstraintValues,
    ConversionDescription,
    Description,
    FieldDescription,
    Layout,
    Member,
    StructDescription,
    TemporalKind,
    TypeDescription,
    ValidatorDescription,
)
from oxiform._introspection.unions import UnionMode, is_union, union_mode, without_none
from oxiform._model_options import model_options
from oxiform._naive import NAIVE
from oxiform._secret import Secret
from oxiform._validators import ValidatorSpec, validators_of
from oxiform.errors import SchemaError
from oxiform.unions import External, Tagged, Untagged

__all__ = [
    "BARE_COLLECTIONS",
    "TEMPORAL_KINDS",
    "UnresolvedAnnotationError",
    "describe",
    "flattened_class",
    "is_enum",
    "is_secret",
    "is_flag",
    "type_hints",
    "unwrap_annotated",
]


class UnresolvedAnnotationError(SchemaError):
    """An annotation names something not defined yet. Models compile on first use instead."""


def describe(target: object) -> Description:
    """The description of ``target`` and of every structure it reaches.

    Raises ``SchemaError`` for types oxiform cannot read yet and for misplaced constraints.
    """
    builder = _DescriptionBuilder()
    root = builder.type_of(target, "root", {})
    return Description(
        root,
        tuple(builder.finished_structs()),
        tuple(builder.choices),
        tuple(builder.conversions),
    )


class _DescriptionBuilder:
    def __init__(self) -> None:
        self._structs: list[StructDescription | None] = []
        self._indices: dict[type, int] = {}
        self.choices: list[ChoiceDescription] = []
        self._choice_indices: dict[object, int] = {}
        self.conversions: list[ConversionDescription] = []
        self._conversion_indices: dict[object, int] = {}

    def finished_structs(self) -> list[StructDescription]:
        finished = [struct for struct in self._structs if struct is not None]
        assert len(finished) == len(self._structs)
        return finished

    def type_of(
        self, annotation: object, location: str, outer: ConstraintValues
    ) -> TypeDescription:
        annotation, constraints, mode, naive = unwrap_annotated(annotation, location, outer)
        if naive:
            if annotation is not datetime.datetime:
                raise SchemaError(f"{location}: ox.NaiveDateTime applies to datetime only")
            return self._without_constraints(
                ("temporal", "naive_datetime"), "ox.NaiveDateTime", constraints, location
            )
        if mode is not None:
            return self._marked_union(annotation, mode, location, constraints)
        if annotation is Secret:
            raise SchemaError(f"{location}: ox.Secret needs the type of its value, as Secret[str]")
        if typing.get_origin(annotation) is Secret:
            return self._secret(typing.get_args(annotation), location, constraints)
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
        if annotation is bytes or annotation is bytearray:
            return ("bytes", annotation is bytearray, constraints)
        if annotation in TEMPORAL_KINDS:
            kind = TEMPORAL_KINDS[annotation]
            return self._without_constraints(("temporal", kind), kind, constraints, location)
        conversion = conversion_of(annotation)
        if conversion is None and is_flag(annotation):
            conversion = flag_conversion(annotation)
        if conversion is not None:
            index = self._conversion_index(annotation, conversion)
            return self._without_constraints(
                ("converted", index), conversion.name, constraints, location
            )
        if annotation is list:
            return ("list", ("any",), constraints)
        if annotation in BARE_COLLECTIONS:
            return ("collection", BARE_COLLECTIONS[annotation], ("any",), constraints)
        if is_enum(annotation):
            index = self._choice_index(annotation, lambda: enum_choice(annotation, location))
            return self._without_constraints(
                ("choice", index), annotation.__qualname__, constraints, location
            )
        if annotation is dict:
            return ("dict", ("any",), constraints)
        origin = typing.get_origin(annotation)
        arguments = typing.get_args(annotation)
        if is_union(annotation):
            return self._union(arguments, location, constraints)
        if origin is list:
            (items,) = arguments
            return ("list", self.type_of(items, location, {}), constraints)
        if origin is Literal:
            # Keyed with the types: Literal[1] and Literal[True] hold values that compare equal.
            index = self._choice_index(
                ("literal", _with_types(arguments)), lambda: literal_choice(annotation, location)
            )
            return self._without_constraints(("choice", index), "Literal", constraints, location)
        if origin is tuple:
            return self._tuple(arguments, location, constraints)
        if origin is set or origin is frozenset:
            (items,) = arguments
            items_type = self.type_of(items, location, {})
            if not _hashable(items_type):
                raise SchemaError(f"{location}: set items must be hashable, not {items!r}")
            return ("collection", BARE_COLLECTIONS[origin], items_type, constraints)
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

    def _secret(
        self, arguments: tuple[object, ...], location: str, constraints: ConstraintValues
    ) -> TypeDescription:
        """``Secret[T]`` reads ``T`` (with the constraints declared around it) and hides it."""
        (inner,) = arguments
        described = self.type_of(inner, location, constraints)
        if described[0] == "secret":
            raise SchemaError(
                f"{location}: a Secret cannot hold a Secret: it already hides its value"
            )
        return ("secret", described)

    def _union(
        self, arguments: tuple[object, ...], location: str, constraints: ConstraintValues
    ) -> TypeDescription:
        present = without_none(arguments)
        if len(present) == 1 and len(arguments) == 2:
            return ("optional", self.type_of(present[0], location, constraints))
        _refuse_secret_variants(present, location)
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
        _refuse_secret_variants(present, location)
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

    def _tuple(
        self, arguments: tuple[object, ...], location: str, constraints: ConstraintValues
    ) -> TypeDescription:
        if len(arguments) == 2 and arguments[1] is Ellipsis:
            return ("collection", "tuple", self.type_of(arguments[0], location, {}), constraints)
        if constraints:
            raise SchemaError(f"{location}: constraints do not apply to a fixed tuple")
        return ("tuple", tuple(self.type_of(item, location, {}) for item in arguments))

    def _choice_index(self, key: object, build: Callable[[], ChoiceDescription]) -> int:
        if key not in self._choice_indices:
            self._choice_indices[key] = len(self.choices)
            self.choices.append(build())
        return self._choice_indices[key]

    def _conversion_index(self, key: object, conversion: ConversionDescription) -> int:
        if key not in self._conversion_indices:
            self._conversion_indices[key] = len(self.conversions)
            self.conversions.append(conversion)
        return self._conversion_indices[key]

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
            validators=_model_validators(cls),
        )

    def _layout(
        self, cls: type, fields: list[FieldDescription], flattening: tuple[type, ...]
    ) -> Layout:
        """The layout of ``cls``, adding the fields it reads to ``fields``. ``flattening`` holds
        the classes whose fields are being read at this level, to catch cycles."""
        hints = type_hints(cls)
        model = model_options(cls)
        members: list[Member] = []
        by_field = _field_validators(cls)
        if flattening:
            _refuse_flattened_model_validators(cls, flattening[-1])
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
                inner = flattened_class(annotation, location)
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
                    validators=tuple(by_field.pop(field.name, ())),
                )
            )
            members.append(("field", field.name, len(fields) - 1))
        _check_validated_fields(cls, by_field)
        return Layout(cls, tuple(members), hasattr(cls, "__post_init__"))


def _described(spec: ValidatorSpec, cls: type) -> ValidatorDescription:
    try:
        reading = spec.signature()
    except NameError as error:
        message = f"{spec.name}: cannot resolve an annotation: {error}"
        raise UnresolvedAnnotationError(message) from error
    return ValidatorDescription(
        name=spec.name,
        function=spec.function,
        cls=cls,
        context=reading.context,
        context_required=reading.required,
        context_none_when_absent=reading.none_when_absent,
    )


def _field_validators(cls: type) -> dict[str, list[ValidatorDescription]]:
    """The field validators of ``cls`` by the name of the field, each list in order."""
    by_field: dict[str, list[ValidatorDescription]] = {}
    for spec in validators_of(cls):
        if spec.model:
            continue
        described = _described(spec, cls)
        for name in spec.fields:
            by_field.setdefault(name, []).append(described)
    return by_field


def _model_validators(cls: type) -> tuple[ValidatorDescription, ...]:
    return tuple(_described(spec, cls) for spec in validators_of(cls) if spec.model)


def _check_validated_fields(cls: type, unused: dict[str, list[ValidatorDescription]]) -> None:
    """Refuses a validator that names something that is not a field read from the input."""
    if not unused:
        return
    own = {field.name: field for field in dataclasses.fields(cls)}
    for name, validators in unused.items():
        where = f"{cls.__qualname__}: {validators[0].name} validates {name!r}"
        if name in own:
            raise SchemaError(
                f"{where}, which the input does not give (it is skipped on read, not part of "
                "__init__ or flattened): validate the fields that are read"
            )
        close = difflib.get_close_matches(name, list(own), n=1)
        hint = f" (did you mean {close[0]!r}?)" if close else ""
        raise SchemaError(f"{where}, but {cls.__qualname__} has no such field{hint}")


def _refuse_flattened_model_validators(cls: type, outer: type) -> None:
    for spec in validators_of(cls):
        if spec.model:
            raise SchemaError(
                f"{spec.name} validates the whole of {cls.__qualname__}, which "
                f"{outer.__qualname__} flattens: its instance is built from the fields of the "
                "outer model, so the validator cannot run. "
                f"Move it to {outer.__qualname__}, or do not flatten"
            )


def _refuse_secret_variants(variants: list[object], location: str) -> None:
    """A union is read by the kind of value it gets, so one of several variants cannot hide
    itself: the whole union can (``Secret[str | int]``)."""
    for variant in variants:
        if is_secret(variant):
            raise SchemaError(
                f"{location}: a Secret cannot be one of several variants of a union; "
                "write Secret[A | B] to hide the whole union"
            )


def is_secret(annotation: object) -> bool:
    """Whether ``annotation`` is ``Secret[T]``, with or without ``Annotated`` around it."""
    while typing.get_origin(annotation) is Annotated:
        annotation = typing.get_args(annotation)[0]
    return annotation is Secret or typing.get_origin(annotation) is Secret


def _with_types(values: tuple[object, ...]) -> tuple[tuple[type, object], ...]:
    """``values`` paired with their types, for a cache key where ``1`` and ``True`` differ."""
    return tuple((type(value), value) for value in values)


def is_enum(annotation: object) -> TypeGuard[type[enum.Enum]]:
    return isinstance(annotation, type) and issubclass(annotation, enum.Enum)


def is_flag(annotation: object) -> TypeGuard[type[enum.Flag]]:
    return isinstance(annotation, type) and issubclass(annotation, enum.Flag)


TEMPORAL_KINDS: dict[object, TemporalKind] = {
    datetime.datetime: "datetime",
    datetime.date: "date",
    datetime.time: "time",
    datetime.timedelta: "timedelta",
}

BARE_COLLECTIONS: dict[object, Literal["tuple", "set", "frozenset"]] = {
    tuple: "tuple",
    set: "set",
    frozenset: "frozenset",
}


def _hashable(description: TypeDescription) -> bool:
    """Whether values read as ``description`` can be set items: lists, dicts and sets cannot."""
    match description:
        case ("list" | "dict", *_) | ("bytes", True, _):
            return False
        case ("collection", kind, *_):
            return kind in ("tuple", "frozenset")
        case _:
            return True


def type_hints(cls: type) -> dict[str, object]:
    try:
        return typing.get_type_hints(cls, include_extras=True)
    except NameError as error:
        message = f"{cls.__qualname__}: cannot resolve an annotation: {error}"
        raise UnresolvedAnnotationError(message) from error


def flattened_class(annotation: object, location: str) -> type:
    if typing.get_origin(annotation) is Annotated:
        raise SchemaError(f"{location}: a flattened field cannot be constrained")
    if not isinstance(annotation, type) or not dataclasses.is_dataclass(annotation):
        raise SchemaError(f"{location}: only a model can be flattened, not {annotation!r}")
    return annotation


def unwrap_annotated(
    annotation: object, location: str, outer: ConstraintValues
) -> tuple[object, ConstraintValues, UnionMode | None, bool]:
    """The annotated type, its constraints merged with those of enclosing ``Annotated``, how it
    is told apart when it is a union, and whether it is marked as ``ox.NaiveDateTime``."""
    constraints = dict(outer)
    mode: UnionMode | None = None
    naive = False
    while typing.get_origin(annotation) is Annotated:
        annotation, *metadata = typing.get_args(annotation)
        naive = naive or any(item is NAIVE for item in metadata)
        found = union_mode(metadata, location)
        if found is not None:
            if mode is not None:
                raise SchemaError(f"{location}: a union is told apart in one way only")
            mode = found
        for key, value in collect_constraints(metadata, location).items():
            if key in constraints:
                raise SchemaError(f"{location}: the same constraint is declared twice")
            constraints[key] = value
    return annotation, constraints, mode, naive
