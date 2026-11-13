"""Validate and (de)serialize Python types with a Rust engine."""

from oxiform import cfg, json, pairs, urlencoded
from oxiform._config import config
from oxiform._context import Context
from oxiform._fields import field
from oxiform._json_schema import (
    JsonSchema,
    SchemaBundle,
    SchemaCoercion,
    SchemaMode,
    SchemaTarget,
    json_schema,
    json_schema_bundle,
)
from oxiform._models import Model, model, replace
from oxiform._naive import NaiveDateTime
from oxiform._oxiform import Limits, __version__, warmup
from oxiform._secret import Secret
from oxiform._validators import validator
from oxiform.constraints import Ge, Le, Len, OneOf, Pattern
from oxiform.errors import (
    DecodeError,
    EncodeError,
    Invalid,
    OxiformError,
    SchemaError,
    ValidationError,
)
from oxiform.unions import External, Tagged, Untagged

__all__ = [
    "Context",
    "DecodeError",
    "EncodeError",
    "External",
    "Ge",
    "Invalid",
    "Le",
    "JsonSchema",
    "Limits",
    "Len",
    "Model",
    "NaiveDateTime",
    "OneOf",
    "OxiformError",
    "Pattern",
    "SchemaBundle",
    "SchemaCoercion",
    "SchemaError",
    "SchemaMode",
    "SchemaTarget",
    "Secret",
    "Tagged",
    "Untagged",
    "ValidationError",
    "__version__",
    "cfg",
    "config",
    "field",
    "json",
    "json_schema",
    "json_schema_bundle",
    "model",
    "pairs",
    "replace",
    "urlencoded",
    "validator",
    "warmup",
]
