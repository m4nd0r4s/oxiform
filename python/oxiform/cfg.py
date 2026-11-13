"""Layered configuration: ``from oxiform import cfg`` (spec section 6, D24).

Declare the config with ``@ox.config`` and load it from sources, the last one winning::

    settings = cfg.load(AppConfig, sources=[
        cfg.pyproject("tool.app"),
        cfg.file("config.toml", optional=True),
        cfg.dotenv(".env", optional=True),
        cfg.env(),
    ])
    cfg.origin(settings, "db.pool_size")   # which source and key set it
"""

from oxiform._config.load import dump, load, origin, origins
from oxiform._config.origin import Origin, OriginKind
from oxiform._config.sources import Source, dotenv, env, file, mapping, pyproject

__all__ = [
    "Origin",
    "OriginKind",
    "Source",
    "dotenv",
    "dump",
    "env",
    "file",
    "load",
    "mapping",
    "origin",
    "origins",
    "pyproject",
]
