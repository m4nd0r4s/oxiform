"""The oxiform options of a model, kept on its class."""

from dataclasses import dataclass
from typing import Literal, TypeAlias

from oxiform._naming import RenameRule

__all__ = ["OPTIONS_ATTRIBUTE", "ModelOptions", "UnknownFields", "model_options"]

UnknownFields: TypeAlias = Literal["ignore", "forbid"]

OPTIONS_ATTRIBUTE = "__oxiform_options__"
"""The class attribute holding the options. Subclasses inherit it like any attribute."""


@dataclass(frozen=True, slots=True)
class ModelOptions:
    """Options of ``@ox.model`` and ``ox.Model`` that are not dataclass options."""

    rename_all: RenameRule | None = None
    unknown: UnknownFields = "ignore"
    skip_if_default: bool = False
    skip_if_none: bool = False
    tag: str | None = None
    """The tag of this class in a tagged union. Not inherited; see ``tag_of``."""
    tag_field: str | None = None
    """The key that holds the tag, written on encode and checked on decode."""


_DEFAULTS = ModelOptions()


def model_options(cls: type) -> ModelOptions:
    """The options of ``cls``, or the defaults when it is not an oxiform model."""
    options = getattr(cls, OPTIONS_ATTRIBUTE, _DEFAULTS)
    return options if isinstance(options, ModelOptions) else _DEFAULTS
