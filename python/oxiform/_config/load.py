"""``cfg.load``: reads the sources, merges them, validates once and says where each problem
came from (spec sections 6.3 to 6.5)."""

import difflib
import json
from collections.abc import Iterable, Sequence
from typing import Any, TypeVar, cast

from oxiform._config.merge import Tree, merge, origins_of, resolve, to_wire, unencodable
from oxiform._config.origin import Origin, recall, remember, where
from oxiform._config.schema import (
    SectionSchema,
    check_declaration,
    config_options,
    section_schema,
)
from oxiform._config.sources import Problem, ReadContext, Source, env
from oxiform._oxiform import Limits, json_decode, json_encode
from oxiform.errors import DecodeError, EncodeError, ErrorDetails, ValidationError

__all__ = ["dump", "load", "origin", "origins"]

_T = TypeVar("_T")

_HINTS_SHOWN = 4


def load(
    cls: type[_T],
    sources: Sequence[Source] | None = None,
    *,
    max_errors: int = 100,
    limits: Limits | None = None,
) -> _T:
    """Reads ``sources`` (the last one wins; ``None`` is the environment alone) and returns
    the validated config. Raises ``ox.DecodeError`` when a source cannot be read and
    ``ox.ValidationError`` when the values do not fit; both say which source and key."""
    outcome = _prepare(cls, sources, max_errors, limits)
    # Raised here, and not where it was made: the frames that held the document, the merged
    # tree and every layer are gone by now, so a traceback capture (Sentry, rich, pytest
    # --showlocals) does not find the values of the sources in them.
    if outcome.error is not None:
        raise outcome.error
    return cast("_T", outcome.settings)


class _Outcome:
    """The config or the error ``_prepare`` made, returned and not raised (see ``load``)."""

    __slots__ = ("error", "settings")

    def __init__(self, settings: object = None, error: ValidationError | None = None) -> None:
        self.settings = settings
        self.error = error


def _prepare(
    cls: type, sources: Sequence[Source] | None, max_errors: int, limits: Limits | None
) -> _Outcome:
    options = config_options(cls)
    if options is None:
        raise TypeError(
            f"cfg.load needs a class made by @ox.config, not {_name(cls)}; "
            "declare it with @ox.config (a model made by @ox.model is not a config)"
        )
    check_declaration(cls)
    schema = section_schema(cls)
    chosen = _sources(sources)
    context = ReadContext(cls, schema, options)
    readings = [source.read(context) for source in chosen]
    unreadable = [problem for reading in readings for problem in reading.problems]
    if unreadable:
        return _Outcome(error=_unreadable(unreadable, cls, max_errors))
    layers = [layer for reading in readings for layer in reading.layers]
    tree = merge(layers)
    kinds = {layer.label: layer.kind for layer in layers}
    unknown = [problem for reading in readings for problem in reading.unknown]
    # Errors are caught into variables and turned into reports after the `except` blocks, so
    # that the report does not carry them (and their frames) as its context.
    unwritable: Exception | None = None
    document = b""
    try:
        document = json_encode(to_wire(tree, schema))
    except (EncodeError, UnicodeEncodeError) as error:
        unwritable = error
    if unwritable is not None:
        pointer = unwritable.pointer if isinstance(unwritable, EncodeError) else None
        return _Outcome(error=_unwritable(pointer, tree, schema, cls, max_errors))
    failure: ValidationError | None = None
    settings: object = None
    try:
        settings = cast(
            "object", json_decode(document, cls, mode="lax", max_errors=max_errors, limits=limits)
        )
    except ValidationError as error:
        failure = error
    if failure is not None or unknown:
        located = _located(failure, unknown, tree, context, chosen, kinds, max_errors)
        return _Outcome(error=located)
    remember(settings, origins_of(tree, schema))
    return _Outcome(settings)


