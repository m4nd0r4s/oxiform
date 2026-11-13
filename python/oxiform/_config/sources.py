"""The sources of a config (spec section 6.2): what each reads and how it names a key.

A source gives ``Entry`` values, keyed by the Python names of the fields, plus the problems of
reading it. It never merges and never validates: ``cfg.load`` does that once for all of them.
"""

import difflib
import os
import re
import tomllib
from abc import ABC, abstractmethod
from collections.abc import Callable, Mapping
from dataclasses import dataclass, field
from pathlib import Path
from typing import Literal, cast

from oxiform._config.dotenv import parse_dotenv
from oxiform._config.origin import OriginKind
from oxiform._config.schema import (
    UNKNOWN,
    ConfigOptions,
    FieldSchema,
    SectionSchema,
    section_schema,
)
from oxiform._config.toml_lines import key_lines, line_of_key
from oxiform._oxiform import json_decode
from oxiform.errors import ErrorDetails, ValidationError

__all__ = [
    "Entry",
    "Layer",
    "Problem",
    "ReadContext",
    "Reading",
    "Source",
    "dotenv",
    "env",
    "file",
    "mapping",
    "pyproject",
]

_NESTING_LIMIT = 16

_RESERVED_NAMES = {
    "env": "the environment",
    "default": "the value of a field that no source set",
}
"""Labels that already mean something in an error or an origin."""


@dataclass(slots=True)
class Entry:
    """A value a source sets, at a path of Python names."""

    path: tuple[str, ...]
    value: object
    key: str
    """The key as the source writes it (``APP_PORT``, ``server.port``)."""
    line: int | None = None


@dataclass(slots=True)
class Layer:
    label: str
    kind: OriginKind
    entries: list[Entry] = field(default_factory=lambda: list[Entry]())


@dataclass(slots=True)
class Problem:
    """Something wrong with a source (``config_source``) or with a key it set
    (``unknown_field``)."""

    label: str
    code: str
    message: str
    ctx: dict[str, object] = field(default_factory=lambda: dict[str, object]())
    path: tuple[str, ...] = ()


@dataclass(slots=True)
class Reading:
    layers: list[Layer] = field(default_factory=lambda: list[Layer]())
    problems: list[Problem] = field(default_factory=lambda: list[Problem]())
    """The source cannot be read."""
    unknown: list[Problem] = field(default_factory=lambda: list[Problem]())
    """Names that no field reads, to be reported with the validation problems."""


@dataclass(frozen=True, slots=True)
class ReadContext:
    cls: type
    schema: SectionSchema
    options: ConfigOptions


class Source(ABC):
    """A layer of configuration. Make one with ``cfg.file``, ``cfg.pyproject``,
    ``cfg.dotenv``, ``cfg.env`` or ``cfg.mapping``; ``read`` and ``hint`` are the engine's."""

    __slots__ = ()

    @abstractmethod
    def read(self, context: ReadContext) -> Reading:
        """Reads the source when ``cfg.load`` runs."""

    @abstractmethod
    def hint(self, context: ReadContext, path: tuple[str, ...]) -> str | None:
        """How to set the field at ``path`` in this source, for the message of a missing field."""


# -- documents (TOML and JSON) and mappings ------------------------------------------------------


def _printable(part: str) -> str:
    """``part``, or its escaped form when it holds bytes that are not UTF-8: a key is not a
    secret, but a message that cannot be printed helps nobody."""
    try:
        part.encode("utf-8")
    except UnicodeEncodeError:
        return ascii(part)
    return part


def _dotted(parts: tuple[str, ...]) -> str:
    shown = [_printable(part) for part in parts]
    return ".".join(
        part if part.replace("_", "").replace("-", "").isalnum() else f'"{part}"' for part in shown
    )


