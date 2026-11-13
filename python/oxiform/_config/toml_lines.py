"""The line of each key of a TOML document, for the origin of a value and for errors.

``tomllib`` has already accepted the text when this runs, so the scanner can be lenient: it
only has to find where each key starts, skipping over strings, arrays and inline tables whose
contents are not keys. The result is a hint, so it never raises: on text it cannot follow it
returns what it found.
"""

import bisect
import re

__all__ = ["key_lines", "line_of_key"]

_BARE_KEY = re.compile(r"[A-Za-z0-9_-]+")
_ESCAPES = {"b": "\b", "t": "\t", "n": "\n", "f": "\f", "r": "\r", '"': '"', "\\": "\\"}


def key_lines(text: str) -> dict[tuple[str, ...], int]:
    """``{key path: line}`` for every key and table of ``text``: ``[db]`` and a key ``url``
    under it are ``("db",)`` and ``("db", "url")``. A dotted key records each prefix. For a
    table that is repeated (an array of tables), the first occurrence."""
    scanner = _Scanner(text)
    try:
        scanner.run()
    except (IndexError, ValueError):
        pass
    return scanner.lines


def line_of_key(lines: dict[tuple[str, ...], int], path: tuple[str, ...]) -> int | None:
    """The line of ``path``, or of its nearest ancestor that has one."""
    while path:
        if path in lines:
            return lines[path]
        path = path[:-1]
    return None


class _Scanner:
    def __init__(self, text: str) -> None:
        self.text = text
        self.position = 0
        self.table: tuple[str, ...] = ()
        self.lines: dict[tuple[str, ...], int] = {}
        self._starts = [0] + [match.end() for match in re.finditer("\n", text)]

    def line(self, position: int) -> int:
        return bisect.bisect_right(self._starts, position)

    def run(self) -> None:
        text = self.text
        while self.position < len(text):
            self._skip_blank()
            if self.position >= len(text):
                return
            character = text[self.position]
            if character == "[":
                self._header()
            else:
                self._key_value()
            self._end_of_line()

    # -- statements ------------------------------------------------------------------------

    def _header(self) -> None:
        text = self.text
        line = self.line(self.position)
        double = text.startswith("[[", self.position)
        self.position += 2 if double else 1
        parts = self._key()
        self.table = tuple(parts)
        for end in range(1, len(parts) + 1):
            self.lines.setdefault(tuple(parts[:end]), line)
        self.position = text.index("]", self.position) + (2 if double else 1)

    def _key_value(self) -> None:
        line = self.line(self.position)
        parts = self._key()
        for end in range(1, len(parts) + 1):
            self.lines.setdefault((*self.table, *parts[:end]), line)
        self._skip_spaces()
        self.position = self.text.index("=", self.position) + 1
        self._skip_spaces()
        self._value()

    def _end_of_line(self) -> None:
        text = self.text
        self._skip_spaces()
        if self.position < len(text) and text[self.position] == "#":
            self._skip_comment()
        if self.position < len(text) and text[self.position] == "\n":
            self.position += 1

    # -- keys ------------------------------------------------------------------------------

    def _key(self) -> list[str]:
        parts: list[str] = []
        text = self.text
        while True:
            self._skip_spaces()
            character = text[self.position]
            if character == '"':
                parts.append(self._basic_key())
            elif character == "'":
                end = text.index("'", self.position + 1)
                parts.append(text[self.position + 1 : end])
                self.position = end + 1
            else:
                match = _BARE_KEY.match(text, self.position)
                if match is None:
                    raise ValueError("not a key")
                parts.append(match.group())
                self.position = match.end()
            self._skip_spaces()
            if text[self.position] != ".":
                return parts
            self.position += 1

    def _basic_key(self) -> str:
        text = self.text
        out: list[str] = []
        index = self.position + 1
        while text[index] != '"':
            if text[index] == "\\":
                index += 1
                out.append(_ESCAPES.get(text[index], "\\" + text[index]))
            else:
                out.append(text[index])
            index += 1
        self.position = index + 1
        return "".join(out)

    # -- values ----------------------------------------------------------------------------

    def _value(self) -> None:
        text = self.text
        for quote in ('"""', "'''"):
            if text.startswith(quote, self.position):
                self._multiline_string(quote)
                return
        character = text[self.position]
        if character in "\"'":
            self._string(character)
        elif character == "[":
            self._array()
        elif character == "{":
            self._inline_table()
        else:
            while self.position < len(text) and text[self.position] not in "\n#,]}":
                self.position += 1

    def _string(self, quote: str) -> None:
        text = self.text
        index = self.position + 1
        while text[index] != quote:
            index += 2 if quote == '"' and text[index] == "\\" else 1
        self.position = index + 1

    def _multiline_string(self, quote: str) -> None:
        text = self.text
        index = self.position + 3
        while not text.startswith(quote, index):
            index += 2 if quote == '"""' and text[index] == "\\" else 1
        index += 3
        while text.startswith(quote[0], index):  # up to two more quotes can end the string
            index += 1
        self.position = index

    def _array(self) -> None:
        text = self.text
        self.position += 1
        while True:
            self._skip_blank()
            character = text[self.position]
            if character == "]":
                self.position += 1
                return
            if character == ",":
                self.position += 1
                continue
            self._value()

    def _inline_table(self) -> None:
        text = self.text
        self.position += 1
        while True:
            self._skip_blank()
            character = text[self.position]
            if character == "}":
                self.position += 1
                return
            if character == ",":
                self.position += 1
                continue
            self._key()
            self._skip_spaces()
            self.position = text.index("=", self.position) + 1
            self._skip_blank()
            self._value()

    # -- whitespace ------------------------------------------------------------------------

    def _skip_spaces(self) -> None:
        text = self.text
        while self.position < len(text) and text[self.position] in " \t\r":
            self.position += 1

    def _skip_comment(self) -> None:
        end = self.text.find("\n", self.position)
        self.position = len(self.text) if end < 0 else end

    def _skip_blank(self) -> None:
        """Spaces, newlines and comments."""
        text = self.text
        while self.position < len(text):
            character = text[self.position]
            if character in " \t\r\n":
                self.position += 1
            elif character == "#":
                self._skip_comment()
            else:
                return
