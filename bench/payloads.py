"""Deterministic JSON payloads for the three benchmark models."""

import json
import random

__all__ = ["PAYLOADS"]

_random = random.Random(20261031)


def _small() -> dict[str, object]:
    return {
        "id": 42,
        "name": "Ada Lovelace",
        "email": "ada@example.org",
        "active": True,
        "score": 9.5,
    }


def _medium() -> dict[str, object]:
    return {
        "id": 1234567,
        "username": "ada_lovelace",
        "full_name": "Ada King, Countess of Lovelace",
        "email": "ada@example.org",
        "age": 36,
        "height": 1.65,
        "weight": 55.2,
        "active": True,
        "verified": False,
        "role": "admin",
        "country": "GB",
        "city": "London",
        "postal_code": "1000-001",
        "phone": None,
        "tags": ["math", "engines", "poetry"],
        "followers": 10423,
        "following": 87,
        "rating": 4.8,
        "bio": "Wrote the first published algorithm for a machine.",
        "website": "https://example.org/ada",
    }


def _nested() -> dict[str, object]:
    def item(index: int) -> dict[str, object]:
        return {
            "sku": f"SKU-{index:05d}",
            "quantity": _random.randint(1, 20),
            "price": round(_random.uniform(1, 500), 2),
            "tags": [f"tag{_random.randint(0, 9)}" for _ in range(3)],
        }

    return {
        "order_id": 99,
        "customer": {"id": 7, "name": "Grace Hopper", "email": "grace@example.org"},
        "items": [item(index) for index in range(50)],
        "notes": None,
    }


PAYLOADS: dict[str, bytes] = {
    name: json.dumps(build()).encode()
    for name, build in {"small": _small, "medium": _medium, "nested": _nested}.items()
}
