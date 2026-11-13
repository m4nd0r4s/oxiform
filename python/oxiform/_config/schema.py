"""What a source needs to know about a config class: its sections, the names of its fields and
how a field reads text (spec sections 6.1 and 6.2). Also the checks of a declaration."""

import collections.abc
import dataclasses
import functools
import typing
from dataclasses import dataclass
from typing import Annotated, Literal, cast

from oxiform._fields import options_of
from oxiform._introspection.describe import type_hints
from oxiform._introspection.fields import default_of, wire_name
from oxiform._introspection.unions import is_union, without_none
from oxiform._model_options import model_options
from oxiform._secret import Secret
from oxiform.errors import SchemaError

__all__ = [
    "CONFIG_ATTRIBUTE",
    "UNKNOWN",
    "ConfigOptions",
    "FieldSchema",
    "SectionSchema",
    "check_declaration",
    "config_options",
    "is_config",
    "section_schema",
]

CONFIG_ATTRIBUTE = "__oxiform_config__"
"""The class attribute that marks a config and holds its ``ConfigOptions``."""

UNKNOWN = "\x00"
"""Put in front of a key that no field reads, in the merged tree, so that it can never be
taken for the Python name of a field."""


@dataclass(frozen=True, slots=True)
class ConfigOptions:
    """The options of ``@ox.config`` that sources read."""

    env_prefix: str = ""
    env_nested: str | None = "__"


def config_options(cls: type) -> ConfigOptions | None:
    """The options of ``cls`` when it was made by ``@ox.config`` itself (not inherited)."""
    own = cls.__dict__.get(CONFIG_ATTRIBUTE)
    return own if isinstance(own, ConfigOptions) else None


def is_config(cls: object) -> bool:
    return isinstance(cls, type) and config_options(cls) is not None


@dataclass(frozen=True, slots=True)
class FieldSchema:
    """One field a source can set."""

    name: str
    """The Python name."""
    wire: str
    aliases: tuple[str, ...]
    env: str | None
    """The exact environment variable of ``ox.field(env=...)``."""
    section: type | None
    """The config class when the field is a section (also ``Section | None``)."""
    optional_section: bool
    text: Literal["text", "json", "either"]
    """How env and dotenv text is read: as the text it is, as JSON, or as JSON when it starts
    with ``[`` or ``{`` and as text otherwise."""
    has_default: bool

    @property
    def keys(self) -> tuple[str, ...]:
        """Every key a document may use for this field: the wire name, then the aliases."""
        return (self.wire, *(alias for alias in self.aliases if alias != self.wire))


class SectionSchema:
    """The fields of one config class, by Python name and by key."""

    __slots__ = ("by_key", "cls", "fields")

    def __init__(self, cls: type, fields: list[FieldSchema]) -> None:
        self.cls = cls
        self.fields = {field.name: field for field in fields}
        self.by_key = {key: field for field in fields for key in field.keys}

    def wire_path(self, path: tuple[str, ...]) -> tuple[str, ...]:
        """``path`` (Python names) as the wire names a document writes; a name that is no
        field stays as it is."""
        names: list[str] = []
        section: SectionSchema | None = self
        for name in path:
            field = section.fields.get(name) if section is not None else None
            names.append(field.wire if field is not None else name)
            section = section_schema(field.section) if field and field.section else None
        return tuple(names)


@functools.cache
def section_schema(cls: type) -> SectionSchema:
    """The schema of the config class ``cls``."""
    hints = type_hints(cls)
    model = model_options(cls)
    fields: list[FieldSchema] = []
    for field in dataclasses.fields(cls):
        options = options_of(field)
        if not field.init or options.skip_decode:
            continue
        shape = _shape(hints[field.name])
        fields.append(
            FieldSchema(
                name=field.name,
                wire=wire_name(field.name, options, model),
                aliases=options.aliases,
                env=options.env,
                section=shape.section,
                optional_section=shape.section is not None and shape.optional,
                text=_text_kind(shape),
                has_default=default_of(field) is not None,
            )
        )
    return SectionSchema(cls, fields)


@dataclass(frozen=True, slots=True)
class _Shape:
    section: type | None = None
    optional: bool = False
    composite: bool = False
    scalar: bool = False


