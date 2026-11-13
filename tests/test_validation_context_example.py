"""The validation context example runs, and its 422 says where each problem is."""

import runpy
from pathlib import Path
from typing import Any

EXAMPLE = Path(__file__).parent.parent / "examples" / "validation_context.py"


def load() -> dict[str, Any]:
    return runpy.run_path(str(EXAMPLE))


def test_a_good_request_decodes_and_the_validator_normalises_the_email() -> None:
    namespace = load()
    request = namespace["Request"]("acme", "ann", frozenset({"member"}))
    values = namespace["handle"](request, "page=2", b'{"email": "Bo@acme", "name": "Bo"}')
    assert values["body"].email == "bo@acme"
    assert values["query"].page == 2


def test_every_part_reports_together_with_the_source_in_the_path() -> None:
    namespace = load()
    request = namespace["Request"]("acme", "ann", frozenset({"member"}))
    problem = namespace["handle"](
        request,
        "page=0&tenant=globex",
        b'{"email": "bo@globex", "name": "Admin", "role": "admin", "password": "short"}',
    )
    assert problem["status"] == 422
    found = [(e["pointer"], e["code"]) for e in problem["errors"]]
    assert found == [
        ("/query/page", "too_small"),
        ("/query/tenant", "forbidden"),
        ("/body/email", "tenant"),
        ("/body/name", "reserved"),
        ("/body/password", "weak"),
    ]
    assert "short" not in str(problem)


def test_an_admin_may_make_admins_and_the_model_validator_reports_at_its_field() -> None:
    namespace = load()
    member = namespace["Request"]("acme", "ann", frozenset({"member"}))
    problem = namespace["handle"](
        member, "", b'{"email": "bo@acme", "name": "Bo", "role": "admin"}'
    )
    assert [(e["pointer"], e["code"]) for e in problem["errors"]] == [("/body/role", "forbidden")]
    admin = namespace["Request"]("acme", "root", frozenset({"admin"}))
    values = namespace["handle"](admin, "", b'{"email": "bo@acme", "name": "Bo", "role": "admin"}')
    assert values["body"].role == "admin"
