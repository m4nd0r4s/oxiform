"""``ox.Secret[T]``: hidden in repr, str, logs, errors, encode and the schema (spec section 6.6)."""

import copy
import json
import logging
import pickle
from typing import Annotated, Any

import pytest
from schema_support import Checker

import oxiform as ox

HUNTER = "hunter2-correct-horse"


@ox.model
class Login:
    user: str
    password: ox.Secret[str]


@ox.model
class Tokens:
    values: list[ox.Secret[str]]
    spare: ox.Secret[str] | None = None
    by_name: dict[str, ox.Secret[int]] = ox.field(default_factory=dict[str, ox.Secret[int]])


def schema_of(target: Any, **options: Any) -> Any:
    return ox.json_schema_bundle([target], **options).schemas[0]


def defs_of(target: Any, **options: Any) -> dict[str, Any]:
    return ox.json_schema_bundle([target], **options).defs


# -- the class ---------------------------------------------------------------------------------


def test_a_secret_shows_a_mask_everywhere_it_is_printed(caplog: pytest.LogCaptureFixture) -> None:
    secret = ox.Secret(HUNTER)
    assert repr(secret) == "Secret('********')"
    assert str(secret) == "********"
    assert f"{secret}" == "********"
    assert f"{secret!r}" == "Secret('********')"
    assert f"{secret:>20}" == "********"
    assert "%s %r" % (secret, secret) == "******** Secret('********')"  # noqa: UP031
    assert format(secret, "") == "********"
    with caplog.at_level(logging.INFO):
        logging.getLogger("test").info("password is %s and %r", secret, secret)
    assert HUNTER not in caplog.text
    assert "password is ******** and Secret('********')" in caplog.text


def test_the_mask_does_not_say_how_long_the_value_is() -> None:
    assert str(ox.Secret("a")) == str(ox.Secret("a" * 500)) == str(ox.Secret(""))


def test_reveal_is_the_only_way_back() -> None:
    secret = ox.Secret(HUNTER)
    assert secret.reveal() == HUNTER
    assert not hasattr(secret, "__dict__")
    assert HUNTER not in repr(vars(type(secret)))
    assert ox.Secret(b"key").reveal() == b"key"
    assert ox.Secret(7).reveal() == 7


def test_a_secret_is_immutable() -> None:
    secret = ox.Secret(HUNTER)
    with pytest.raises(AttributeError, match="immutable"):
        secret.__setattr__("_value", "other")
    with pytest.raises(AttributeError, match="immutable"):
        secret.__delattr__("_value")
    assert secret.reveal() == HUNTER


def test_secrets_compare_by_value_and_never_equal_a_bare_value() -> None:
    assert ox.Secret("a") == ox.Secret("a")
    assert ox.Secret("a") != ox.Secret("b")
    assert ox.Secret(b"a") == ox.Secret(b"a")
    assert ox.Secret(1) == ox.Secret(1)
    assert ox.Secret("a") != "a"
    assert "a" != ox.Secret("a")
    assert hash(ox.Secret("a")) == hash(ox.Secret("a"))
    assert len({ox.Secret("a"), ox.Secret("a"), ox.Secret("b")}) == 2


def test_truthiness_is_that_of_the_value_and_shows_nothing_more() -> None:
    assert ox.Secret("x")
    assert not ox.Secret("")
    assert not ox.Secret(0)


def test_a_secret_survives_pickle_and_copy_because_workers_need_the_value() -> None:
    secret = ox.Secret(HUNTER)
    assert pickle.loads(pickle.dumps(secret)).reveal() == HUNTER
    assert copy.copy(secret).reveal() == HUNTER
    assert copy.deepcopy(secret).reveal() == HUNTER
    assert repr(pickle.loads(pickle.dumps(secret))) == "Secret('********')"


def test_a_secret_cannot_hold_a_secret() -> None:
    with pytest.raises(TypeError, match="already hides its value"):
        ox.Secret(ox.Secret("x"))


def test_a_secret_works_as_a_dataclass_default_and_in_a_frozen_hashable_config() -> None:
    @ox.model(frozen=True)
    class Holder:
        token: ox.Secret[str] = ox.Secret("fallback")

    assert Holder().token.reveal() == "fallback"
    assert hash(Holder()) == hash(Holder())
    assert "fallback" not in repr(Holder())


