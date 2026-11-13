"""``ox.Context[T]``: the type of the validation context a validator asks for.

``context=`` of a decode is any object, and oxiform hands it to every validator that has a
parameter annotated ``ox.Context``, as it is. To a type checker ``Context[T]`` is ``T`` (and
``Context`` alone is ``Mapping[str, Any]``); at run time it is ``Annotated[T, CONTEXT]``, and the
marker is how oxiform finds the parameter: by its annotation, never by its name.
"""

import types
import typing
from collections.abc import Mapping
from typing import TYPE_CHECKING, Annotated, Any, Final, TypeAlias

__all__ = ["CONTEXT", "Context", "is_context_annotation"]


class _ContextMarker:
    """The metadata that marks an annotation as the validation context."""

    __slots__ = ()

    def __repr__(self) -> str:
        return "ox.Context"


CONTEXT: Final = _ContextMarker()

if TYPE_CHECKING:
    # PEP 696: a bare ``ox.Context`` is ``Mapping[str, Any]``. The default lives in
    # ``typing_extensions`` on the Python versions oxiform supports before 3.13, and the run-time
    # alias below does not need it.
    from typing_extensions import TypeVar

    _C = TypeVar("_C", default=Mapping[str, Any])
else:
    _C = typing.TypeVar("_C")

Context: TypeAlias = Annotated[_C, CONTEXT]
"""The annotation of the parameter of a validator that receives the validation context:
``def check(cls, value, ctx: ox.Context[Tenant]) -> ...``. ``Tenant`` is any type, usually a
``TypedDict`` or a class of the host."""


def is_context_annotation(annotation: object) -> bool:
    """Whether ``annotation`` is ``ox.Context``, ``ox.Context[T]``, or either of them or ``None``
    (a context that may be absent)."""
    if typing.get_origin(annotation) is Annotated:
        return any(item is CONTEXT for item in typing.get_args(annotation)[1:])
    if _is_union(annotation):
        arguments = typing.get_args(annotation)
        present = [argument for argument in arguments if argument is not type(None)]
        return len(present) == 1 and len(arguments) == 2 and is_context_annotation(present[0])
    return False


def _is_union(annotation: object) -> bool:
    return typing.get_origin(annotation) in (typing.Union, types.UnionType)
