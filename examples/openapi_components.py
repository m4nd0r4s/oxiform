"""One JSON Schema bundle for the types of an API, ready for the ``components/schemas`` of an
OpenAPI 3.1 document (docs/spec.md, section 8.5).

Run it with ``python examples/openapi_components.py``.
"""

import json
from typing import Annotated, Literal

import oxiform as ox


@ox.model(rename_all="camelCase")
class Address:
    street: str
    postal_code: Annotated[str, ox.Pattern(r"^\d{4}-\d{3}$")]
    country: Literal["PT", "DE", "BR"] = "PT"


@ox.model(rename_all="camelCase")
class CreateUser:
    """The body of POST /users."""

    name: Annotated[str, ox.Len(1, 80)]
    email: str
    addresses: list[Address] = ox.field(default_factory=list[Address])
    referral_code: str | None = None


@ox.model(rename_all="camelCase")
class User:
    """What the API answers for a user."""

    id: int
    name: str
    email: str
    addresses: list[Address]
    password_hash: str = ox.field(default="", skip_encode=True)
    referral_code: str | None = ox.field(default=None, skip_if_none=True)


@ox.model(tag_field="kind")
class ApiError:
    message: str


@ox.model(tag="not_found")
class NotFound(ApiError):
    resource: str


@ox.model(tag="conflict")
class Conflict(ApiError):
    existing_id: int


# Requests are written in validation mode, responses in serialization mode, in one bundle.
bundle = ox.json_schema_bundle(
    [
        CreateUser,
        ox.SchemaTarget(User, mode="serialization"),
        ox.SchemaTarget(list[User], mode="serialization"),
        ox.SchemaTarget(NotFound | Conflict, mode="serialization"),
    ],
    ref_template="#/components/schemas/{name}",
)

document = {
    "openapi": "3.1.0",
    "info": {"title": "Example", "version": "1"},
    "paths": {
        "/users": {
            "post": {
                "requestBody": {"content": {"application/json": {"schema": bundle.schemas[0]}}},
                "responses": {
                    "201": {"content": {"application/json": {"schema": bundle.schemas[1]}}},
                    "404": {"content": {"application/json": {"schema": bundle.schemas[3]}}},
                },
            },
            "get": {
                "responses": {
                    "200": {"content": {"application/json": {"schema": bundle.schemas[2]}}}
                },
            },
        }
    },
    "components": {"schemas": bundle.defs},
}

if __name__ == "__main__":
    print(json.dumps(document, indent=2))