# -- decoding ----------------------------------------------------------------------------------


def test_decode_wraps_the_value_of_a_secret_field() -> None:
    login = ox.json.decode(b'{"user": "ana", "password": "hunter2-correct-horse"}', Login)
    assert isinstance(login.password, ox.Secret)
    assert login.password.reveal() == HUNTER
    assert HUNTER not in repr(login)
    assert HUNTER not in str(login)


def test_decode_reads_secrets_in_lists_optionals_and_dicts() -> None:
    tokens = ox.json.decode(b'{"values": ["a", "b"], "spare": "c", "by_name": {"x": 1}}', Tokens)
    assert [item.reveal() for item in tokens.values] == ["a", "b"]
    assert tokens.spare is not None and tokens.spare.reveal() == "c"
    assert tokens.by_name["x"].reveal() == 1
    assert ox.json.decode(b'{"values": [], "spare": null}', Tokens).spare is None


def test_a_secret_follows_lax_mode_of_its_type() -> None:
    @ox.model
    class Pin:
        code: ox.Secret[int]
        maybe: ox.Secret[int] | None = None

    pin = ox.json.decode(b'{"code": "1234", "maybe": ""}', Pin, mode="lax")
    assert pin.code.reveal() == 1234
    assert pin.maybe is None
    with pytest.raises(ox.ValidationError):
        ox.json.decode(b'{"code": "1234"}', Pin)


def test_a_secret_is_read_from_a_query_string_and_from_pairs() -> None:
    assert ox.urlencoded.decode("user=ana&password=p%40ss", Login).password.reveal() == "p@ss"
    assert ox.pairs.decode({"user": "ana", "password": "x"}, Login).password.reveal() == "x"


def test_constraints_apply_to_the_real_value_inside_or_outside() -> None:
    @ox.model
    class Inside:
        key: ox.Secret[Annotated[str, ox.Len(8)]]

    @ox.model
    class Outside:
        key: Annotated[ox.Secret[str], ox.Len(8)]

    for model in (Inside, Outside):
        assert ox.json.decode(b'{"key": "long-enough"}', model).key.reveal() == "long-enough"
        with pytest.raises(ox.ValidationError) as caught:
            ox.json.decode(b'{"key": "short"}', model)
        (error,) = caught.value.errors()
        assert error["code"] == "too_short"
        assert error["pointer"] == "/key"


def test_a_problem_of_a_secret_has_no_input_and_never_the_text() -> None:
    @ox.model
    class Pin:
        code: ox.Secret[int]
        other: int = 0

    with pytest.raises(ox.ValidationError) as caught:
        ox.json.decode(b'{"code": "hunter2-correct-horse", "other": "visible-text"}', Pin)
    errors = caught.value.errors()
    by_pointer = {error["pointer"]: error for error in errors}
    assert by_pointer["/code"]["code"] == "type"
    assert "input" not in by_pointer["/code"]
    assert by_pointer["/other"].get("input") == "visible-text"
    shown = " ".join([str(caught.value), repr(errors), repr(caught.value.to_problem())])
    assert HUNTER not in shown
    assert "visible-text" in shown


def test_a_secret_of_a_union_redacts_each_variant_of_the_union() -> None:
    @ox.model
    class Either:
        value: ox.Secret[int | bool]

    with pytest.raises(ox.ValidationError) as caught:
        ox.json.decode(b'{"value": "hunter2-correct-horse"}', Either)
    assert HUNTER not in repr(caught.value.errors()) + str(caught.value)


def test_a_model_inside_a_secret_is_hidden_as_a_whole() -> None:
    @ox.model
    class Credentials:
        user: str
        password: str

    @ox.model
    class Holder:
        credentials: ox.Secret[Credentials]

    with pytest.raises(ox.ValidationError) as caught:
        ox.json.decode(b'{"credentials": {"user": 1, "password": "hunter2"}}', Holder)
    assert "hunter2" not in repr(caught.value.errors())
    holder = ox.json.decode(b'{"credentials": {"user": "a", "password": "hunter2"}}', Holder)
    assert "hunter2" not in repr(holder)
    assert holder.credentials.reveal().password == "hunter2"


