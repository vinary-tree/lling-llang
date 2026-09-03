"""Idiomatic ownership and composition for Unicode/tropical WFSTs."""

from __future__ import annotations

import ctypes
import math

from vinary_tree_interop import NativeResource, ScalarWfst, VtResource

from ._abi import NativeError, Status, check, lib, native_resource


def _u32(value: object, subject: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{subject} must be an integer")
    if not 0 <= value < 2**32:
        raise ValueError(f"{subject} must fit an unsigned 32-bit integer")
    return value


def _size(value: object, subject: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError(f"{subject} must be an integer")
    if not 0 <= value < 2 ** (ctypes.sizeof(ctypes.c_size_t) * 8):
        raise ValueError(f"{subject} does not fit size_t")
    return value


def _weight(value: float) -> float:
    result = float(value)
    if math.isnan(result) or result == -math.inf:
        raise ValueError("tropical weights must be finite or positive infinity")
    return result


def _label(value: object) -> tuple[int, int]:
    if value is None:
        return 0, 0
    if isinstance(value, str):
        if len(value) != 1:
            raise ValueError("a WFST label must contain one Unicode scalar")
        value = ord(value)
    if isinstance(value, bool) or not isinstance(value, int):
        raise TypeError("a WFST label must be str, int, or None")
    if not 0 <= value <= 0x10FFFF or 0xD800 <= value <= 0xDFFF:
        raise ValueError("a WFST label must be a Unicode scalar")
    return value, 1


class Wfst(ScalarWfst):
    """Immutable lling-llang WFST with shared resource traversal methods."""


def _adopt_native_wfst(handle: ctypes.c_void_p) -> Wfst:
    if not handle.value:
        raise NativeError(
            Status.PANIC,
            "wfst_adopt",
            "native operation returned a null successful WFST handle",
        )
    output = VtResource()
    try:
        check(
            lib.lling_wfst_resource(handle, ctypes.byref(output)),
            "wfst_resource",
        )
    finally:
        lib.lling_wfst_free(handle)
    if not output.context or not output.vtable:
        raise NativeError(
            Status.PANIC,
            "wfst_resource",
            "native operation returned a null successful resource",
        )
    return Wfst.adopt(output)


class WfstBuilder:
    """Mutable Unicode/tropical builder consumed by :meth:`build`."""

    __slots__ = ("_handle",)

    def __init__(self, *, size_hint: int = 0) -> None:
        self._handle = ctypes.c_void_p()
        check(
            lib.lling_wfst_builder_new(ctypes.byref(self._handle)),
            "wfst_builder_new",
        )
        try:
            if size_hint:
                self.reserve_states(size_hint)
        except BaseException:
            self.close()
            raise

    def _open_handle(self) -> ctypes.c_void_p:
        if not self._handle.value:
            raise NativeError(Status.CLOSED, "wfst_builder", "builder is closed")
        return self._handle

    def reserve_states(self, additional: int) -> WfstBuilder:
        """Reserve space for additional states without changing the graph."""
        check(
            lib.lling_wfst_builder_reserve_states(
                self._open_handle(), _size(additional, "additional state count")
            ),
            "wfst_builder_reserve_states",
        )
        return self

    def add_state(self) -> int:
        """Append a state and return its zero-based identifier."""
        output = ctypes.c_uint32()
        check(
            lib.lling_wfst_builder_add_state(self._open_handle(), ctypes.byref(output)),
            "wfst_builder_add_state",
        )
        return output.value

    def set_start(self, state: int) -> WfstBuilder:
        """Select the graph's start state."""
        check(
            lib.lling_wfst_builder_set_start(self._open_handle(), _u32(state, "state")),
            "wfst_builder_set_start",
        )
        return self

    def set_final(self, state: int, weight: float = 0.0) -> WfstBuilder:
        """Mark a state final with a tropical final weight."""
        check(
            lib.lling_wfst_builder_set_final(
                self._open_handle(), _u32(state, "state"), _weight(weight)
            ),
            "wfst_builder_set_final",
        )
        return self

    def clear_final(self, state: int) -> WfstBuilder:
        """Remove finality and its weight from one state."""
        check(
            lib.lling_wfst_builder_clear_final(
                self._open_handle(), _u32(state, "state")
            ),
            "wfst_builder_clear_final",
        )
        return self

    def add_arc(
        self,
        source: int,
        input_label: str | int | None,
        output_label: str | int | None,
        target: int,
        weight: float = 0.0,
    ) -> WfstBuilder:
        """Append a Unicode arc; ``None`` denotes epsilon on either tape."""
        input_value, has_input = _label(input_label)
        output_value, has_output = _label(output_label)
        check(
            lib.lling_wfst_builder_add_arc(
                self._open_handle(),
                _u32(source, "source state"),
                input_value,
                has_input,
                output_value,
                has_output,
                _u32(target, "target state"),
                _weight(weight),
            ),
            "wfst_builder_add_arc",
        )
        return self

    def build(self) -> Wfst:
        """Consume the builder and return an immutable interoperable graph."""
        output = ctypes.c_void_p()
        check(
            lib.lling_wfst_builder_build(self._open_handle(), ctypes.byref(output)),
            "wfst_builder_build",
        )
        self.close()
        return _adopt_native_wfst(output)

    def close(self) -> None:
        """Release an unconsumed or consumed builder exactly once."""
        if self._handle.value:
            lib.lling_wfst_builder_free(self._handle)
            self._handle = ctypes.c_void_p()

    def __enter__(self) -> WfstBuilder:  # noqa: PYI034 - Python 3.10
        return self

    def __exit__(self, *_args: object) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except BaseException:  # noqa: BLE001 - finalizers must not escape
            return


def import_wfst(source: NativeResource | VtResource) -> Wfst:
    """Copy a compatible scalar resource into a native lling-llang WFST."""
    raw = native_resource(source)
    output = ctypes.c_void_p()
    check(
        lib.lling_wfst_import_ref(ctypes.byref(raw), ctypes.byref(output)),
        "wfst_import",
    )
    return _adopt_native_wfst(output)


def compose(
    first: NativeResource | VtResource,
    second: NativeResource | VtResource,
) -> Wfst:
    """Lazily compose captured scalar-WFST snapshots at the shared tape."""
    first_raw = native_resource(first)
    second_raw = native_resource(second)
    output = ctypes.c_void_p()
    check(
        lib.lling_wfst_compose_refs(
            ctypes.byref(first_raw), ctypes.byref(second_raw), ctypes.byref(output)
        ),
        "wfst_compose",
    )
    return _adopt_native_wfst(output)