def _strip(annotation: object) -> object:
    """``annotation`` without ``Annotated`` and ``Secret`` around it."""
    while True:
        origin = typing.get_origin(annotation)
        if origin is Annotated or origin is Secret:
            annotation = typing.get_args(annotation)[0]
        else:
            return annotation


_COMPOSITES: tuple[object, ...] = (
    tuple,
    frozenset,
    set,
    list,
    dict,
    collections.abc.Mapping,
    collections.abc.Sequence,
)


def _shape(annotation: object) -> _Shape:
    annotation = _strip(annotation)
    if is_union(annotation):
        arguments = typing.get_args(annotation)
        present = without_none(arguments)
        optional = len(present) < len(arguments)
        shapes = [_shape(argument) for argument in present]
        if len(shapes) == 1:
            only = shapes[0]
            return dataclasses.replace(only, optional=only.optional or optional)
        return _Shape(
            optional=optional,
            composite=any(shape.composite or shape.section is not None for shape in shapes),
            scalar=any(shape.scalar for shape in shapes),
        )
    if isinstance(annotation, type) and dataclasses.is_dataclass(annotation):
        return _Shape(section=annotation, composite=True)
    origin = cast("object", typing.get_origin(annotation) or annotation)
    if origin in _COMPOSITES:
        return _Shape(composite=True)
    return _Shape(scalar=True)


def _text_kind(shape: _Shape) -> Literal["text", "json", "either"]:
    if shape.section is not None or not shape.scalar:
        return "json"
    return "either" if shape.composite else "text"


# -- the checks of a declaration -----------------------------------------------------------------


def check_declaration(cls: type) -> None:
    """Refuses what a config cannot hold (spec section 6.1): a model that is not a config,
    a mutable collection, a flattened field. Raises ``SchemaError``, or
    ``UnresolvedAnnotationError`` when a field names a class that is not defined yet."""
    _check_class(cls, set())


def _check_class(cls: type, seen: set[type]) -> None:
    if cls in seen:
        return
    seen.add(cls)
    hints = type_hints(cls)
    for field in dataclasses.fields(cls):
        location = f"{cls.__qualname__}.{field.name}"
        options = options_of(field)
        if options.flatten:
            raise SchemaError(
                f"{location}: a config field cannot be flattened, because a source cannot say "
                "where a flattened section starts; make it a section"
            )
        if not field.init or options.skip_decode:
            continue
        _check_annotation(hints[field.name], location, seen)


_MUTABLE: dict[object, str] = {
    list: "use tuple[T, ...]: a config is immutable end to end",
    set: "use frozenset[T]: a config is immutable end to end",
}

_MAPPINGS = "Mapping fields are not built yet (spec section 6.7); for keys you know, make a section"


def _check_annotation(annotation: object, location: str, seen: set[type]) -> None:
    origin = typing.get_origin(annotation)
    if origin is Annotated or origin is Secret:
        _check_annotation(typing.get_args(annotation)[0], location, seen)
        return
    if is_union(annotation):
        for argument in without_none(typing.get_args(annotation)):
            _check_annotation(argument, location, seen)
        return
    kind = origin or annotation
    if kind in _MUTABLE:
        raise SchemaError(f"{location}: {_name(kind)} is mutable; {_MUTABLE[kind]}")
    if kind is dict or kind is collections.abc.Mapping or kind is collections.abc.MutableMapping:
        raise SchemaError(f"{location}: {_name(kind)} is not allowed in a config; {_MAPPINGS}")
    if kind is tuple or kind is frozenset:
        for argument in typing.get_args(annotation):
            if argument is not Ellipsis:
                _check_annotation(argument, location, seen)
        return
    if isinstance(annotation, type) and dataclasses.is_dataclass(annotation):
        if not is_config(annotation):
            raise SchemaError(
                f"{location}: {annotation.__qualname__} is a section of a config, so declare it "
                "with @ox.config (a model made by @ox.model or a plain dataclass is not one)"
            )
        _check_class(annotation, seen)


def _name(kind: object) -> str:
    return getattr(kind, "__name__", str(kind))
