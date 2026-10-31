"""Wire names from Python names, for ``rename_all``."""

import re
from typing import Literal, TypeAlias, get_args

__all__ = ["RENAME_RULES", "RenameRule", "rename"]

RenameRule: TypeAlias = Literal[
    "camelCase", "PascalCase", "snake_case", "kebab-case", "SCREAMING_SNAKE_CASE"
]

RENAME_RULES: tuple[RenameRule, ...] = get_args(RenameRule)

# Words of a name: runs of capitals and digits not followed by a lowercase letter (``HTTP`` in
# ``HTTPServer``, ``V2``), or an optional capital followed by lowercase letters and digits.
_WORD = re.compile(r"[A-Z]+[0-9]*(?![a-z])|[A-Z]?[a-z0-9]+")


def rename(name: str, rule: RenameRule) -> str:
    """``name`` written with ``rule``. Words are split at underscores and at capitals, so
    ``postal_code`` and ``postalCode`` give the same result."""
    words = [word.lower() for part in name.split("_") for word in _WORD.findall(part)]
    if not words:
        return name
    match rule:
        case "camelCase":
            return words[0] + "".join(word.capitalize() for word in words[1:])
        case "PascalCase":
            return "".join(word.capitalize() for word in words)
        case "snake_case":
            return "_".join(words)
        case "kebab-case":
            return "-".join(words)
        case "SCREAMING_SNAKE_CASE":
            return "_".join(words).upper()
