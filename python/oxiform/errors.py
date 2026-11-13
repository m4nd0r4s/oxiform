"""Exceptions raised by oxiform."""

from collections.abc import Iterable, Mapping, Sequence
from typing import NotRequired, Self, TypedDict, cast

from oxiform._problem import check_status, status_title

_UNREADABLE_CODES = frozenset({"syntax", "limit_exceeded", "config_source"})
"""The codes of a ``DecodeError``: the input could not be read at all."""

__all__ = [
    "DecodeError",
    "EncodeError",
    "ErrorDetails",
    "Invalid",
    "OxiformError",
    "SchemaError",
    "ValidationError",
]


class OxiformError(Exception):
    """Base class of every oxiform exception."""


class SchemaError(OxiformError, TypeError):
    """A type declaration oxiform cannot use, found when its plan is compiled."""


class EncodeError(OxiformError):
    """An object that cannot be written in the requested format."""

    def __init__(self, message: str, pointer: str = "") -> None:
        super().__init__(message)
        self.pointer = pointer
        """Where the object is, as a JSON Pointer from the value passed to ``encode``."""


class Invalid(OxiformError, ValueError):
    """What a validator raises to reject a value: one problem of the decode, with its own
    message and, if it wants, its own ``code`` and ``path`` (spec section 4.3).

    ``path`` is one segment (``"password2"``, ``0``) or a sequence of them, relative to the value
    the validator checks. ``ValueError`` and ``TypeError`` raised in a validator are the same
    problem with the code ``"invalid"``; raise ``Invalid`` for a code of your own, a path, or a
    message written for the client that stays when the value is a secret.
    """

    def __init__(
        self,
        message: str,
        *,
        code: str = "invalid",
        path: str | int | Sequence[str | int] | None = None,
    ) -> None:
        if not isinstance(message, str):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise TypeError(
                f"ox.Invalid takes the message as a str, not {type(message).__name__}; "
                'write raise ox.Invalid("what is wrong, and what to do")'
            )
        if not isinstance(code, str) or not code:  # pyright: ignore[reportUnnecessaryIsInstance]
            raise TypeError(
                f"ox.Invalid code must be a non-empty str, not {code!r}; "
                'use a short stable name such as code="reserved", or leave it out for "invalid"'
            )
        super().__init__(message)
        self.message = message
        self.code = code
        self.path = _segments(path)
        """The segments of the path, relative to the checked value; empty for the value itself."""


def _segments(path: object) -> tuple[str | int, ...]:
    if path is None:
        return ()
    if isinstance(path, (str, int)):
        path = (path,)
    elif not isinstance(path, Sequence):
        raise TypeError(
            f"ox.Invalid path must be a str, an int or a sequence of them, not "
            f"{type(path).__name__}; write path='password2' or path=('items', 0)"
        )
    segments: list[str | int] = []
    for segment in cast("Sequence[object]", path):
        if isinstance(segment, bool) or not isinstance(segment, (str, int)):
            raise TypeError(
                f"ox.Invalid path segments are str keys or int indices, not {segment!r}"
            )
        if isinstance(segment, int) and segment < 0:
            raise TypeError(f"ox.Invalid path indices start at 0, not {segment}")
        segments.append(segment)
    return tuple(segments)


class ErrorDetails(TypedDict):
    """One problem in the input. This shape is public API; the message text is not."""

    code: str
    """Stable error code, such as ``"too_short"``."""
    path: list[str | int]
    """Keys (as written in the input) and indices leading to the value."""
    pointer: str
    """The path as a JSON Pointer: ``"/addresses/0/street"``."""
    message: str
    input: NotRequired[str]
    """The received value as text, truncated to 80 characters."""
    ctx: dict[str, object]