class _Walker:
    """Turns a nested mapping into entries, translating keys to Python names."""

    def __init__(
        self,
        label: str,
        wire_names: bool,
        locate: Callable[[tuple[str, ...]], tuple[str, int | None]],
        problems: list[Problem],
    ) -> None:
        self.label = label
        self.wire_names = wire_names
        self.locate = locate
        self.problems = problems

    def walk(
        self,
        raw: Mapping[object, object],
        section: SectionSchema | None,
        python: tuple[str, ...],
        written: tuple[str, ...],
        entries: list[Entry],
        depth: int = 0,
    ) -> None:
        seen: dict[str, str] = {}
        for key, value in raw.items():
            if not isinstance(key, str):
                self.problems.append(
                    Problem(
                        self.label,
                        "config_source",
                        f"{self.label}: the key {key!r} under "
                        f"{_dotted(written) or 'the top level'} is not a string; keys are text",
                        {"source": "mapping", "key": repr(key)},
                    )
                )
                continue
            known = None
            if section is not None:
                known = section.by_key.get(key) if self.wire_names else section.fields.get(key)
            name = known.name if known is not None else UNKNOWN + key
            if known is not None:
                if name in seen:
                    self.problems.append(self._twice(written, seen[name], key))
                    continue
                seen[name] = key
            path = (*python, name)
            here = (*written, key)
            if (
                known is not None
                and known.section is not None
                and isinstance(value, Mapping)
                and depth < _NESTING_LIMIT
            ):
                nested = cast("Mapping[object, object]", value)
                if not nested:
                    self._emit(entries, path, here, {})
                self.walk(nested, section_schema(known.section), path, here, entries, depth + 1)
            else:
                self._emit(entries, path, here, cast("object", value))

    def _emit(
        self, entries: list[Entry], path: tuple[str, ...], written: tuple[str, ...], value: object
    ) -> None:
        key, line = self.locate(written)
        entries.append(Entry(path, value, key, line))

    def _twice(self, written: tuple[str, ...], first: str, second: str) -> Problem:
        where = _dotted(written) or "the top level"
        return Problem(
            self.label,
            "config_source",
            f"{self.label}: {where} sets one field twice, as {first!r} and as {second!r}; keep one",
            {"source": "file", "key": _dotted((*written, second))},
        )


def _problem(label: str, kind: str, message: str, **ctx: object) -> Problem:
    return Problem(label, "config_source", message, {"source": kind, **ctx})


def _read_document(
    path: Path, label: str, kind: str, optional: bool, format_: str | None
) -> tuple[object, dict[tuple[str, ...], int] | None, Problem | None]:
    """The data of a TOML or JSON file, the lines of its keys (TOML), or why it cannot be read."""
    try:
        raw = path.read_bytes()
    except FileNotFoundError:
        if optional:
            return None, None, None
        return (
            None,
            None,
            _problem(
                label,
                kind,
                f"config file {label!r} not found (looked for {path.resolve()}); create it, fix "
                "the path, or pass optional=True if it may be absent",
                file=label,
            ),
        )
    except OSError as error:
        reason = error.strerror or type(error).__name__
        return (
            None,
            None,
            _problem(
                label,
                kind,
                f"cannot read config file {label!r}: {reason}; check the path and its permissions",
                file=label,
            ),
        )
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as error:
        return (
            None,
            None,
            _problem(
                label,
                kind,
                f"config file {label!r} is not valid UTF-8 (byte {error.start}); save it as UTF-8",
                file=label,
            ),
        )
    chosen = format_ or {".toml": "toml", ".json": "json"}.get(path.suffix.lower())
    if chosen == "toml":
        return _parse_toml(text, label, kind)
    if chosen == "json":
        return _parse_json(raw, label, kind)
    return (
        None,
        None,
        _problem(
            label,
            kind,
            f"config file {label!r} has the extension {path.suffix or '(none)'!r}, which oxiform "
            "does not read yet; use a .toml or .json file (YAML comes later), or pass format=",
            file=label,
        ),
    )


def _parse_toml(
    text: str, label: str, kind: str
) -> tuple[object, dict[tuple[str, ...], int] | None, Problem | None]:
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        detail = str(error)
        position = _toml_position(detail)
        line = position[0] if position else None
        column = position[1] if position else None
        at = f", line {line}, column {column}" if position else ""
        reason = _redacted(detail.split(" (at ")[0])
        return (
            None,
            None,
            _problem(
                label,
                kind,
                f"invalid TOML in {label!r}{at}: {reason}; fix the syntax at that position",
                file=label,
                line=line,
                column=column,
            ),
        )
    return data, key_lines(text), None


_GRAMMAR = frozenset({"=", "[", "]", "[[", "]]", "{", "}", ".", ",", ":", "#", '"', "'"})
"""What a reader quotes that is the syntax of TOML and not the input: a single such character
says nothing about a value."""


