from importlib.metadata import version

import oxiform


def test_extension_version_matches_package() -> None:
    assert oxiform.__version__ == version("oxiform")