class ValidationError(OxiformError, ValueError):
    """The input does not match the type. Every problem found is in ``errors()``."""

    def __init__(
        self,
        errors: list[ErrorDetails],
        *,
        truncated: bool = False,
        target: str = "value",
        format: str = "python",
    ) -> None:
        super().__init__(errors)
        self._errors = errors
        self.truncated = truncated
        """Whether collection stopped at ``max_errors``."""
        self.target = target
        self.format = format

    def errors(self) -> list[ErrorDetails]:
        """Every problem, in the order found."""
        return [ErrorDetails(**error) for error in self._errors]

    def by_path(self) -> dict[str, list[ErrorDetails]]:
        """Problems grouped by JSON Pointer, for forms."""
        grouped: dict[str, list[ErrorDetails]] = {}
        for error in self.errors():
            grouped.setdefault(error["pointer"], []).append(error)
        return grouped

    def by_source(self) -> dict[str, list[ErrorDetails]]:
        """Problems grouped by their first path segment, which for an error made by
        ``combine`` or ``with_source`` is the source: ``{"body": [...], "query": [...]}``."""
        grouped: dict[str, list[ErrorDetails]] = {}
        for error in self.errors():
            first = error["path"][0] if error["path"] else ""
            grouped.setdefault(str(first), []).append(error)
        return grouped

    def unreadable_sources(self) -> list[str]:
        """The sources, in order, whose input could not be read at all: those with a problem coded
        ``syntax``, ``limit_exceeded`` or ``config_source`` (what a ``DecodeError`` holds). Meant
        for an error made by ``combine``, to pick the status of the response: see the spec."""
        return [
            source
            for source, errors in self.by_source().items()
            if any(error["code"] in _UNREADABLE_CODES for error in errors)
        ]

    def with_source(self, source: str) -> Self:
        """The same problems found in ``source`` (``"body"``, ``"query"``, ``"path"``,
        ``"header"``, ``"cookie"``): the source becomes the first path segment and so the first
        part of every pointer, as in ``/query/page``. Codes, messages, ``input`` and ``ctx`` are
        kept, and so is the class: a ``DecodeError`` stays one. The error itself is not changed.
        """
        _check_source(source)
        return type(self)(
            [_in_source(error, source) for error in self._errors],
            truncated=self.truncated,
            target=self.target,
            format=self.format,
        )

    @classmethod
    def combine(
        cls,
        parts: Mapping[str, "ValidationError"] | Iterable[tuple[str, "ValidationError"]],
        *,
        max_errors: int = 100,
        target: str = "request",
    ) -> "ValidationError":
        """One error for the input of several sources, such as the parts of a request.

        ``parts`` maps a source name to the error its decode call raised (or is an iterable of
        ``(source, error)`` pairs, in which a source may repeat). Problems keep the order of the
        parts and their own order inside a part; each path starts with its source, and codes,
        messages, ``input`` and ``ctx`` are kept. At most ``max_errors`` problems are kept, the
        first ones, and ``truncated`` is set when one was dropped or a part was itself truncated.
        The budget is first come, first served, so pass the parts in order of priority (path,
        query, then body). The result is a ``DecodeError`` only when every part is one; with no
        parts it is an empty ``ValidationError``. Raise the result, or call ``to_problem()`` on
        it for a single RFC 9457 response, and use ``unreadable_sources()`` to pick its status.
        """
        limit = _max_errors(max_errors)
        pairs = _pairs(parts)
        errors: list[ErrorDetails] = []
        truncated = False
        formats: list[str] = []
        for source, part in pairs:
            _check_source(source)
            if not isinstance(part, ValidationError):  # pyright: ignore[reportUnnecessaryIsInstance]
                raise TypeError(
                    f"combine() takes ValidationError objects, but the part for {source!r} is "
                    f"{type(part).__name__}; pass the error a decode call raised"
                )
            truncated |= part.truncated
            if part.format not in formats:
                formats.append(part.format)
            for error in part._errors:  # noqa: SLF001
                if len(errors) < limit:
                    errors.append(_in_source(error, source))
                else:
                    truncated = True
        every_part_is_a_decode_error = bool(pairs) and all(
            isinstance(part, DecodeError) for _, part in pairs
        )
        result = DecodeError if every_part_is_a_decode_error else ValidationError
        return result(errors, truncated=truncated, target=target, format=", ".join(formats))

    def to_problem(
        self,
        *,
        status: int = 422,
        type: str = "about:blank",  # noqa: A002
        title: str | None = None,
        instance: str | None = None,
    ) -> dict[str, object]:
        """An RFC 9457 problem details object (``application/problem+json``).

        ``status`` is the HTTP status the response is sent with, ``type`` the URI that names the
        kind of problem, ``instance`` a URI for this occurrence (left out when ``None``) and
        ``title`` defaults to the standard phrase of ``status``. ``errors`` lists every problem
        as ``{"pointer", "code", "detail"}``.
        """
        status = check_status(status)
        _check_text("type", type, "the URI that names the kind of problem, or leave the default")
        if title is not None:
            _check_text("title", title, "a short summary of the kind of problem, or omit it")
        if instance is not None:
            _check_text("instance", instance, "a URI for this occurrence, or omit it")
        problem: dict[str, object] = {
            "type": type,
            "title": title if title is not None else status_title(status),
            "status": status,
            "detail": self._summary(),
        }
        if instance is not None:
            problem["instance"] = instance
        problem["errors"] = [
            {"pointer": error["pointer"], "code": error["code"], "detail": error["message"]}
            for error in self._errors
        ]
        return problem

    def _summary(self) -> str:
        count = len(self._errors)
        noun = "error" if count == 1 else "errors"
        more = " (stopped at the maximum)" if self.truncated else ""
        shown_format = f" ({self.format})" if self.format else ""
        return f"{count} {noun} validating {self.target}{shown_format}{more}"

    def __str__(self) -> str:
        lines = [self._summary() + ":"]
        width = max((len(error["pointer"] or "/") for error in self._errors), default=1)
        for error in self._errors:
            pointer = error["pointer"] or "/"
            received_input = error.get("input")
            shown = received_input is not None and error["code"] == "type"
            received = f" {received_input!r}" if shown else ""
            lines.append(f"  {pointer:<{width}}  {error['message']}{received}  [{error['code']}]")
        return "\n".join(lines)


