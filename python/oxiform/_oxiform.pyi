from typing import Any, TypeVar, overload

_T = TypeVar("_T")

__version__: str

@overload
def json_decode(data: bytes | bytearray | memoryview | str) -> Any: ...
@overload
def json_decode(
    data: bytes | bytearray | memoryview | str, type: type[_T], *, max_errors: int = 100
) -> _T: ...
@overload
def json_decode(
    data: bytes | bytearray | memoryview | str, type: object, *, max_errors: int = 100
) -> Any: ...

class PlanHandle: ...
