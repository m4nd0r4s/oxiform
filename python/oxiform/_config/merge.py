"""Merges the layers of a config into one tree whose leaves remember their origin (spec 6.3)."""

from dataclasses import dataclass
from typing import Any, cast

from oxiform._config.origin import DEFAULT_ORIGIN, Origin
from oxiform._config.schema import UNKNOWN, FieldSchema, SectionSchema, section_schema
from oxiform._config.sources import Entry, Layer
from oxiform._secret import Secret

__all__ = ["Leaf", "Tree", "merge", "origins_of", "plain", "resolve", "to_wire"]


@dataclass(slots=True)
class Leaf:
    """A value that one layer set, and where it came from."""

    value: object
    origin: Origin


Tree = dict[str, Any]
"""Section name to a ``Tree`` (a section, merged key by key) or a ``Leaf`` (a value)."""


def merge(layers: list[Layer]) -> Tree:
    """The tree of all layers: a later layer replaces what an earlier one set at a path, and
    sections merge key by key."""
    tree: Tree = {}
    for layer in layers:
        for entry in layer.entries:
            _apply(tree, layer, entry)
    return tree


def _apply(tree: Tree, layer: Layer, entry: Entry) -> None:
    node = tree
    for part in entry.path[:-1]:
        child = node.get(part)
        if not isinstance(child, dict):
            child = {}
            node[part] = child
        node = cast("Tree", child)
    last = entry.path[-1]
    replaced = _origins_under(node.get(last))
    origin = Origin(layer.kind, layer.label, entry.key, entry.line, tuple(replaced))
    node[last] = Leaf(entry.value, origin)


def _origins_under(node: object) -> list[str]:
    """What a value replaces, nearest layer first."""
    if isinstance(node, Leaf):
        return [str(node.origin), *node.origin.overrides]
    if isinstance(node, dict):
        found: list[str] = []
        for child in cast("Tree", node).values():
            found.extend(_origins_under(child))
        return found
    return []


def plain(value: object) -> object:
    """``value`` with every ``Secret`` replaced by what it holds, which is what a source gave:
    the engine reads a secret from its plain value."""
    if isinstance(value, Secret):
        return plain(cast("Secret[object]", value).reveal())
    if isinstance(value, dict):
        return {key: plain(item) for key, item in cast("dict[str, object]", value).items()}
    if isinstance(value, (list, tuple)):
        return [plain(item) for item in cast("list[object]", value)]
    return value


def to_wire(tree: Tree, section: SectionSchema | None, default: object = None) -> dict[str, object]:
    """The tree as a document the engine reads: wire names instead of Python names. A key that
    is no field stays as it is, so that the engine reports it.

    ``default`` is the instance the section has as its default, when that is not the class's
    own defaults (``db: Db = Db(url="custom")``): the keys no source set come from it.
    """
    document: dict[str, object] = {}
    for name, node in tree.items():
        field = section.fields.get(name) if section is not None else None
        key = field.wire if field is not None else name.removeprefix(UNKNOWN)
        if isinstance(node, dict):
            below = section_schema(field.section) if field is not None and field.section else None
            inner = _default_of(field, default)
            document[key] = to_wire(cast("Tree", node), below, inner)
        elif field is not None and field.section is not None and _is_empty_marker(node):
            # `[db]` with nothing under it: the section, built from its defaults.
            document[key] = to_wire({}, section_schema(field.section), _default_of(field, default))
        else:
            document[key] = plain(cast("Leaf", node).value)
    if section is None:
        return document
    for field in section.fields.values():
        if field.section is not None and not field.has_default and field.name not in tree:
            # A required section that no source set is an empty one, so that the engine
            # reports its missing fields (db.url) and not the section as a whole.
            document[field.wire] = to_wire({}, section_schema(field.section))
        elif default is not None and field.wire not in document:
            document[field.wire] = _seed_value(getattr(default, field.name), field)
    return document


def _default_of(field: FieldSchema | None, parent: object) -> object:
    """The instance that is the default of the section ``field``: what the default of the
    section around it holds, else the default declared on the field itself."""
    if field is None or field.section is None:
        return None
    if parent is not None:
        held = getattr(parent, field.name, None)
        if isinstance(held, field.section):
            return held
    if field.default_section is not None:
        made = field.default_section()
        if isinstance(made, field.section):
            return made
    return None


def _seed_value(value: object, field: FieldSchema) -> object:
    if field.section is not None and isinstance(value, field.section):
        below = section_schema(field.section)
        return {
            inner.wire: _seed_value(getattr(value, inner.name), inner)
            for inner in below.fields.values()
        }
    return plain(value)


def unencodable(tree: Tree, prefix: tuple[str, ...] = ()) -> tuple[tuple[str, ...], Leaf] | None:
    """The first value of the tree that holds text with bytes that are not UTF-8, where it is."""
    for name, node in tree.items():
        path = (*prefix, name.removeprefix(UNKNOWN))
        if isinstance(node, dict):
            found = unencodable(cast("Tree", node), path)
            if found is not None:
                return found
        elif _bad_text(plain(cast("Leaf", node).value)):
            return path, cast("Leaf", node)
    return None


def _bad_text(value: object) -> bool:
    if isinstance(value, str):
        try:
            value.encode("utf-8")
        except UnicodeEncodeError:
            return True
        return False
    if isinstance(value, dict):
        return any(
            _bad_text(key) or _bad_text(item)
            for key, item in cast("dict[object, object]", value).items()
        )
    if isinstance(value, (list, tuple, set, frozenset)):
        return any(_bad_text(item) for item in cast("list[object]", value))
    return False


def resolve(
    tree: Tree, section: SectionSchema, wire_path: list[str | int]
) -> tuple[tuple[str, ...], Leaf | None]:
    """The Python path of a problem found at ``wire_path`` and the value that set it (or, for a
    path inside a value, the value), or ``None`` when no source set anything there."""
    python: list[str] = []
    node: Any = tree
    current: SectionSchema | None = section
    for segment in wire_path:
        if isinstance(node, Leaf):
            return tuple(name.removeprefix(UNKNOWN) for name in python), node
        if not isinstance(segment, str):
            return tuple(name.removeprefix(UNKNOWN) for name in python), None
        field = current.by_key.get(segment) if current is not None else None
        name = field.name if field is not None else UNKNOWN + segment
        python.append(name)
        node = cast("Tree", node).get(name) if node is not None else None
        current = section_schema(field.section) if field is not None and field.section else None
    shown = tuple(name.removeprefix(UNKNOWN) for name in python)
    return shown, node if isinstance(node, Leaf) else None


def origins_of(tree: Tree, section: SectionSchema) -> dict[str, Origin]:
    """The origin of every field, by dotted Python path (spec section 6.4)."""
    found: dict[str, Origin] = {}
    _collect(tree, section, (), found)
    return found


def _collect(
    tree: Tree | None, section: SectionSchema, prefix: tuple[str, ...], found: dict[str, Origin]
) -> None:
    for field in section.fields.values():
        path = (*prefix, field.name)
        node = tree.get(field.name) if tree is not None else None
        if field.section is not None and (isinstance(node, dict) or _is_empty_marker(node)):
            below = cast("Tree", node) if isinstance(node, dict) else None
            _collect(below, section_schema(field.section), path, found)
        elif field.section is not None and node is None and not field.optional_section:
            _collect(None, section_schema(field.section), path, found)
        elif isinstance(node, Leaf):
            found[".".join(path)] = node.origin
        else:
            found[".".join(path)] = DEFAULT_ORIGIN


def _is_empty_marker(node: object) -> bool:
    return isinstance(node, Leaf) and node.value == {}
