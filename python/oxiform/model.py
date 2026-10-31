"""``ox.Model``: a base class whose subclasses are compact, slotted models.

``class Point(ox.Model, frozen=True): ...`` behaves like a dataclass with ``slots=True``: no
instance ``__dict__``, so each instance is as small as a slotted dataclass, with ``__init__``,
``__repr__``, ``__eq__``, ``__match_args__``, pickling and copying. The options are class
keyword arguments: ``frozen``, ``kw_only``, ``order`` and ``eq``.
"""

import dataclasses
from typing import Any, cast, dataclass_transform

__all__ = ["Model"]

# Set on each class once it is converted. Copying a class to add slots carries the marker in
# its namespace, which tells the metaclass the copy needs no conversion.
_CONVERTED = "__oxiform_model__"


@dataclass_transform(
    eq_default=True, order_default=False, kw_only_default=False, frozen_default=False
)
class _ModelMeta(type):
    def __new__(
        mcs,
        name: str,
        bases: tuple[type, ...],
        namespace: dict[str, Any],
        /,
        *,
        frozen: bool = False,
        kw_only: bool = False,
        order: bool = False,
        eq: bool = True,
        **kwargs: Any,
    ) -> "_ModelMeta":
        cls = super().__new__(mcs, name, bases, namespace, **kwargs)
        is_base = not any(isinstance(base, _ModelMeta) for base in bases)
        if is_base or namespace.get(_CONVERTED):
            return cls
        setattr(cls, _CONVERTED, True)
        convert = dataclasses.dataclass(
            frozen=frozen, kw_only=kw_only, order=order, eq=eq, slots=True
        )
        # The decorator is typed for a plain class; here `cls` is the class being made.
        return cast("_ModelMeta", convert(cls))

    def __init__(
        cls, name: str, bases: tuple[type, ...], namespace: dict[str, Any], /, **kwargs: Any
    ) -> None:
        super().__init__(name, bases, namespace)


class Model(metaclass=_ModelMeta):
    """Base class of compact models. Subclasses are slotted dataclasses."""

    __slots__ = ()