def _redacted(reason: str) -> str:
    """A reader's reason without anything it quotes from the input, except the punctuation of
    the grammar (``Expected '='``)."""

    def hide(quoted: re.Match[str]) -> str:
        return quoted.group() if quoted.group()[1:-1] in _GRAMMAR else "'...'"

    return re.sub(r"'[^']*'|\"[^\"]*\"", hide, reason)


def _toml_position(detail: str) -> tuple[int, int] | None:
    marker = "(at line "
    start = detail.find(marker)
    if start < 0:
        return None
    rest = detail[start + len(marker) :].rstrip(")")
    try:
        line, column = rest.split(", column ")
        return int(line), int(column)
    except ValueError:
        return None


def _parse_json(
    raw: bytes, label: str, kind: str
) -> tuple[object, dict[tuple[str, ...], int] | None, Problem | None]:
    try:
        data = json_decode(raw)
    except ValidationError as error:
        first = error.errors()[0]
        line, column = first["ctx"].get("line"), first["ctx"].get("column")
        return (
            None,
            None,
            _problem(
                label,
                kind,
                f"invalid JSON in {label!r}: {_json_reason(first)}; fix it there",
                file=label,
                line=line,
                column=column,
            ),
        )
    return data, None, None


def _json_reason(problem: ErrorDetails) -> str:
    """Where the reader stopped, never what it found there: that may be a secret. A duplicate
    names the key (not its value)."""
    if problem["code"] == "duplicate_key":
        key = problem["path"][-1] if problem["path"] else ""
        return f"the key {key!r} appears twice, so keep one"
    ctx = problem["ctx"]
    line, column = ctx.get("line"), ctx.get("column")
    if line is not None and column is not None:
        return f"syntax error at line {line}, column {column} (the text is not shown)"
    return "syntax error (the text is not shown)"


@dataclass(frozen=True, slots=True)
class _File(Source):
    path: str | os.PathLike[str]
    optional: bool
    format: Literal["toml", "json"] | None

    def read(self, context: ReadContext) -> Reading:
        label = os.fspath(self.path)
        data, lines, problem = _read_document(
            Path(label), label, "file", self.optional, self.format
        )
        reading = Reading()
        if problem is not None:
            reading.problems.append(problem)
            return reading
        if data is None:
            return reading
        if not isinstance(data, Mapping):
            reading.problems.append(
                _problem(
                    label,
                    "file",
                    f"config file {label!r} holds {type(data).__name__}, not an object; "
                    "put the settings in a table (TOML) or an object (JSON)",
                    file=label,
                )
            )
            return reading
        reading.layers.append(
            _document_layer(
                label, "file", cast("Mapping[object, object]", data), (), lines, context, reading
            )
        )
        return reading

    def hint(self, context: ReadContext, path: tuple[str, ...]) -> str | None:
        return f"{_dotted(context.schema.wire_path(path))} in {os.fspath(self.path)}"


def _document_layer(
    label: str,
    kind: OriginKind,
    data: Mapping[object, object],
    prefix: tuple[str, ...],
    lines: dict[tuple[str, ...], int] | None,
    context: ReadContext,
    reading: Reading,
) -> Layer:
    def locate(written: tuple[str, ...]) -> tuple[str, int | None]:
        full = (*prefix, *written)
        return _dotted(full), (line_of_key(lines, full) if lines is not None else None)

    layer = Layer(label, kind)
    _Walker(label, True, locate, reading.problems).walk(data, context.schema, (), (), layer.entries)
    return layer


@dataclass(frozen=True, slots=True)
class _Pyproject(Source):
    table: str
    path: str | os.PathLike[str]
    optional: bool

    def read(self, context: ReadContext) -> Reading:
        label = os.fspath(self.path)
        data, lines, problem = _read_document(
            Path(label), label, "pyproject", self.optional, "toml"
        )
        reading = Reading()
        if problem is not None:
            reading.problems.append(problem)
            return reading
        if data is None:
            return reading
        parts = tuple(self.table.split("."))
        node: object = data
        for part in parts:
            node = cast("dict[str, object]", node).get(part) if isinstance(node, dict) else None
        if not isinstance(node, dict):
            if node is None and self.optional:
                return reading
            what = "has no" if node is None else "holds something that is not a table at"
            reading.problems.append(
                _problem(
                    label,
                    "pyproject",
                    f"{label!r} {what} [{self.table}] table; add it, or pass optional=True "
                    "if it may be absent",
                    file=label,
                )
            )
            return reading
        reading.layers.append(
            _document_layer(
                label,
                "pyproject",
                cast("Mapping[object, object]", node),
                parts,
                lines,
                context,
                reading,
            )
        )
        return reading

    def hint(self, context: ReadContext, path: tuple[str, ...]) -> str | None:
        key = f"{self.table}.{_dotted(context.schema.wire_path(path))}"
        return f"{key} in {os.fspath(self.path)}"