def test_secret_declarations_that_cannot_work_say_what_to_write() -> None:
    with pytest.raises(ox.SchemaError, match=r"Secret\[str\]"):

        @ox.model
        class Bare:
            key: ox.Secret  # type: ignore[type-arg]  # pyright: ignore[reportMissingTypeArgument]

    with pytest.raises(ox.SchemaError, match="cannot hold a Secret"):

        @ox.model
        class Nested:
            key: ox.Secret[ox.Secret[str]]

    with pytest.raises(ox.SchemaError, match=r"Secret\[A \| B\]"):

        @ox.model
        class InUnion:
            key: ox.Secret[str] | int

    with pytest.raises(ox.SchemaError, match=r"Secret\[A \| B\]"):

        @ox.model
        class InMarkedUnion:
            key: Annotated[ox.Secret[str] | int, ox.Untagged()]

    with pytest.raises(ox.SchemaError, match="keys must be str"):

        @ox.model
        class AsKey:
            key: dict[ox.Secret[str], int]


# -- encoding ----------------------------------------------------------------------------------


def encoded(obj: object, **options: Any) -> Any:
    return json.loads(ox.json.encode(obj, **options))


def test_encode_writes_the_mask_for_a_secret_and_never_the_value() -> None:
    login = Login(user="ana", password=ox.Secret(HUNTER))
    raw = ox.json.encode(login)
    assert HUNTER.encode() not in raw
    assert json.loads(raw) == {"user": "ana", "password": "********"}
    assert encoded(ox.Secret(HUNTER)) == "********"
    assert encoded({"k": [ox.Secret(HUNTER)]}) == {"k": ["********"]}
    assert encoded(ox.Secret(b"key")) == "********"
    assert encoded(ox.Secret(12)) == "********"
    assert encoded(ox.Secret(["a", "b"])) == "********"


def test_encode_masks_a_plain_value_in_a_field_declared_secret() -> None:
    login = Login(user="ana", password=HUNTER)  # type: ignore[arg-type]  # pyright: ignore[reportArgumentType]
    assert encoded(login) == {"user": "ana", "password": "********"}
    tokens = Tokens(values=[HUNTER, "b"], spare=HUNTER, by_name={"x": 7})  # type: ignore[list-item]  # pyright: ignore[reportArgumentType]
    assert encoded(tokens) == {
        "values": ["********", "********"],
        "spare": "********",
        "by_name": {"x": "********"},
    }


def test_encode_writes_null_for_an_absent_optional_secret() -> None:
    tokens = Tokens(values=[], spare=None)
    assert encoded(tokens)["spare"] is None


def test_encode_masks_with_every_option_and_with_a_reusable_encoder() -> None:
    login = Login(user="ana", password=ox.Secret(HUNTER))
    assert encoded(login, sort_keys=True) == {"password": "********", "user": "ana"}
    assert HUNTER.encode() not in ox.json.encode(login, indent=2)
    assert HUNTER.encode() not in ox.json.Encoder().encode(login)
    assert HUNTER.encode() not in ox.json.Encoder(type=Login).encode(login)


def test_encode_as_a_declared_secret_masks_whatever_is_given() -> None:
    assert encoded(HUNTER, type=ox.Secret[str]) == "********"
    assert encoded(ox.Secret(HUNTER), type=ox.Secret[str]) == "********"
    assert encoded({"a": HUNTER}, type=dict[str, ox.Secret[str]]) == {"a": "********"}
    assert encoded([HUNTER], type=list[ox.Secret[str]]) == ["********"]
    assert encoded(None, type=ox.Secret[str] | None) is None
    assert encoded(HUNTER, type=ox.Secret[str] | None) == "********"
    assert encoded(Login(user="ana", password=ox.Secret(HUNTER)), type=Login) == {
        "user": "ana",
        "password": "********",
    }


def test_there_is_no_switch_that_reveals_and_reveal_is_explicit() -> None:
    login = Login(user="ana", password=ox.Secret(HUNTER))
    with pytest.raises(TypeError):
        ox.json.encode(login, secrets="reveal")  # type: ignore[call-arg]  # pyright: ignore[reportCallIssue]
    plain = {"user": login.user, "password": login.password.reveal()}
    assert encoded(plain) == {"user": "ana", "password": HUNTER}


def test_a_secret_round_trips_through_the_mask_only_as_a_mask() -> None:
    login = Login(user="ana", password=ox.Secret(HUNTER))
    again = ox.json.decode(ox.json.encode(login), Login)
    assert again.password.reveal() == "********"
    assert again.password != login.password