def _sources(sources: object) -> tuple[Source, ...]:
    if sources is None:
        return (env(),)
    hint = "pass a list such as [cfg.file('app.toml', optional=True), cfg.env()]"
    if isinstance(sources, Source) or isinstance(sources, (str, bytes)):
        raise TypeError(f"sources is a list of sources, not {type(sources).__name__}; {hint}")
    if not isinstance(sources, Iterable):
        raise TypeError(f"sources is a list of sources, not {type(sources).__name__}; {hint}")
    chosen = tuple(cast("Iterable[object]", sources))
    for item in chosen:
        if not isinstance(item, Source):
            raise TypeError(
                f"sources holds {item!r}, which is not a source; make one with cfg.file, "
                "cfg.pyproject, cfg.dotenv, cfg.env or cfg.mapping"
            )
    return cast("tuple[Source, ...]", chosen)


# -- errors ----------------------------------------------------------------------------------------


def _pointer(path: Iterable[str | int]) -> str:
    return "".join("/" + str(part).replace("~", "~0").replace("/", "~1") for part in path)


def _detail(code: str, path: list[str | int], message: str, ctx: dict[str, object]) -> ErrorDetails:
    return ErrorDetails(code=code, path=path, pointer=_pointer(path), message=message, ctx=ctx)


def _combine(
    parts: dict[str, list[ErrorDetails]],
    part_class: type[ValidationError],
    formats: dict[str, str],
    truncated: bool,
    cls: type,
    max_errors: int,
) -> ValidationError:
    """One error out of the problems of each source, with the source as first path segment."""
    made = {
        label: part_class(errors, truncated=truncated, target=_name(cls), format=formats[label])
        for label, errors in parts.items()
    }
    return ValidationError.combine(made, max_errors=max_errors, target=_name(cls))


def _unreadable(problems: list[Problem], cls: type, max_errors: int) -> ValidationError:
    parts: dict[str, list[ErrorDetails]] = {}
    formats: dict[str, str] = {}
    for problem in problems:
        parts.setdefault(problem.label, []).append(
            _detail(problem.code, [], problem.message, dict(problem.ctx))
        )
        formats[problem.label] = str(problem.ctx.get("source", "config"))
    return _combine(parts, DecodeError, formats, False, cls, max_errors)


def _unwritable(
    pointer: str | None, tree: Tree, schema: SectionSchema, cls: type, max_errors: int
) -> ValidationError:
    """A value that cannot be written as JSON (``nan``, an unsupported object, text with
    bytes that are not UTF-8). Nothing of the value is shown."""
    wire = [part.replace("~1", "/").replace("~0", "~") for part in (pointer or "").split("/")[1:]]
    python, leaf = resolve(tree, schema, cast("list[str | int]", wire))
    if pointer is None:
        found = unencodable(tree)
        python, leaf = (found[0], found[1]) if found is not None else ((), None)
    label = leaf.origin.source if leaf else "default"
    source = f" in {where(leaf.origin)}" if leaf else ""
    what = f"{'.'.join(python)}{source}" if python else "a value"
    message = (
        f"{what} cannot be used: it has no JSON form (NaN, infinity, an unsupported object, or "
        "text that is not valid UTF-8); give it a value the field's type reads"
    )
    problem = _detail("config_source", [], message, {"key": ".".join(python)})
    kind = leaf.origin.kind if leaf else "config"
    return _combine({label: [problem]}, DecodeError, {label: kind}, False, cls, max_errors)


