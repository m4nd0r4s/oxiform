"""Each source of a config: what it reads, how it names a key and what it says when it is wrong
(spec section 6.2)."""

import datetime
import os
from pathlib import Path
from typing import Annotated

import pytest

import oxiform as ox
from oxiform import cfg
from oxiform._config.toml_lines import key_lines, line_of_key
from oxiform.errors import ErrorDetails


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
    tags: frozenset[str] = frozenset()
    limit: int | None = None
    retries: int | tuple[int, ...] = 3


REQUIRED = {"APP_DB__URL": "postgres://db", "APP_DB__PASSWORD": "hunter2"}


def from_env(variables: dict[str, str], *, forbid_unknown: bool = False) -> App:
    environ = {**REQUIRED, **variables}
    return cfg.load(App, [cfg.env(environ=environ, forbid_unknown=forbid_unknown)])


def failure(
    sources: list[cfg.Source], error: type[ox.ValidationError] = ox.ValidationError
) -> ox.ValidationError:
    with pytest.raises(error) as caught:
        cfg.load(App, sources)
    return caught.value


# -- environment variables -----------------------------------------------------------------------


def test_env_names_are_the_prefix_and_the_nested_names_in_uppercase() -> None:
    settings = from_env(
        {"APP_DEBUG": "true", "APP_SERVER__PORT": "9000", "APP_DB__POOL_SIZE": "20"}
    )
    assert settings.debug is True
    assert settings.server.port == 9000
    assert settings.db.pool_size == 20
    assert settings.db.url == "postgres://db"
    assert settings.db.password.reveal() == "hunter2"


def test_env_names_are_matched_without_regard_to_case() -> None:
    settings = from_env({"app_server__port": "9001", "App_Debug": "1"})
    assert settings.server.port == 9001
    assert settings.debug is True


def test_a_field_env_is_the_exact_name_and_replaces_the_derived_one() -> None:
    assert from_env({"WEB_CONCURRENCY": "4"}).server.workers == 4
    assert from_env({"APP_SERVER__WORKERS": "4"}).server.workers == 1
    assert from_env({"WEB_CONCURRENCY": "4", "APP_SERVER__WORKERS": "9"}).server.workers == 4


def test_env_text_is_read_in_lax_mode() -> None:
    settings = from_env({"APP_DEBUG": "yes", "APP_SERVER__PORT": "80", "APP_LIMIT": ""})
    assert settings.debug is True
    assert settings.server.port == 80
    assert settings.limit is None
    assert from_env({"APP_LIMIT": "5"}).limit == 5


def test_composite_values_are_written_in_json() -> None:
    settings = from_env({"APP_PLUGINS": '["auth", "billing"]', "APP_TAGS": '["b", "a"]'})
    assert settings.plugins == ("auth", "billing")
    assert settings.tags == frozenset({"a", "b"})
    whole = from_env({"APP_SERVER": '{"host": "0.0.0.0", "port": 9000}'})
    assert (whole.server.host, whole.server.port) == ("0.0.0.0", 9000)
    section = cfg.load(
        App, [cfg.env(environ={"APP_DB": '{"url": "u", "password": "p", "pool_size": 3}'})]
    )
    assert (section.db.url, section.db.pool_size) == ("u", 3)


def test_a_whole_section_merges_key_by_key_and_a_specific_name_wins() -> None:
    settings = from_env({"APP_SERVER": '{"host": "h", "port": 1}', "APP_SERVER__PORT": "2"})
    assert (settings.server.host, settings.server.port) == ("h", 2)


def test_a_union_of_a_scalar_and_a_composite_reads_by_the_first_character() -> None:
    assert from_env({"APP_RETRIES": "5"}).retries == 5
    assert from_env({"APP_RETRIES": "[1, 2]"}).retries == (1, 2)


def test_without_env_nested_only_top_level_fields_have_a_name() -> None:
    @ox.config(env_prefix="X_", env_nested=None)
    class Flat:
        name: str = "n"
        server: Server = Server()

    environ = {"X_NAME": "m", "X_SERVER__PORT": "9", "X_SERVER": '{"port": 7}'}
    settings = cfg.load(Flat, [cfg.env(environ=environ)])
    assert settings.name == "m"
    assert settings.server.port == 7


