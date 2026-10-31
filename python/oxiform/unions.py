"""How a union of models is told apart, declared with ``Annotated``:
``Annotated[Card | Pix, ox.Tagged("type")]``."""

from dataclasses import dataclass

__all__ = ["External", "Tagged", "Untagged"]


@dataclass(frozen=True, slots=True)
class Tagged:
    """The tag is the key ``field`` of the model: ``{"type": "card", ...}``. With ``content``,
    the model goes under that key, next to the tag: ``{"type": "card", "data": {...}}``."""

    field: str
    content: str | None = None


@dataclass(frozen=True, slots=True)
class External:
    """The tag is the only key of a map that holds the model: ``{"card": {...}}``."""


@dataclass(frozen=True, slots=True)
class Untagged:
    """No tag: each variant is tried in order, and the first that fits is taken."""
