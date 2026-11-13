"""``ox.json_schema`` and ``ox.json_schema_bundle`` (section 8.5)."""

import json
import typing
from collections.abc import Callable, Iterable, Mapping
from dataclasses import dataclass
from typing import Any

from oxiform._json_schema.names import assign_names
from oxiform._json_schema.writer import (
    JsonSchema,
    NodeKey,
    NodeRegistry,
    Ref,
    SchemaCoercion,
    SchemaMode,
    SchemaWriter,
    Variant,
)
from oxiform._oxiform import warmup

__all__ = ["SchemaBundle", "SchemaTarget", "json_schema", "json_schema_bundle"]

DRAFT = "https://json-schema.org/draft/2020-12/schema"

_MODES = ("validation", "serialization")
_COERCIONS = ("strict", "lax")

# The order in which a class's variants are named, and the suffix each one gives.
_SUFFIXES: dict[Variant, str] = {
    ("validation", "strict"): "-Input",
    ("validation", "lax"): "-Input-Lax",
    ("serialization", "strict"): "-Output",
}


@dataclass(frozen=True, slots=True)
class SchemaTarget:
    """A type to write a schema for. ``mode`` and ``coercion`` left as ``None`` take the
    values of the call."""

    type: Any
    mode: SchemaMode | None = None
    coercion: SchemaCoercion | None = None


@dataclass(frozen=True, slots=True, eq=False)
class SchemaBundle:
    """The schemas of several types that share their definitions."""

    defs: dict[str, JsonSchema]
    """The named schemas, models and enums, in the order of their names."""
    schemas: tuple[JsonSchema, ...]
    """The schema of each target, in the order of ``targets``."""
    targets: tuple[SchemaTarget, ...]
    """The targets with ``mode`` and ``coercion`` filled in."""
    ref_template: str
    mode: SchemaMode
    coercion: SchemaCoercion

    def schema_for(self, target: object) -> JsonSchema:
        """The schema of ``target``, a type or a ``SchemaTarget``, as given to the bundle."""
        wanted = _resolve(target, self.mode, self.coercion)
        for known, schema in zip(self.targets, self.schemas, strict=True):
            if known == wanted:
                return schema
        listing = ", ".join(repr(known) for known in self.targets) or "none"
        raise KeyError(f"{wanted!r} is not in this bundle; its targets are: {listing}")


def json_schema(
    type: object,  # noqa: A002
    *,
    mode: SchemaMode = "validation",
    coercion: SchemaCoercion = "strict",
) -> JsonSchema:
    """The JSON Schema 2020-12 document of one type, with its definitions under ``$defs``."""
    bundle = json_schema_bundle([type], mode=mode, coercion=coercion)
    document: JsonSchema = {"$schema": DRAFT, **bundle.schemas[0]}
    if bundle.defs:
        document["$defs"] = bundle.defs
    return document


def json_schema_bundle(
    targets: Iterable[object],
    *,
    mode: SchemaMode = "validation",
    coercion: SchemaCoercion = "strict",
    ref_template: str = "#/$defs/{name}",
) -> SchemaBundle:
    """The schemas of ``targets`` with shared definitions, named without collisions."""
    _check_variant(mode, coercion, "")
    _check_template(ref_template)
    listed = _listed(targets)
    resolved = tuple(_resolve(target, mode, coercion) for target in listed)
    for target in resolved:
        warmup(type(None) if target.type is None else target.type)
    registry = NodeRegistry()
    roots = [
        SchemaWriter(_variant(target), registry).schema(target.type, "root") for target in resolved
    ]
    nodes = registry.build()
    names, group = _name_nodes(nodes)

    def reference(ref: Ref) -> str:
        return ref_template.replace("{name}", names[group[(ref.cls, ref.variant)]])

    defs: dict[str, JsonSchema] = {}
    for identifier, name in names.items():
        first = next(key for key in nodes if group[key] == identifier)
        defs[name] = _resolved(nodes[first], reference)
    return SchemaBundle(
        defs=dict(sorted(defs.items())),
        schemas=tuple(_resolved(root, reference) for root in roots),
        targets=resolved,
        ref_template=ref_template,
        mode=mode,
        coercion=coercion,
    )


def _listed(targets: object) -> list[object]:
    if isinstance(targets, set | frozenset | Mapping):
        kind = type(typing.cast("object", targets)).__name__
        raise TypeError(
            "json_schema_bundle takes a list or a tuple of targets, because the order of a "
            f"{kind} is not the order of the schemas; pass [type, ...]"
        )
    bad = (
        isinstance(targets, str | bytes | type)
        or typing.get_origin(targets) is not None
        or not isinstance(targets, Iterable)
    )
    if bad:
        raise TypeError(
            "json_schema_bundle takes a list of types or ox.SchemaTarget, "
            f"not {targets!r}; for one type call ox.json_schema(type), or pass [type]"
        )
    return list(typing.cast("Iterable[object]", targets))