def test_the_default_environment_is_the_process() -> None:
    original = dict(os.environ)
    try:
        os.environ.update({**REQUIRED, "APP_DEBUG": "true"})
        assert cfg.load(App).debug is True
        assert cfg.load(App, [cfg.env()]).db.url == "postgres://db"
    finally:
        os.environ.clear()
        os.environ.update(original)


def test_invalid_json_in_a_variable_is_a_source_problem_that_says_how_to_write_it() -> None:
    error = failure([cfg.env(environ={**REQUIRED, "APP_PLUGINS": "[auth"})], ox.DecodeError)
    (problem,) = error.errors()
    assert problem["code"] == "config_source"
    assert problem["path"] == ["env"]
    assert "APP_PLUGINS holds text that is not valid JSON" in problem["message"]
    assert '["a", "b"]' in problem["message"]
    assert problem["ctx"]["key"] == "APP_PLUGINS"
    assert error.unreadable_sources() == ["env"]


def test_two_variables_that_differ_only_in_case_are_a_problem() -> None:
    both = {**REQUIRED, "APP_DEBUG": "true", "app_debug": "false"}
    error = failure([cfg.env(environ=both)], ox.DecodeError)
    assert "differ only in case" in error.errors()[0]["message"]
    assert from_env({"APP_DEBUG": "true", "app_debug": "true"}).debug is True


def test_unknown_variables_are_ignored_unless_asked_for() -> None:
    assert from_env({"APP_SERVR__PORT": "9", "OTHER": "x"}).server.port == 8000
    typo = {**REQUIRED, "APP_SERVR__PORT": "9", "OTHER": "x"}
    error = failure([cfg.env(environ=typo, forbid_unknown=True)])
    (problem,) = error.errors()
    assert problem["code"] == "unknown_field"
    assert problem["path"] == ["env", "servr", "port"]
    assert "unknown variable APP_SERVR__PORT" in problem["message"]
    assert "did you mean APP_SERVER__PORT?" in problem["message"]
    assert problem["ctx"]["key"] == "APP_SERVR__PORT"
    assert from_env({"APP_SERVER__PORT": "9"}, forbid_unknown=True).server.port == 9


def test_forbid_unknown_needs_a_prefix_because_every_variable_would_be_unknown() -> None:
    @ox.config
    class NoPrefix:
        name: str = "n"

    with pytest.raises(ValueError, match="needs an env_prefix"):
        cfg.load(NoPrefix, [cfg.env(environ={"NAME": "x"}, forbid_unknown=True)])


# -- dotenv --------------------------------------------------------------------------------------


def write(path: Path, text: str) -> str:
    path.write_text(text, encoding="utf-8")
    return str(path)


def dotenv_app(tmp_path: Path, text: str) -> App:
    file = write(tmp_path / ".env", text)
    return cfg.load(App, [cfg.dotenv(file)])


DOTENV_BASE = "APP_DB__URL=u\nAPP_DB__PASSWORD=p\n"


def test_dotenv_reads_names_like_the_environment(tmp_path: Path) -> None:
    text = DOTENV_BASE + 'APP_SERVER__PORT=9000\nWEB_CONCURRENCY=3\nAPP_PLUGINS=["x"]\n'
    settings = dotenv_app(tmp_path, text)
    assert settings.server.port == 9000
    assert settings.server.workers == 3
    assert settings.plugins == ("x",)


@pytest.mark.parametrize(
    ("line", "expected"),
    [
        ("APP_DB__URL=plain", "plain"),
        ("export APP_DB__URL=plain", "plain"),
        ("  APP_DB__URL  =  spaced out  ", "spaced out"),
        ("APP_DB__URL=with # comment", "with"),
        ("APP_DB__URL=a#b", "a#b"),
        ("APP_DB__URL=# only a comment", ""),
        ('APP_DB__URL="quoted # kept"', "quoted # kept"),
        ("APP_DB__URL='single $HOME \\n'", "single $HOME \\n"),
        ('APP_DB__URL="tab\\there\\n"', "tab\there\n"),
        ('APP_DB__URL="say \\"hi\\" \\\\"', 'say "hi" \\'),
        ('APP_DB__URL="x"  # trailing', "x"),
        ("APP_DB__URL=$HOME", "$HOME"),
        ("APP_DB__URL=", ""),
        ('APP_DB__URL="multi\nline"', "multi\nline"),
    ],
)
def test_dotenv_values(tmp_path: Path, line: str, expected: str) -> None:
    text = "# a comment\n\nAPP_DB__PASSWORD=p\n" + line + "\n"
    assert dotenv_app(tmp_path, text).db.url == expected


