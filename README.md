# oxiform

Validate and (de)serialize your Python types to JSON, YAML, TOML and MessagePack in a single
pass, with a Rust engine. Load layered configuration with the same models.

```python
from typing import Annotated
import oxiform as ox

@ox.model(rename_all="camelCase")
class User:
    id: int
    name: Annotated[str, ox.Len(1, 80)]
    email: ox.Email

user = ox.json.decode(raw_bytes, User)
ox.yaml.encode(user)
```

**Status:** early development (phase 0, prototype). Nothing is published yet. The design is in
[docs/spec.md](docs/spec.md).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at
your option.
