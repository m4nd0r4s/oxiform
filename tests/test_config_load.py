"""``@ox.config`` and ``cfg.load``: declaration, precedence, nesting, origins, the errors and
the secrets (spec sections 6.1 and 6.3 to 6.6)."""

import copy
import dataclasses
import json
import pickle
from collections.abc import Mapping
from pathlib import Path
from typing import Annotated, Any, Literal

import pytest

import oxiform as ox
from oxiform import cfg

PASSWORD = "hunter2-correct-horse"


@ox.config
class Database:
    url: str
    pool_size: Annotated[int, ox.Ge(1), ox.Le(100)] = 10
    password: ox.Secret[str]


@ox.config
class Server:
    host: str = "127.0.0.1"
    port: Annotated[int, ox.Ge(1), ox.Le(65535)] = 8000
    workers: int = ox.field(default=1, env="WEB_CONCURRENCY")


@ox.config(env_prefix="APP_", env_nested="__")
class App:
    debug: bool = False
    server: Server = Server()
    db: Database
    plugins: tuple[str, ...] = ()
    cache: Server | None = None


BASE_ENV = {"APP_DB__URL": "postgres://db", "APP_DB__PASSWORD": PASSWORD}


@ox.config
class Early:
    later: "Later"


@ox.config
class Later:
    x: int = 1


@ox.config
class EarlyPlain:
    plain: "NotAConfig" = ox.field(default_factory=lambda: NotAConfig())  # noqa: F821  # pyright: ignore[reportUndefinedVariable]


@ox.model
class NotAConfig:
    y: int = 1


def write(path: Path, text: str) -> str:
    path.write_text(text, encoding="utf-8")
    return str(path)


# -- declaring ---------------------------------------------------------------------------------


def test_a_config_is_a_frozen_keyword_only_dataclass_with_slots() -> None:
    settings = App(db=Database(url="u", password=ox.Secret("p")))
    assert dataclasses.is_dataclass(settings)
    with pytest.raises(dataclasses.FrozenInstanceError):
        settings.debug = True  # type: ignore[misc]  # pyright: ignore[reportAttributeAccessIssue]
    with pytest.raises(TypeError):
        Server("0.0.0.0")  # type: ignore[misc]  # pyright: ignore[reportCallIssue]
    assert not hasattr(settings, "__dict__")
    assert hash(settings) == hash(copy.copy(settings))


def test_required_and_optional_fields_come_in_any_order() -> None:
    @ox.config
    class Mixed:
        first: int = 1
        second: str
        third: int = 3

    assert cfg.load(Mixed, [cfg.mapping({"second": "x"})]) == Mixed(second="x")


def test_unknown_keys_are_forbidden_unless_the_config_says_otherwise() -> None:
    @ox.config(unknown="ignore")
    class Lenient:
        name: str = "n"

    assert cfg.load(Lenient, [cfg.mapping({"name": "m", "other": 1})]).name == "m"
    with pytest.raises(ox.ValidationError, match="unknown key 'other'"):
        cfg.load(Server, [cfg.mapping({"other": 1})])


def test_a_section_may_be_shared_by_two_configs_and_only_the_outer_options_count() -> None:
    @ox.config(env_prefix="INNER_")
    class Inner:
        value: int = 0

    @ox.config(env_prefix="OUTER_")
    class Outer:
        inner: Inner = Inner()

    settings = cfg.load(Outer, [cfg.env(environ={"OUTER_INNER__VALUE": "5", "INNER_VALUE": "9"})])
    assert settings.inner.value == 5
    assert cfg.load(Inner, [cfg.env(environ={"INNER_VALUE": "9"})]).value == 9


def test_an_optional_section_stays_none_until_a_source_sets_a_key() -> None:
    absent = cfg.load(App, [cfg.env(environ=BASE_ENV)])
    assert absent.cache is None
    present = cfg.load(App, [cfg.env(environ={**BASE_ENV, "APP_CACHE__PORT": "6379"})])
    assert present.cache is not None
    assert present.cache.port == 6379
    assert cfg.origin(absent, "cache").kind == "default"
    assert cfg.origin(present, "cache.port").key == "APP_CACHE__PORT"


def test_a_required_section_with_defaults_only_is_built_from_them() -> None:
    @ox.config
    class Outer:
        server: Server

    assert cfg.load(Outer, [cfg.mapping({})]).server == Server()


