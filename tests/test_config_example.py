"""The server config example runs, and says what the Douro spec says about precedence."""

import runpy
from pathlib import Path
from typing import Any

from oxiform import cfg

EXAMPLE = Path(__file__).parent.parent / "examples" / "server_config.py"


def test_the_example_loads_every_source_in_the_documented_order() -> None:
    result: dict[str, Any] = runpy.run_path(str(EXAMPLE))["build"]()
    settings: Any = result["settings"]
    # CLI flags > DOURO_* > .env > douro.toml > [tool.douro] > defaults
    assert settings.port == 9300
    assert settings.log_level == "debug"
    assert settings.admin_token.reveal() == "s3cr3t-admin-token"
    assert settings.backlog == 4096
    assert settings.access_log is True
    assert settings.workers == 4
    assert settings.host == "0.0.0.0"
    assert settings.allowed_hosts == ("example.com",)
    assert settings.lanes == "auto"
    assert settings.tls.cert == "cert.pem"
    assert settings.tls.key_password.reveal() == "from-the-file"
    port = cfg.origin(settings, "port")
    assert (port.kind, port.source) == ("mapping", "cli")
    assert len(port.overrides) == 3
    assert cfg.origin(settings, "admin_token").key == "DOURO_ADMIN_TOKEN"


def test_the_example_table_and_error_hide_the_secrets_and_name_the_source() -> None:
    result: dict[str, Any] = runpy.run_path(str(EXAMPLE))["build"]()
    table: str = result["table"]
    for secret in ("s3cr3t-admin-token", "from-the-file"):
        assert secret not in table
        assert secret not in repr(result["settings"])
    assert "'********'" in table
    assert "cli port" in table
    assert "DOURO_BACKLOG" in table
    assert result["mistake"] == "/env/port: must be at most 65535 (from env DOURO_PORT)"