def test_dotenv_skips_a_byte_order_mark_and_windows_line_ends(tmp_path: Path) -> None:
    file = tmp_path / ".env"
    file.write_bytes(b"\xef\xbb\xbfAPP_DB__URL=u\r\nAPP_DB__PASSWORD=p\r\n")
    assert cfg.load(App, [cfg.dotenv(file)]).db.url == "u"


@pytest.mark.parametrize(
    ("text", "line", "words"),
    [
        (DOTENV_BASE + "not a pair\n", 3, ["not KEY=value", "quote a value that spans lines"]),
        (DOTENV_BASE + 'APP_DEBUG="open\n', 3, ["quote is never closed"]),
        (DOTENV_BASE + "APP_DEBUG='open\n", 3, ["' quote is never closed"]),
        (
            DOTENV_BASE + 'APP_DEBUG="a" b\n',
            3,
            ["text follows the closing", "quote the whole value"],
        ),
        (
            "APP_DEBUG=1\nAPP_DB__URL=u\nAPP_DEBUG=0\n",
            3,
            ["APP_DEBUG is set twice, on lines 1 and 3"],
        ),
        ("1BAD=x\n", 1, ["not KEY=value"]),
    ],
)
def test_dotenv_syntax_problems_name_the_file_and_line(
    tmp_path: Path, text: str, line: int, words: list[str]
) -> None:
    file = write(tmp_path / ".env", text)
    error = failure([cfg.dotenv(file)], ox.DecodeError)
    problem = error.errors()[0]
    assert problem["code"] == "config_source"
    assert problem["ctx"]["line"] == line
    assert problem["ctx"]["file"] == file
    assert f"{file}:{line}:" in problem["message"]
    for word in words:
        assert word in problem["message"]
    assert error.unreadable_sources() == [file]


def test_every_fault_of_a_dotenv_file_is_reported(tmp_path: Path) -> None:
    file = write(tmp_path / ".env", "bad one\nAPP_DEBUG=1\nbad two\n")
    error = failure([cfg.dotenv(file)], ox.DecodeError)
    assert [problem["ctx"]["line"] for problem in error.errors()] == [1, 3]


def test_a_missing_dotenv_file_is_a_problem_unless_optional(tmp_path: Path) -> None:
    missing = str(tmp_path / "nope.env")
    error = failure([cfg.dotenv(missing)], ox.DecodeError)
    message = error.errors()[0]["message"]
    assert "not found" in message
    assert "optional=True" in message
    assert cfg.load(App, [cfg.env(environ=REQUIRED), cfg.dotenv(missing, optional=True)])


def test_several_dotenv_paths_are_layers_and_later_ones_win(tmp_path: Path) -> None:
    first = write(tmp_path / "a.env", DOTENV_BASE + "APP_SERVER__PORT=1\nAPP_DEBUG=true\n")
    second = write(tmp_path / "b.env", "APP_SERVER__PORT=2\n")
    settings = cfg.load(App, [cfg.dotenv(first, second)])
    assert settings.server.port == 2
    assert settings.debug is True
    assert cfg.origin(settings, "server.port").source == second
    assert cfg.origin(settings, "debug").source == first


def test_dotenv_does_not_change_the_process_environment(tmp_path: Path) -> None:
    before = dict(os.environ)
    dotenv_app(tmp_path, DOTENV_BASE + "APP_DEBUG=true\n")
    assert dict(os.environ) == before


def test_dotenv_can_forbid_unknown_names_and_names_the_line(tmp_path: Path) -> None:
    file = write(tmp_path / ".env", DOTENV_BASE + "APP_DEBGU=true\n")
    error = failure([cfg.dotenv(file, forbid_unknown=True)])
    problem = error.errors()[0]
    assert problem["code"] == "unknown_field"
    assert f"{file}:3:" in problem["message"]
    assert "did you mean APP_DEBUG?" in problem["message"]
    assert cfg.load(App, [cfg.dotenv(file)]).debug is False


