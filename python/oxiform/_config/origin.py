"""Where each value of a loaded config came from (spec section 6.4)."""

import threading
import weakref
from dataclasses import dataclass
from typing import Any, Literal, TypeAlias

__all__ = [
    "DEFAULT_ORIGIN",
    "Origin",
    "OriginKind",
    "forget_origins",
    "recall",
    "remember",
    "where",
]

OriginKind: TypeAlias = Literal["default", "file", "pyproject", "dotenv", "env", "mapping"]


@dataclass(frozen=True, slots=True)
class Origin:
    """The source and key that set one value of a config. It never holds the value, so a
    ``Secret`` cannot leak through it."""

    kind: OriginKind
    """``"default"``, ``"file"``, ``"pyproject"``, ``"dotenv"``, ``"env"`` or ``"mapping"``."""
    source: str
    """What the layer is called: the path of a file as it was given, ``"env"``, the ``name`` of
    a mapping, or ``"default"`` for a value no source set."""
    key: str | None = None
    """The key as the source writes it: ``APP_DB__POOL_SIZE``, the TOML key path
    ``db.pool_size``, the dotted Python path in a mapping. ``None`` for a default."""
    line: int | None = None
    """The line of the value in a TOML or dotenv file."""
    overrides: tuple[str, ...] = ()
    """The origins this one replaced, nearest layer first, as text."""

    def __str__(self) -> str:
        if self.kind == "default":
            return "default"
        if self.kind in ("env", "mapping"):
            return f"{self.source} {self.key}"
        place = f"{self.source}:{self.line}" if self.line is not None else self.source
        return f"{place} ({self.key})" if self.key else place


DEFAULT_ORIGIN = Origin("default", "default")


def where(origin: Origin) -> str:
    """The origin as an error message says it: ``env APP_PORT`` or ``a.toml:3, key server.port``."""
    if origin.kind in ("env", "mapping"):
        return f"{origin.source} {origin.key}"
    place = f"{origin.source}:{origin.line}" if origin.line is not None else origin.source
    return f"{place}, key {origin.key}" if origin.key else place


_REGISTRY: dict[int, dict[str, Origin]] = {}
# Re-entrant: a weakref callback can run while this thread holds the lock (a collection).
_LOCK = threading.RLock()


def remember(settings: object, origins: dict[str, Origin]) -> None:
    """Keeps ``origins`` for as long as ``settings`` lives (it has a weak reference slot)."""
    key = id(settings)

    def forget(_: Any) -> None:
        forget_origins(key)

    reference = weakref.ref(settings, forget)
    with _LOCK:
        _REGISTRY[key] = origins
        _REFERENCES[key] = reference


_REFERENCES: dict[int, "weakref.ref[object]"] = {}


def forget_origins(key: int) -> None:
    with _LOCK:
        _REGISTRY.pop(key, None)
        _REFERENCES.pop(key, None)


def recall(settings: object) -> dict[str, Origin] | None:
    """The origins of ``settings``, or ``None`` when ``cfg.load`` did not make it."""
    key = id(settings)
    with _LOCK:
        origins = _REGISTRY.get(key)
        reference = _REFERENCES.get(key)
    if origins is None or reference is None or reference() is not settings:
        return None
    return origins
