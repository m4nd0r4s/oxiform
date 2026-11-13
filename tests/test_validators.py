"""``@ox.validator``, ``ox.Invalid`` and ``ox.Context`` (spec section 4.3, OX-11)."""

import dataclasses
import gc
import json
import threading
import weakref
from collections.abc import Callable, Mapping
from concurrent.futures import ThreadPoolExecutor
from typing import Annotated, Any, TypedDict

import pytest

import oxiform as ox
from oxiform import cfg


class Tenant(TypedDict):
    tenant: str


RESERVED = {"admin", "root"}


@ox.model
class Signup:
    username: Annotated[str, ox.Len(3, 30)]
    password: str
    password2: str

    @ox.validator("username")
    def not_reserved(cls, v: str) -> str:
        if v in RESERVED:
            raise ox.Invalid("reserved name", code="reserved")
        return v

    @ox.validator("username")
    def lowercase(cls, v: str) -> str:
        return v.lower()

    @ox.validator
    def passwords_match(self) -> None:
        if self.password != self.password2:
            raise ox.Invalid("passwords do not match", path="password2")


def problems(error: ox.ValidationError) -> list[tuple[str, str, str]]:
    return [(e["code"], e["pointer"], e["message"]) for e in error.errors()]


def decode_problems(data: bytes, target: Any, **options: Any) -> list[tuple[str, str, str]]:
    with pytest.raises(ox.ValidationError) as raised:
        ox.json.decode(data, target, **options)
    return problems(raised.value)


# --- field and model validators ---------------------------------------------------------------


def test_a_field_validator_returns_the_value_and_may_replace_it() -> None:
    signup = ox.json.decode(b'{"username": "Ann", "password": "p", "password2": "p"}', Signup)
    assert signup.username == "ann"


def test_ox_invalid_is_a_problem_at_the_field_with_its_own_code() -> None:
    found = decode_problems(b'{"username": "admin", "password": "p", "password2": "p"}', Signup)
    assert found == [("reserved", "/username", "reserved name")]


def test_a_model_validator_rejects_with_a_path_relative_to_the_model() -> None:
    found = decode_problems(b'{"username": "ann", "password": "a", "password2": "b"}', Signup)
    assert found == [("invalid", "/password2", "passwords do not match")]


def test_the_problem_has_no_input_and_the_shape_of_every_other_problem() -> None:
    with pytest.raises(ox.ValidationError) as raised:
        ox.json.decode(b'{"username": "root", "password": "a", "password2": "a"}', Signup)
    (error,) = raised.value.errors()
    assert error == {
        "code": "reserved",
        "path": ["username"],
        "pointer": "/username",
        "message": "reserved name",
        "ctx": {},
    }
    assert raised.value.to_problem()["errors"] == [
        {"pointer": "/username", "code": "reserved", "detail": "reserved name"}
    ]


def test_a_failed_field_stops_the_model_validator_but_the_report_has_every_problem() -> None:
    found = decode_problems(b'{"username": "admin", "password": 1, "password2": "b"}', Signup)
    assert found == [
        ("type", "/password", "expected str, received int"),
        ("reserved", "/username", "reserved name"),
    ]


def test_a_chain_stops_at_the_first_rejection_and_later_validators_do_not_run() -> None:
    calls: list[str] = []

    @ox.model
    class Chain:
        name: str

        @ox.validator("name")
        def first(cls, v: str) -> str:
            calls.append("first")
            return v + "1"

        @ox.validator("name")
        def second(cls, v: str) -> str:
            calls.append("second")
            if v.endswith("1"):
                raise ox.Invalid("second says no")
            return v

        @ox.validator("name")
        def third(cls, v: str) -> str:
            calls.append("third")
            return v

    assert decode_problems(b'{"name": "x"}', Chain) == [("invalid", "/name", "second says no")]
    assert calls == ["first", "second"]


def test_validators_of_one_field_see_what_the_previous_one_returned() -> None:
    @ox.model
    class Steps:
        n: int

        @ox.validator("n")
        def double(cls, v: int) -> int:
            return v * 2

        @ox.validator("n")
        def plus_one(cls, v: int) -> int:
            return v + 1

    assert ox.json.decode(b'{"n": 5}', Steps).n == 11