def _located(
    failure: ValidationError | None,
    unknown: list[Problem],
    tree: Tree,
    context: ReadContext,
    sources: tuple[Source, ...],
    kinds: dict[str, str],
    max_errors: int,
) -> ValidationError:
    """The problems of one decode, each moved under the source that set the value and saying
    where it came from."""
    parts: dict[str, list[ErrorDetails]] = {}
    formats: dict[str, str] = {}
    problems = failure.errors() if failure is not None else []
    for problem in problems:
        python, leaf = resolve(tree, context.schema, problem["path"])
        origin = leaf.origin if leaf is not None else None
        label = origin.source if origin is not None else "default"
        formats.setdefault(label, kinds.get(label, "default"))
        parts.setdefault(label, []).append(_explain(problem, python, origin, context, sources))
    for extra in unknown:
        formats.setdefault(extra.label, kinds.get(extra.label, "env"))
        detail = _detail(extra.code, list(extra.path), extra.message, dict(extra.ctx))
        parts.setdefault(extra.label, []).append(detail)
    part_class = type(failure) if failure is not None else ValidationError
    truncated = failure.truncated if failure is not None else False
    return _combine(parts, part_class, formats, truncated, context.cls, max_errors)


def _explain(
    problem: ErrorDetails,
    python: tuple[str, ...],
    origin: Origin | None,
    context: ReadContext,
    sources: tuple[Source, ...],
) -> ErrorDetails:
    key = ".".join(python)
    message = problem["message"]
    code = problem["code"]
    if code == "missing" and origin is None:
        message = _missing(python, context, sources)
    elif code == "unknown_field":
        message = _unknown(python, context)
    if origin is not None:
        message = f"{message} (from {where(origin)})"
    ctx: dict[str, object] = {**problem["ctx"], "key": key}
    if origin is not None:
        ctx["origin"] = str(origin)
    explained = ErrorDetails(
        code=code,
        path=problem["path"],
        pointer=problem["pointer"],
        message=message,
        ctx=ctx,
    )
    if "input" in problem:
        explained["input"] = problem["input"]
    return explained


def _missing(python: tuple[str, ...], context: ReadContext, sources: tuple[Source, ...]) -> str:
    hints = [hint for source in sources if (hint := source.hint(context, python))]
    if not hints:
        return f"field required: no source sets {'.'.join(python)}"
    shown = "; ".join(hints[:_HINTS_SHOWN]) + ("; ..." if len(hints) > _HINTS_SHOWN else "")
    return f"field required: no source sets {'.'.join(python)}; set {shown}"


def _unknown(python: tuple[str, ...], context: ReadContext) -> str:
    *parents, name = python
    section: SectionSchema | None = context.schema
    for parent in parents:
        field = section.fields.get(parent) if section is not None else None
        section = section_schema(field.section) if field and field.section else None
    close = difflib.get_close_matches(name, sorted(section.by_key) if section else [], n=1)
    hint = f" (did you mean {close[0]!r}?)" if close else ""
    return f"unknown key {name!r}{hint}; fix the name, or remove it"


# -- origins and dump ------------------------------------------------------------------------------


def _name(cls: object) -> str:
    return getattr(cls, "__qualname__", repr(cls))


def origins(settings: object) -> dict[str, Origin]:
    """The origin of every field of ``settings``, by dotted Python path."""
    found = recall(settings)
    if found is None:
        raise ValueError(
            f"{type(settings).__name__} was not made by cfg.load, so it has no origins; "
            "(a copy, a pickle and ox.replace do not keep them) ask the object cfg.load returned"
        )
    return dict(found)


def origin(settings: object, path: str) -> Origin:
    """The origin of the field at the dotted Python ``path`` (``"db.pool_size"``)."""
    found = origins(settings)
    if path in found:
        return found[path]
    below = [name for name in found if name.startswith(path + ".")]
    if below:
        raise ValueError(
            f"{path!r} is a section, so ask for one of its fields: {', '.join(below[:4])}"
        )
    close = difflib.get_close_matches(path, found, n=3)
    hint = f"; did you mean {', '.join(repr(name) for name in close)}?" if close else ""
    raise ValueError(
        f"{type(settings).__name__} has no setting {path!r}{hint} "
        "(use the dotted Python names, as in 'db.pool_size')"
    )


def dump(settings: object) -> dict[str, Any]:
    """The config as plain data with wire names, as ``ox.json.encode`` writes it: secrets
    are ``"********"``."""
    return cast("dict[str, Any]", json.loads(json_encode(settings)))