def test_a_dotenv_file_that_is_not_utf8_says_so(tmp_path: Path) -> None:
    file = tmp_path / ".env"
    file.write_bytes(b"APP_DEBUG=\xff\n")
    error = failure([cfg.dotenv(file)], ox.DecodeError)
    assert "not valid UTF-8" in error.errors()[0]["message"]


# -- TOML files ----------------------------------------------------------------------------------


TOML = """\
debug = true            # a comment
plugins = [
  "auth",
  "billing",
]

[server]
host = "0.0.0.0"
port = 9000

[db]
url = "postgres://file"
pool_size = 30
password = "from-file"
"""


def test_a_toml_file_is_read_with_its_sections(tmp_path: Path) -> None:
    settings = cfg.load(App, [cfg.file(write(tmp_path / "app.toml", TOML))])
    assert settings.debug is True
    assert settings.plugins == ("auth", "billing")
    assert (settings.server.host, settings.server.port) == ("0.0.0.0", 9000)
    assert (settings.db.url, settings.db.pool_size) == ("postgres://file", 30)
    assert settings.db.password.reveal() == "from-file"


def test_a_toml_file_knows_the_line_of_each_value(tmp_path: Path) -> None:
    file = write(tmp_path / "app.toml", TOML)
    settings = cfg.load(App, [cfg.file(file)])
    port = cfg.origin(settings, "server.port")
    assert (port.kind, port.source, port.key, port.line) == ("file", file, "server.port", 9)
    assert cfg.origin(settings, "debug").line == 1
    assert cfg.origin(settings, "plugins").line == 2
    assert cfg.origin(settings, "db.url").line == 12


@ox.config(rename_all="camelCase")
class Wire:
    pool_size: int = 1
    max_age: int = ox.field(default=2, aliases=["maxAgeSeconds"])
    dsn_url: str = ox.field(default="", rename="DSN")


def test_files_use_wire_names_and_aliases_but_mappings_use_python_names(tmp_path: Path) -> None:
    file = write(tmp_path / "w.toml", 'poolSize = 5\nmaxAgeSeconds = 9\nDSN = "d"\n')
    settings = cfg.load(Wire, [cfg.file(file)])
    assert (settings.pool_size, settings.max_age, settings.dsn_url) == (5, 9, "d")
    python = cfg.load(Wire, [cfg.mapping({"pool_size": 6, "dsn_url": "m"})])
    assert (python.pool_size, python.dsn_url) == (6, "m")
    assert cfg.origin(settings, "max_age").key == "maxAgeSeconds"
    assert cfg.dump(settings) == {"poolSize": 5, "maxAge": 9, "DSN": "d"}


def test_a_file_that_sets_one_field_twice_through_an_alias_is_a_problem(tmp_path: Path) -> None:
    file = write(tmp_path / "w.toml", "maxAge = 1\nmaxAgeSeconds = 2\n")
    with pytest.raises(ox.DecodeError, match="sets one field twice") as caught:
        cfg.load(Wire, [cfg.file(file)])
    assert "'maxAge' and as 'maxAgeSeconds'" in caught.value.errors()[0]["message"]


def test_the_python_name_of_a_field_is_not_its_wire_name_in_a_file(tmp_path: Path) -> None:
    file = write(tmp_path / "w.toml", "pool_size = 5\n")
    with pytest.raises(ox.ValidationError) as caught:
        cfg.load(Wire, [cfg.file(file)])
    (problem,) = caught.value.errors()
    assert problem["code"] == "unknown_field"
    assert "did you mean 'poolSize'?" in problem["message"]


def test_toml_dates_and_times_are_read_as_text_the_engine_understands(tmp_path: Path) -> None:
    @ox.config
    class Schedule:
        at: datetime.datetime
        day: datetime.date
        clock: datetime.time

    file = write(
        tmp_path / "s.toml", "at = 2026-01-02T03:04:05Z\nday = 2026-01-02\nclock = 03:04:05\n"
    )
    settings = cfg.load(Schedule, [cfg.file(file)])
    assert settings.at == datetime.datetime(2026, 1, 2, 3, 4, 5, tzinfo=datetime.UTC)
    assert settings.day == datetime.date(2026, 1, 2)
    assert settings.clock == datetime.time(3, 4, 5)