@dataclass(frozen=True, slots=True, repr=False)
class _Mapping(Source):
    data: Mapping[str, object]
    name: str

    def __repr__(self) -> str:
        # The values are not shown: a mapping may hold credentials.
        return f"cfg.mapping(<{len(self.data)} keys>, name={self.name!r})"

    def read(self, context: ReadContext) -> Reading:
        reading = Reading()

        def locate(written: tuple[str, ...]) -> tuple[str, int | None]:
            return ".".join(_printable(part) for part in written), None

        layer = Layer(self.name, "mapping")
        walker = _Walker(self.name, False, locate, reading.problems)
        walker.walk(
            cast("Mapping[object, object]", self.data), context.schema, (), (), layer.entries
        )
        reading.layers.append(layer)
        return reading

    def hint(self, context: ReadContext, path: tuple[str, ...]) -> str | None:
        return f"{'.'.join(path)} in the mapping {self.name!r}"


# -- environment variables and .env files --------------------------------------------------------

Variables = Mapping[str, tuple[str, int | None]]


def _variable_name(
    options: ConfigOptions, field_: FieldSchema, path: tuple[str, ...]
) -> str | None:
    """The variable that sets the field at ``path``, or ``None`` when it has no name."""
    if field_.env:
        return field_.env
    if len(path) > 1 and options.env_nested is None:
        return None
    return options.env_prefix + (options.env_nested or "").join(name.upper() for name in path)


def _read_variables(
    variables: Variables,
    label: str,
    kind: OriginKind,
    context: ReadContext,
    forbid_unknown: bool,
) -> Reading:
    reading = Reading()
    folded: dict[str, list[tuple[str, str, int | None]]] = {}
    for name, (value, line) in variables.items():
        folded.setdefault(name.upper(), []).append((name, value, line))
    layer = Layer(label, kind)
    used: set[str] = set()
    known: set[str] = set()

    def visit(section: SectionSchema, prefix: tuple[str, ...], ancestors: tuple[type, ...]) -> None:
        for field_ in section.fields.values():
            path = (*prefix, field_.name)
            name = _variable_name(context.options, field_, path)
            if name is not None:
                known.add(name.upper())
                hits = folded.get(name.upper())
                if hits:
                    used.add(name.upper())
                    _take(hits, field_, path, layer, reading, label, kind, context)
            if field_.section is not None and field_.section not in ancestors:
                visit(section_schema(field_.section), path, (*ancestors, field_.section))

    visit(context.schema, (), (context.cls,))
    if forbid_unknown:
        _report_unknown(folded, used, known, label, context, reading)
    reading.layers.append(layer)
    return reading


def _take(
    hits: list[tuple[str, str, int | None]],
    field_: FieldSchema,
    path: tuple[str, ...],
    layer: Layer,
    reading: Reading,
    label: str,
    kind: OriginKind,
    context: ReadContext,
) -> None:
    if len({value for _, value, _ in hits}) > 1:
        names = " and ".join(repr(name) for name, _, _ in hits)
        reading.problems.append(
            _problem(
                label,
                kind,
                f"{label}: the variables {names} differ only in case and hold different values, "
                "so it is not clear which is meant; keep one",
                key=hits[0][0],
            )
        )
        return
    name, text, line = hits[0]
    try:
        text.encode("utf-8")
    except UnicodeEncodeError:
        # Neither the value nor the offending character is shown, only which variable it is.
        where = f"{label}:{line}: " if line is not None else ""
        reading.problems.append(
            _problem(
                label,
                kind,
                f"{where}the value of {name} is not valid text (it holds bytes that are not "
                "UTF-8); set the variable again with text",
                key=name,
                line=line,
            )
        )
        return
    parsed: object = text
    if field_.text == "json" or (field_.text == "either" and text.lstrip().startswith(("[", "{"))):
        try:
            parsed = json_decode(text.encode())
        except ValidationError as error:
            reading.problems.append(_json_problem(label, kind, name, line, error, field_))
            return
    if isinstance(parsed, dict) and field_.section is not None:

        def locate(_: tuple[str, ...]) -> tuple[str, int | None]:
            return name, line

        walker = _Walker(label, True, locate, reading.problems)
        nested = cast("Mapping[object, object]", parsed)
        if not nested:
            layer.entries.append(Entry(path, {}, name, line))
        walker.walk(nested, section_schema(field_.section), path, (), layer.entries)
    else:
        layer.entries.append(Entry(path, cast("object", parsed), name, line))


