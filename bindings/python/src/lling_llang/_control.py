"""Typed evidence, budget, outcome, and cancellation controls."""

from __future__ import annotations

import ctypes

from ._abi import (
    AbiV2Header,
    Budget,
    CancellationReason,
    NativeError,
    Outcome,
    Status,
    WfstDescriptor,
    check,
    lib,
)


def validate_header(
    header: AbiV2Header,
    *,
    required_size: int | None = None,
    known_flags: int = 0,
) -> AbiV2Header:
    """Validate one typed ABI prefix and return the same object."""
    if required_size is None:
        required_size = ctypes.sizeof(AbiV2Header)
    if not 0 <= required_size < 2**32:
        raise ValueError("required_size must be an unsigned 32-bit integer")
    if not 0 <= known_flags < 2**64:
        raise ValueError("known_flags must be an unsigned 64-bit integer")
    check(
        lib.lling_abi_v2_validate_header(
            ctypes.byref(header), required_size, known_flags
        ),
        "abi_v2_validate_header",
    )
    return header


def typed_evidence_allowed(descriptor: WfstDescriptor) -> bool:
    """Return whether a descriptor carries sufficient replay identity."""
    output = ctypes.c_uint8()
    check(
        lib.lling_abi_v2_validate_descriptor(
            ctypes.byref(descriptor), ctypes.byref(output)
        ),
        "abi_v2_validate_descriptor",
    )
    return bool(output.value)


def validate_budget(budget: Budget) -> Budget:
    """Validate one bounded-work request and return the same object."""
    check(
        lib.lling_abi_v2_validate_budget(ctypes.byref(budget)),
        "abi_v2_validate_budget",
    )
    return budget


def authoritative_exact(
    outcome: Outcome,
    *,
    resource_present: bool,
    evidence_present: bool,
) -> bool:
    """Return whether an outcome may be consumed as authoritative and exact."""
    output = ctypes.c_uint8()
    check(
        lib.lling_abi_v2_validate_outcome(
            ctypes.byref(outcome),
            resource_present,
            evidence_present,
            ctypes.byref(output),
        ),
        "abi_v2_validate_outcome",
    )
    return bool(output.value)


def identity_matches(expected: WfstDescriptor, observed: WfstDescriptor) -> bool:
    """Compare all descriptor fields made present by their flags."""
    output = ctypes.c_uint8()
    check(
        lib.lling_abi_v2_identity_matches(
            ctypes.byref(expected), ctypes.byref(observed), ctypes.byref(output)
        ),
        "abi_v2_identity_matches",
    )
    return bool(output.value)


class Cancellation:
    """Thread-safe, first-reason-wins cooperative cancellation owner."""

    __slots__ = ("_handle",)

    def __init__(self) -> None:
        self._handle = ctypes.c_void_p()
        check(
            lib.lling_cancellation_v2_new(ctypes.byref(self._handle)),
            "cancellation_v2_new",
        )

    def _open_handle(self) -> ctypes.c_void_p:
        if not self._handle.value:
            raise NativeError(Status.CLOSED, "cancellation", "handle is closed")
        return self._handle

    def request(self, reason: CancellationReason) -> None:
        """Request cancellation; the first nonzero reason remains authoritative."""
        check(
            lib.lling_cancellation_v2_request(
                self._open_handle(), CancellationReason(reason)
            ),
            "cancellation_v2_request",
        )

    @property
    def reason(self) -> CancellationReason | None:
        """Return the winning reason, or ``None`` before cancellation."""
        output = ctypes.c_uint32()
        check(
            lib.lling_cancellation_v2_reason(self._open_handle(), ctypes.byref(output)),
            "cancellation_v2_reason",
        )
        return None if not output.value else CancellationReason(output.value)

    def close(self) -> None:
        """Release the cancellation resource exactly once."""
        if self._handle.value:
            check(
                lib.lling_cancellation_v2_free(ctypes.byref(self._handle)),
                "cancellation_v2_free",
            )

    def __enter__(self) -> Cancellation:  # noqa: PYI034 - Python 3.10
        return self

    def __exit__(self, *_args: object) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except BaseException:  # noqa: BLE001 - finalizers must not escape
            return
