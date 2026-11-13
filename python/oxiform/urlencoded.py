"""Query strings and ``application/x-www-form-urlencoded`` bodies, read in lax mode."""

from oxiform._oxiform import urlencoded_decode as decode

__all__ = ["decode"]
