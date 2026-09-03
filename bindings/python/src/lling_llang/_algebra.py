"""Validated consumers for host-defined lattice and semiring resources."""

from __future__ import annotations

import ctypes
import math
import threading
from collections.abc import Sequence

from vinary_tree_interop import (
    DomainId,
    LatticeFlag,
    NativeResource,
    SemiringOrder,
    SemiringProperty,
    VtResource,
)

from ._abi import MAX_LAW_SAMPLES, NativeError, Status, check, lib, native_resource


class _InterfaceId(ctypes.Structure):
    _fields_ = [("value", ctypes.c_uint8 * 16)]


def _thread_error(operation: str) -> NativeError:
    return NativeError(
        Status.PROVIDER_ERROR,
        operation,
        "dynamic algebra handles are bound to their creating thread",
    )


class LatticeValue:
    """Owned same-thread adapter for one immutable host lattice value."""

    __hash__ = None  # pyright: ignore[reportAssignmentType]
    __slots__ = ("_handle", "_thread")

    def __init__(self, resource: NativeResource | VtResource) -> None:
        raw = native_resource(resource)
        self._handle = ctypes.c_void_p()
        self._thread = threading.get_ident()
        check(
            lib.lling_lattice_open(ctypes.byref(raw), ctypes.byref(self._handle)),
            "lattice_open",
        )

    @classmethod
    def _adopt(cls, handle: ctypes.c_void_p, thread: int) -> LatticeValue:
        if not handle.value:
            raise NativeError(
                Status.PANIC,
                "lattice_adopt",
                "native operation returned a null successful lattice value",
            )
        value = cls.__new__(cls)
        value._handle = handle
        value._thread = thread
        return value

    def _open_handle(self, operation: str = "lattice") -> ctypes.c_void_p:
        if threading.get_ident() != self._thread:
            raise _thread_error(operation)
        if not self._handle.value:
            raise NativeError(Status.CLOSED, operation, "lattice value is closed")
        return self._handle

    @property
    def domain_id(self) -> DomainId:
        """Return the exact provider-defined lattice domain identifier."""
        output = _InterfaceId()
        check(
            lib.lling_lattice_domain_id(
                self._open_handle("lattice_domain_id"), ctypes.byref(output)
            ),
            "lattice_domain_id",
        )
        return DomainId(bytes(output.value))

    @property
    def flags(self) -> LatticeFlag:
        """Return the validated provider capability flags."""
        output = ctypes.c_uint64()
        check(
            lib.lling_lattice_flags(
                self._open_handle("lattice_flags"), ctypes.byref(output)
            ),
            "lattice_flags",
        )
        return LatticeFlag(output.value)

    def _binary(self, other: object, operation: str) -> LatticeValue:
        if not isinstance(other, LatticeValue):
            raise TypeError("lattice operands must be LatticeValue instances")
        output = ctypes.c_void_p()
        function = getattr(lib, f"lling_lattice_{operation}")
        check(
            function(
                self._open_handle(f"lattice_{operation}"),
                other._open_handle(f"lattice_{operation}"),
                ctypes.byref(output),
            ),
            f"lattice_{operation}",
        )
        return LatticeValue._adopt(output, self._thread)

    def join(self, other: LatticeValue) -> LatticeValue:
        """Return the least upper bound of two same-domain values."""
        return self._binary(other, "join")

    def meet(self, other: LatticeValue) -> LatticeValue:
        """Return the greatest lower bound of two same-domain values."""
        return self._binary(other, "meet")

    def equivalent(self, other: object) -> bool:
        """Compare same-domain values for semantic equality."""
        if not isinstance(other, LatticeValue):
            return False
        output = ctypes.c_uint8()
        check(
            lib.lling_lattice_equal(
                self._open_handle("lattice_equal"),
                other._open_handle("lattice_equal"),
                ctypes.byref(output),
            ),
            "lattice_equal",
        )
        if output.value not in (0, 1):
            raise NativeError(
                Status.PROVIDER_ERROR,
                "lattice_equal",
                "provider returned a non-boolean equality result",
            )
        return bool(output.value)

    def _bytes(self, operation: str) -> bytes:
        function = getattr(lib, f"lling_lattice_{operation}")
        written = ctypes.c_size_t()
        required = ctypes.c_size_t()
        handle = self._open_handle(f"lattice_{operation}")
        check(
            function(handle, None, 0, ctypes.byref(written), ctypes.byref(required)),
            f"lattice_{operation}",
        )
        output = (ctypes.c_uint8 * required.value)()
        second_required = ctypes.c_size_t()
        check(
            function(
                handle,
                output,
                required.value,
                ctypes.byref(written),
                ctypes.byref(second_required),
            ),
            f"lattice_{operation}",
        )
        if written.value != required.value or second_required.value != required.value:
            raise NativeError(
                Status.PROVIDER_ERROR,
                f"lattice_{operation}",
                "provider changed its byte result between sizing and copy",
            )
        return bytes(output)

    def stable_bytes(self) -> bytes:
        """Copy the provider's canonical byte representation."""
        return self._bytes("stable_bytes")

    def diagnostic(self) -> str:
        """Copy and decode the provider's advisory UTF-8 diagnostic."""
        return self._bytes("diagnostic").decode("utf-8")

    def _many(self, others: Sequence[LatticeValue], operation: str) -> LatticeValue:
        values = tuple(others)
        handles = (ctypes.c_void_p * len(values))(
            *(value._open_handle(f"lattice_{operation}_many") for value in values)
        )
        output = ctypes.c_void_p()
        function = getattr(lib, f"lling_lattice_{operation}_many")
        check(
            function(
                self._open_handle(f"lattice_{operation}_many"),
                handles,
                len(values),
                ctypes.byref(output),
            ),
            f"lattice_{operation}_many",
        )
        return LatticeValue._adopt(output, self._thread)

    def join_many(self, others: Sequence[LatticeValue]) -> LatticeValue:
        """Fold least-upper-bound over an ordered finite sequence."""
        return self._many(others, "join")

    def meet_many(self, others: Sequence[LatticeValue]) -> LatticeValue:
        """Fold greatest-lower-bound over an ordered finite sequence."""
        return self._many(others, "meet")

    def close(self) -> None:
        """Release the owned lattice value exactly once."""
        if self._handle.value:
            lib.lling_lattice_free(self._handle)
            self._handle = ctypes.c_void_p()

    def __or__(self, other: LatticeValue) -> LatticeValue:
        return self.join(other)

    def __and__(self, other: LatticeValue) -> LatticeValue:
        return self.meet(other)

    def __eq__(self, other: object) -> bool:
        return isinstance(other, LatticeValue) and self.equivalent(other)

    def __enter__(self) -> LatticeValue:  # noqa: PYI034 - Python 3.10
        return self

    def __exit__(self, *_args: object) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except BaseException:  # noqa: BLE001 - finalizers must not escape
            return


