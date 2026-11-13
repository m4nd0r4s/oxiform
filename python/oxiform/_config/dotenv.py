"""The ``.env`` syntax of ``cfg.dotenv`` (spec section 6.2).

One ``KEY=value`` per line, with ``export KEY=value`` allowed. A blank line or a line that
starts with ``#`` is skipped. An unquoted value ends at the end of the line or at `` #``; a
single-quoted value is literal; a double-quoted value reads ``\\n``, ``\\r``, ``\\t``, ``\\"``,
``\\'`` and ``\\\\``; both quoted forms may span lines. There is no ``${VAR}`` interpolation.
"""

import bisect
import re
from dataclasses import dataclass

__all__ = ["DotenvProblem", "DotenvValue", "parse_dotenv"]

_KEY = re.compile(r"[ \t]*(?:export[ \t]+)?([A-Za-z_][A-Za-z0-9_.\-]*)[ \t]*=[ \t]*")
_COMMENT = re.compile(r"[ \t]#")
_ESCAPES = {"n": "\n", "r": "\r", "t": "\t", '"': '"', "'": "'", "\\": "\\"}


@dataclass(frozen=True, slots=True)
class DotenvValue:
    value: str
    line: int


@dataclass(frozen=True, slots=True)
class DotenvProblem:
    line: int
    message: str
    """What is wrong and what to write instead; the file name is added by the caller."""


def parse_dotenv(text: str) -> tuple[dict[str, DotenvValue], list[DotenvProblem]]:
    """The values of a ``.env`` text by name, and what is wrong with it."""
    if text.startswith("﻿"):
        text = text[1:]
    starts = [0] + [match.end() for match in re.finditer("\n", text)]

    def line_of(position: int) -> int:
        return bisect.bisect_right(starts, position)

    entries: dict[str, DotenvValue] = {}
    problems: list[DotenvProblem] = []
    position = 0
    length = len(text)
    while position < length:
        end_of_line = _end_of_line(text, position)
        stripped = text[position:end_of_line].strip()
        if not stripped or stripped.startswith("#"):
            position = end_of_line + 1
            continue
        line = line_of(position)
        match = _KEY.match(text, position)
        if match is None:
            shown = stripped if len(stripped) <= 40 else stripped[:40] + "..."
            problems.append(
                DotenvProblem(line, f"expected KEY=value, found {shown!r}; write NAME=value")
            )
            position = end_of_line + 1
            continue
        name = match.group(1)
        start = match.end()
        if start < length and text[start] in "\"'":
            value, position, problem = _quoted(text, start, line)
        else:
            value, position, problem = _unquoted(text, start), end_of_line + 1, None
        if problem is not None:
            problems.append(problem)
        elif name in entries:
            problems.append(
                DotenvProblem(
                    line,
                    f"{name} is set twice, on lines {entries[name].line} and {line}; keep one",
                )
            )
        else:
            entries[name] = DotenvValue(value, line)
    return entries, problems


def _end_of_line(text: str, position: int) -> int:
    end = text.find("\n", position)
    return len(text) if end < 0 else end


def _unquoted(text: str, start: int) -> str:
    value = text[start : _end_of_line(text, start)]
    if value.lstrip().startswith("#"):
        return ""
    comment = _COMMENT.search(value)
    if comment is not None:
        value = value[: comment.start()]
    return value.strip()


def _quoted(text: str, start: int, line: int) -> tuple[str, int, DotenvProblem | None]:
    """The value that starts with the quote at ``start``, where the next statement starts, and
    the problem if there is one."""
    quote = text[start]
    out: list[str] = []
    index = start + 1
    length = len(text)
    while index < length:
        character = text[index]
        if quote == '"' and character == "\\" and index + 1 < length:
            following = text[index + 1]
            out.append(_ESCAPES.get(following, "\\" + following))
            index += 2
            continue
        if character == quote:
            break
        out.append(character)
        index += 1
    else:
        problem = DotenvProblem(
            line,
            f"the {quote} quote is never closed; close it, or remove it if it is part of the value",
        )
        return "", length, problem
    end_of_line = _end_of_line(text, index)
    rest = text[index + 1 : end_of_line].strip()
    if rest and not rest.startswith("#"):
        problem = DotenvProblem(
            line,
            f"text after the closing {quote} quote ({rest[:20]!r}); "
            "end the line there, or quote the whole value",
        )
        return "", end_of_line + 1, problem
    return "".join(out), end_of_line + 1, None
