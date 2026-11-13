"""``@ox.validator``: Python logic that runs on a value after the engine has read it.

The decorator only marks a function (it returns a descriptor that behaves as the function would in
the class body) and checks what can be checked without the types of the module, such as its kind.
What needs the annotations (which parameter, if any, receives the validation context) is read when
the plan of the model is compiled, so that a context type defined further down the module resolves.
Spec section 4.3.
"""

import dataclasses
import inspect
import types
import typing
from collections.abc import Callable
from typing import Any, Literal, TypeVar, overload

from oxiform._context import is_context_annotation
from oxiform.errors import SchemaError

__all__ = ["FieldValidatorSpec", "ValidatorSpec", "validator", "validators_of"]

_F = TypeVar("_F", bound=Callable[..., object])


@dataclasses.dataclass(frozen=True, slots=True)
class FieldValidatorSpec:
    """What the signature of a validator says about the validation context."""

    context: str | None
    """The name of the parameter that receives the context, or ``None`` if it takes none."""
    required: bool
    """Whether the validator cannot run without a context: the parameter has no default and its
    annotation does not allow ``None``."""
    none_when_absent: bool
    """Whether the validator is given ``None`` when there is no context: the annotation is
    ``ox.Context[T] | None`` and the parameter has no default to fall back on."""


class ValidatorSpec:
    """A function marked by ``@ox.validator``, as it sits in the namespace of a class.

    A field validator (``fields`` is not empty) is called with the class; a model validator with
    the instance. Reading it from the class or an instance gives the function bound that way, so
    a test can call ``Signup.not_reserved("x")`` as it would a ``classmethod``.
    """

    __slots__ = ("_signature", "fields", "function", "model", "name")

    def __init__(self, function: Callable[..., object], fields: tuple[str, ...]) -> None:
        self.function = function
        self.fields = fields
        self.model = not fields
        self.name = getattr(function, "__qualname__", repr(function))
        self._signature: FieldValidatorSpec | None = None

    def __get__(self, instance: object, owner: type | None = None) -> Callable[..., object]:
        if self.model:
            return self.function if instance is None else types.MethodType(self.function, instance)
        return types.MethodType(self.function, owner if owner is not None else type(instance))

    def __repr__(self) -> str:
        kind = "model" if self.model else f"field {', '.join(self.fields)}"
        return f"<ox.validator {self.name} ({kind})>"

    def signature(self) -> FieldValidatorSpec:
        """Which parameter takes the context, checked once. Raises ``NameError`` when an
        annotation names something that is not defined yet, and ``SchemaError`` when the
        validator cannot be called as the spec says."""
        if self._signature is None:
            self._signature = _read_signature(self)
        return self._signature


@overload
def validator(function: _F, /) -> _F: ...
@overload
def validator(*fields: str, mode: Literal["after", "before"] = "after") -> Callable[[_F], _F]: ...
def validator(*arguments: Any, mode: Literal["after", "before"] = "after") -> Any:
    """Marks a function of a model as a validator.

    ``@ox.validator("a", "b")`` is a field validator, called ``f(cls, value)`` and returning the
    value (changed, if it wants); bare ``@ox.validator`` is a model validator, called ``f(self)``
    on the finished instance and returning ``None``. A parameter annotated ``ox.Context``
    receives the ``context=`` of the decode. ``ox.Invalid``, ``ValueError`` and ``TypeError``
    reject the value; any other exception propagates.
    """
    if mode != "after":
        raise SchemaError(
            f'@ox.validator: mode={mode!r} is not supported, only mode="after": a decode reads '
            "the input once, straight into the final types, so there is no raw value to hand to "
            "a validator before that; check the value after it is read, or declare the rule "
            "with a constraint"
        )
    if len(arguments) == 1 and (
        callable(arguments[0]) or isinstance(arguments[0], (classmethod, staticmethod))
    ):
        return _mark(typing.cast("object", arguments[0]), ())
    names = _field_names(arguments)

    def mark(function: object) -> object:
        return _mark(function, names)

    return mark


def _field_names(arguments: tuple[object, ...]) -> tuple[str, ...]:
    if not arguments:
        raise SchemaError(
            "@ox.validator() with parentheses and no field: name the fields it checks, "
            '@ox.validator("name"), or drop the parentheses for a validator of the whole model'
        )
    for argument in arguments:
        if not isinstance(argument, str):
            raise SchemaError(
                f"@ox.validator takes the names of fields as str, not {argument!r}; "
                'write @ox.validator("username"), or bare @ox.validator for a whole model'
            )
    names = typing.cast("tuple[str, ...]", arguments)
    if len(set(names)) != len(names):
        raise SchemaError(f"@ox.validator names a field twice: {', '.join(names)}")
    return names


