"""``Pattern`` is a Rust ``regex`` and a JSON Schema ``pattern`` is ECMA 262, so a pattern is
carried over only where both mean the same.

What is written as is: literals, classes, groups, alternation, repetition, anchors and the
escapes the two share. What is translated, because Rust reads it in Unicode and ECMA 262 in
ASCII or by another name: ``\\d`` and ``\\w`` become ASCII classes, ``(?P<name>`` becomes
``(?<name>``, ``\\A`` and ``\\z`` become ``^`` and ``$`` (a ``Pattern`` has no multi-line mode),
``\\a`` becomes ``\\x07``, ``.`` becomes ``[^\\n]`` (ECMA 262's dot also excludes ``\\r`` and
U+2028/9), and an escaped punctuation mark that is not syntax (``\\-``, ``\\_``, ``\\#``) is
written as itself, since the ``u`` flag that validators use refuses it escaped. What is refused,
with an error that says how to write it: inline flags, Unicode classes, POSIX classes, nested
classes and class operations, ``\\x{..}``, ``\\u{..}``, ``\\U``, ``\\<``, ``\\>`` and ``\\b{..}``.

Left as is, with a difference the schema cannot state: ``\\b`` and ``\\s`` (Unicode in Rust).
``\\d`` and ``\\w`` read only ASCII in the schema, where the decoder also reads other scripts'
digits and letters, so the schema is stricter there; ``\\D`` and ``\\W`` are the negations, so
it is looser on those characters. For ASCII text the two agree.
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
        elif char == ".":
            out.append("[^\\n]")  # Rust's dot excludes only \n; ECMA 262's also \r and U+2028/9
            i += 1
            continue
        out.append(char)
        i += 1
    return "".join(out)


# The characters an ECMA 262 pattern may escape under the ``u`` flag outside a class: the syntax.
_SYNTAX = frozenset("^$\\.*+?()[]{}|/")
# Escapes that Rust and ECMA 262 read alike (``\\u`` only with four digits).
_SHARED = frozenset("nrtvfbBsSxu")


def _escape(source: str, i: int, in_class: bool, out: list[str], location: str) -> int:
    if i + 1 >= len(source):
        _refuse(source, location, "a trailing backslash", "escape it as \\\\")
    char = source[i + 1]
    rest = source[i + 2 : i + 3]
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
    elif char == "a":
        out.append("\\x07")
    elif char in "pP":
        _refuse(source, location, f"the Unicode class \\{char}", "write the characters out")
    elif char in "<>":
        _refuse(source, location, f"the word boundary \\{char}", "write the characters out")
    elif char == "U" or (char in "xu" and rest == "{"):
        _refuse(source, location, f"the escape \\{char}{rest}", "write the characters out")
    elif char == "b" and rest == "{":
        _refuse(source, location, "the boundary \\b{...}", "use \\b or write the characters out")
    elif char in _SYNTAX or char in _SHARED or char.isalnum():
        out.append(source[i : i + 2])
    elif char == "-":
        out.append("\\-" if in_class else "-")
    else:
        # Rust lets any punctuation be escaped; ECMA 262 with the ``u`` flag only the syntax.
        out.append(char)
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