def test_a_section_may_be_defined_after_the_config_that_uses_it() -> None:
    assert cfg.load(Early, [cfg.mapping({"later": {"x": 2}})]).later.x == 2
    assert cfg.load(Early, [cfg.mapping({})]).later.x == 1


def test_a_forward_reference_to_something_that_is_not_a_config_is_found_on_first_load() -> None:
    with pytest.raises(ox.SchemaError, match="declare it with @ox.config"):
        cfg.load(EarlyPlain, [cfg.mapping({})])


@pytest.mark.parametrize(
    ("annotation", "words"),
    [
        (list[int], ["list is mutable", "tuple[T, ...]"]),
        (list, ["list is mutable", "tuple[T, ...]"]),
        (set[str], ["set is mutable", "frozenset[T]"]),
        (dict[str, int], ["dict is not allowed", "make a section"]),
        (dict, ["dict is not allowed"]),
        (Mapping[str, int], ["Mapping is not allowed", "not built yet"]),
        (Annotated[list[int], ox.Len(1)], ["list is mutable"]),
        (tuple[list[int], ...], ["list is mutable"]),
        (ox.Secret[list[int]], ["list is mutable"]),
        (list[int] | None, ["list is mutable"]),
    ],
)
def test_a_config_refuses_mutable_collections_and_says_what_to_write(
    annotation: Any, words: list[str]
) -> None:
    with pytest.raises(ox.SchemaError) as caught:

        @ox.config
        class Bad:
            items: annotation = ()  # pyright: ignore[reportInvalidTypeForm]

    assert "Bad.items" in str(caught.value)
    for word in words:
        assert word in str(caught.value)


def test_a_config_refuses_a_section_that_is_not_a_config() -> None:
    @ox.model
    class Plain:
        x: int = 1

    with pytest.raises(ox.SchemaError, match="declare it with @ox.config") as caught:

        @ox.config
        class Bad:
            plain: Plain = ox.field(default_factory=Plain)

    assert "Plain is a section of a config" in str(caught.value)
    with pytest.raises(ox.SchemaError, match="declare it with @ox.config"):

        @ox.config
        class InCollection:
            many: tuple[Plain, ...] = ()

    with pytest.raises(ox.SchemaError, match="declare it with @ox.config"):

        @ox.config
        class InOptional:
            maybe: Plain | None = None


def test_a_config_refuses_a_flattened_field() -> None:
    with pytest.raises(ox.SchemaError, match="cannot be flattened"):

        @ox.config
        class Bad:
            server: Server = ox.field(default=Server(), flatten=True)


def test_options_that_cannot_work_say_what_to_write() -> None:
    with pytest.raises(ox.SchemaError, match="env_prefix is a str"):
        ox.config(env_prefix=3)(type("X", (), {"__annotations__": {}}))  # type: ignore[arg-type]  # pyright: ignore[reportArgumentType]
    with pytest.raises(ox.SchemaError, match="env_nested is the text that separates"):

        @ox.config(env_nested="")
        class EmptyNested:
            x: int = 0

    with pytest.raises(ValueError, match="env must be the name of an environment variable"):
        ox.field(default=1, env="")


def test_a_class_that_is_not_a_config_cannot_be_loaded() -> None:
    @ox.model
    class Plain:
        x: int = 1

    with pytest.raises(TypeError, match="needs a class made by @ox.config"):
        cfg.load(Plain, [cfg.mapping({})])
    with pytest.raises(TypeError, match="needs a class made by @ox.config"):
        cfg.load(int, [])  # type: ignore[type-var]  # pyright: ignore[reportCallIssue, reportArgumentType]


def test_a_secret_section_is_a_section_read_from_json_text() -> None:
    @ox.config(env_prefix="S_")
    class WithSecretSection:
        server: ox.Secret[int] = ox.Secret(0)

    assert cfg.load(WithSecretSection, [cfg.env(environ={"S_SERVER": "5"})]).server.reveal() == 5


# -- precedence and merging ----------------------------------------------------------------------


def test_the_last_source_wins_and_the_defaults_are_the_lowest_layer(tmp_path: Path) -> None:
    file = write(
        tmp_path / "a.toml",
        '[server]\nport = 1111\nhost = "file"\n[db]\nurl = "f"\npassword = "p"\n',
    )
    env = cfg.env(environ={"APP_SERVER__PORT": "2222"})
    mapped = cfg.mapping({"server": {"port": 3333}})
    assert cfg.load(App, [cfg.file(file)]).server.port == 1111
    assert cfg.load(App, [cfg.file(file), env]).server.port == 2222
    assert cfg.load(App, [cfg.file(file), env, mapped]).server.port == 3333
    assert cfg.load(App, [mapped, env, cfg.file(file)]).server.port == 1111
    assert cfg.load(App, [cfg.file(file)]).server.workers == 1