def _mark(function: object, fields: tuple[str, ...]) -> ValidatorSpec:
    if isinstance(function, classmethod):
        if not fields:
            raise SchemaError(
                "a validator of the whole model receives the instance: write it as a plain "
                "method, def check(self), without @classmethod"
            )
        function = typing.cast("Any", function).__func__
    if isinstance(function, staticmethod):
        raise SchemaError(
            "@ox.validator does not take a staticmethod: a field validator receives the class "
            "(def check(cls, value)) and a model validator the instance (def check(self))"
        )
    if not inspect.isfunction(function):
        raise SchemaError(
            f"@ox.validator marks a function in the body of the model, not {function!r}"
        )
    name = function.__qualname__
    if function.__name__ == "<lambda>":
        raise SchemaError(f"@ox.validator needs a def, not a lambda ({name}): give it a name")
    if (
        inspect.iscoroutinefunction(function)
        or inspect.isasyncgenfunction(function)
        or inspect.isgeneratorfunction(function)
    ):
        raise SchemaError(
            f"{name}: a validator is an ordinary function. A decode is not asynchronous, so a "
            "coroutine would never be awaited: do the awaiting before the decode and pass the "
            "result in context="
        )
    return ValidatorSpec(function, fields)


def _read_signature(spec: ValidatorSpec) -> FieldValidatorSpec:
    function = spec.function
    hints = typing.get_type_hints(function, include_extras=True)
    context: str | None = None
    required = False
    none_when_absent = False
    for position, parameter in enumerate(inspect.signature(function).parameters.values()):
        if not is_context_annotation(hints.get(parameter.name, parameter.annotation)):
            continue
        # The first parameters are `cls` or `self`, and the value of a field validator.
        if position < (1 if spec.model else 2):
            raise SchemaError(
                f"{spec.name}: the parameter {parameter.name!r} is the "
                f"{'instance' if spec.model else 'class or the value'}, not the context; put "
                "the ox.Context parameter after it"
            )
        if context is not None:
            raise SchemaError(
                f"{spec.name}: two parameters are annotated ox.Context ({context!r} and "
                f"{parameter.name!r}); a validator takes the context once"
            )
        if parameter.kind in (parameter.POSITIONAL_ONLY, parameter.VAR_POSITIONAL):
            raise SchemaError(
                f"{spec.name}: the ox.Context parameter {parameter.name!r} is passed by keyword, "
                "so it cannot be positional-only: remove the / before it"
            )
        context = parameter.name
        optional = typing.get_origin(hints.get(parameter.name)) is not typing.Annotated
        has_default = parameter.default is not parameter.empty
        required = not has_default and not optional
        none_when_absent = optional and not has_default
    probe: dict[str, object] = {} if context is None else {context: None}
    arguments: tuple[object, ...] = (None,) if spec.model else (None, None)
    try:
        inspect.signature(function).bind(*arguments, **probe)
    except TypeError as error:
        shape = "def check(self)" if spec.model else "def check(cls, value)"
        raise SchemaError(
            f"{spec.name}: it cannot be called as a validator ({error}); write it as {shape}, "
            "with an optional ctx: ox.Context for the validation context"
        ) from error
    return FieldValidatorSpec(context, required, none_when_absent)


def validators_of(cls: type) -> list[ValidatorSpec]:
    """The validators of ``cls``, bases first, each in the order it is defined in its class. A
    validator that a subclass redefines under the same name is the subclass's, in its place; a
    name redefined as anything else is not a validator any more."""
    found: dict[str, ValidatorSpec] = {}
    for klass in reversed(cls.__mro__):
        for name, value in vars(klass).items():
            # `@classmethod` above `@ox.validator` wraps the marked function: still a validator.
            marked = value.__func__ if isinstance(value, classmethod) else value  # pyright: ignore[reportUnknownMemberType, reportUnknownVariableType]
            if isinstance(marked, ValidatorSpec):
                if marked is not value and marked.model:
                    raise SchemaError(
                        f"{marked.name}: a validator of the whole model receives the instance: "
                        "do not make it a classmethod"
                    )
                found.pop(name, None)
                found[name] = marked
            elif name in found:
                del found[name]
    return list(found.values())