def test_a_malformed_toml_file_says_where(tmp_path: Path) -> None:
    file = write(tmp_path / "bad.toml", "debug = true\n[db\nurl = 'x'\n")
    error = failure([cfg.file(file)], ox.DecodeError)
    (problem,) = error.errors()
    assert problem["code"] == "config_source"
    assert problem["path"] == [file]
    assert (problem["ctx"]["line"], problem["ctx"]["column"]) == (2, 4)
    assert f"invalid TOML in {file!r}, line 2, column 4" in problem["message"]
    assert "fix the syntax at that position" in problem["message"]


def test_a_missing_file_says_where_it_looked_and_what_to_do(tmp_path: Path) -> None:
    file = str(tmp_path / "nope.toml")
    error = failure([cfg.file(file)], ox.DecodeError)
    message = error.errors()[0]["message"]
    assert f"config file {file!r} not found" in message
    assert "looked for" in message
    assert "create it, fix the path, or pass optional=True" in message
    assert cfg.load(App, [cfg.env(environ=REQUIRED), cfg.file(file, optional=True)])


def test_an_extension_oxiform_does_not_read_says_what_to_use(tmp_path: Path) -> None:
    file = write(tmp_path / "app.yaml", "debug: true\n")
    message = failure([cfg.file(file)], ox.DecodeError).errors()[0]["message"]
    assert "does not read yet" in message
    assert ".toml or .json" in message
    assert "format=" in message
    forced = write(tmp_path / "app.conf", TOML)
    assert cfg.load(App, [cfg.file(forced, format="toml")]).server.port == 9000


def test_a_file_that_is_not_utf8_or_not_a_file_is_a_problem(tmp_path: Path) -> None:
    binary = tmp_path / "b.toml"
    binary.write_bytes(b"debug = \xff")
    assert "not valid UTF-8" in failure([cfg.file(binary)], ox.DecodeError).errors()[0]["message"]
    directory = tmp_path / "dir.toml"
    directory.mkdir()
    message = failure([cfg.file(directory)], ox.DecodeError).errors()[0]["message"]
    assert "cannot read config file" in message


def test_an_unknown_key_names_its_file_line_and_the_closest_field(tmp_path: Path) -> None:
    file = write(tmp_path / "app.toml", TOML.replace("pool_size = 30", "pool_sze = 30"))
    problem = failure([cfg.file(file)]).errors()[0]
    assert problem["code"] == "unknown_field"
    assert problem["path"] == [file, "db", "pool_sze"]
    assert "unknown key 'pool_sze' (did you mean 'pool_size'?)" in problem["message"]
    assert f"(from {file}:13, key db.pool_sze)" in problem["message"]


# -- JSON files ----------------------------------------------------------------------------------


def test_a_json_file_is_read_and_has_no_line(tmp_path: Path) -> None:
    file = write(
        tmp_path / "app.json",
        '{"debug": true, "server": {"port": 9000}, "db": {"url": "u", "password": "p"}}',
    )
    settings = cfg.load(App, [cfg.file(file)])
    assert (settings.debug, settings.server.port) == (True, 9000)
    origin = cfg.origin(settings, "server.port")
    assert (origin.kind, origin.source, origin.key) == ("file", file, "server.port")
    assert origin.line is None
    assert str(origin) == f"{file} (server.port)"


def test_json_problems_say_what_and_where(tmp_path: Path) -> None:
    syntax = write(tmp_path / "bad.json", '{"debug": tru}')
    problem = failure([cfg.file(syntax)], ox.DecodeError).errors()[0]
    assert f"invalid JSON in {syntax!r}" in problem["message"]
    assert problem["ctx"]["line"] == 1
    twice = write(tmp_path / "dup.json", '{"debug": true, "debug": false}')
    duplicate = failure([cfg.file(twice)], ox.DecodeError).errors()[0]["message"]
    assert "the key 'debug' appears twice" in duplicate
    scalar = write(tmp_path / "list.json", "[1, 2]")
    message = failure([cfg.file(scalar)], ox.DecodeError).errors()[0]["message"]
    assert "holds list, not an object" in message


# -- pyproject.toml --------------------------------------------------------------------------------