def test_the_documented_order_defaults_pyproject_file_dotenv_env_mapping(tmp_path: Path) -> None:
    pyproject = write(
        tmp_path / "pyproject.toml",
        '[tool.app]\ndebug = false\n[tool.app.server]\nhost = "py"\nport = 1\nworkers = 1\n'
        '[tool.app.db]\nurl = "py"\npassword = "py"\n',
    )
    file = write(tmp_path / "a.toml", "[server]\nport = 2\nworkers = 2\n")
    dotenv = write(tmp_path / ".env", "APP_SERVER__PORT=3\nWEB_CONCURRENCY=3\n")
    environ = {"APP_SERVER__PORT": "4"}
    sources = [
        cfg.pyproject("tool.app", path=pyproject),
        cfg.file(file),
        cfg.dotenv(dotenv),
        cfg.env(environ=environ),
        cfg.mapping({"server": {"workers": 5}}),
    ]
    settings = cfg.load(App, sources)
    assert settings.server.host == "py"
    assert settings.server.port == 4
    assert settings.server.workers == 5
    port = cfg.origin(settings, "server.port")
    assert port.kind == "env"
    assert port.overrides == (
        f"{dotenv}:1 (APP_SERVER__PORT)",
        f"{file}:2 (server.port)",
        f"{pyproject}:5 (tool.app.server.port)",
    )
    workers = cfg.origin(settings, "server.workers")
    assert workers.kind == "mapping"
    assert len(workers.overrides) == 3
    assert cfg.origin(settings, "server.host").overrides == ()


def test_sections_merge_key_by_key_and_collections_are_replaced(tmp_path: Path) -> None:
    first = write(tmp_path / "1.toml", '[server]\nhost = "one"\nport = 1\nplugins = 1\n')
    first = write(
        tmp_path / "1.toml",
        'plugins = ["a", "b"]\n[server]\nhost = "one"\nport = 1\n[db]\nurl = "u"\npassword = "p"\n',
    )
    second = write(tmp_path / "2.toml", 'plugins = ["c"]\n[server]\nport = 2\n')
    settings = cfg.load(App, [cfg.file(first), cfg.file(second)])
    assert (settings.server.host, settings.server.port) == ("one", 2)
    assert settings.plugins == ("c",)
    assert settings.db.url == "u"


def test_a_value_that_is_not_a_section_replaces_the_keys_below_it(tmp_path: Path) -> None:
    first = write(tmp_path / "1.toml", '[server]\nport = 5\n[db]\nurl = "u"\npassword = "p"\n')
    second = write(tmp_path / "2.toml", "server = 5\n")
    with pytest.raises(ox.ValidationError) as caught:
        cfg.load(App, [cfg.file(first), cfg.file(second)])
    (problem,) = caught.value.errors()
    assert problem["code"] == "type"
    assert problem["path"] == [second, "server"]
    assert "(from" in problem["message"]


def test_an_alias_and_a_wire_name_in_different_layers_are_the_same_field(tmp_path: Path) -> None:
    @ox.config(rename_all="camelCase")
    class Wire:
        max_age: int = ox.field(default=0, aliases=["maxAgeSeconds"])

    first = write(tmp_path / "1.toml", "maxAgeSeconds = 1\n")
    second = write(tmp_path / "2.toml", "maxAge = 2\n")
    settings = cfg.load(Wire, [cfg.file(first), cfg.file(second)])
    assert settings.max_age == 2
    assert cfg.origin(settings, "max_age").overrides == (f"{first}:1 (maxAgeSeconds)",)


def test_a_source_may_be_repeated_and_each_is_a_layer() -> None:
    sources = [
        cfg.env(environ=BASE_ENV),
        cfg.mapping({"debug": True}, name="one"),
        cfg.mapping({"debug": False}, name="two"),
    ]
    settings = cfg.load(App, sources)
    assert settings.debug is False
    assert cfg.origin(settings, "debug").overrides == ("one debug",)


