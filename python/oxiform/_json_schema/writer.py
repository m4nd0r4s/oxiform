"""Writes the JSON Schema of a type for one variant: validation, validation with lax coercion
or serialization. Models and enums are not written in place: they become nodes of a registry,
and the schema holds a ``Ref`` to each, which the bundle turns into a ``$ref`` once every node
has a name."""

import dataclasses
import enum
import inspect
import json
import types
import typing
from dataclasses import dataclass
from typing import Any, Literal, TypeAlias, cast

from oxiform._fields import options_of
from oxiform._introspection.choices import enum_choice, literal_choice
from oxiform._introspection.constraints import collect_constraints
from oxiform._introspection.conversions import conversion_of
from oxiform._introspection.describe import (
    BARE_COLLECTIONS,
    TEMPORAL_KINDS,
    flattened_class,
    is_enum,
    is_flag,
    type_hints,
    unwrap_annotated,
)
from oxiform._introspection.encoding import encoded_fields
from oxiform._introspection.fields import checked_options, default_of, tag_of, wire_name
from oxiform._introspection.model import ConstraintValues, Default
from oxiform._introspection.unions import UnionMode, is_union, without_none
from oxiform._json_schema import patterns
from oxiform._json_schema.regex import portable_pattern
from oxiform._model_options import model_options
from oxiform._oxiform import json_encode
from oxiform.errors import EncodeError, SchemaError
from oxiform.unions import External, Tagged, Untagged

__all__ = [
    "JsonSchema",
    "NodeRegistry",
    "Ref",
    "SchemaCoercion",
    "SchemaMode",
    "SchemaWriter",
    "Variant",
]

JsonSchema: TypeAlias = dict[str, Any]
SchemaMode = Literal["validation", "serialization"]
SchemaCoercion = Literal["strict", "lax"]
Variant: TypeAlias = tuple[SchemaMode, SchemaCoercion]
NodeKey: TypeAlias = tuple[type, Variant]


@dataclass(frozen=True, slots=True)
class Ref:
    """A reference to the node of a class in a variant, before it has a name."""

    cls: type
    variant: Variant


class NodeRegistry:
    """The models and enums reached from the targets, in the order they were met."""

    def __init__(self) -> None:
        self.nodes: dict[NodeKey, JsonSchema | None] = {}
        self._pending: list[NodeKey] = []

    def reference(self, cls: type, variant: Variant) -> Ref:
        key = (cls, variant)
        if key not in self.nodes:
            self.nodes[key] = None
            self._pending.append(key)
        return Ref(cls, variant)

    def build(self) -> dict[NodeKey, JsonSchema]:
        """Writes every node that was referenced, and the nodes those reference."""
        while self._pending:
            key = self._pending.pop(0)
            cls, variant = key
            self.nodes[key] = SchemaWriter(variant, self).node(cls)
        built: dict[NodeKey, JsonSchema] = {}
        for key, schema in self.nodes.items():
            assert schema is not None
            built[key] = schema
        return built


_FORMATS = {
    "uuid": "uuid",
    "ipv4_address": "ipv4",
    "ipv6_address": "ipv6",
    "ipv4_network": "ipv4-network",
    "ipv6_network": "ipv6-network",
    "ipv4_interface": "ipv4-interface",
    "ipv6_interface": "ipv6-interface",
    "path": "path",
    "zoneinfo": "zoneinfo",
}

_TEMPORAL_SCHEMAS: dict[str, JsonSchema] = {
    "datetime": {"type": "string", "format": "date-time"},
    "naive_datetime": {"type": "string", "pattern": patterns.NAIVE_DATETIME_TEXT},
    "date": {"type": "string", "format": "date"},
    "time": {"type": "string", "pattern": patterns.TIME_TEXT},
    "timedelta": {"type": "string", "format": "duration"},
}

_NO_DEFAULT = object()

_FLAG_VALUES_LIMIT = 1023


def _is_flag_value(cls: type[enum.Flag], value: int) -> bool:
    try:
        cls(value)
    except ValueError:
        return False
    return True