def _json_problem(
    label: str,
    kind: OriginKind,
    name: str,
    line: int | None,
    error: ValidationError,
    field_: FieldSchema,
) -> Problem:
    first = error.errors()[0]
    column = first["ctx"].get("column")
    example = '\'{"key": "value"}\'' if field_.section is not None else '\'["a", "b"]\''
    where = f"{label}:{line}: " if line is not None else ""
    return _problem(
        label,
        kind,
        f"{where}{name} holds text that is not valid JSON: {_json_reason(first)}; "
        f"a composite value is written as JSON, such as {example}",
        key=name,
        line=line,
        column=column,
    )


def _report_unknown(
    folded: dict[str, list[tuple[str, str, int | None]]],
    used: set[str],
    known: set[str],
    label: str,
    context: ReadContext,
    reading: Reading,
) -> None:
    prefix = context.options.env_prefix.upper()
    nested = context.options.env_nested
    for upper, items in folded.items():
        if upper in used or not upper.startswith(prefix):
            continue
        rest = upper[len(prefix) :]
        guess = tuple(part.lower() for part in (rest.split(nested.upper()) if nested else [rest]))
        close = difflib.get_close_matches(upper, sorted(known), n=1)
        hint = f" (did you mean {close[0]}?)" if close else ""
        for name, _, line in items:
            where = f"{label}:{line}: " if line is not None else ""
            reading.unknown.append(
                Problem(
                    label,
                    "unknown_field",
                    f"{where}unknown variable {name}: no field reads it{hint}; "
                    "fix the name or remove it",
                    {"key": name, "field": name},
                    guess,
                )
            )


@dataclass(frozen=True, slots=True, repr=False)
class _Env(Source):
    environ: Mapping[str, str] | None
    forbid_unknown: bool

    def __repr__(self) -> str:
        # The variables are not shown: they may hold credentials.
        shown = "os.environ" if self.environ is None else f"<{len(self.environ)} variables>"
        return f"cfg.env(environ={shown}, forbid_unknown={self.forbid_unknown})"

    def read(self, context: ReadContext) -> Reading:
        _check_prefix(self.forbid_unknown, context)
        source = os.environ if self.environ is None else self.environ
        variables: Variables = {name: (value, None) for name, value in source.items()}
        return _read_variables(variables, "env", "env", context, self.forbid_unknown)

    def hint(self, context: ReadContext, path: tuple[str, ...]) -> str | None:
        field_ = _field_at(context.schema, path)
        name = _variable_name(context.options, field_, path) if field_ else None
        return f"{name} in the environment" if name else None


@dataclass(frozen=True, slots=True)
class _Dotenv(Source):
    paths: tuple[str | os.PathLike[str], ...]
    optional: bool
    forbid_unknown: bool

    def read(self, context: ReadContext) -> Reading:
        _check_prefix(self.forbid_unknown, context)
        reading = Reading()
        for path in self.paths:
            label = os.fspath(path)
            one = self._read_file(Path(label), label, context)
            reading.layers.extend(one.layers)
            reading.problems.extend(one.problems)
            reading.unknown.extend(one.unknown)
        return reading

    def _read_file(self, path: Path, label: str, context: ReadContext) -> Reading:
        try:
            text = path.read_bytes().decode("utf-8")
        except FileNotFoundError:
            reading = Reading()
            if not self.optional:
                reading.problems.append(
                    _problem(
                        label,
                        "dotenv",
                        f"dotenv file {label!r} not found (looked for {path.resolve()}); create "
                        "it, fix the path, or pass optional=True if it may be absent",
                        file=label,
                    )
                )
            return reading
        except (OSError, UnicodeDecodeError) as error:
            reading = Reading()
            reason = (
                f"is not valid UTF-8 (byte {error.start})"
                if isinstance(error, UnicodeDecodeError)
                else f"cannot be read: {error.strerror or type(error).__name__}"
            )
            reading.problems.append(
                _problem(
                    label,
                    "dotenv",
                    f"dotenv file {label!r} {reason}; check the path and save it as UTF-8",
                    file=label,
                )
            )
            return reading
        values, faults = parse_dotenv(text)
        if faults:
            reading = Reading()
            for fault in faults:
                reading.problems.append(
                    _problem(
                        label,
                        "dotenv",
                        f"{label}:{fault.line}: {fault.message}",
                        file=label,
                        line=fault.line,
                    )
                )
            return reading
        variables: Variables = {name: (item.value, item.line) for name, item in values.items()}
        return _read_variables(variables, label, "dotenv", context, self.forbid_unknown)

    def hint(self, context: ReadContext, path: tuple[str, ...]) -> str | None:
        field_ = _field_at(context.schema, path)
        name = _variable_name(context.options, field_, path) if field_ else None
        return f"{name} in {os.fspath(self.paths[0])}" if name and self.paths else None


