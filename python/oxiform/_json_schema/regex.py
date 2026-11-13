"""``Pattern`` is a Rust ``regex`` and a JSON Schema ``pattern`` is ECMA 262, so a pattern is
carried over only where both mean the same.

What is written as is: literals, classes, groups, alternation, repetition, anchors and the
escapes the two share. What is translated, because Rust reads it in Unicode and ECMA 262 in
ASCII or by another name: ``\\d`` and ``\\w`` become ASCII classes, ``(?P<name>`` becomes
``(?<name>``, ``\\A`` and ``\\z`` become ``^`` and ``$`` (a ``Pattern`` has no multi-line mode).
What is refused, with an error that says how to write it: inline flags, Unicode classes,
POSIX classes, nested classes and class operations, ``\\x{..}`` and ``\\U``.

Left as is, with a difference the schema cannot state: ``\\b`` and ``\\s`` (Unicode in Rust),
and ``\\d`` and ``\\w`` reading only ASCII in the schema, where the decoder also reads other
scripts' digits and letters. A schema is never looser than that for ASCII text.
"""

from oxiform.errors import SchemaError

__all__ = ["portable_pattern"]

_ASCII_CLASSES = {
    "d": ("0-9", "[0-9]"),
    "w": ("A-Za-z0-9_", "[A-Za-z0-9_]"),
}
_NEGATED_CLASSES = {"D": "[^0-9]", "W": "[^A-Za-z0-9_]"}

_HINT = "write it with plain classes, for example [Aa] for a case-insensitive a"


def portable_pattern(source: str, location: str) -> str:
    """``source`` as an ECMA 262 pattern, or ``SchemaError`` when it has no such form."""
    out: list[str] = []
    in_class = False
    i = 0
    while i < len(source):
        char = source[i]
        if char == "\\":
            i = _escape(source, i, in_class, out, location)
            continue
        if in_class:
            if char == "[":
                _refuse(source, location, "a nested class or a POSIX class", _HINT)
            if source[i : i + 2] in ("&&", "~~", "--"):
                _refuse(source, location, f"the class operation {source[i : i + 2]!r}", _HINT)
            in_class = char != "]"
        elif char == "[":
            in_class = True
            out.append(char)
            i += 1
            if source[i : i + 1] == "^":
                out.append("^")
                i += 1
            if source[i : i + 1] == "]":
                _refuse(source, location, "a ] first in a class", "write it as \\]")
            continue
        elif char == "(" and source.startswith("(?", i):
            i = _group(source, i, out, location)
            continue
        out.append(char)
        i += 1
    return "".join(out)


def _escape(source: str, i: int, in_class: bool, out: list[str], location: str) -> int:
    if i + 1 >= len(source):
        _refuse(source, location, "a trailing backslash", "escape it as \\\\")
    char = source[i + 1]
    if char in _ASCII_CLASSES:
        inside, outside = _ASCII_CLASSES[char]
        out.append(inside if in_class else outside)
    elif char in _NEGATED_CLASSES:
        if in_class:
            _refuse(source, location, f"\\{char} inside a class", "write the class out")
        out.append(_NEGATED_CLASSES[char])
    elif char == "A" and not in_class:
        out.append("^")
    elif char == "z" and not in_class:
        out.append("$")
    elif char in "pP":
        _refuse(source, location, f"the Unicode class \\{char}", "write the characters out")
    elif char == "U" or (char == "x" and source[i + 2 : i + 3] == "{"):
        _refuse(source, location, f"the escape \\{char}", "write the characters out")
    else:
        out.append(source[i : i + 2])
    return i + 2


def _group(source: str, i: int, out: list[str], location: str) -> int:
    if source.startswith("(?:", i):
        out.append("(?:")
        return i + 3
    if source.startswith("(?P<", i):
        out.append("(?<")
        return i + 4
    if source.startswith("(?<", i) and source[i + 3 : i + 4] not in ("=", "!"):
        out.append("(?<")
        return i + 3
    end = source.find(")", i)
    shown = source[i : (end + 1 if end != -1 else len(source))]
    _refuse(source, location, f"the group syntax {shown!r}", _HINT)
    return i


def _refuse(source: str, location: str, what: str, hint: str) -> None:
    raise SchemaError(
        f"{location}: Pattern {source!r} uses {what}, which a JSON Schema pattern (ECMA 262) "
        f"cannot say the same way; {hint}, so that the schema and the decoder agree"
    )