class SchemaWriter:
    """The schemas of one variant."""

    def __init__(self, variant: Variant, registry: NodeRegistry) -> None:
        self.variant = variant
        self.registry = registry
        self.serializing = variant[0] == "serialization"
        self.lax = variant[1] == "lax"

    # -- types ---------------------------------------------------------------------------

    def schema(
        self,
        annotation: object,
        location: str = "root",
        outer: ConstraintValues | None = None,
        *,
        drop_none: bool = False,
    ) -> JsonSchema:
        """The schema of ``annotation``. With ``drop_none``, a ``None`` that the type allows
        is left out, for a field that is not written when it is ``None``."""
        annotation, constraints, mode, naive = unwrap_annotated(annotation, location, outer or {})
        if naive:
            return dict(_TEMPORAL_SCHEMAS["naive_datetime"])
        if mode is not None:
            return self._marked_union(annotation, mode, location, constraints, drop_none)
        if annotation is Any:
            return {}
        if annotation is bool:
            return self._with_text({"type": "boolean"}, patterns.BOOL_TEXT)
        if annotation is None or annotation is types.NoneType:
            return {"type": "null"}
        if annotation is int:
            choices = cast("list[object]", constraints.get("one_of", []))
            texts = patterns.texts_of(tuple(choices))
            return self._with_text(
                self._number("integer", constraints), texts or patterns.INTEGER_TEXT
            )
        if annotation is float:
            return self._with_text(self._number("number", constraints), patterns.FLOAT_TEXT)
        if annotation is str:
            return self._string(constraints, location)
        if annotation is bytes or annotation is bytearray:
            return {
                "type": "string",
                "contentEncoding": "base64",
                "pattern": patterns.BASE64_TEXT,
            }
        if annotation in TEMPORAL_KINDS:
            return dict(_TEMPORAL_SCHEMAS[TEMPORAL_KINDS[annotation]])
        conversion = conversion_of(annotation)
        if conversion is None and is_flag(annotation):
            return self._flag(annotation)
        if conversion is not None:
            return self._converted(conversion.format)
        if annotation is list:
            return {"type": "array", "items": {}, **self._length(constraints, "Items")}
        if annotation in BARE_COLLECTIONS:
            return self._collection(BARE_COLLECTIONS[annotation], {}, constraints)
        if is_enum(annotation):
            return {"$ref": self.registry.reference(annotation, self.variant)}
        if annotation is dict:
            return self._dict({}, constraints)
        origin = typing.get_origin(annotation)
        arguments = typing.get_args(annotation)
        if is_union(annotation):
            return self._union(arguments, location, constraints, drop_none)
        if origin is list:
            items = self.schema(arguments[0], location)
            return {"type": "array", "items": items, **self._length(constraints, "Items")}
        if origin is typing.Literal:
            return self._choice(literal_choice(annotation, location).values)
        if origin is tuple:
            return self._tuple(arguments, location, constraints)
        if origin is set or origin is frozenset:
            return self._collection(
                "set" if origin is set else "frozenset",
                self.schema(arguments[0], location),
                constraints,
            )
        if origin is dict:
            return self._dict(self.schema(arguments[1], location), constraints)
        if isinstance(annotation, type) and dataclasses.is_dataclass(annotation):
            return {"$ref": self.registry.reference(annotation, self.variant)}
        raise SchemaError(f"{location}: unsupported type {annotation!r}")

    def _with_text(self, schema: JsonSchema, pattern: str) -> JsonSchema:
        """Lax mode also reads the type from text."""
        if not self.lax:
            return schema
        return {"anyOf": [schema, {"type": "string", "pattern": pattern}]}

    @staticmethod
    def _number(kind: str, constraints: ConstraintValues) -> JsonSchema:
        schema: JsonSchema = {"type": kind}
        for key, keyword in (("minimum", "minimum"), ("maximum", "maximum"), ("one_of", "enum")):
            if key in constraints:
                schema[keyword] = constraints[key]
        return schema

    @staticmethod
    def _string(constraints: ConstraintValues, location: str) -> JsonSchema:
        schema: JsonSchema = {"type": "string"}
        for key, keyword in (
            ("min_length", "minLength"),
            ("max_length", "maxLength"),
            ("pattern", "pattern"),
            ("one_of", "enum"),
        ):
            if key in constraints:
                value = constraints[key]
                if key == "pattern":
                    value = portable_pattern(str(value), location)
                schema[keyword] = value
        return schema

    @staticmethod
    def _length(constraints: ConstraintValues, what: str) -> JsonSchema:
        keywords = {
            "Items": ("minItems", "maxItems"),
            "Properties": ("minProperties", "maxProperties"),
        }
        low, high = keywords[what]
        schema: JsonSchema = {}
        if "min_length" in constraints:
            schema[low] = constraints["min_length"]
        if "max_length" in constraints:
            schema[high] = constraints["max_length"]
        return schema

    def _converted(self, format_name: str) -> JsonSchema:
        if format_name == "decimal":
            text: JsonSchema = {
                "type": "string",
                "format": "decimal",
                "pattern": patterns.DECIMAL_TEXT,
            }
            return text if self.serializing else {"anyOf": [{"type": "number"}, text]}
        return {"type": "string", "format": _FORMATS[format_name]}

    def _choice(self, values: tuple[int | str | bool | None, ...]) -> JsonSchema:
        schema: JsonSchema = {}
        kinds = {_json_type(value) for value in values}
        if len(kinds) == 1:
            schema["type"] = next(iter(kinds))
        if len(values) == 1:
            schema["const"] = values[0]
        else:
            schema["enum"] = list(values)
        texts = patterns.texts_of(values) if self.lax else None
        if texts:
            return {"anyOf": [schema, {"type": "string", "pattern": texts}]}
        return schema

    def _flag(self, cls: type[enum.Flag]) -> JsonSchema:
        """The integers ``cls(value)`` takes: those of a strict ``Flag``, written out when there
        are few (any combination of members, and the complement of each), else its bounds; any
        integer for a flag that keeps or conforms unknown bits, as ``IntFlag`` does."""
        mask = 0
        for member in cls.__members__.values():  # every member, multi-bit and aliases included
            mask |= int(member.value)
        if getattr(cls, "_boundary_", None) is not enum.STRICT:
            return self._with_text({"type": "integer"}, patterns.INTEGER_TEXT)
        lowest = -(1 << mask.bit_length())  # the complement of every bit
        if mask > _FLAG_VALUES_LIMIT:
            bounds: JsonSchema = {"type": "integer", "minimum": lowest, "maximum": mask}
            return self._with_text(bounds, patterns.INTEGER_TEXT)
        return self._choice(tuple(v for v in range(lowest, mask + 1) if _is_flag_value(cls, v)))

    def _collection(
        self, kind: str, items: JsonSchema, constraints: ConstraintValues
    ) -> JsonSchema:
        schema: JsonSchema = {"type": "array", "items": items, **self._length(constraints, "Items")}
        if kind != "tuple" and self.serializing:
            # Written sorted and without repeats; read from any array (a repeat is one item).
            schema["uniqueItems"] = True
        return schema

    def _dict(self, values: JsonSchema, constraints: ConstraintValues) -> JsonSchema:
        return {
            "type": "object",
            "additionalProperties": values,
            **self._length(constraints, "Properties"),
        }

    def _tuple(
        self, arguments: tuple[object, ...], location: str, constraints: ConstraintValues
    ) -> JsonSchema:
        if len(arguments) == 2 and arguments[1] is Ellipsis:
            return self._collection("tuple", self.schema(arguments[0], location), constraints)
        if not arguments:
            return {"type": "array", "maxItems": 0}
        return {
            "type": "array",
            "prefixItems": [self.schema(item, location) for item in arguments],
            "minItems": len(arguments),
            "maxItems": len(arguments),
        }

    # -- unions --------------------------------------------------------------------------

    def _union(
        self,
        arguments: tuple[object, ...],
        location: str,
        constraints: ConstraintValues,
        drop_none: bool,
    ) -> JsonSchema:
        present = without_none(arguments)
        if len(present) == 1 and len(arguments) == 2:
            inner = self.schema(present[0], location, constraints, drop_none=False)
            return self._optional(inner, present, drop_none)
        if constraints:
            raise SchemaError(f"{location}: constraints do not apply to a union")
        shared = _shared_tag_field(present)
        variants = [self._plain_variant(argument, shared, location) for argument in present]
        schema: JsonSchema = {"oneOf": variants} if shared else {"anyOf": variants}
        if shared:
            schema["discriminator"] = self._discriminator(shared, present)
        return (
            self._optional(schema, present, drop_none) if len(present) < len(arguments) else schema
        )

    def _plain_variant(self, argument: object, shared: str | None, location: str) -> JsonSchema:
        cls = _model_class(argument)
        if shared is None or cls is None:
            return self.schema(argument, location)
        return {**self._ref(cls), "required": [shared]}

    def _optional(self, schema: JsonSchema, present: list[object], drop_none: bool) -> JsonSchema:
        """``schema`` or ``null``, plus the empty string in lax mode where it reads as ``None``."""
        if drop_none:
            return schema
        branches = list(schema["anyOf"]) if set(schema) == {"anyOf"} else [schema]
        branches.append({"type": "null"})
        if self.lax and not any(_keeps_empty(argument) for argument in present):
            branches.append({"const": ""})
        return {"anyOf": branches}

    def _marked_union(
        self,
        annotation: object,
        mode: UnionMode,
        location: str,
        constraints: ConstraintValues,
        drop_none: bool,
    ) -> JsonSchema:
        name = type(mode).__name__
        if not is_union(annotation):
            raise SchemaError(f"{location}: ox.{name} applies to a union, not {annotation!r}")
        if constraints:
            raise SchemaError(f"{location}: constraints do not apply to a union")
        arguments = typing.get_args(annotation)
        present = without_none(arguments)
        schema: JsonSchema
        if isinstance(mode, Untagged):
            schema = {"anyOf": [self.schema(argument, location) for argument in present]}
        else:
            classes = [_model_class(argument) for argument in present]
            models = [cls for cls in classes if cls is not None]
            if len(models) != len(classes):
                raise SchemaError(f"{location}: only models can be variants of ox.{name}")
            schema = self._tagged(mode, models)
        if len(present) < len(arguments):
            return self._optional(schema, present, drop_none)
        return schema

    def _tagged(self, mode: Tagged | External, models: list[type]) -> JsonSchema:
        variants: list[JsonSchema] = []
        for cls in models:
            tag = tag_of(cls)
            ref = self._ref(cls)
            if isinstance(mode, External):
                variants.append(
                    {
                        "type": "object",
                        "properties": {tag: ref},
                        "required": [tag],
                        "additionalProperties": False,
                    }
                )
            elif mode.content is not None:
                variants.append(
                    {
                        "type": "object",
                        "properties": {mode.field: _tag_const(tag), mode.content: ref},
                        "required": [mode.field, mode.content],
                    }
                )
            elif model_options(cls).tag_field == mode.field:
                variants.append({**ref, "required": [mode.field]})
            elif model_options(cls).unknown == "forbid" and not self.serializing:
                # additionalProperties cannot see through allOf: write the object in place.
                variants.append(self.object_schema(cls, injected_tag=(mode.field, tag)))
            else:
                injected = {"properties": {mode.field: _tag_const(tag)}, "required": [mode.field]}
                variants.append({"allOf": [ref, injected]})
        schema: JsonSchema = {"oneOf": variants}
        if isinstance(mode, Tagged) and mode.content is None:
            if all(model_options(cls).tag_field == mode.field for cls in models):
                schema["discriminator"] = self._discriminator(mode.field, list(models))
        return schema

    def _discriminator(self, field: str, variants: list[object]) -> JsonSchema:
        mapping: dict[str, Ref] = {}
        for variant in variants:
            cls = _model_class(variant)
            if cls is not None:
                mapping[tag_of(cls)] = self.registry.reference(cls, self.variant)
        return {"propertyName": field, "mapping": mapping}

    def _ref(self, cls: type) -> JsonSchema:
        return {"$ref": self.registry.reference(cls, self.variant)}

    # -- nodes ---------------------------------------------------------------------------

    def node(self, cls: type) -> JsonSchema:
        """The schema of a model or an enum, as a definition."""
        if is_enum(cls):
            return self._enum(cls)
        return self.object_schema(cls)

    def _enum(self, cls: type) -> JsonSchema:
        values = enum_choice(cls, cls.__qualname__).values  # pyright: ignore[reportArgumentType]
        return self._choice(values)

    def object_schema(
        self, cls: type, *, injected_tag: tuple[str, str] | None = None
    ) -> JsonSchema:
        properties: dict[str, JsonSchema] = {}
        required: list[str] = []
        alternatives: list[list[str]] = []
        if self.serializing:
            self._write_serialized(cls, properties, required)
        else:
            self._write_validated(cls, properties, required, alternatives)
        options = model_options(cls)
        if injected_tag is not None:
            properties[injected_tag[0]] = _tag_const(injected_tag[1])
            required.append(injected_tag[0])
        elif options.tag_field is not None and not self.serializing:
            properties = {options.tag_field: _tag_const(tag_of(cls)), **properties}
        schema: JsonSchema = {"type": "object"}
        description = _description(cls)
        if description:
            schema["description"] = description
        schema["properties"] = properties
        if required:
            schema["required"] = required
        groups = [{"anyOf": [{"required": [name]} for name in names]} for names in alternatives]
        if len(groups) == 1:
            schema.update(groups[0])
        elif groups:
            schema["allOf"] = groups
        if options.unknown == "forbid" and not self.serializing:
            schema["additionalProperties"] = False
        return schema

    def _write_validated(
        self,
        cls: type,
        properties: dict[str, JsonSchema],
        required: list[str],
        alternatives: list[list[str]],
    ) -> None:
        hints = type_hints(cls)
        model = model_options(cls)
        for field in dataclasses.fields(cls):
            location = f"{cls.__qualname__}.{field.name}"
            options = checked_options(field, location)
            if not field.init or options.skip_decode:
                continue
            if options.flatten:
                inner = flattened_class(hints[field.name], location)
                self._write_validated(inner, properties, required, alternatives)
                continue
            annotation = hints[field.name]
            schema = self.schema(
                annotation, location, collect_constraints(options.constraints, location)
            )
            name = wire_name(field.name, options, model)
            default = default_of(field)
            if default is None:
                names = [name, *(alias for alias in options.aliases if alias != name)]
                if len(names) == 1:
                    required.append(name)
                else:
                    alternatives.append(names)
            else:
                value = _default_json(default, annotation)
                if value is not _NO_DEFAULT:
                    schema = {**schema, "default": value}
            properties[name] = schema
            for alias in options.aliases:
                if alias != name:
                    alias_schema = {key: value for key, value in schema.items() if key != "default"}
                    properties[alias] = {**alias_schema, "deprecated": True}

    def _write_serialized(
        self, cls: type, properties: dict[str, JsonSchema], required: list[str]
    ) -> None:
        hints = type_hints(cls)
        fields = {field.name: field for field in dataclasses.fields(cls)}
        for encoded in encoded_fields(cls):
            if encoded.constant is not None:
                properties[encoded.wire_name] = _tag_const(encoded.constant)
                required.append(encoded.wire_name)
                continue
            location = f"{cls.__qualname__}.{encoded.name}"
            annotation = hints[encoded.name]
            if encoded.flatten:
                inner = flattened_class(annotation, location)
                self._write_serialized(inner, properties, required)
                continue
            options = options_of(fields[encoded.name])
            default_is_none = encoded.omit_if_default == ("value", None)
            drop_none = encoded.omit_if_none or default_is_none
            schema = self.schema(
                annotation,
                location,
                collect_constraints(options.constraints, location),
                drop_none=drop_none,
            )
            omittable = (
                encoded.omit_if_default is not None
                or encoded.omit_if is not None
                or (encoded.omit_if_none and _allows_none(annotation))
            )
            if not omittable:
                required.append(encoded.wire_name)
            properties[encoded.wire_name] = schema


