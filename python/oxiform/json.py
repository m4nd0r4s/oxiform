"""JSON (RFC 8259)."""

from oxiform._oxiform import JsonDecoder as Decoder
from oxiform._oxiform import JsonEncoder as Encoder
from oxiform._oxiform import json_decode as decode
from oxiform._oxiform import json_encode as encode

__all__ = ["Decoder", "Encoder", "decode", "encode"]
