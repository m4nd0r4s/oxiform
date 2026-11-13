"""``@ox.config``: ``@ox.model`` with the defaults of a config (spec section 6.1)."""

from collections.abc import Callable
from typing import TypeVar, dataclass_transform, overload

from oxiform._config.schema import CONFIG_ATTRIBUTE, ConfigOptions, check_declaration
from oxiform._fields import field
from oxiform._introspection.describe import UnresolvedAnnotationError
from oxiform._model_options import UnknownFields
from oxiform._models import make_model_class
from oxiform._naming import RenameRule
from oxiform.errors import SchemaError

__all__ = ["config"]

_T = TypeVar("_T")


@overload
def config(cls: type[_T], /) -> type[_T]: ...
@overload
def config(
    *,
    env_prefix: str = "",
    env_nested: str | None = "__",
    rename_all: RenameRule | None = None,
    unknown: UnknownFields = "forbid",
) -> Callable[[type[_T]], type[_T]]: ...
@dataclass_transform(kw_only_default=True, frozen_default=True, field_specifiers=(field,))
def config(
    cls: type[_T] | None = None,
    /,
    *,
    env_prefix: str = "",
    env_nested: str | None = "__",
    rename_all: RenameRule | None = None,
    unknown: UnknownFields = "forbid",
) -> type[_T] | Callable[[type[_T]], type[_T]]:
    """Makes a class a config: a frozen, keyword-only dataclass with slots that ``cfg.load``
    fills from several sources, and that rejects unknown keys.

    ``env_prefix`` and ``env_nested`` name the environment variables of its fields
    (``APP_DB__POOL_SIZE`` for ``db.pool_size``) and count only on the class given to
    ``cfg.load``. Sections are nested configs, declared with ``@ox.config`` too.
    """

    def convert(target: type[_T]) -> type[_T]:
        _check_options(target, env_prefix, env_nested)
        options = ConfigOptions(env_prefix, env_nested)

        def mark_and_check(made: type[_T]) -> None:
            setattr(made, CONFIG_ATTRIBUTE, options)
            try:
                check_declaration(made)
            except UnresolvedAnnotationError:
                pass  # a class that is not defined yet: checked on the first load

        return make_model_class(
            target,
            frozen=True,
            kw_only=True,
            rename_all=rename_all,
            unknown=unknown,
            weakref_slot=True,
            before_finish=mark_and_check,
        )

    return convert if cls is None else convert(cls)


def _check_options(cls: type, env_prefix: object, env_nested: object) -> None:
    name = cls.__qualname__
    if not isinstance(env_prefix, str):
        raise SchemaError(f"{name}: env_prefix is a str such as 'APP_', not {env_prefix!r}")
    if env_nested is not None and (not isinstance(env_nested, str) or not env_nested):
        raise SchemaError(
            f"{name}: env_nested is the text that separates a section from its field in a "
            f"variable name, such as '__', or None for no nesting; not {env_nested!r}"
        )