def _tag_const(tag: str) -> JsonSchema:
    return {"type": "string", "const": tag}


def _json_type(value: object) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    return "integer" if isinstance(value, int) else "string"


def _model_class(annotation: object) -> type | None:
    while typing.get_origin(annotation) is typing.Annotated:
        annotation = typing.get_args(annotation)[0]
    if isinstance(annotation, type) and dataclasses.is_dataclass(annotation):
        return annotation
    return None


def _shared_tag_field(variants: list[object]) -> str | None:
    """The tag field of a plain union of models that all declare the same one, which is how
    the engine tells them apart without an ``ox.Tagged``."""
    fields: set[str | None] = set()
    maps = 0
    for variant in variants:
        cls = _model_class(variant)
        if cls is not None:
            maps += 1
            fields.add(model_options(cls).tag_field)
        elif typing.get_origin(variant) is dict or variant is dict:
            return None
    if maps < 2 or len(fields) != 1:
        return None
    return next(iter(fields))


def _keeps_empty(annotation: object) -> bool:
    """Whether the empty string is a value of the type, so that lax mode keeps it there instead
    of reading it as ``None`` (section 4.5)."""
    annotation, _, _, _ = unwrap_annotated(annotation, "", {})
    if annotation is Any or annotation is str or annotation is bytes or annotation is bytearray:
        return True
    if typing.get_origin(annotation) is typing.Literal:
        return "" in typing.get_args(annotation)
    if isinstance(annotation, type) and issubclass(annotation, enum.Enum):
        return any(member.value == "" for member in annotation)
    return False


def _allows_none(annotation: object) -> bool:
    annotation, _, _, _ = unwrap_annotated(annotation, "", {})
    if annotation is None or annotation is types.NoneType:
        return True
    return is_union(annotation) and len(without_none(typing.get_args(annotation))) < len(
        typing.get_args(annotation)
    )


def _description(cls: type) -> str | None:
    """The docstring of the class itself, not the one a dataclass writes when there is none."""
    raw = cls.__dict__.get("__doc__")
    if not isinstance(raw, str):
        return None
    text = inspect.cleandoc(raw)
    if not text or (text.startswith(f"{cls.__name__}(") and text.endswith(")")):
        return None
    return text


def _default_json(default: Default, annotation: object) -> object:
    """The default as JSON, or ``_NO_DEFAULT`` for a factory and for a value that cannot be
    written."""
    if default is None or default[0] != "value":
        return _NO_DEFAULT
    try:
        return json.loads(json_encode(default[1], type=annotation))
    except (EncodeError, SchemaError, ValueError, TypeError):
        return _NO_DEFAULT
