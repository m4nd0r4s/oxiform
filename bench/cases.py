"""The timed cases: decoding each payload and encoding the values it decodes to."""

import json
from collections.abc import Callable

import msgspec
import orjson
import pydantic

import oxiform as ox
from bench.models import DECODERS
from bench.payloads import PAYLOADS

__all__ = ["CASES"]

Case = dict[str, Callable[[], object]]


def _payload(model: str) -> bytes:
    return PAYLOADS["nested" if model == "untyped" else model]


def _decode_case(model: str) -> Case:
    payload = _payload(model)
    return {
        library: (lambda decode=decode: decode(payload))
        for library, decode in DECODERS[model].items()
    }


def _encode_case(model: str) -> Case:
    """Each library encodes the value it decoded itself from the same payload."""
    payload = _payload(model)
    values = {library: decode(payload) for library, decode in DECODERS[model].items()}
    oxiform_encode = ox.json.Encoder().encode
    msgspec_encode = msgspec.json.Encoder().encode
    case: Case = {
        "oxiform": lambda: oxiform_encode(values["oxiform"]),
        "msgspec": lambda: msgspec_encode(values["msgspec"]),
    }
    if model == "untyped":
        case["orjson"] = lambda: orjson.dumps(values["orjson"])
    else:
        model_value = values["pydantic"]
        assert isinstance(model_value, pydantic.BaseModel)
        case["pydantic"] = model_value.model_dump_json
    for library, encode in case.items():
        if json.loads(encode()) != json.loads(payload):  # type: ignore[arg-type]
            raise SystemExit(f"{library} does not round-trip the {model} payload")
    return case


CASES: dict[str, Case] = {
    **{f"decode {model}": _decode_case(model) for model in DECODERS},
    **{f"encode {model}": _encode_case(model) for model in DECODERS},
}