PYPROJECT = """\
[project]
name = "demo"

[tool.app]
debug = true

[tool.app.server]
port = 9100

[tool.app.db]
url = "u"
password = "p"
"""


def test_pyproject_reads_a_table_by_its_dotted_path(tmp_path: Path) -> None:
    file = write(tmp_path / "pyproject.toml", PYPROJECT)
    settings = cfg.load(App, [cfg.pyproject("tool.app", path=file)])
    assert (settings.debug, settings.server.port, settings.db.url) == (True, 9100, "u")
    port = cfg.origin(settings, "server.port")
    assert (port.kind, port.source, port.key) == ("pyproject", file, "tool.app.server.port")
    assert port.line == 8
    assert str(port) == f"{file}:8 (tool.app.server.port)"


def test_pyproject_defaults_to_the_file_in_the_current_directory(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    write(tmp_path / "pyproject.toml", PYPROJECT)
    monkeypatch.chdir(tmp_path)
    settings = cfg.load(App, [cfg.pyproject("tool.app")])
    assert settings.db.url == "u"
    assert cfg.origin(settings, "debug").source == "pyproject.toml"


def test_a_missing_pyproject_or_table_is_empty_unless_required(tmp_path: Path) -> None:
    sources: list[cfg.Source] = [cfg.env(environ=REQUIRED)]
    missing = str(tmp_path / "absent" / "pyproject.toml")
    assert cfg.load(App, [*sources, cfg.pyproject("tool.app", path=missing)])
    file = write(tmp_path / "pyproject.toml", "[project]\nname = 'demo'\n")
    assert cfg.load(App, [*sources, cfg.pyproject("tool.app", path=file)])
    error = failure([cfg.pyproject("tool.app", path=file, optional=False)], ox.DecodeError)
    message = error.errors()[0]["message"]
    assert "has no [tool.app] table" in message
    assert "optional=True" in message
    gone = failure([cfg.pyproject("tool.app", path=missing, optional=False)], ox.DecodeError)
    assert "not found" in gone.errors()[0]["message"]


def test_a_pyproject_value_that_is_not_a_table_is_a_problem(tmp_path: Path) -> None:
    file = write(tmp_path / "pyproject.toml", "[tool]\napp = 3\n")
    error = failure([cfg.pyproject("tool.app", path=file)], ox.DecodeError)
    assert "not a table at [tool.app]" in error.errors()[0]["message"]


def test_a_malformed_pyproject_is_a_problem(tmp_path: Path) -> None:
    file = write(tmp_path / "pyproject.toml", "[tool.app\n")
    error = failure([cfg.pyproject("tool.app", path=file)], ox.DecodeError)
    assert error.errors()[0]["ctx"]["source"] == "pyproject"


# -- mappings --------------------------------------------------------------------------------------


def test_a_mapping_gives_python_names_with_nested_mappings() -> None:
    mapped = cfg.mapping({"server": {"port": "9200"}, "debug": True})
    settings = cfg.load(App, [cfg.env(environ=REQUIRED), mapped])
    assert (settings.server.port, settings.debug) == (9200, True)
    origin = cfg.origin(settings, "server.port")
    assert (origin.kind, origin.source, origin.key) == ("mapping", "mapping", "server.port")
    assert origin.line is None
    assert str(origin) == "mapping server.port"


def test_a_mapping_has_a_name_that_errors_and_origins_use() -> None:
    bad = cfg.mapping({"server": {"port": 0}}, name="cli")
    problem = failure([cfg.env(environ=REQUIRED), bad]).errors()[0]
    assert problem["path"] == ["cli", "server", "port"]
    assert "(from cli server.port)" in problem["message"]
    good = cfg.load(App, [cfg.env(environ=REQUIRED), cfg.mapping({"debug": True}, name="cli")])
    assert str(cfg.origin(good, "debug")) == "cli debug"


def test_a_mapping_may_hold_a_secret_and_reveals_it_to_the_engine() -> None:
    data: dict[str, object] = {"db": {"url": "u", "password": ox.Secret("from-code")}}
    settings = cfg.load(App, [cfg.mapping(data)])
    assert settings.db.password.reveal() == "from-code"


def test_a_mapping_key_that_is_not_a_field_or_not_text_is_a_problem() -> None:
    unknown = failure([cfg.env(environ=REQUIRED), cfg.mapping({"debgu": True})])
    assert "did you mean 'debug'?" in unknown.errors()[0]["message"]
    odd: dict[object, object] = {1: True}
    wrong = failure([cfg.mapping(odd)], ox.DecodeError)  # type: ignore[arg-type]  # pyright: ignore[reportArgumentType]
    assert "is not a string" in wrong.errors()[0]["message"]


def test_a_mapping_is_a_snapshot_of_its_top_level() -> None:
    data: dict[str, object] = {"debug": True}
    source = cfg.mapping(data)
    data["debug"] = False
    assert cfg.load(App, [cfg.env(environ=REQUIRED), source]).debug is True


# -- the TOML line scanner ------------------------------------------------------------


def test_key_lines_finds_tables_keys_and_dotted_keys() -> None:
    text = """\
a = 1
[server]
host = "h"   # trailing
port.number = 9
"quoted.key" = 1
'lit' = 2

[[items]]
name = "x"
[[items]]
name = "y"
"""
    lines = key_lines(text)
    assert lines[("a",)] == 1
    assert lines[("server",)] == 2
    assert lines[("server", "host")] == 3
    assert lines[("server", "port")] == 4
    assert lines[("server", "port", "number")] == 4
    assert lines[("server", "quoted.key")] == 5
    assert lines[("server", "lit")] == 6
    assert lines[("items",)] == 8
    assert lines[("items", "name")] == 9


def test_key_lines_skips_the_inside_of_strings_arrays_and_inline_tables() -> None:
    text = (
        'first = """\nnot = "a key"\n[nor_this]\n"""\n'
        "second = '''\nx = 1\n'''\n"
        'array = [\n  # comment = 1\n  { inner = 1, other = [1, 2] },\n  "a]b",\n]\n'
        "inline = { a = 1, b = { c = 2 } }\n"
        "last = 5\n"
    )
    lines = key_lines(text)
    assert lines[("first",)] == 1
    assert lines[("second",)] == 5
    assert lines[("array",)] == 8
    assert lines[("inline",)] == 13
    assert lines[("last",)] == 14
    assert ("not",) not in lines
    assert ("nor_this",) not in lines
    assert ("x",) not in lines
    assert ("inner",) not in lines


def test_line_of_key_falls_back_to_the_nearest_ancestor() -> None:
    lines = key_lines("[db]\nurl = 1\n")
    assert line_of_key(lines, ("db", "url")) == 2
    assert line_of_key(lines, ("db", "other", "deeper")) == 1
    assert line_of_key(lines, ("none",)) is None


def test_key_lines_never_raises_on_text_it_cannot_follow() -> None:
    assert key_lines("a = [1, 2") == {("a",): 1}
    assert key_lines("= nothing") == {}


# -- a source that cannot be read never quotes what is in it ---------------------------------------

PEM = (
    "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAA\n"
    "S3CR3TKEYMATERIAL\n-----END PRIVATE KEY-----\n"
)


def only_problem(sources: list[cfg.Source]) -> ErrorDetails:
    error = failure(sources, ox.DecodeError)
    (problem,) = error.errors()
    assert problem["code"] == "config_source"
    return problem


def test_an_unquoted_multi_line_key_in_dotenv_is_reported_by_line_without_quoting_it(
    tmp_path: Path,
) -> None:
    file = write(tmp_path / ".env", "APP_TOKEN=" + PEM)
    error = failure([cfg.dotenv(file)], ox.DecodeError)
    messages = " ".join(item["message"] for item in error.errors())
    assert f"{file}:2: this line is not KEY=value" in messages
    assert "quote a value that spans lines" in messages
    for text in ("MIIEvQ", "S3CR3T", "BEGIN", "END PRIVATE"):
        assert text not in messages + repr(error.errors())


def test_text_after_a_closing_quote_and_a_line_without_an_equals_sign_are_not_quoted(
    tmp_path: Path,
) -> None:
    junk = write(tmp_path / "a.env", 'A_TOK="abc"S3CR3T\n')
    problem = only_problem([cfg.dotenv(junk)])
    assert "text follows the closing" in problem["message"]
    assert "S3CR3T" not in problem["message"] and "abc" not in problem["message"]
    bare = write(tmp_path / "b.env", "S3CR3TVALUEWITHNOEQUALS\n")
    problem = only_problem([cfg.dotenv(bare)])
    assert "not KEY=value" in problem["message"]
    assert "S3CR3T" not in problem["message"]


def test_a_dotenv_duplicate_names_the_variable_and_not_its_value(tmp_path: Path) -> None:
    file = write(tmp_path / ".env", "APP_DEBUG=S3CR3Tone\nAPP_DEBUG=S3CR3Ttwo\n")
    message = only_problem([cfg.dotenv(file)])["message"]
    assert "APP_DEBUG is set twice, on lines 1 and 2" in message
    assert "S3CR3T" not in message


def test_invalid_json_in_a_variable_or_a_file_does_not_quote_the_text(tmp_path: Path) -> None:
    variable = only_problem([cfg.env(environ={**REQUIRED, "APP_PLUGINS": "[S3CR3T, oops"})])
    assert "not valid JSON" in variable["message"]
    assert "syntax error at line 1, column 2 (the text is not shown)" in variable["message"]
    assert "S3CR3T" not in variable["message"]
    file = write(tmp_path / "bad.json", '{"debug": S3CR3T}')
    problem = only_problem([cfg.file(file)])
    assert "syntax error at line 1" in problem["message"]
    assert "S3CR3T" not in problem["message"]


def test_invalid_toml_does_not_quote_the_text(tmp_path: Path) -> None:
    for text in (
        "password = hunter2-S3CR3T\n",
        'password = "S3CR3T\n',
        "[S3CR3T\n",
        'a = "x" S3CR3T\n',
    ):
        file = write(tmp_path / "bad.toml", text)
        problem = only_problem([cfg.file(file)])
        assert "S3CR3T" not in problem["message"] + repr(problem["ctx"])
        assert "line 1" in problem["message"]


def test_a_value_that_is_not_utf8_is_a_located_problem_without_the_value() -> None:
    bad = "tok\udcffen-S3CR3T"
    for name, value in (("APP_DEBUG", bad), ("APP_PLUGINS", f'["{bad}"]')):
        error = failure([cfg.env(environ={**REQUIRED, name: value})], ox.DecodeError)
        (problem,) = error.errors()
        assert f"the value of {name} is not valid text" in problem["message"]
        assert problem["ctx"]["key"] == name
        assert "S3CR3T" not in repr(error.errors()) + str(error)
        assert error.__context__ is None and error.__cause__ is None


def test_a_mapping_value_that_is_not_utf8_is_a_problem_without_the_value() -> None:
    data: dict[str, object] = {"db": {"url": "u", "password": "p\udcffS3CR3T"}}
    error = failure([cfg.mapping(data)], ox.DecodeError)
    (problem,) = error.errors()
    assert problem["code"] == "config_source"
    assert "S3CR3T" not in repr(error.errors())
    assert error.__context__ is None
    assert error.unreadable_sources() == ["mapping"]


def test_secrets_with_text_that_is_not_utf8_still_compare() -> None:
    assert ox.Secret("a\udcffb") == ox.Secret("a\udcffb")
    assert ox.Secret("a\udcffb") != ox.Secret("a\udcfec")


def test_the_repr_of_an_env_or_mapping_source_shows_no_value() -> None:
    environ = {"APP_DB__PASSWORD": "S3CR3T"}
    mapped = cfg.mapping({"db": {"password": "S3CR3T"}}, name="cli")
    assert "S3CR3T" not in repr(cfg.env(environ=environ)) + repr(mapped)
    assert repr(cfg.env(environ=environ)) == "cfg.env(environ=<1 variables>, forbid_unknown=False)"
    assert repr(mapped) == "cfg.mapping(<1 keys>, name='cli')"
    assert repr(cfg.env()) == "cfg.env(environ=os.environ, forbid_unknown=False)"


def test_a_mapping_cannot_take_a_name_that_already_means_something() -> None:
    with pytest.raises(
        ValueError, match="name 'env' is how errors and origins call the environment"
    ):
        cfg.mapping({}, name="env")
    with pytest.raises(ValueError, match="name 'default' is how errors and origins call"):
        cfg.mapping({}, name="default")
