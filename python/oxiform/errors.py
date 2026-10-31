"""Exceptions raised by oxiform."""

__all__ = ["OxiformError", "SchemaError"]


class OxiformError(Exception):
    """Base class of every oxiform exception."""


class SchemaError(OxiformError, TypeError):
    """A type declaration oxiform cannot use, found when its plan is compiled."""
