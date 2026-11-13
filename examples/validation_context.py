"""Validators that read a validation context, as a web framework such as Douro would use them
(docs/spec.md, section 4.3).

The framework decodes the parts of a request and passes what it knows (here, who is asking and
which tenant they belong to) as ``context=``; the validators of the models read it. The problems
of every part come back together, with their paths, as one 422 body.

Run it with ``python examples/validation_context.py``.
"""

import json
from dataclasses import dataclass
from typing import Annotated, Any

import oxiform as ox

RESERVED_NAMES = {"admin", "root"}


@dataclass(frozen=True)
class Request:
    """What the framework hands to the validators: any object, passed through as it is."""

    tenant: str
    user: str
    roles: frozenset[str]


@ox.model
class NewMember:
    """The body of ``POST /members``."""

    email: str
    name: Annotated[str, ox.Len(2, 40)]
    role: str = "member"
    password: ox.Secret[str] = ox.field(default=ox.Secret(""))

    @ox.validator("email")
    def in_your_tenant(cls, value: str, request: ox.Context[Request]) -> str:
        # `ox.Context[Request]` is `Request` to the type checker: this is all checked.
        if not value.endswith("@" + request.tenant):
            raise ox.Invalid(f"not an address of {request.tenant}", code="tenant")
        return value.lower()

    @ox.validator("name")
    def not_reserved(cls, value: str) -> str:  # no context needed
        if value.lower() in RESERVED_NAMES:
            raise ox.Invalid("reserved name", code="reserved")
        return value

    @ox.validator("password")
    def strong_enough(cls, value: ox.Secret[str]) -> ox.Secret[str]:
        # A secret arrives as a `Secret`; what is wrong with it is said without quoting it.
        if value.reveal() and len(value.reveal()) < 8:
            raise ox.Invalid("must have at least 8 characters", code="weak")
        return value

    @ox.validator
    def only_admins_make_admins(self, request: ox.Context[Request]) -> None:
        if self.role == "admin" and "admin" not in request.roles:
            raise ox.Invalid("only an admin can make an admin", path="role", code="forbidden")


@ox.model
class MemberQuery:
    """The query string of ``GET /members``."""

    page: Annotated[int, ox.Ge(1)] = 1
    tenant: str | None = None

    @ox.validator("tenant")
    def own_tenant_only(cls, value: str | None, request: ox.Context[Request]) -> str | None:
        if value is not None and value != request.tenant and "admin" not in request.roles:
            raise ox.Invalid("you can only list your own tenant", code="forbidden")
        return value


def handle(request: Request, query: str, body: bytes) -> dict[str, Any]:
    """What a framework does for one request: decode each part with the same context, answer with
    the one problem of everything that was wrong."""
    parts: dict[str, ox.ValidationError] = {}
    values: dict[str, Any] = {}
    try:
        values["query"] = ox.urlencoded.decode(query, MemberQuery, context=request)
    except ox.ValidationError as error:
        parts["query"] = error
    try:
        values["body"] = ox.json.decode(body, NewMember, mode="strict", context=request)
    except ox.ValidationError as error:
        parts["body"] = error
    if parts:
        return ox.ValidationError.combine(parts).to_problem(status=422)
    return values


def main() -> None:
    ann = Request(tenant="acme", user="ann", roles=frozenset({"member"}))
    good = handle(ann, "page=2", b'{"email": "Bo@acme", "name": "Bo"}')
    print("ok:", good["body"].email, "page", good["query"].page)

    problem = handle(
        ann,
        "page=0&tenant=globex",
        b'{"email": "bo@globex", "name": "Admin", "role": "admin", "password": "short"}',
    )
    print(json.dumps(problem, indent=2))


if __name__ == "__main__":
    main()
