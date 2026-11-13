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

Python logic that needs data from outside the input (who is asking, which tenant) is a validator,
and the host passes that data as `context=`:

```python
@ox.model
class Member:
    email: str

    @ox.validator("email")
    def in_your_tenant(cls, value: str, request: ox.Context[Request]) -> str:
        if not value.endswith("@" + request.tenant):
            raise ox.Invalid("not an address of your tenant", code="tenant")
        return value

member = ox.json.decode(raw_bytes, Member, context=request)
```

See [examples/validation_context.py](examples/validation_context.py) and section 4.3 of the spec.

**Status:** early development (phase 0, prototype). Nothing is published yet. The design is in
[docs/spec.md](docs/spec.md).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at
your option.