# -- JSON Schema -------------------------------------------------------------------------------


def test_the_validation_schema_marks_a_secret_write_only() -> None:
    properties = defs_of(Login)["Login"]["properties"]
    assert properties["password"] == {
        "type": "string",
        "writeOnly": True,
        "format": "password",
    }
    assert properties["user"] == {"type": "string"}


def test_a_secret_that_is_not_a_plain_string_has_no_password_format() -> None:
    @ox.model
    class Pin:
        code: ox.Secret[int]
        blob: ox.Secret[bytes]
        stamp: ox.Secret[Annotated[str, ox.Pattern("^k-[0-9]+$")]]

    properties = defs_of(Pin)["Pin"]["properties"]
    assert properties["code"] == {"type": "integer", "writeOnly": True}
    assert properties["blob"]["writeOnly"] is True
    assert "format" not in properties["blob"]
    assert properties["stamp"] == {
        "type": "string",
        "pattern": "^k-[0-9]+$",
        "writeOnly": True,
        "format": "password",
    }


def test_constraints_on_the_secret_reach_the_schema_of_its_value() -> None:
    @ox.model
    class Strong:
        key: Annotated[ox.Secret[str], ox.Len(8, 64)]

    assert defs_of(Strong)["Strong"]["properties"]["key"] == {
        "type": "string",
        "minLength": 8,
        "maxLength": 64,
        "writeOnly": True,
        "format": "password",
    }


def test_the_serialization_schema_is_the_mask() -> None:
    properties = defs_of(Login, mode="serialization")["Login"]["properties"]
    assert properties["password"] == {"type": "string", "const": "********"}
    assert defs_of(Login, mode="serialization")["Login"]["required"] == ["user", "password"]


def test_a_secret_default_is_not_published() -> None:
    @ox.model
    class Holder:
        token: ox.Secret[str] = ox.Secret("fallback")
        tag: str = "x"

    properties = defs_of(Holder)["Holder"]["properties"]
    assert "default" not in properties["token"]
    assert properties["tag"]["default"] == "x"
    assert "fallback" not in json.dumps(ox.json_schema(Holder))


def test_lax_and_optional_variants_keep_the_marks() -> None:
    properties = defs_of(Tokens)["Tokens"]["properties"]
    assert properties["values"]["items"] == {
        "type": "string",
        "writeOnly": True,
        "format": "password",
    }
    spare = properties["spare"]
    assert {"type": "null"} in spare["anyOf"]
    lax = defs_of(Tokens, coercion="lax")["Tokens"]["properties"]["by_name"]
    assert lax["additionalProperties"]["writeOnly"] is True


def test_the_schemas_accept_what_decode_accepts_and_what_encode_writes() -> None:
    validation = Checker(ox.json_schema(Login))
    serialization = Checker(ox.json_schema(Login, mode="serialization"))
    assert validation.is_valid({"user": "ana", "password": HUNTER})
    assert not validation.is_valid({"user": "ana", "password": 5})
    written = json.loads(ox.json.encode(Login(user="ana", password=ox.Secret(HUNTER))))
    assert serialization.is_valid(written)
    assert not serialization.is_valid({"user": "ana", "password": HUNTER})


def test_a_bare_secret_has_no_schema() -> None:
    with pytest.raises(ox.SchemaError, match=r"Secret\[str\]"):
        ox.json_schema(ox.Secret)


def test_a_default_that_holds_a_secret_is_never_published() -> None:
    import dataclasses

    @dataclasses.dataclass
    class Wrapper:
        token: ox.Secret[str]

    @ox.model
    class Holder:
        many: tuple[ox.Secret[str], ...] = (ox.Secret("a"), ox.Secret("b"))
        mapped: dict[str, ox.Secret[str]] = ox.field(default_factory=lambda: {"k": ox.Secret("v")})
        plain: ox.Secret[str] = "x"  # type: ignore[assignment]  # pyright: ignore[reportAssignmentType]
        spare: ox.Secret[str] | None = None
        tag: str = "t"

    text = json.dumps(ox.json_schema(Holder))
    properties = defs_of(Holder)["Holder"]["properties"]
    for name in ("many", "plain"):
        assert "default" not in properties[name]
    assert "********" not in text
    assert properties["spare"].get("default") is None
    assert properties["tag"]["default"] == "t"