def validate_lattice_laws(values: Sequence[LatticeValue]) -> None:
    """Try to falsify lattice laws over one to sixteen representatives."""
    samples = tuple(values)
    if not 1 <= len(samples) <= MAX_LAW_SAMPLES:
        raise ValueError("lattice law validation requires one to sixteen samples")
    handles = (ctypes.c_void_p * len(samples))(
        *(sample._open_handle("lattice_validate_laws") for sample in samples)
    )
    check(
        lib.lling_lattice_validate_laws(handles, len(samples)),
        "lattice_validate_laws",
    )


class SemiringContext:
    """Owned same-thread operation context for one host-defined semiring."""

    __slots__ = ("_handle", "_thread")

    def __init__(self, resource: NativeResource | VtResource) -> None:
        raw = native_resource(resource)
        self._handle = ctypes.c_void_p()
        self._thread = threading.get_ident()
        check(
            lib.lling_semiring_open(ctypes.byref(raw), ctypes.byref(self._handle)),
            "semiring_open",
        )

    def _open_handle(self, operation: str = "semiring") -> ctypes.c_void_p:
        if threading.get_ident() != self._thread:
            raise _thread_error(operation)
        if not self._handle.value:
            raise NativeError(Status.CLOSED, operation, "semiring context is closed")
        return self._handle

    def _adopt(self, handle: ctypes.c_void_p) -> SemiringWeight:
        return SemiringWeight._adopt(handle, self)

    @property
    def properties(self) -> SemiringProperty:
        """Return the provider's declared algebraic properties."""
        output = ctypes.c_uint64()
        check(
            lib.lling_semiring_properties(
                self._open_handle("semiring_properties"), ctypes.byref(output)
            ),
            "semiring_properties",
        )
        return SemiringProperty(output.value)

    def _identity(self, operation: str) -> SemiringWeight:
        output = ctypes.c_void_p()
        function = getattr(lib, f"lling_semiring_{operation}")
        check(
            function(self._open_handle(f"semiring_{operation}"), ctypes.byref(output)),
            f"semiring_{operation}",
        )
        return self._adopt(output)

    def zero(self) -> SemiringWeight:
        """Construct the additive identity."""
        return self._identity("zero")

    def one(self) -> SemiringWeight:
        """Construct the multiplicative identity."""
        return self._identity("one")

    def _checked_weight(self, value: object, operation: str) -> ctypes.c_void_p:
        if not isinstance(value, SemiringWeight) or value.context is not self:
            raise ValueError("semiring weights must share the exact context")
        return value._open_handle(operation)

    def _binary(
        self, left: SemiringWeight, right: SemiringWeight, operation: str
    ) -> SemiringWeight:
        output = ctypes.c_void_p()
        function = getattr(lib, f"lling_semiring_{operation}")
        check(
            function(
                self._open_handle(f"semiring_{operation}"),
                self._checked_weight(left, f"semiring_{operation}"),
                self._checked_weight(right, f"semiring_{operation}"),
                ctypes.byref(output),
            ),
            f"semiring_{operation}",
        )
        return self._adopt(output)

    def plus(self, left: SemiringWeight, right: SemiringWeight) -> SemiringWeight:
        """Add two weights in this exact provider context."""
        return self._binary(left, right, "plus")

    def times(self, left: SemiringWeight, right: SemiringWeight) -> SemiringWeight:
        """Multiply two weights in this exact provider context."""
        return self._binary(left, right, "times")

    def equal(self, left: SemiringWeight, right: SemiringWeight) -> bool:
        """Compare two weights for exact semantic equality."""
        return self._compare(left, right, "equal")

    def approximately_equal(
        self, left: SemiringWeight, right: SemiringWeight, epsilon: float
    ) -> bool:
        """Compare weights using the provider's metric and tolerance."""
        epsilon = float(epsilon)
        if not math.isfinite(epsilon) or epsilon < 0:
            raise ValueError("epsilon must be finite and nonnegative")
        return self._compare(left, right, "approx_equal", epsilon)

    def _compare(
        self,
        left: SemiringWeight,
        right: SemiringWeight,
        operation: str,
        epsilon: float | None = None,
    ) -> bool:
        output = ctypes.c_uint8()
        function = getattr(lib, f"lling_semiring_{operation}")
        arguments: list[object] = [
            self._open_handle(f"semiring_{operation}"),
            self._checked_weight(left, f"semiring_{operation}"),
            self._checked_weight(right, f"semiring_{operation}"),
        ]
        if epsilon is not None:
            arguments.append(epsilon)
        arguments.append(ctypes.byref(output))
        check(function(*arguments), f"semiring_{operation}")
        if output.value not in (0, 1):
            raise NativeError(
                Status.PROVIDER_ERROR,
                f"semiring_{operation}",
                "provider returned a non-boolean comparison",
            )
        return bool(output.value)

    def natural_order(
        self, left: SemiringWeight, right: SemiringWeight
    ) -> SemiringOrder:
        """Compare two weights in the provider's natural order."""
        output = ctypes.c_int32()
        check(
            lib.lling_semiring_natural_order(
                self._open_handle("semiring_natural_order"),
                self._checked_weight(left, "semiring_natural_order"),
                self._checked_weight(right, "semiring_natural_order"),
                ctypes.byref(output),
            ),
            "semiring_natural_order",
        )
        try:
            return SemiringOrder(output.value)
        except ValueError as error:
            raise NativeError(
                Status.PROVIDER_ERROR,
                "semiring_natural_order",
                "provider returned an unknown natural-order discriminant",
            ) from error

    def _partial_binary(
        self, left: SemiringWeight, right: SemiringWeight, operation: str
    ) -> SemiringWeight | None:
        output = ctypes.c_void_p()
        defined = ctypes.c_uint8()
        function = getattr(lib, f"lling_semiring_{operation}")
        check(
            function(
                self._open_handle(f"semiring_{operation}"),
                self._checked_weight(left, f"semiring_{operation}"),
                self._checked_weight(right, f"semiring_{operation}"),
                ctypes.byref(output),
                ctypes.byref(defined),
            ),
            f"semiring_{operation}",
        )
        if defined.value not in (0, 1) or bool(output.value) != bool(defined.value):
            raise NativeError(
                Status.PROVIDER_ERROR,
                f"semiring_{operation}",
                "provider returned inconsistent optional-weight output",
            )
        return self._adopt(output) if defined.value else None

    def divide(
        self, dividend: SemiringWeight, divisor: SemiringWeight
    ) -> SemiringWeight | None:
        """Return right division when defined by the provider."""
        return self._partial_binary(dividend, divisor, "divide")

    def left_divide(
        self, value: SemiringWeight, divisor: SemiringWeight
    ) -> SemiringWeight | None:
        """Return left division when defined by the provider."""
        return self._partial_binary(value, divisor, "left_divide")

    def star(self, value: SemiringWeight) -> SemiringWeight | None:
        """Return the Kleene closure when defined by the provider."""
        output = ctypes.c_void_p()
        defined = ctypes.c_uint8()
        check(
            lib.lling_semiring_star(
                self._open_handle("semiring_star"),
                self._checked_weight(value, "semiring_star"),
                ctypes.byref(output),
                ctypes.byref(defined),
            ),
            "semiring_star",
        )
        if defined.value not in (0, 1) or bool(output.value) != bool(defined.value):
            raise NativeError(
                Status.PROVIDER_ERROR,
                "semiring_star",
                "provider returned inconsistent optional-weight output",
            )
        return self._adopt(output) if defined.value else None

    def _scalar(self, value: SemiringWeight, operation: str) -> float:
        output = ctypes.c_double()
        function = getattr(lib, f"lling_semiring_{operation}")
        check(
            function(
                self._open_handle(f"semiring_{operation}"),
                self._checked_weight(value, f"semiring_{operation}"),
                ctypes.byref(output),
            ),
            f"semiring_{operation}",
        )
        return output.value

    def numerical_value(self, value: SemiringWeight) -> float:
        """Project a weight into the provider's numerical coordinate."""
        return self._scalar(value, "numerical_value")

    def probability(self, value: SemiringWeight) -> float:
        """Project a weight into probability space."""
        return self._scalar(value, "to_probability")

    def quantize(self, value: SemiringWeight, epsilon: float) -> int:
        """Return the provider's stable integer quantization bucket."""
        epsilon = float(epsilon)
        if not math.isfinite(epsilon) or epsilon <= 0:
            raise ValueError("quantization epsilon must be finite and positive")
        output = ctypes.c_int64()
        check(
            lib.lling_semiring_quantize(
                self._open_handle("semiring_quantize"),
                self._checked_weight(value, "semiring_quantize"),
                epsilon,
                ctypes.byref(output),
            ),
            "semiring_quantize",
        )
        return output.value

    @property
    def closure_bound(self) -> int | None:
        """Return a finite closure bound, or ``None`` when unknown."""
        output = ctypes.c_size_t()
        known = ctypes.c_uint8()
        check(
            lib.lling_semiring_closure_bound(
                self._open_handle("semiring_closure_bound"),
                ctypes.byref(output),
                ctypes.byref(known),
            ),
            "semiring_closure_bound",
        )
        if known.value not in (0, 1):
            raise NativeError(
                Status.PROVIDER_ERROR,
                "semiring_closure_bound",
                "provider returned a non-boolean known flag",
            )
        return output.value if known.value else None

    def stable_bytes(self, value: SemiringWeight) -> bytes:
        """Copy one weight's canonical provider-defined bytes."""
        handle = self._checked_weight(value, "semiring_stable_bytes")
        written = ctypes.c_size_t()
        required = ctypes.c_size_t()
        context = self._open_handle("semiring_stable_bytes")
        check(
            lib.lling_semiring_stable_bytes(
                context,
                handle,
                None,
                0,
                ctypes.byref(written),
                ctypes.byref(required),
            ),
            "semiring_stable_bytes",
        )
        output = (ctypes.c_uint8 * required.value)()
        second_required = ctypes.c_size_t()
        check(
            lib.lling_semiring_stable_bytes(
                context,
                handle,
                output,
                required.value,
                ctypes.byref(written),
                ctypes.byref(second_required),
            ),
            "semiring_stable_bytes",
        )
        if written.value != required.value or second_required.value != required.value:
            raise NativeError(
                Status.PROVIDER_ERROR,
                "semiring_stable_bytes",
                "provider changed stable bytes between sizing and copy",
            )
        return bytes(output)

    def validate_laws(
        self, weights: Sequence[SemiringWeight], *, epsilon: float = 0.0
    ) -> None:
        """Try to falsify declared laws over one to sixteen representatives."""
        samples = tuple(weights)
        if not 1 <= len(samples) <= MAX_LAW_SAMPLES:
            raise ValueError("semiring law validation requires one to sixteen weights")
        epsilon = float(epsilon)
        if not math.isfinite(epsilon) or epsilon < 0:
            raise ValueError("law-validation epsilon must be finite and nonnegative")
        handles = (ctypes.c_void_p * len(samples))(
            *(
                self._checked_weight(value, "semiring_validate_laws")
                for value in samples
            )
        )
        check(
            lib.lling_semiring_validate_laws(
                self._open_handle("semiring_validate_laws"),
                handles,
                len(samples),
                epsilon,
            ),
            "semiring_validate_laws",
        )

    def close(self) -> None:
        """Release the operation context exactly once."""
        if self._handle.value:
            lib.lling_semiring_free(self._handle)
            self._handle = ctypes.c_void_p()

    def __enter__(self) -> SemiringContext:  # noqa: PYI034 - Python 3.10
        return self

    def __exit__(self, *_args: object) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except BaseException:  # noqa: BLE001 - finalizers must not escape
            return


