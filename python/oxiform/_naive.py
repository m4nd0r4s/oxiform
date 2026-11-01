"""``ox.NaiveDateTime``: a ``datetime`` read and written without a timezone (D5)."""

import datetime
from typing import Annotated, TypeAlias

__all__ = ["NAIVE", "NaiveDateTime"]


class _Naive:
    """The ``Annotated`` marker of ``ox.NaiveDateTime``."""

    __slots__ = ()

    def __repr__(self) -> str:
        return "ox.NaiveDateTime"


NAIVE = _Naive()

NaiveDateTime: TypeAlias = Annotated[datetime.datetime, NAIVE]
"""A ``datetime`` without a timezone. A plain ``datetime`` field requires one."""
