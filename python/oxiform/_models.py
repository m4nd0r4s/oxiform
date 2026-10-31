"""``@ox.model`` and ``ox.Model``: slotted dataclasses with oxiform options.

``@ox.model`` turns a class into a dataclass with ``slots=True``. ``class Point(ox.Model): ...``
does the same through a base class, with the options as class keyword arguments. Both keep the
oxiform options on the class and compile the plan right away, so a wrong declaration fails at
import with ``ox.SchemaError``. A plan that refers to a class not defined yet is compiled on
first use instead.
"""

import dataclasses
from collections.abc import Callable
from typing import Any, TypeVar, cast, dataclass_transform, overload

from oxiform._fields import field
from oxiform._introspection import UnresolvedAnnotationError
from oxiform._model_options import OPTIONS_ATTRIBUTE, ModelOptions, UnknownFields
from oxiform._naming import RENAME_RULES, RenameRule
from oxiform._oxiform import warmup
from oxiform.errors import SchemaError

__all__ = ["Model", "model", "replace"]

_T = TypeVar("_T")

# Set on each class once it is converted. Copying a class to add slots carries the marker in
# its namespace, which tells the metaclass the copy needs no conversion.
_CONVERTED = "__oxiform_model__"


@overload
def model(cls: type[_T], /) -> type[_T]: ...
@overload
def model(
    *,
    frozen: bool = False,
    kw_only: bool = False,
    order: bool = False,
    eq: bool = True,
    hash: bool | None = None,
    rename_all: RenameRule | None = None,
    unknown: UnknownFields | None = None,
    skip_if_default: bool | None = None,
    skip_if_none: bool | None = None,
) -> Callable[[type[_T]], type[_T]]: ...
@dataclass_transform(field_specifiers=(field,))
def model(
    cls: type[_T] | None = None,
    /,
    *,
    frozen: bool = False,
    kw_only: bool = False,
    order: bool = False,
    eq: bool = True,
    hash: bool | None = None,
    rename_all: RenameRule | None = None,
    unknown: UnknownFields | None = None,
    skip_if_default: bool | None = None,
    skip_if_none: bool | None = None,
) -> type[_T] | Callable[[type[_T]], type[_T]]:
    """Makes a class a model: a dataclass with slots, read and written by oxiform.

    ``rename_all``, ``unknown``, ``skip_if_default`` and ``skip_if_none`` are inherited by
    subclasses that do not set them.
    """

    def convert(target: type[_T]) -> type[_T]:
        options = _options(target, rename_all, unknown, skip_if_default, skip_if_none)
        made = _dataclass(target, frozen, kw_only, order, eq, hash)
        return _finish(made, options)

    return convert if cls is None else convert(cls)


def replace(obj: _T, /, **changes: Any) -> _T:
    """A copy of the model ``obj`` with ``changes`` applied."""
    return cast("_T", dataclasses.replace(cast(Any, obj), **changes))


@dataclass_transform(
    eq_default=True,
    order_default=False,
    kw_only_default=False,
    frozen_default=False,
    field_specifiers=(field,),
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
        hash: bool | None = None,
        rename_all: RenameRule | None = None,
        unknown: UnknownFields | None = None,
        skip_if_default: bool | None = None,
        skip_if_none: bool | None = None,
        **kwargs: Any,
    ) -> "_ModelMeta":
        cls = super().__new__(mcs, name, bases, namespace, **kwargs)
        is_base = not any(isinstance(base, _ModelMeta) for base in bases)
        if is_base or namespace.get(_CONVERTED):
            return cls
        setattr(cls, _CONVERTED, True)
        options = _options(cls, rename_all, unknown, skip_if_default, skip_if_none)
        made = _dataclass(cls, frozen, kw_only, order, eq, hash)
        return cast("_ModelMeta", _finish(made, options))

    def __init__(
        cls, name: str, bases: tuple[type, ...], namespace: dict[str, Any], /, **kwargs: Any
    ) -> None:
        super().__init__(name, bases, namespace)


class Model(metaclass=_ModelMeta):
    """Base class of compact models. Subclasses are slotted dataclasses."""

    __slots__ = ()


def _options(
    cls: type,
    rename_all: RenameRule | None,
    unknown: UnknownFields | None,
    skip_if_default: bool | None,
    skip_if_none: bool | None,
) -> ModelOptions:
    """The options given, falling back to those inherited from a base model."""
    name = cls.__qualname__
    if rename_all is not None and rename_all not in RENAME_RULES:
        rules = ", ".join(RENAME_RULES)
        raise SchemaError(f"{name}: rename_all must be one of {rules}, not {rename_all!r}")
    if unknown is not None and unknown not in ("ignore", "forbid"):
        raise SchemaError(f'{name}: unknown must be "ignore" or "forbid", not {unknown!r}')
    inherited = getattr(cls, OPTIONS_ATTRIBUTE, ModelOptions())
    return ModelOptions(
        rename_all=inherited.rename_all if rename_all is None else rename_all,
        unknown=inherited.unknown if unknown is None else unknown,
        skip_if_default=(inherited.skip_if_default if skip_if_default is None else skip_if_default),
        skip_if_none=inherited.skip_if_none if skip_if_none is None else skip_if_none,
    )


def _dataclass(
    cls: type[_T], frozen: bool, kw_only: bool, order: bool, eq: bool, hash: bool | None
) -> type[_T]:
    try:
        made = dataclasses.dataclass(
            frozen=frozen,
            kw_only=kw_only,
            order=order,
            eq=eq,
            unsafe_hash=hash is True,
            slots=True,
        )(cls)
    except (TypeError, ValueError) as error:
        raise SchemaError(f"{cls.__qualname__}: {error}") from error
    if hash is False:
        made.__hash__ = None  # type: ignore[assignment]  # pyright: ignore[reportAttributeAccessIssue]
    return made


def _finish(cls: type[_T], options: ModelOptions) -> type[_T]:
    setattr(cls, OPTIONS_ATTRIBUTE, options)
    try:
        warmup(cls)
    except UnresolvedAnnotationError:
        pass
    return cls