class SemiringWeight:
    """One owned immutable weight scoped to an exact semiring context."""

    __hash__ = None  # pyright: ignore[reportAssignmentType]
    __slots__ = ("_handle", "context")
    _handle: ctypes.c_void_p
    context: SemiringContext

    @classmethod
    def _adopt(
        cls, handle: ctypes.c_void_p, context: SemiringContext
    ) -> SemiringWeight:
        if not handle.value:
            raise NativeError(
                Status.PANIC,
                "semiring_weight_adopt",
                "native operation returned a null successful weight",
            )
        value = cls.__new__(cls)
        value._handle = handle
        value.context = context
        return value

    def _open_handle(self, operation: str = "semiring_weight") -> ctypes.c_void_p:
        self.context._open_handle(operation)
        if not self._handle.value:
            raise NativeError(Status.CLOSED, operation, "semiring weight is closed")
        return self._handle

    def clone(self) -> SemiringWeight:
        """Clone this provider-owned immutable weight."""
        output = ctypes.c_void_p()
        check(
            lib.lling_semiring_weight_clone(
                self._open_handle("semiring_weight_clone"), ctypes.byref(output)
            ),
            "semiring_weight_clone",
        )
        return self.context._adopt(output)

    def stable_bytes(self) -> bytes:
        """Copy this weight's canonical provider-defined bytes."""
        return self.context.stable_bytes(self)

    def close(self) -> None:
        """Release the weight exactly once."""
        if self._handle.value:
            lib.lling_semiring_weight_free(self._handle)
            self._handle = ctypes.c_void_p()

    def __add__(self, other: SemiringWeight) -> SemiringWeight:
        return self.context.plus(self, other)

    def __mul__(self, other: SemiringWeight) -> SemiringWeight:
        return self.context.times(self, other)

    def __eq__(self, other: object) -> bool:
        return (
            isinstance(other, SemiringWeight)
            and other.context is self.context
            and self.context.equal(self, other)
        )

    def __enter__(self) -> SemiringWeight:  # noqa: PYI034 - Python 3.10
        return self

    def __exit__(self, *_args: object) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except BaseException:  # noqa: BLE001 - finalizers must not escape
            return