def test_no_source_is_needed_to_be_complete_but_the_merge_must_be() -> None:
    parts = [cfg.mapping({"db": {"url": "u"}}), cfg.mapping({"db": {"password": "p"}})]
    assert cfg.load(App, parts).db.password.reveal() == "p"
    with pytest.raises(ox.ValidationError, match="field required"):
        cfg.load(App, parts[:1])


# -- origin ------------------------------------------------------------------------------------


def test_origin_and_origins_say_which_source_and_key_set_each_value(tmp_path: Path) -> None:
    file = write(tmp_path / "a.toml", '[db]\nurl = "u"\npool_size = 3\n')
    environ = {"APP_DB__PASSWORD": "p", "APP_DB__POOL_SIZE": "4"}
    settings = cfg.load(App, [cfg.file(file), cfg.env(environ=environ)])
    pool = cfg.origin(settings, "db.pool_size")
    assert pool == cfg.Origin(
        kind="env",
        source="env",
        key="APP_DB__POOL_SIZE",
        line=None,
        overrides=(f"{file}:3 (db.pool_size)",),
    )
    assert str(pool) == "env APP_DB__POOL_SIZE"
    assert str(cfg.origin(settings, "db.url")) == f"{file}:2 (db.url)"
    table = cfg.origins(settings)
    assert list(table) == [
        "debug",
        "server.host",
        "server.port",
        "server.workers",
        "db.url",
        "db.pool_size",
        "db.password",
        "plugins",
        "cache",
    ]
    assert table["debug"] == cfg.Origin("default", "default")
    assert str(table["server.workers"]) == "default"


def test_an_origin_never_holds_a_value() -> None:
    settings = cfg.load(App, [cfg.env(environ=BASE_ENV)])
    shown = " ".join(repr(origin) + str(origin) for origin in cfg.origins(settings).values())
    assert PASSWORD not in shown
    assert "postgres://db" not in shown


def test_origin_mistakes_say_what_to_pass() -> None:
    settings = cfg.load(App, [cfg.env(environ=BASE_ENV)])
    with pytest.raises(ValueError, match="is a section, so ask for one of its fields: db.url"):
        cfg.origin(settings, "db")
    with pytest.raises(ValueError, match="did you mean 'db.pool_size'"):
        cfg.origin(settings, "db.pool_sze")
    with pytest.raises(ValueError, match="dotted Python names"):
        cfg.origin(settings, "nothing")
    with pytest.raises(ValueError, match="was not made by cfg.load"):
        cfg.origin(App(db=Database(url="u", password=ox.Secret("p"))), "debug")


def test_origins_are_kept_with_the_object_and_not_with_its_copies() -> None:
    settings = cfg.load(App, [cfg.env(environ=BASE_ENV)])
    assert cfg.origin(settings, "debug").kind == "default"
    for other in (
        copy.copy(settings),
        pickle.loads(pickle.dumps(settings)),
        ox.replace(settings, debug=True),
    ):
        with pytest.raises(ValueError, match="was not made by cfg.load"):
            cfg.origins(other)


def test_origins_are_forgotten_with_the_object() -> None:
    from oxiform._config import origin as registry

    settings = cfg.load(App, [cfg.env(environ=BASE_ENV)])
    key = id(settings)
    assert key in registry._REGISTRY  # pyright: ignore[reportPrivateUsage]
    del settings
    assert key not in registry._REGISTRY  # pyright: ignore[reportPrivateUsage]


# -- errors --------------------------------------------------------------------------------------


def test_a_value_that_does_not_fit_says_the_source_and_key(tmp_path: Path) -> None:
    file = write(tmp_path / "base.toml", "[db]\npool_size = 500\n")
    sources = [cfg.file(file), cfg.env(environ={**BASE_ENV, "APP_SERVER__PORT": "70000"})]
    with pytest.raises(ox.ValidationError) as caught:
        cfg.load(App, sources)
    error = caught.value
    assert error.target == "App"
    by_source = error.by_source()
    assert set(by_source) == {file, "env"}
    (pool,) = by_source[file]
    assert pool["code"] == "too_large"
    assert pool["path"] == [file, "db", "pool_size"]
    assert pool["pointer"].endswith("/db/pool_size")
    assert pool["message"] == f"must be at most 100 (from {file}:2, key db.pool_size)"
    assert pool["ctx"]["key"] == "db.pool_size"
    assert pool["ctx"]["origin"] == f"{file}:2 (db.pool_size)"
    assert pool["ctx"]["limit"] == 100
    assert pool.get("input") == "500"
    (port,) = by_source["env"]
    assert port["message"] == "must be at most 65535 (from env APP_SERVER__PORT)"
    assert error.unreadable_sources() == []
    problem = error.to_problem(status=500)
    assert problem["status"] == 500
    assert str(error).startswith("2 errors validating App (file, env)")