class DecodeError(ValidationError):
    """The input could not be read: it is malformed, or a limit was exceeded."""


def _pairs(parts: object) -> list[tuple[str, ValidationError]]:
    hint = "pass a mapping of source to ValidationError, or (source, error) pairs"
    if isinstance(parts, Mapping):
        return list(cast("Mapping[str, ValidationError]", parts).items())
    if isinstance(parts, (str, bytes)) or not isinstance(parts, Iterable):
        raise TypeError(f"combine() cannot read parts of type {type(parts).__name__}; {hint}")
    pairs: list[tuple[str, ValidationError]] = []
    for item in cast("Iterable[object]", parts):
        if not isinstance(item, (tuple, list)) or len(cast("Sequence[object]", item)) != 2:
            raise TypeError(
                f"combine() found {item!r} where a (source, error) pair belongs; {hint}"
            )
        source, part = cast("Sequence[object]", item)
        pairs.append((cast("str", source), cast("ValidationError", part)))
    return pairs


def _check_text(name: str, value: object, meaning: str) -> None:
    if not isinstance(value, str) or not value:
        raise ValueError(f"{name} must be a non-empty string, not {value!r}; pass {meaning}")


def _check_source(source: object) -> None:
    if not isinstance(source, str) or not source:
        raise ValueError(
            f'a source must be a non-empty string such as "body" or "query", not {source!r}'
        )


def _max_errors(value: object) -> int:
    largest = 2**64 - 1
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"max_errors must be a whole number, not {type(value).__name__}")
    if value == 0:
        raise ValueError(
            "max_errors must be at least 1, not 0; use max_errors=1 to stop at the first error"
        )
    if not 1 <= value <= largest:
        raise ValueError(f"max_errors must be a whole number from 1 to {largest}, not {value}")
    return value


def _escape_pointer_segment(segment: str) -> str:
    """One path segment as a JSON Pointer reference token (RFC 6901)."""
    return segment.replace("~", "~0").replace("/", "~1")


def _in_source(error: ErrorDetails, source: str) -> ErrorDetails:
    """A copy of ``error`` with ``source`` as the first segment of its path and pointer."""
    # The keys stay in the order the engine writes them: input before ctx.
    moved: dict[str, object] = {
        "code": error["code"],
        "path": [source, *error["path"]],
        "pointer": "/" + _escape_pointer_segment(source) + error["pointer"],
        "message": error["message"],
    }
    if "input" in error:
        moved["input"] = error["input"]
    moved["ctx"] = {key: _context_in_source(value, source) for key, value in error["ctx"].items()}
    return cast("ErrorDetails", moved)


def _context_in_source(value: object, source: str) -> object:
    """The issues of the union variants kept in a context move with their parent."""
    if not isinstance(value, dict):
        return value
    by_variant = cast("dict[str, object]", value)
    if not by_variant or not all(_is_error_list(item) for item in by_variant.values()):
        return value  # pyright: ignore[reportUnknownVariableType]
    moved: dict[str, object] = {}
    for name, errors in by_variant.items():
        moved[name] = [_in_source(error, source) for error in cast("list[ErrorDetails]", errors)]
    return moved


def _is_error_list(value: object) -> bool:
    if not isinstance(value, list) or not value:
        return False
    items = cast("list[object]", value)
    return all(
        isinstance(item, dict)
        and {"code", "path", "pointer"} <= cast("dict[str, object]", item).keys()
        for item in items
    )
