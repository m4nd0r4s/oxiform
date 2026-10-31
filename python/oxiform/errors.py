"""Exceptions raised by oxiform."""

from typing import NotRequired, TypedDict

__all__ = [
    "DecodeError",
    "ErrorDetails",
    "OxiformError",
    "SchemaError",
    "ValidationError",
]


class OxiformError(Exception):
    """Base class of every oxiform exception."""


class SchemaError(OxiformError, TypeError):
    """A type declaration oxiform cannot use, found when its plan is compiled."""


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

    def to_problem(self) -> dict[str, object]:
        """An RFC 9457 problem details object (``application/problem+json``)."""
        return {
            "type": "about:blank",
            "title": "Unprocessable Content",
            "status": 422,
            "detail": self._summary(),
            "errors": [
                {"pointer": error["pointer"], "code": error["code"], "detail": error["message"]}
                for error in self._errors
            ],
        }

    def _summary(self) -> str:
        count = len(self._errors)
        noun = "error" if count == 1 else "errors"
        more = " (stopped at the maximum)" if self.truncated else ""
        return f"{count} {noun} validating {self.target} ({self.format}){more}"

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
