"""JSON Schema 2020-12 for the types oxiform reads (section 8.5)."""

from oxiform._json_schema.bundle import SchemaBundle, SchemaTarget, json_schema, json_schema_bundle
from oxiform._json_schema.writer import JsonSchema, SchemaCoercion, SchemaMode

__all__ = [
    "JsonSchema",
    "SchemaBundle",
    "SchemaCoercion",
    "SchemaMode",
    "SchemaTarget",
    "json_schema",
    "json_schema_bundle",
]