def test_one_validator_may_name_several_fields() -> None:
    @ox.model
    class Pair:
        a: int
        b: int

        @ox.validator("a", "b")
        def positive(cls, v: int) -> int:
            if v <= 0:
                raise ValueError("must be positive")
            return v

    assert decode_problems(b'{"a": 0, "b": -1}', Pair) == [
        ("invalid", "/a", "must be positive"),
        ("invalid", "/b", "must be positive"),
    ]


def test_a_classmethod_below_or_above_the_decorator_is_accepted() -> None:
    @ox.model
    class Both:
        a: int
        b: int

        @ox.validator("a")
        @classmethod
        def check_a(cls, v: int) -> int:
            return v + 1

        check_b = classmethod(ox.validator("b")(lambda_free_plus_two))  # type: ignore[arg-type]  # pyright: ignore

    both = ox.json.decode(b'{"a": 1, "b": 1}', Both)
    assert (both.a, both.b) == (2, 3)


def lambda_free_plus_two(cls: type, v: int) -> int:
    return v + 2


def test_the_validator_can_be_called_from_the_class_like_a_classmethod() -> None:
    caller: Any = Signup
    assert caller.lowercase("FINE") == "fine"
    with pytest.raises(ox.Invalid):
        caller.not_reserved("root")


# --- ordering, aggregation, nesting -----------------------------------------------------------


def test_validators_run_in_field_order_whatever_the_order_of_the_keys() -> None:
    seen: list[str] = []

    @ox.model
    class Order:
        first: int
        second: int

        @ox.validator("second")
        def see_second(cls, v: int) -> int:
            seen.append("second")
            return v

        @ox.validator("first")
        def see_first(cls, v: int) -> int:
            seen.append("first")
            return v

    ox.json.decode(b'{"second": 1, "first": 2}', Order)
    assert seen == ["first", "second"]


@ox.model
class Item:
    sku: str
    qty: int

    @ox.validator("sku")
    def sku_format(cls, v: str) -> str:
        if not v.startswith("SKU-"):
            raise ox.Invalid("must start with SKU-", code="sku")
        return v


@ox.model
class Cart:
    items: list[Item]
    total: int

    @ox.validator
    def not_empty(self) -> None:
        if not self.items:
            raise ox.Invalid("a cart needs items", path="items")


def test_problems_are_aggregated_with_their_paths_across_items_and_models() -> None:
    found = decode_problems(
        b'{"items": [{"sku": "SKU-1", "qty": 1}, {"sku": "x", "qty": "q"}, {"sku": "y", "qty": 3}],'
        b' "total": "t"}',
        Cart,
    )
    assert found == [
        ("type", "/items/1/qty", "expected int, received str"),
        ("sku", "/items/1/sku", "must start with SKU-"),
        ("sku", "/items/2/sku", "must start with SKU-"),
        ("type", "/total", "expected int, received str"),
    ]


def test_a_model_problem_of_a_nested_model_is_at_that_model() -> None:
    assert decode_problems(b'{"items": [], "total": 1}', Cart) == [
        ("invalid", "/items", "a cart needs items")
    ]

    @ox.model
    class Holder:
        cart: Cart

    assert decode_problems(b'{"cart": {"items": [], "total": 1}}', Holder) == [
        ("invalid", "/cart/items", "a cart needs items")
    ]


def test_a_model_validator_without_a_path_is_at_the_model_itself() -> None:
    @ox.model
    class Range:
        low: int
        high: int

        @ox.validator
        def ordered(self) -> None:
            if self.low > self.high:
                raise ox.Invalid("low is above high")

    @ox.model
    class Wrapper:
        inner: Range

    assert decode_problems(b'{"low": 2, "high": 1}', Range) == [
        ("invalid", "", "low is above high")
    ]
    assert decode_problems(b'{"inner": {"low": 2, "high": 1}}', Wrapper) == [
        ("invalid", "/inner", "low is above high")
    ]


def test_max_errors_counts_validator_problems_and_one_stops_at_the_first() -> None:
    body = b'{"items": [{"sku": "a", "qty": 1}, {"sku": "b", "qty": 2}], "total": 1}'
    with pytest.raises(ox.ValidationError) as raised:
        ox.json.decode(body, Cart, max_errors=1)
    assert len(raised.value.errors()) == 1 and raised.value.truncated
    with pytest.raises(ox.ValidationError) as every:
        ox.json.decode(body, Cart)
    assert len(every.value.errors()) == 2 and not every.value.truncated