def test_a_missing_field_lists_where_each_source_could_set_it(tmp_path: Path) -> None:
    file = write(tmp_path / "base.toml", "[server]\nport = 1\n")
    dotenv = write(tmp_path / ".env", "APP_DEBUG=true\n")
    sources = [
        cfg.pyproject("tool.app", path=str(tmp_path / "none.toml")),
        cfg.file(file),
        cfg.dotenv(dotenv),
        cfg.env(environ={}),
        cfg.mapping({}, name="cli"),
    ]
    with pytest.raises(ox.ValidationError) as caught:
        cfg.load(App, sources)
    by_path = {tuple(error["path"]): error for error in caught.value.errors()}
    url = by_path[("default", "db", "url")]
    assert url["code"] == "missing"
    assert url["message"].startswith("field required: no source sets db.url; set ")
    assert f"tool.app.db.url in {tmp_path / 'none.toml'}" in url["message"]
    assert f"db.url in {file}" in url["message"]
    assert f"APP_DB__URL in {dotenv}" in url["message"]
    assert url["message"].endswith("; ...")
    assert "db.url in the mapping" not in url["message"]
    assert url["ctx"]["key"] == "db.url"
    assert "origin" not in url["ctx"]
    assert ("default", "db", "password") in by_path


def test_a_missing_field_with_few_sources_lists_them_all() -> None:
    with pytest.raises(ox.ValidationError) as caught:
        cfg.load(App, [cfg.env(environ={}), cfg.mapping({}, name="cli")])
    message = caught.value.errors()[0]["message"]
    assert message == (
        "field required: no source sets db.url; set APP_DB__URL in the environment; "
        "db.url in the mapping 'cli'"
    )


def test_a_source_that_cannot_be_read_is_a_decode_error_with_every_problem(tmp_path: Path) -> None:
    bad = write(tmp_path / "bad.toml", "[db\n")
    missing = str(tmp_path / "missing.toml")
    sources = [cfg.file(bad), cfg.file(missing), cfg.env(environ={"APP_PLUGINS": "[x"})]
    with pytest.raises(ox.DecodeError) as caught:
        cfg.load(App, sources)
    error = caught.value
    assert error.unreadable_sources() == [bad, missing, "env"]
    assert [item["code"] for item in error.errors()] == ["config_source"] * 3
    assert all(item["ctx"]["source"] in ("file", "env") for item in error.errors())
    assert error.to_problem(status=500)["status"] == 500


def test_nothing_is_validated_while_a_source_is_unreadable(tmp_path: Path) -> None:
    sources = [cfg.file(str(tmp_path / "gone.toml")), cfg.mapping({"server": {"port": 0}})]
    with pytest.raises(ox.DecodeError) as caught:
        cfg.load(App, sources)
    assert [item["code"] for item in caught.value.errors()] == ["config_source"]


def test_max_errors_bounds_the_report() -> None:
    sources = [cfg.mapping({"db": {"url": 1, "password": 2, "pool_size": 0}, "debug": "maybe"})]
    with pytest.raises(ox.ValidationError) as caught:
        cfg.load(App, sources, max_errors=2)
    assert len(caught.value.errors()) == 2
    assert caught.value.truncated is True


def test_limits_apply_to_the_merged_document() -> None:
    sources = [cfg.mapping({"db": {"url": "x" * 100, "password": "p"}})]
    with pytest.raises(ox.DecodeError) as caught:
        cfg.load(App, sources, limits=ox.Limits(max_str_len=10))
    assert caught.value.errors()[0]["code"] == "limit_exceeded"


def test_a_value_that_cannot_be_written_as_json_is_a_source_problem() -> None:
    sources = [cfg.mapping({"server": {"port": float("nan")}})]
    with pytest.raises(ox.DecodeError) as caught:
        cfg.load(App, sources)
    (problem,) = caught.value.errors()
    assert problem["code"] == "config_source"
    assert "server.port in mapping server.port" in problem["message"]
    assert caught.value.unreadable_sources() == ["mapping"]


# -- secrets -------------------------------------------------------------------------------------


