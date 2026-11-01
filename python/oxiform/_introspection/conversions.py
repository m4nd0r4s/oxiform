"""Types read from a string or a number and built by calling the class: ``Decimal``, ``UUID``,
``Path``, the ``ipaddress`` classes, ``ZoneInfo`` and ``Flag`` enums."""

import decimal
import enum
import ipaddress
import pathlib
import uuid
import zoneinfo

from oxiform._introspection.model import ConversionDescription

__all__ = ["TEXT_TYPES", "conversion_of", "flag_conversion"]

_CONVERSIONS: dict[object, ConversionDescription] = {
    decimal.Decimal: ConversionDescription("Decimal", "decimal", "decimal", decimal.Decimal),
    uuid.UUID: ConversionDescription("UUID", "uuid", "str", uuid.UUID),
    zoneinfo.ZoneInfo: ConversionDescription("ZoneInfo", "zoneinfo", "str", zoneinfo.ZoneInfo),
    **{
        cls: ConversionDescription(cls.__name__, "path", "str", cls)
        for cls in (
            pathlib.Path,
            pathlib.PurePath,
            pathlib.PosixPath,
            pathlib.PurePosixPath,
            pathlib.WindowsPath,
            pathlib.PureWindowsPath,
        )
    },
    **{
        cls: ConversionDescription(cls.__name__, format, "str", cls)
        for cls, format in (
            (ipaddress.IPv4Address, "ipv4_address"),
            (ipaddress.IPv6Address, "ipv6_address"),
            (ipaddress.IPv4Network, "ipv4_network"),
            (ipaddress.IPv6Network, "ipv6_network"),
            (ipaddress.IPv4Interface, "ipv4_interface"),
            (ipaddress.IPv6Interface, "ipv6_interface"),
        )
    },
}

TEXT_TYPES: tuple[type, ...] = (
    decimal.Decimal,
    uuid.UUID,
    pathlib.PurePath,
    ipaddress.IPv4Address,
    ipaddress.IPv6Address,
    ipaddress.IPv4Network,
    ipaddress.IPv6Network,
)
"""Types written as ``str(value)``. Interfaces are subclasses of the address classes."""


def conversion_of(annotation: object) -> ConversionDescription | None:
    """The conversion of a class read from a string or a number, or ``None``."""
    try:
        return _CONVERSIONS.get(annotation)
    except TypeError:  # an unhashable annotation
        return None


def flag_conversion(cls: type[enum.Flag]) -> ConversionDescription:
    """A ``Flag`` is read from the integer of its value: any combination of its members."""
    return ConversionDescription(cls.__qualname__, "enum", "int", cls)