def test_invalid_path_maps_a_python_name_to_the_wire_name_and_takes_segments() -> None:
    @ox.model(rename_all="camelCase")
    class Camel:
        first_name: str
        tags: list[str]

        @ox.validator
        def check(self) -> None:
            raise ox.Invalid("names", path="first_name")

    assert decode_problems(b'{"firstName": "a", "tags": []}', Camel) == [
        ("invalid", "/firstName", "names")
    ]

    @ox.model
    class Deep:
        tags: list[str]

        @ox.validator("tags")
        def each(cls, v: list[str]) -> list[str]:
            for index, tag in enumerate(v):
                if tag == "bad":
                    raise ox.Invalid("bad tag", path=(index, "name"))
            return v

    assert decode_problems(b'{"tags": ["ok", "bad"]}', Deep) == [
        ("invalid", "/tags/1/name", "bad tag")
    ]


# --- exceptions -------------------------------------------------------------------------------


def test_value_error_and_type_error_are_problems_and_other_exceptions_propagate() -> None:
    @ox.model
    class Raises:
        kind: str

        @ox.validator("kind")
        def go(cls, v: str) -> str:
            if v == "value":
                raise ValueError("no good")
            if v == "type":
                raise TypeError("wrong")
            if v == "empty":
                raise ValueError()
            if v == "key":
                raise KeyError("missing")
            if v == "zero":
                return str(1 // 0)
            return v

    assert decode_problems(b'{"kind": "value"}', Raises) == [("invalid", "/kind", "no good")]
    assert decode_problems(b'{"kind": "type"}', Raises) == [("invalid", "/kind", "wrong")]
    assert decode_problems(b'{"kind": "empty"}', Raises) == [("invalid", "/kind", "invalid value")]
    with pytest.raises(KeyError):
        ox.json.decode(b'{"kind": "key"}', Raises)
    with pytest.raises(ZeroDivisionError):
        ox.json.decode(b'{"kind": "zero"}', Raises)


def test_a_nested_decode_error_inside_a_validator_is_an_invalid_problem() -> None:
    @ox.model
    class Embedded:
        blob: str

        @ox.validator("blob")
        def parses(cls, v: str) -> str:
            ox.json.decode(v.encode(), int)
            return v

    found = decode_problems(b'{"blob": "\\"x\\""}', Embedded)
    assert found[0][:2] == ("invalid", "/blob")


def test_ox_invalid_survives_pickle_and_copy() -> None:
    import copy
    import pickle

    error = ox.Invalid("text", code="c", path=("a", 1))
    for clone in (pickle.loads(pickle.dumps(error)), copy.copy(error)):  # noqa: S301
        assert isinstance(clone, ox.Invalid)
        assert (clone.message, clone.code, clone.path) == ("text", "c", ("a", 1))


def test_ox_invalid_is_a_value_error_and_checks_its_arguments() -> None:
    error = ox.Invalid("text", code="c", path=("a", 1))
    assert isinstance(error, ValueError) and isinstance(error, ox.OxiformError)
    assert (error.message, error.code, error.path) == ("text", "c", ("a", 1))
    assert ox.Invalid("x").code == "invalid" and ox.Invalid("x").path == ()
    assert ox.Invalid("x", path="a").path == ("a",) and ox.Invalid("x", path=0).path == (0,)
    bad: list[dict[str, Any]] = [
        {"code": ""},
        {"code": 3},
        {"path": 1.5},
        {"path": (True,)},
        {"path": (-1,)},
    ]
    for arguments in bad:
        with pytest.raises(TypeError):
            ox.Invalid("x", **arguments)
    not_text: Any = 3
    with pytest.raises(TypeError, match="message as a str"):
        ox.Invalid(not_text)


# --- the validation context -------------------------------------------------------------------


@ox.model
class Member:
    email: str
    role: str = "user"

    @ox.validator("email")
    def in_tenant(cls, v: str, ctx: ox.Context[Tenant]) -> str:
        if not v.endswith("@" + ctx["tenant"]):
            raise ox.Invalid(f"not an address of {ctx['tenant']}")
        return v

    @ox.validator
    def admins_are_internal(self, ctx: ox.Context[Tenant]) -> None:
        if self.role == "admin" and not self.email.startswith("it@"):
            raise ox.Invalid(f"only IT may be an admin of {ctx['tenant']}", path="role")


def test_a_field_and_a_model_validator_receive_the_context_of_decode() -> None:
    member = ox.json.decode(b'{"email": "a@acme"}', Member, context={"tenant": "acme"})
    assert member.email == "a@acme"
    assert decode_problems(b'{"email": "a@other"}', Member, context={"tenant": "acme"}) == [
        ("invalid", "/email", "not an address of acme")
    ]
    assert decode_problems(
        b'{"email": "a@acme", "role": "admin"}', Member, context={"tenant": "acme"}
    ) == [("invalid", "/role", "only IT may be an admin of acme")]


def test_the_context_is_the_same_object_not_a_copy_and_any_object_works() -> None:
    received: list[object] = []

    class Request:
        user = "ann"

    @ox.model
    class Needs:
        n: int

        @ox.validator("n")
        def look(cls, v: int, ctx: ox.Context[Request]) -> int:
            received.append(ctx)
            return v

        @ox.validator
        def look_again(self, ctx: ox.Context[Request]) -> None:
            received.append(ctx)

    request = Request()
    ox.json.decode(b'{"n": 1}', Needs, context=request)
    assert len(received) == 2 and received[0] is request and received[1] is request

    mapping: Mapping[str, int] = {"a": 1}
    received.clear()
    ox.json.decode(b'{"n": 1}', Needs, context=mapping)
    assert received[0] is mapping


def test_no_context_is_a_clear_type_error_that_says_to_pass_one() -> None:
    with pytest.raises(TypeError) as raised:
        ox.json.decode(b'{"email": "a@acme"}', Member)
    message = str(raised.value)
    assert "Member.in_tenant" in message
    assert "context=" in message
    assert not isinstance(raised.value, ox.ValidationError)
    with pytest.raises(TypeError, match="needs a validation context"):
        ox.json.decode(b'{"email": "a@acme"}', Member, context=None)


def test_a_missing_context_is_only_an_error_where_a_validator_runs() -> None:
    # The type is rejected before the validator would run, so no context is needed.
    assert decode_problems(b'{"email": 5}', Member)[0][:2] == ("type", "/email")

    # A field the input does not give is never validated.
    @ox.model
    class Absent:
        email: str = "x@y"

        @ox.validator("email")
        def in_tenant(cls, v: str, ctx: ox.Context[Tenant]) -> str:
            return v

    assert ox.json.decode(b"{}", Absent).email == "x@y"


def test_a_context_parameter_with_a_default_or_none_is_optional() -> None:
    @ox.model
    class Maybe:
        n: int

        @ox.validator("n")
        def by_default(cls, v: int, ctx: ox.Context[Tenant] | None = None) -> int:
            return v + (1 if ctx else 0)

        @ox.validator("n")
        def only_none(cls, v: int, ctx: ox.Context[Tenant] | None) -> int:
            return v * (10 if ctx else 1)

    assert ox.json.decode(b'{"n": 1}', Maybe).n == 1
    assert ox.json.decode(b'{"n": 1}', Maybe, context={"tenant": "t"}).n == (1 + 1) * 10


def test_the_context_may_sit_anywhere_after_the_value_and_be_keyword_only() -> None:
    @ox.model
    class Where:
        n: int

        @ox.validator("n")
        def keyword(cls, v: int, *, ctx: ox.Context) -> int:
            return v + ctx["add"]

    assert ox.json.decode(b'{"n": 1}', Where, context={"add": 2}).n == 3


def test_an_annotation_as_a_string_to_a_name_defined_later_resolves_at_first_use() -> None:
    @ox.model
    class Lazy:
        n: int

        @ox.validator("n")
        def check(cls, v: int, ctx: "ox.Context[LaterContext]") -> int:
            return v + ctx.extra

    class LaterContext:
        extra = 5

    globals()["LaterContext"] = LaterContext
    try:
        assert ox.json.decode(b'{"n": 1}', Lazy, context=LaterContext()).n == 6
    finally:
        del globals()["LaterContext"]


def test_context_reaches_every_decode_entry_point() -> None:
    @ox.model
    class Query:
        page: int = 1
        q: str = ""

        @ox.validator("page")
        def check_page(cls, v: int, ctx: ox.Context) -> int:
            return v + ctx["bump"]

        @ox.validator
        def whole(self, ctx: ox.Context) -> None:
            if self.q == ctx["forbidden"]:
                raise ox.Invalid("forbidden query", path="q")

    context = {"bump": 10, "forbidden": "bad"}
    assert ox.json.decode(b'{"page": 1}', Query, context=context).page == 11
    assert ox.json.Decoder(Query).decode(b'{"page": 2}', context=context).page == 12
    assert ox.urlencoded.decode("page=3", Query, context=context).page == 13
    assert ox.pairs.decode([("page", "4")], Query, context=context).page == 14
    with pytest.raises(ox.ValidationError) as raised:
        ox.urlencoded.decode("page=1&q=bad", Query, context=context)
    assert problems(raised.value) == [("invalid", "/q", "forbidden query")]
    with pytest.raises(TypeError, match="context="):
        ox.pairs.decode([("page", "4")], Query)
    with pytest.raises(TypeError, match="context="):
        ox.json.Decoder(Query).decode(b'{"page": 1}')


def test_lax_mode_validators_see_the_coerced_value() -> None:
    @ox.model
    class Coerced:
        n: int

        @ox.validator("n")
        def check(cls, v: int) -> int:
            assert type(v) is int
            return v + 1

    assert ox.json.decode(b'{"n": "41"}', Coerced, mode="lax").n == 42
    assert ox.urlencoded.decode("n=41", Coerced).n == 42


def test_the_context_is_not_kept_after_the_decode() -> None:
    class Marker:
        pass

    @ox.model
    class Holds:
        n: int

        @ox.validator("n")
        def look(cls, v: int, ctx: ox.Context) -> int:
            return v

    marker = Marker()
    reference = weakref.ref(marker)
    ox.json.decode(b'{"n": 1}', Holds, context=marker)
    del marker
    gc.collect()
    assert reference() is None


# --- inheritance, flatten, defaults ------------------------------------------------------------


def test_a_base_validator_runs_before_the_subclass_and_a_redefinition_replaces_it() -> None:
    order: list[str] = []

    @ox.model
    class Base:
        n: int

        @ox.validator("n")
        def base_one(cls, v: int) -> int:
            order.append("base_one")
            return v

        @ox.validator("n")
        def shared(cls, v: int) -> int:
            order.append("base_shared")
            return v

    @ox.model
    class Child(Base):
        @ox.validator("n")
        def child_one(cls, v: int) -> int:
            order.append("child_one")
            return v

        @ox.validator("n")
        def shared(cls, v: int) -> int:
            order.append("child_shared")
            return v

    ox.json.decode(b'{"n": 1}', Child)
    assert order == ["base_one", "child_one", "child_shared"]
    order.clear()
    ox.json.decode(b'{"n": 1}', Base)
    assert order == ["base_one", "base_shared"]


def test_a_validator_receives_the_class_being_decoded() -> None:
    seen: list[object] = []

    @ox.model
    class Base:
        n: int

        @ox.validator("n")
        def remember(cls, v: int) -> int:
            seen.append(cls)
            return v

    @ox.model
    class Child(Base):
        pass

    ox.json.decode(b'{"n": 1}', Base)
    ox.json.decode(b'{"n": 1}', Child)
    assert seen == [Base, Child]


def test_a_subclass_that_redefines_the_name_as_a_plain_method_removes_the_validator() -> None:
    @ox.model
    class Base:
        n: int

        @ox.validator("n")
        def check(cls, v: int) -> int:
            raise ValueError("always")

    @ox.model
    class Child(Base):
        def check(self, v: int) -> int:  # not a validator any more
            return v

    assert decode_problems(b'{"n": 1}', Base) == [("invalid", "/n", "always")]
    assert ox.json.decode(b'{"n": 1}', Child).n == 1


def test_flattened_models_keep_their_field_validators_and_refuse_model_validators() -> None:
    @ox.model
    class Inner:
        a: int

        @ox.validator("a")
        def positive(cls, v: int) -> int:
            if v < 0:
                raise ValueError("negative")
            return v

    @ox.model
    class Outer:
        b: int
        inner: Inner = ox.field(flatten=True)

    assert decode_problems(b'{"a": -1, "b": 1}', Outer) == [("invalid", "/a", "negative")]

    @ox.model
    class WithModel:
        a: int

        @ox.validator
        def whole(self) -> None:
            pass

    with pytest.raises(ox.SchemaError, match="flattens.*Move it"):

        @ox.model
        class Bad:
            inner: WithModel = ox.field(flatten=True)


def test_a_default_is_not_validated() -> None:
    @ox.model
    class Defaulted:
        n: int = -1

        @ox.validator("n")
        def positive(cls, v: int) -> int:
            raise ValueError("never reached for a default")

    assert ox.json.decode(b"{}", Defaulted).n == -1
    assert decode_problems(b'{"n": 5}', Defaulted) == [
        ("invalid", "/n", "never reached for a default")
    ]


def test_a_validator_in_a_union_variant_runs_and_a_rejection_is_reported() -> None:
    @ox.model(tag_field="kind", tag="a")
    class A:
        n: int

        @ox.validator("n")
        def small(cls, v: int) -> int:
            if v > 5:
                raise ValueError("too big")
            return v

    @ox.model(tag_field="kind", tag="b")
    class B:
        n: int

    @ox.model
    class Holder:
        item: Annotated[A | B, ox.Tagged("kind")]

    assert ox.json.decode(b'{"item": {"kind": "a", "n": 1}}', Holder).item == A(1)
    assert decode_problems(b'{"item": {"kind": "a", "n": 9}}', Holder) == [
        ("invalid", "/item/n", "too big")
    ]


def test_a_validator_on_a_frozen_model_reads_the_instance() -> None:
    @ox.model(frozen=True)
    class Frozen:
        n: int

        @ox.validator
        def check(self) -> None:
            if self.n < 0:
                raise ox.Invalid("negative", path="n")

    assert decode_problems(b'{"n": -1}', Frozen) == [("invalid", "/n", "negative")]
    assert ox.json.decode(b'{"n": 1}', Frozen).n == 1


def test_a_validator_works_on_a_plain_dataclass_and_on_ox_model() -> None:
    @dataclasses.dataclass
    class Plain:
        n: int

        @ox.validator("n")
        def positive(cls, v: int) -> int:
            if v < 0:
                raise ValueError("negative")
            return v

    class Based(ox.Model):
        n: int

        @ox.validator("n")
        def positive(cls, v: int) -> int:
            if v < 0:
                raise ValueError("negative")
            return v

    assert decode_problems(b'{"n": -1}', Plain) == [("invalid", "/n", "negative")]
    assert decode_problems(b'{"n": -1}', Based) == [("invalid", "/n", "negative")]


# --- a validator that returns the wrong thing -------------------------------------------------


def test_a_field_validator_that_forgets_to_return_is_a_schema_error_unless_none_is_allowed() -> (
    None
):
    @ox.model
    class Forgot:
        name: str

        @ox.validator("name")
        def check(cls, v: str) -> str:
            if not v:
                raise ValueError("empty")
            return None  # type: ignore[return-value]  # pyright: ignore[reportReturnType]

    with pytest.raises(ox.SchemaError, match="returned None for the field 'name'.*return"):
        ox.json.decode(b'{"name": "x"}', Forgot)

    @ox.model
    class Allowed:
        name: str | None

        @ox.validator("name")
        def blank_is_none(cls, v: str | None) -> str | None:
            return None if v == "" else v

    assert ox.json.decode(b'{"name": ""}', Allowed).name is None


def test_a_model_validator_that_returns_something_is_a_schema_error() -> None:
    @ox.model
    class Returns:
        n: int

        @ox.validator
        def check(self) -> int:
            return 1

    with pytest.raises(ox.SchemaError, match="returns None, not int"):
        ox.json.decode(b'{"n": 1}', Returns)


# --- secrets ----------------------------------------------------------------------------------


def test_a_validator_on_a_secret_gets_the_secret_and_a_bare_error_does_not_leak_it() -> None:
    @ox.model
    class Login:
        password: ox.Secret[str]
        user: str

        @ox.validator("password")
        def numeric(cls, v: ox.Secret[str]) -> ox.Secret[str]:
            assert isinstance(v, ox.Secret)
            int(v.reveal())  # raises a ValueError that quotes the secret
            return v

        @ox.validator("user")
        def user_check(cls, v: str) -> str:
            int(v)
            return v

    found = decode_problems(b'{"password": "hunter2", "user": "ann"}', Login)
    assert found[0][:2] == ("invalid", "/password")
    assert "hunter2" not in found[0][2] and "withheld" in found[0][2]
    # A plain field keeps what the exception says, as it holds no secret.
    assert "invalid literal" in found[1][2]
    assert all("hunter2" not in text for entry in found for text in entry)


def test_ox_invalid_keeps_its_message_for_a_secret_because_its_author_wrote_it() -> None:
    @ox.model
    class Login:
        password: ox.Secret[str]

        @ox.validator("password")
        def long_enough(cls, v: ox.Secret[str]) -> ox.Secret[str]:
            if len(v.reveal()) < 8:
                raise ox.Invalid("must have 8 characters", code="short")
            return v

    assert decode_problems(b'{"password": "x"}', Login) == [
        ("short", "/password", "must have 8 characters")
    ]


def test_a_model_validator_message_is_withheld_when_the_model_holds_a_secret_anywhere() -> None:
    @ox.model
    class Inner:
        token: ox.Secret[str]

    @ox.model
    class Outer:
        inners: list[Inner]
        name: str

        @ox.validator
        def check(self) -> None:
            int(self.inners[0].token.reveal())

    found = decode_problems(b'{"inners": [{"token": "s3cret"}], "name": "n"}', Outer)
    assert found[0][0] == "invalid" and "s3cret" not in found[0][2]

    @ox.model
    class NoSecret:
        name: str

        @ox.validator
        def check(self) -> None:
            raise ValueError(f"name is {self.name}")

    assert decode_problems(b'{"name": "n"}', NoSecret) == [("invalid", "", "name is n")]


# --- the declaration is checked ---------------------------------------------------------------


def test_mode_before_is_refused_with_the_reason() -> None:
    before: Any = ox.validator
    with pytest.raises(ox.SchemaError, match='only mode="after".*once|one'):
        before("x", mode="before")


def test_a_validator_naming_a_field_that_does_not_exist_suggests_the_closest() -> None:
    with pytest.raises(ox.SchemaError, match="no such field.*did you mean 'username'"):

        @ox.model
        class Typo:
            username: str

            @ox.validator("usrname")
            def check(cls, v: str) -> str:
                return v


def test_a_validator_naming_a_field_that_is_not_read_is_refused() -> None:
    with pytest.raises(ox.SchemaError, match="does not give"):

        @ox.model
        class Skipped:
            a: int
            b: int = ox.field(default=0, skip_decode=True)

            @ox.validator("b")
            def check(cls, v: int) -> int:
                return v


def _plain(cls: object, v: object) -> object:
    return v


_any_validator: Any = ox.validator


@pytest.mark.parametrize(
    "build",
    [
        lambda: _any_validator(),
        lambda: _any_validator(3),
        lambda: _any_validator("a", "a"),
        lambda: _any_validator("a")(lambda cls, v: v),  # pyright: ignore
        lambda: _any_validator("a")(staticmethod(_plain)),
        lambda: _any_validator("a")(42),
        lambda: _any_validator(classmethod(_plain)),
    ],
)
def test_misuse_of_the_decorator_is_a_schema_error(build: Callable[[], object]) -> None:
    with pytest.raises(ox.SchemaError):
        build()


def test_an_async_or_generator_validator_is_refused_and_says_why() -> None:
    async def coroutine(cls: Any, v: int) -> int:
        return v

    def generator(cls: Any, v: int) -> Any:
        yield v

    with pytest.raises(ox.SchemaError, match="not asynchronous"):
        ox.validator("a")(coroutine)
    with pytest.raises(ox.SchemaError, match="ordinary function"):
        ox.validator("a")(generator)


def _function(source: str) -> Callable[..., object]:
    scope: dict[str, Any] = {"ox": ox}
    exec(source, scope)  # builds a function whose signature is the point of the test
    return scope["f"]


@pytest.mark.parametrize(
    ("source", "fragment"),
    [
        ("def f(cls): ...", "cannot be called as a validator"),
        ("def f(cls, v, extra): ...", "cannot be called as a validator"),
        ("def f(cls, v, a: ox.Context, b: ox.Context): ...", "takes the context once"),
        ("def f(cls, v, ctx: ox.Context, /): ...", "cannot be positional-only"),
        ("def f(cls, ctx: ox.Context): ...", "not the context"),
    ],
)
def test_a_validator_that_cannot_be_called_as_the_spec_says_is_refused_when_compiled(
    source: str, fragment: str
) -> None:
    namespace: dict[str, Any] = {
        "__annotations__": {"a": int},
        "check": ox.validator("a")(_function(source)),
    }
    with pytest.raises(ox.SchemaError, match=fragment):
        ox.model(type("Wrong", (), namespace))


def test_a_whole_model_validator_cannot_be_a_classmethod_in_either_order() -> None:
    with pytest.raises(ox.SchemaError, match="receives the instance"):
        ox.validator(classmethod(_plain))  # pyright: ignore
    spec: Any = ox.validator(_plain)
    with pytest.raises(ox.SchemaError, match="do not make it a classmethod"):
        ox.model(type("Wrong", (), {"__annotations__": {"a": int}, "check": classmethod(spec)}))


# --- cfg.load ---------------------------------------------------------------------------------


def test_config_classes_take_validators_and_the_context_of_load() -> None:
    @ox.config
    class Db:
        pool_size: int = 5

        @ox.validator("pool_size")
        def within_budget(cls, v: int, ctx: ox.Context) -> int:
            if v > ctx["max_pool"]:
                raise ox.Invalid(f"pool_size above {ctx['max_pool']}", code="budget")
            return v

    @ox.config
    class Settings:
        db: Db = ox.field(default_factory=Db)
        name: str = "app"

        @ox.validator
        def named(self, ctx: ox.Context) -> None:
            if self.name == ctx["reserved"]:
                raise ox.Invalid("reserved name", path="name")

    sources = [cfg.mapping({"db": {"pool_size": 50}, "name": "app"})]
    with pytest.raises(ox.ValidationError) as raised:
        cfg.load(Settings, sources, context={"max_pool": 10, "reserved": "x"})
    assert [e["code"] for e in raised.value.errors()] == ["budget"]
    assert "pool_size" in raised.value.errors()[0]["pointer"]
    settings = cfg.load(Settings, sources, context={"max_pool": 100, "reserved": "x"})
    assert settings.db.pool_size == 50
    with pytest.raises(TypeError, match="context="):
        cfg.load(Settings, sources)
    with pytest.raises(ox.ValidationError) as model_problem:
        cfg.load(Settings, [cfg.mapping({"name": "x"})], context={"max_pool": 1, "reserved": "x"})
    assert model_problem.value.errors()[0]["code"] == "invalid"


# --- the validators and the other parts of the library ----------------------------------------


def test_validators_do_not_change_the_json_schema_and_encode_never_runs_them() -> None:
    @ox.model
    class Plain:
        n: int

    @ox.model
    class Checked:
        n: int

        @ox.validator("n")
        def positive(cls, v: int) -> int:
            raise ValueError("never on encode")

    plain = json.dumps(ox.json_schema(Plain), sort_keys=True)
    checked = json.dumps(ox.json_schema(Checked), sort_keys=True)
    assert checked.replace("Checked", "Plain") == plain
    assert ox.json.encode(Checked(1)) == b'{"n":1}'


def test_a_type_with_no_validators_decodes_the_same_with_or_without_a_context() -> None:
    @ox.model
    class Plain:
        n: int

    assert ox.json.decode(b'{"n": 1}', Plain, context={"x": 1}) == Plain(1)
    assert ox.json.decode(b'{"n": 1}', Plain) == Plain(1)


# --- threads ----------------------------------------------------------------------------------


def test_threads_decode_through_one_plan_with_their_own_contexts() -> None:
    barrier = threading.Barrier(8)

    @ox.model
    class Shared:
        n: int

        @ox.validator("n")
        def add(cls, v: int, ctx: ox.Context) -> int:
            return v + ctx["add"]

    ox.warmup(Shared)

    def work(index: int) -> list[int]:
        barrier.wait()
        return [
            ox.json.decode(b'{"n": 1}', Shared, context={"add": index * 1000 + step}).n
            for step in range(200)
        ]

    with ThreadPoolExecutor(8) as pool:
        results = list(pool.map(work, range(8)))
    for index, values in enumerate(results):
        assert values == [1 + index * 1000 + step for step in range(200)]
