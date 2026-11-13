"""A server's configuration, the way ``douro.ServerConfig`` needs it: layered sources, where
each value came from, and credentials that never show (docs/spec.md, section 6).

The precedence is the one of the Douro spec: CLI flags > ``DOURO_*`` environment > ``.env`` >
``douro.toml`` > ``[tool.douro]`` of ``pyproject.toml`` > the defaults of the class.

Run it with ``python examples/server_config.py``.
"""

import tempfile
from pathlib import Path
from typing import Annotated, Literal

import oxiform as ox
from oxiform import cfg


@ox.config
class Tls:
    cert: str
    key: str
    key_password: ox.Secret[str] | None = None


@ox.config(env_prefix="DOURO_", env_nested="__")
class ServerConfig:
    host: str = "127.0.0.1"
    port: Annotated[int, ox.Ge(1), ox.Le(65535)] = 8000
    uds: str | None = None
    workers: int | Literal["auto"] = "auto"
    lanes: int | Literal["auto"] = "auto"
    http: Literal["auto", "1", "2"] = "auto"
    backlog: int = 2048
    graceful_timeout: float = 30.0
    max_requests: int | None = None
    reuse_port: bool = False
    log_level: Literal["debug", "info", "warning", "error"] = "info"
    access_log: bool = False
    allowed_hosts: tuple[str, ...] = ()
    tls: Tls | None = None
    admin_token: ox.Secret[str] | None = None


def load_server_config(
    root: Path, environ: dict[str, str], cli_flags: dict[str, object]
) -> ServerConfig:
    return cfg.load(
        ServerConfig,
        [
            cfg.pyproject("tool.douro", path=root / "pyproject.toml"),
            cfg.file(root / "douro.toml", optional=True),
            cfg.dotenv(root / ".env", optional=True),
            cfg.env(environ=environ),
            cfg.mapping(cli_flags, name="cli"),
        ],
    )


def explain(settings: ServerConfig, root: Path) -> str:
    """The table `douro config explain` shows: key, value and origin, secrets hidden."""
    values = cfg.dump(settings)
    rows: list[str] = []
    for path, origin in cfg.origins(settings).items():
        value: object = values
        for part in path.split("."):
            value = value[part] if isinstance(value, dict) else None  # pyright: ignore[reportUnknownVariableType]
        where = str(origin).replace(f"{root}/", "")
        rows.append(f"{path:<20} {value!r:<28} {where}")
    return "\n".join(rows)


def build() -> dict[str, object]:
    """Loads the config from files in a temporary directory, as a real project would."""
    root = Path(tempfile.mkdtemp(prefix="douro-example-"))
    (root / "pyproject.toml").write_text(
        '[tool.douro]\nhost = "0.0.0.0"\nport = 9000\nlog_level = "warning"\n'
        'allowed_hosts = ["example.com"]\n'
    )
    (root / "douro.toml").write_text(
        'port = 9100\nworkers = 4\n\n[tls]\ncert = "cert.pem"\nkey = "key.pem"\n'
        'key_password = "from-the-file"\n'
    )
    (root / ".env").write_text("DOURO_BACKLOG=4096\nDOURO_ACCESS_LOG=yes\n")
    environ = {"DOURO_PORT": "9200", "DOURO_ADMIN_TOKEN": "s3cr3t-admin-token"}
    cli_flags: dict[str, object] = {"port": 9300, "log_level": "debug"}
    settings = load_server_config(root, environ, cli_flags)

    # A mistake in the environment names the variable and what to do about it.
    try:
        load_server_config(root, {**environ, "DOURO_PORT": "70000"}, {})
    except ox.ValidationError as error:
        problem = error.errors()[0]
        mistake = f"{problem['pointer']}: {problem['message']}"
    return {"settings": settings, "table": explain(settings, root), "mistake": mistake}


if __name__ == "__main__":
    result = build()
    print(result["settings"])
    print()
    print(result["table"])
    print()
    print(result["mistake"])
