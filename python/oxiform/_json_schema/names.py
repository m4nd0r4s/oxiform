"""Names for the definitions of a bundle: after the class, told apart by module when two
classes share a name, and never by the order of the targets."""

import re
from collections.abc import Sequence

__all__ = ["assign_names"]

_UNSAFE = re.compile(r"[^A-Za-z0-9._-]")


def assign_names(classes: Sequence[type]) -> dict[type, str]:
    """One distinct, valid name for each class of ``classes``.

    A name that no other class has is the class name. Classes that share one are written with
    the shortest tail of their module path that tells them apart. Two classes that still share a
    name (the same module and qualified name) get ``_2``, ``_3``... in the order of ``classes``,
    which is the only place that order is seen.
    """
    by_base: dict[str, list[type]] = {}
    for cls in classes:
        by_base.setdefault(_base_name(cls), []).append(cls)
    chosen: dict[type, str] = {}
    for base, members in by_base.items():
        if len(members) == 1:
            chosen[members[0]] = base
        else:
            chosen.update(_qualified(base, members))
    return _numbered(classes, chosen)


def _base_name(cls: type) -> str:
    qualified = cls.__qualname__.rsplit("<locals>.", 1)[-1]
    return _UNSAFE.sub("_", qualified)


def _qualified(base: str, members: list[type]) -> dict[type, str]:
    modules = {cls: _UNSAFE.sub("_", cls.__module__).split(".") for cls in members}
    longest = max(len(parts) for parts in modules.values())
    names: dict[type, str] = {}
    for tail in range(1, longest + 1):
        names = {cls: ".".join([*parts[-tail:], base]) for cls, parts in modules.items()}
        if len(set(names.values())) == len(names):
            break
    return names


def _numbered(classes: Sequence[type], chosen: dict[type, str]) -> dict[type, str]:
    taken: set[str] = set()
    names: dict[type, str] = {}
    for cls in classes:
        name = candidate = chosen[cls]
        number = 1
        while name in taken:
            number += 1
            name = f"{candidate}_{number}"
        taken.add(name)
        names[cls] = name
    return names
