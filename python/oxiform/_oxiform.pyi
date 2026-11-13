from collections.abc import Iterable, Mapping
from typing import Any, ClassVar, Generic, Literal, TypeVar, overload

_T = TypeVar("_T")

Mode = Literal["strict", "lax"]
Unknown = Literal["ignore", "forbid"]

__version__: str

ABI_VERSION: tuple[int, int]
"""`(major, minor)` of the embedding C ABI this build exports through the capsule `_ABI_V1`
(1.1 added `decode_pairs`)."""
_ABI_MAJORS: tuple[int, ...]
"""Every major version of the ABI that has a capsule, as `_ABI_V<major>`."""
_ABI_V1: object
"""The capsule (a `PyCapsule`) of the function table of ABI 1, for native extensions."""

def _plan_id(target: object) -> int:
    """The address of the cached compiled type of `target`: one number per type."""

def _plan_stats() -> tuple[int, int]:
    """`(types compiled in this process, types in the plan cache)`: a host that asks for the plan
    of a known type compiles nothing."""

class Limits:
    """What protects a decode against hostile input. Exceeding one is a ``DecodeError`` with the
    code ``limit_exceeded``.

    Every size is a whole number of at least 0. ``max_depth`` is capped at ``MAX_DEPTH``: a
    larger value is read as ``MAX_DEPTH``, so nesting that deep is an error and never a crash,
    also with ``Limits.none()``.
    """

    MAX_DEPTH: ClassVar[int]
    """The most nesting levels any decode reads (128, which is also the default)."""

    def __new__(
        cls,
        *,
        max_bytes: int = 67_108_864,
        max_depth: int = 128,
        max_items: int = 1_000_000,
        max_str_len: int = 16_777_216,
        max_int_digits: int = 4300,
    ) -> Limits: ...
    @staticmethod
    def none() -> Limits:
        """No limits, for trusted input. Nesting stops at ``MAX_DEPTH`` levels, and integers at
        the digit limit of Python (``sys.get_int_max_str_digits()``)."""
    @property
    def max_bytes(self) -> int: ...
    @property
    def max_depth(self) -> int: ...
    @property
    def max_items(self) -> int: ...
    @property
    def max_str_len(self) -> int: ...
    @property
    def max_int_digits(self) -> int: ...

@overload
def json_decode(
    data: bytes | bytearray | memoryview | str,
    *,
    mode: Mode = "strict",
    limits: Limits | None = None,
    max_errors: int = 100,
    unknown: Unknown | None = None,
) -> Any: ...
@overload
def json_decode(
    data: bytes | bytearray | memoryview | str,
    type: type[_T],
    *,
    mode: Mode = "strict",
    limits: Limits | None = None,
    max_errors: int = 100,
    unknown: Unknown | None = None,
) -> _T: ...
@overload
def json_decode(
    data: bytes | bytearray | memoryview | str,
    type: object,
    *,
    mode: Mode = "strict",
    limits: Limits | None = None,
    max_errors: int = 100,
    unknown: Unknown | None = None,
) -> Any: ...

_Text = str | bytes
_PairsLike = Iterable[tuple[_Text, _Text]] | Mapping[_Text, _Text]

@overload
def urlencoded_decode(
    data: bytes | bytearray | memoryview | str,
    type: type[_T],
    *,
    limits: Limits | None = None,
    max_errors: int = 100,
    unknown: Unknown | None = None,
) -> _T:
    """Reads a query string or an ``application/x-www-form-urlencoded`` body as a model, in lax
    mode: ``%XX`` and ``+`` are decoded, a name given more than once gives a ``list``, ``tuple`` or
    ``set`` field its values in order and is a ``duplicate_key`` problem for any other field."""

@overload
def urlencoded_decode(
    data: bytes | bytearray | memoryview | str,
    type: object,
    *,
    limits: Limits | None = None,
    max_errors: int = 100,
    unknown: Unknown | None = None,
) -> Any: ...
@overload
def pairs_decode(
    pairs: _PairsLike,
    type: type[_T],
    *,
    case_insensitive: bool = False,
    limits: Limits | None = None,
    max_errors: int = 100,
    unknown: Unknown | None = None,
) -> _T:
    """Reads ``(name, value)`` pairs (headers, cookies) as a model, in lax mode and without
    decoding anything. ``case_insensitive=True`` matches names as header names are matched."""

@overload
def pairs_decode(
    pairs: _PairsLike,
    type: object,
    *,
    case_insensitive: bool = False,
    limits: Limits | None = None,
    max_errors: int = 100,
    unknown: Unknown | None = None,
) -> Any: ...
def warmup(*types: object) -> None:
    """Compiles the plans of ``types`` now instead of on first use."""

def json_encode(
    obj: object, *, type: object = None, indent: int | None = None, sort_keys: bool = False
) -> bytes:
    """Writes ``obj`` as JSON.

    With ``type``, ``obj`` is written as that declared type and not as its own class: only the
    fields of ``type`` are written, and the fields of a subclass outside ``type`` never are. A
    model of ``type`` is read as attributes from an object and as keys from a mapping. Without
    ``type``, the class of each value decides what is written.
    """

class JsonEncoder:
    """A reusable JSON encoder with fixed options."""

    def __init__(
        self, *, type: object = None, indent: int | None = None, sort_keys: bool = False
    ) -> None: ...
    def encode(self, obj: object, *, type: object = None) -> bytes:
        """Writes ``obj`` as the ``type`` given here or else the one of the encoder."""
    @property
    def type(self) -> object:
        """The type written when ``encode`` is not given one, or ``None``."""

class JsonDecoder(Generic[_T]):
    """A reusable JSON decoder for one type."""

    @overload
    def __new__(
        cls,
        type: type[_T],
        *,
        mode: Mode = "strict",
        limits: Limits | None = None,
        max_errors: int = 100,
        unknown: Unknown | None = None,
    ) -> JsonDecoder[_T]: ...
    @overload
    def __new__(
        cls,
        type: object = ...,
        *,
        mode: Mode = "strict",
        limits: Limits | None = None,
        max_errors: int = 100,
        unknown: Unknown | None = None,
    ) -> JsonDecoder[Any]: ...
    @property
    def type(self) -> object: ...
    def decode(self, data: bytes | bytearray | memoryview | str) -> _T: ...

class PlanHandle: ...