def _check_prefix(forbid_unknown: bool, context: ReadContext) -> None:
    if forbid_unknown and not context.options.env_prefix:
        raise ValueError(
            f"forbid_unknown=True needs an env_prefix on {context.cls.__qualname__}: without "
            "one, every variable of the process would be unknown; set "
            "@ox.config(env_prefix='APP_') or drop forbid_unknown"
        )


def _field_at(schema: SectionSchema, path: tuple[str, ...]) -> FieldSchema | None:
    current: SectionSchema | None = schema
    found: FieldSchema | None = None
    for name in path:
        if current is None:
            return None
        found = current.fields.get(name)
        if found is None:
            return None
        current = section_schema(found.section) if found.section else None
    return found


# -- the public constructors ---------------------------------------------------------------------


def file(
    path: str | os.PathLike[str],
    *,
    optional: bool = False,
    format: Literal["toml", "json"] | None = None,  # noqa: A002
) -> Source:
    """A TOML or JSON file (by its extension, or ``format=``), read with wire names. A missing
    file is a ``config_source`` problem unless ``optional``."""
    if format not in (None, "toml", "json"):
        raise ValueError(
            f"format is 'toml' or 'json' (or None to use the extension), not {format!r}"
        )
    return _File(path, optional, format)


def pyproject(
    table: str,
    *,
    path: str | os.PathLike[str] = "pyproject.toml",
    optional: bool = True,
) -> Source:
    """A table of ``pyproject.toml`` given as a dotted path (``"tool.myapp"``). With
    ``optional`` a missing file or table is an empty layer."""
    if not isinstance(table, str) or not table or any(not part for part in table.split(".")):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise ValueError(
            f"table is a dotted path such as 'tool.myapp', not {table!r}; "
            "name the table of pyproject.toml that holds the settings"
        )
    return _Pyproject(table, path, optional)


def dotenv(
    *paths: str | os.PathLike[str], optional: bool = False, forbid_unknown: bool = False
) -> Source:
    """One or more ``.env`` files, one layer each (later paths win), with the names of
    ``cfg.env``. ``os.environ`` is not changed."""
    if not paths:
        raise ValueError(
            "dotenv needs at least one path, such as cfg.dotenv('.env', optional=True)"
        )
    return _Dotenv(paths, optional, forbid_unknown)


def env(*, environ: Mapping[str, str] | None = None, forbid_unknown: bool = False) -> Source:
    """Environment variables: ``environ`` if given, else ``os.environ``. With
    ``forbid_unknown``, a variable that starts with the config's ``env_prefix`` and names no
    field is a problem."""
    return _Env(environ, forbid_unknown)


def mapping(data: Mapping[str, object], *, name: str = "mapping") -> Source:
    """Overrides coming from code, with Python names and nested mappings for sections."""
    if not isinstance(data, Mapping):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise TypeError(
            f"cfg.mapping takes a mapping such as {{'server': {{'port': 9000}}}}, not "
            f"{type(data).__name__}"
        )
    if not isinstance(name, str) or not name:  # pyright: ignore[reportUnnecessaryIsInstance]
        raise ValueError(f"name is the text errors call this source, such as 'cli', not {name!r}")
    if name in _RESERVED_NAMES:
        raise ValueError(
            f"name {name!r} is how errors and origins call {_RESERVED_NAMES[name]}; "
            "choose another, such as 'cli' or 'overrides'"
        )
    return _Mapping(dict(data), name)