def test_a_secret_is_masked_in_repr_str_logs_dump_encode_and_origins(
    tmp_path: Path, caplog: pytest.LogCaptureFixture
) -> None:
    import logging

    file = write(tmp_path / "a.toml", f'[db]\nurl = "u"\npassword = "{PASSWORD}"\n')
    settings = cfg.load(App, [cfg.file(file)])
    assert settings.db.password.reveal() == PASSWORD
    with caplog.at_level(logging.INFO):
        logging.getLogger("t").info("loaded %s %r", settings, settings.db)
    shown = " ".join(
        [
            repr(settings),
            str(settings),
            f"{settings.db}",
            caplog.text,
            json.dumps(cfg.dump(settings)),
            ox.json.encode(settings).decode(),
            ox.json.encode(settings, indent=2).decode(),
            repr(cfg.origins(settings)),
            repr(dataclasses.asdict(settings)),
        ]
    )
    assert PASSWORD not in shown
    assert "********" in shown
    assert cfg.dump(settings)["db"]["password"] == "********"


def test_a_secret_never_appears_in_an_error_whatever_the_source_or_problem(tmp_path: Path) -> None:
    @ox.config(env_prefix="K_")
    class Keys:
        pin: ox.Secret[int]
        token: Annotated[ox.Secret[str], ox.Len(40)] = ox.Secret("x" * 40)
        keys: tuple[ox.Secret[int], ...] = ()
        section: Database | None = None

    toml = write(tmp_path / "k.toml", f'pin = "{PASSWORD}"\n')
    for source in (
        cfg.file(toml),
        cfg.env(environ={"K_PIN": PASSWORD}),
        cfg.mapping({"pin": PASSWORD}),
        cfg.env(environ={"K_PIN": "1", "K_TOKEN": PASSWORD}),
        cfg.env(environ={"K_PIN": "1", "K_KEYS": f'["{PASSWORD}", 3]'}),
        cfg.env(
            environ={
                "K_PIN": "1",
                "K_SECTION": '{"url": 1, "password": 2, "pool_size": "many"}',
            }
        ),
        cfg.env(
            environ={
                "K_PIN": "1",
                "K_SECTION": f'{{"url": "u", "password": {{"x": "{PASSWORD}"}}}}',
            }
        ),
    ):
        with pytest.raises(ox.ValidationError) as caught:
            cfg.load(Keys, [source])
        error = caught.value
        text = " ".join(
            [str(error), repr(error.errors()), repr(error.to_problem()), repr(error.args)]
        )
        assert PASSWORD not in text
        for item in error.errors():
            if (
                item["pointer"].endswith(("/pin", "/token", "/password"))
                or "/keys/" in item["pointer"]
            ):
                assert "input" not in item


def test_secrets_are_masked_when_loaded_from_every_source(tmp_path: Path) -> None:
    file = write(tmp_path / "s.toml", f'[db]\nurl = "u"\npassword = "{PASSWORD}"\n')
    dotenv = write(tmp_path / ".env", f"APP_DB__URL=u\nAPP_DB__PASSWORD={PASSWORD}\n")
    for source in (
        cfg.file(file),
        cfg.dotenv(dotenv),
        cfg.env(environ={"APP_DB__URL": "u", "APP_DB__PASSWORD": PASSWORD}),
        cfg.mapping({"db": {"url": "u", "password": PASSWORD}}),
        cfg.mapping({"db": {"url": "u", "password": ox.Secret(PASSWORD)}}),
    ):
        settings = cfg.load(App, [source])
        assert settings.db.password.reveal() == PASSWORD
        assert PASSWORD not in repr(settings) + str(settings)


def test_a_secret_survives_pickle_for_worker_processes() -> None:
    settings = cfg.load(App, [cfg.env(environ=BASE_ENV)])
    again = pickle.loads(pickle.dumps(settings))
    assert again == settings
    assert again.db.password.reveal() == PASSWORD


def test_the_schema_of_a_config_marks_its_secret_write_only() -> None:
    defs = ox.json_schema_bundle([App]).defs
    assert defs["Database"]["properties"]["password"] == {
        "type": "string",
        "writeOnly": True,
        "format": "password",
    }
    out = ox.json_schema_bundle([App], mode="serialization").defs
    assert out["Database"]["properties"]["password"] == {"type": "string", "const": "********"}


# -- mistakes in the call ------------------------------------------------------------------------


