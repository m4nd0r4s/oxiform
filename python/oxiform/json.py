"""JSON (RFC 8259)."""

from oxiform._oxiform import JsonDecoder as Decoder
from oxiform._oxiform import json_decode as decode

__all__ = ["Decoder", "decode"]