def _resolve(target: object, mode: SchemaMode, coercion: SchemaCoercion) -> SchemaTarget:
    if isinstance(target, SchemaTarget):
        resolved = SchemaTarget(
            target.type,
            mode if target.mode is None else target.mode,
            coercion if target.coercion is None else target.coercion,
        )
    else:
        resolved = SchemaTarget(target, mode, coercion)
    assert resolved.mode is not None and resolved.coercion is not None
    _check_variant(resolved.mode, resolved.coercion, f"{resolved.type!r}: ")
    return resolved


def _check_variant(mode: object, coercion: object, where: str) -> None:
    if mode not in _MODES:
        raise ValueError(
            f'{where}mode must be "validation" (what decode accepts) or "serialization" '
            f"(what encode writes), not {mode!r}"
        )
    if coercion not in _COERCIONS:
        raise ValueError(f'{where}coercion must be "strict" or "lax", not {coercion!r}')
    if mode == "serialization" and coercion == "lax":
        raise ValueError(
            f'{where}coercion="lax" describes what decode reads, so it applies to '
            'mode="validation" only; drop coercion or use mode="validation"'
        )


def _check_template(ref_template: object) -> None:
    if not isinstance(ref_template, str):
        raise TypeError(
            "ref_template must be a str such as "
            f'"#/components/schemas/{{name}}", not {ref_template!r}'
        )
    if ref_template.count("{name}") != 1:
        raise ValueError(
            f'ref_template must contain "{{name}}" exactly once, as in '
            f'"#/components/schemas/{{name}}", not {ref_template!r}'
        )


def _variant(target: SchemaTarget) -> Variant:
    assert target.mode is not None and target.coercion is not None
    return (target.mode, "strict" if target.mode == "serialization" else target.coercion)


def _name_nodes(nodes: dict[NodeKey, JsonSchema]) -> tuple[dict[int, str], dict[NodeKey, int]]:
    """Groups the nodes whose schemas are equal and names each group.

    Two variants of one class are the same definition when their schemas are equal with every
    reference taken to the group of its target, found by refining groups until they are stable
    (so a cycle of equal classes stays one group). A class with one group keeps its name; one with
    several gets the suffix of the first variant of each.
    """
    classes = list(dict.fromkeys(cls for cls, _ in nodes))
    index = {cls: number for number, cls in enumerate(classes)}
    group: dict[NodeKey, int] = {key: index[key[0]] for key in nodes}
    while True:
        signatures: dict[NodeKey, str] = {
            key: json.dumps([index[key[0]], _frozen(schema, group)], sort_keys=True)
            for key, schema in nodes.items()
        }
        numbered: dict[str, int] = {}
        refined = {key: numbered.setdefault(sig, len(numbered)) for key, sig in signatures.items()}
        if len(numbered) == len(set(group.values())):
            break
        group = refined
    base = assign_names(classes)
    names: dict[int, str] = {}
    for cls in classes:
        members = [key for key in nodes if key[0] is cls]
        identifiers = list(dict.fromkeys(group[key] for key in members))
        for identifier in identifiers:
            if len(identifiers) == 1:
                names[identifier] = base[cls]
            else:
                first = min(
                    (key[1] for key in members if group[key] == identifier),
                    key=lambda variant: list(_SUFFIXES).index(variant),
                )
                names[identifier] = base[cls] + _SUFFIXES[first]
    return names, group


def _frozen(value: Any, group: dict[NodeKey, int]) -> Any:
    """``value`` with each reference replaced by the group of its target."""
    if isinstance(value, Ref):
        return f"@{group[(value.cls, value.variant)]}"
    if isinstance(value, dict):
        return {key: _frozen(item, group) for key, item in typing.cast("JsonSchema", value).items()}
    if isinstance(value, list):
        return [_frozen(item, group) for item in typing.cast("list[Any]", value)]
    return value


def _resolved(value: Any, reference: Callable[[Ref], str]) -> Any:
    """``value`` with each reference written as the text ``reference`` gives."""
    if isinstance(value, Ref):
        return reference(value)
    if isinstance(value, dict):
        node = typing.cast("JsonSchema", value)
        return {key: _resolved(item, reference) for key, item in node.items()}
    if isinstance(value, list):
        return [_resolved(item, reference) for item in typing.cast("list[Any]", value)]
    return value