def test_sources_must_be_a_list_of_sources() -> None:
    with pytest.raises(TypeError, match="sources is a list of sources, not _Env"):
        cfg.load(App, cfg.env())  # type: ignore[arg-type]  # pyright: ignore[reportArgumentType]
    with pytest.raises(TypeError, match="sources is a list of sources, not str"):
        cfg.load(App, "app.toml")  # type: ignore[arg-type]  # pyright: ignore[reportArgumentType]
    with pytest.raises(TypeError, match="sources is a list of sources, not int"):
        cfg.load(App, 3)  # type: ignore[arg-type]  # pyright: ignore[reportArgumentType]
    with pytest.raises(TypeError, match="'app.toml', which is not a source"):
        cfg.load(App, ["app.toml"])  # type: ignore[list-item]  # pyright: ignore[reportArgumentType]
    assert cfg.load(Server, []) == Server()
    assert cfg.load(Server, ()) == Server()


def test_source_constructors_refuse_what_cannot_work() -> None:
    with pytest.raises(ValueError, match="dotted path such as 'tool.myapp'"):
        cfg.pyproject("")
    with pytest.raises(ValueError, match="dotted path such as 'tool.myapp'"):
        cfg.pyproject("tool..app")
    with pytest.raises(ValueError, match="needs at least one path"):
        cfg.dotenv()
    with pytest.raises(ValueError, match="format is 'toml' or 'json'"):
        cfg.file("a.toml", format="yaml")  # type: ignore[arg-type]  # pyright: ignore[reportArgumentType]
    with pytest.raises(TypeError, match="takes a mapping"):
        cfg.mapping([("a", 1)])  # type: ignore[arg-type]  # pyright: ignore[reportArgumentType]
    with pytest.raises(ValueError, match="name is the text errors call this source"):
        cfg.mapping({}, name="")


def test_the_default_sources_are_the_environment_alone(monkeypatch: pytest.MonkeyPatch) -> None:
    for name, value in BASE_ENV.items():
        monkeypatch.setenv(name, value)
    monkeypatch.setenv("APP_DEBUG", "true")
    assert cfg.load(App).debug is True


def test_dump_is_plain_data_with_wire_names() -> None:
    settings = cfg.load(App, [cfg.env(environ={**BASE_ENV, "APP_PLUGINS": '["a"]'})])
    dumped = cfg.dump(settings)
    assert dumped == {
        "debug": False,
        "server": {"host": "127.0.0.1", "port": 8000, "workers": 1},
        "db": {"url": "postgres://db", "pool_size": 10, "password": "********"},
        "plugins": ["a"],
        "cache": None,
    }


# -- defaults of sections, naming, frames, union messages -------------------------------------


@ox.config
class Db:
    url: str = "class-default"
    pool_size: int = 10
    password: ox.Secret[str] = ox.Secret("default-password")
    replicas: tuple[str, ...] = ("a",)


@ox.config
class WithCustomDefault:
    db: Db = Db(url="custom", replicas=("x", "y"))
    inner: Db | None = None


def test_a_section_default_that_is_not_the_class_default_fills_the_keys_a_source_leaves_out() -> (
    None
):
    settings = cfg.load(WithCustomDefault, [cfg.env(environ={"DB__POOL_SIZE": "3"})])
    assert settings.db.pool_size == 3
    assert settings.db.url == "custom"
    assert settings.db.replicas == ("x", "y")
    assert settings.db.password.reveal() == "default-password"
    assert cfg.origin(settings, "db.pool_size").kind == "env"
    for key in ("db.url", "db.replicas", "db.password"):
        assert cfg.origin(settings, key) == cfg.Origin("default", "default")
    untouched = cfg.load(WithCustomDefault, [cfg.env(environ={})])
    assert untouched.db == Db(url="custom", replicas=("x", "y"))
    whole = cfg.load(WithCustomDefault, [cfg.mapping({"db": {"url": "given"}})])
    assert (whole.db.url, whole.db.replicas) == ("given", ("x", "y"))


def test_an_empty_table_in_a_file_takes_the_section_default(tmp_path: Path) -> None:
    file = write(tmp_path / "a.toml", "[db]\n")
    assert cfg.load(WithCustomDefault, [cfg.file(file)]).db.url == "custom"


def test_a_default_with_a_section_inside_seeds_it_too() -> None:
    @ox.config
    class Outer:
        pair: WithCustomDefault = WithCustomDefault(db=Db(url="deep", pool_size=7))

    settings = cfg.load(Outer, [cfg.mapping({"pair": {"db": {"url": "given"}}})])
    assert settings.pair.db.url == "given"
    assert settings.pair.db.pool_size == 7


