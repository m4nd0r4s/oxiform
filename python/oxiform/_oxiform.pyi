from typing import Any, ClassVar, Generic, Literal, TypeVar, overload

_T = TypeVar("_T")

Mode = Literal["strict", "lax"]

__version__: str

class Limits:
    """What protects a decode against hostile input. Exceeding one is a ``DecodeError`` with the
    code ``limit_exceeded``.

    Every size is a whole number of at least 0. ``max_depth`` is capped at ``MAX_DEPTH``: a
    larger value is read as ``MAX_DEPTH``, so nesting that deep is an error and never a crash,
    also with ``Limits.none()``.
    """

    MAX_DEPTH: ClassVar[int]
    """The most nesting levels any decode reads (512)."""

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
) -> Any: ...
@overload
def json_decode(
    data: bytes | bytearray | memoryview | str,
    type: type[_T],
    *,
    mode: Mode = "strict",
    limits: Limits | None = None,
    max_errors: int = 100,
) -> _T: ...
@overload
def json_decode(
    data: bytes | bytearray | memoryview | str,
    type: object,
    *,
    mode: Mode = "strict",
    limits: Limits | None = None,
    max_errors: int = 100,
) -> Any: ...
def warmup(*types: object) -> None:
    """Compiles the plans of ``types`` now instead of on first use."""

def json_encode(obj: object, *, indent: int | None = None, sort_keys: bool = False) -> bytes: ...

class JsonEncoder:
    """A reusable JSON encoder with fixed options."""

    def __init__(self, *, indent: int | None = None, sort_keys: bool = False) -> None: ...
    def encode(self, obj: object) -> bytes: ...

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
    ) -> JsonDecoder[_T]: ...
    @overload
    def __new__(
        cls,
        type: object = ...,
        *,
        mode: Mode = "strict",
        limits: Limits | None = None,
        max_errors: int = 100,
    ) -> JsonDecoder[Any]: ...
    @property
    def type(self) -> object: ...
    def decode(self, data: bytes | bytearray | memoryview | str) -> _T: ...

class PlanHandle: ...
