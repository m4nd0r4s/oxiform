"""The OpenAPI example runs, and its components are valid JSON Schema that fits real data."""

import json
import runpy
from pathlib import Path
from typing import Any

from schema_support import Checker, check_schema

import oxiform as ox

EXAMPLE = Path(__file__).parent.parent / "examples" / "openapi_components.py"


def test_the_example_builds_valid_components() -> None:
    namespace = runpy.run_path(str(EXAMPLE))
    document: dict[str, Any] = namespace["document"]
    components = document["components"]["schemas"]
    assert sorted(components) == [
        "Address-Input",
        "Address-Output",
        "Conflict",
        "CreateUser",
        "NotFound",
        "User",
    ]
    for schema in components.values():
        check_schema(schema)
    post = document["paths"]["/users"]["post"]
    request = post["requestBody"]["content"]["application/json"]["schema"]
    created = post["responses"]["201"]["content"]["application/json"]["schema"]
    assert request == {"$ref": "#/components/schemas/CreateUser"}
    assert created == {"$ref": "#/components/schemas/User"}

    def check(schema: dict[str, Any]) -> Checker:
        return Checker({**schema, "components": {"schemas": components}})

    body = {
        "name": "Ana",
        "email": "a@b.c",
        "addresses": [{"street": "Rua", "postalCode": "1000-001"}],
    }
    assert check(request).is_valid(body)
    assert not check(request).is_valid({**body, "name": ""})
    user_type = namespace["User"]
    user = user_type(1, "Ana", "a@b.c", [namespace["Address"]("Rua", "1000-001")], "secret")
    written = json.loads(ox.json.encode(user))
    assert "passwordHash" not in written
    assert check(created).is_valid(written)
    assert not check(created).is_valid({**written, "id": "x"})