def test_a_section_default_factory_is_used_the_same_way() -> None:
    @ox.config
    class Made:
        db: Db = ox.field(default_factory=lambda: Db(url="made"))

    assert cfg.load(Made, [cfg.mapping({"db": {"pool_size": 2}})]).db.url == "made"


@ox.config
class Camel:
    pool_size: int = 1


@ox.config(rename_all="camelCase")
class CamelSection:
    pool_size: int = 1
    inner: Camel = Camel()


@ox.config
class Plain:
    section: CamelSection = CamelSection()
    top_level: int = 0


def test_each_class_owns_its_naming_wherever_it_is_used(tmp_path: Path) -> None:
    file = write(
        tmp_path / "a.toml",
        "top_level = 1\n[section]\npoolSize = 5\n[section.inner]\npool_size = 6\n",
    )
    settings = cfg.load(Plain, [cfg.file(file)])
    assert settings.top_level == 1
    assert settings.section.pool_size == 5
    assert settings.section.inner.pool_size == 6
    with pytest.raises(ox.ValidationError, match="did you mean 'poolSize'"):
        cfg.load(Plain, [cfg.file(write(tmp_path / "b.toml", "[section]\npool_size = 5\n"))])


PACKAGE = str(Path(ox.__file__).parent)


def frame_text(error: BaseException) -> str:
    """Everything the local variables of the frames of a traceback, and of the exceptions it
    chains to, can show: what an error tracker or --showlocals captures."""
    seen: list[str] = []
    pending: list[BaseException | None] = [error]
    while pending:
        current = pending.pop()
        if current is None:
            continue
        pending.extend([current.__context__, current.__cause__])
        traceback = current.__traceback__
        while traceback is not None:
            if traceback.tb_frame.f_code.co_filename.startswith(PACKAGE):  # not the test's frames
                for name, value in traceback.tb_frame.f_locals.items():
                    seen.append(f"{name}={value!r}")
            traceback = traceback.tb_next
    return "\n".join(seen)


def test_a_failed_load_keeps_no_values_in_the_frames_of_its_traceback(tmp_path: Path) -> None:
    secret = "S3CR3T-from-a-file"
    file = write(tmp_path / "a.toml", f'[db]\nurl = "u"\npassword = "{secret}"\npool_size = 5000\n')
    dotenv = write(tmp_path / ".env", f'APP_PLUGINS=["{secret}"]\nAPP_SERVER__PORT=0\n')
    for sources in ([cfg.file(file)], [cfg.file(file), cfg.dotenv(dotenv)]):
        with pytest.raises(ox.ValidationError) as caught:
            cfg.load(App, sources)
        shown = frame_text(caught.value)
        assert secret not in shown
        for name in ("document", "tree", "layers", "readings", "settings="):
            assert f"\n{name}" not in "\n" + shown
        assert caught.value.__context__ is None
    unreadable = write(tmp_path / "bad.toml", f'password = "{secret}\n')
    with pytest.raises(ox.DecodeError) as caught:
        cfg.load(App, [cfg.file(unreadable)])
    assert secret not in frame_text(caught.value)


def test_the_origin_registry_can_forget_while_its_lock_is_held() -> None:
    from oxiform._config import origin as registry

    settings = cfg.load(App, [cfg.env(environ=BASE_ENV)])
    with registry._LOCK:  # pyright: ignore[reportPrivateUsage]
        del settings  # the weakref callback takes the same lock, on the same thread


@ox.config(env_prefix="W_")
class Workers:
    workers: int | Literal["auto"] = "auto"
    mode: int | float | Literal["fast", "slow"] = "fast"


def test_a_union_of_a_number_and_a_literal_says_what_it_accepts() -> None:
    def message(**variables: str) -> str:
        with pytest.raises(ox.ValidationError) as caught:
            cfg.load(Workers, [cfg.env(environ=variables)])
        return caught.value.errors()[0]["message"]

    assert message(W_WORKERS="abc").startswith("expected an integer or 'auto' (from env W_WORKERS)")
    assert message(W_WORKERS="4.5").startswith("expected an integer or 'auto'")
    assert message(W_MODE="abc").startswith(
        "expected an integer or a number or one of 'fast', 'slow'"
    )
    assert cfg.load(Workers, [cfg.env(environ={"W_WORKERS": "4"})]).workers == 4
    assert cfg.load(Workers, [cfg.env(environ={"W_WORKERS": "auto"})]).workers == "auto"
