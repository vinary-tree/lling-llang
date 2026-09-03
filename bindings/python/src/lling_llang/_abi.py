"""Exact ``ctypes`` layouts and signatures for the lling-llang C ABI."""

from __future__ import annotations

import ctypes
import ctypes.util
import os
import platform
from enum import IntEnum, IntFlag
from pathlib import Path
from typing import Any

from vinary_tree_interop import NativeResource, VtResource

ABI_VERSION = 1
API_REVISION = 5
TYPED_ABI_VERSION = 2
MAX_LAW_SAMPLES = 16


class Status(IntEnum):
    """Stable lling-llang status discriminants."""

    OK = 0
    INVALID_ARGUMENT = 1
    NULL_POINTER = 2
    PANIC = 3
    INCOMPATIBLE_RESOURCE = 4
    PROVIDER_ERROR = 5
    LIMIT_EXCEEDED = 6
    CLOSED = 7


class DescriptorFlag(IntFlag):
    """Presence flags for replay-critical WFST descriptor fields."""

    NONE = 0
    SIGNATURE_KNOWN = 1
    SNAPSHOT_PRESENT = 2
    CONTEXT_PRESENT = 4


class BudgetFlag(IntFlag):
    """Enabled resource-budget dimensions."""

    NONE = 0
    STATES = 1
    ARCS = 2
    BYTES = 4
    WORK = 8


class Precision(IntEnum):
    """Semantic precision reported by a typed outcome."""

    EXACT = 1
    APPROXIMATE = 2
    UNKNOWN = 3


class Completeness(IntEnum):
    """Whether an outcome covers its requested result space."""

    COMPLETE = 1
    INCOMPLETE = 2


class Applicability(IntEnum):
    """Whether a result applies to the requested operation."""

    APPLICABLE = 1
    UNSUPPORTED = 2
    UNKNOWN = 3


class Termination(IntEnum):
    """Why a typed operation stopped."""

    SUCCEEDED = 1
    CANCELLED = 2
    BUDGET_EXHAUSTED = 3
    FAILED = 4


class EvidenceState(IntEnum):
    """Validation state of evidence attached to a typed result."""

    NONE = 0
    CANDIDATE = 1
    VERIFIED = 2
    STALE = 3
    INVALID = 4


class CancellationReason(IntEnum):
    """First-writer-wins cooperative cancellation reason."""

    REQUESTED = 1
    DEADLINE = 2
    BUDGET = 3
    SOURCE = 4


class AbiV2Header(ctypes.Structure):
    """Fixed prefix carried by every typed ABI-v2 structure."""

    _fields_ = [
        ("struct_size", ctypes.c_uint32),
        ("abi_version", ctypes.c_uint32),
        ("flags", ctypes.c_uint64),
        ("reserved", ctypes.c_uint64),
    ]

    def __init__(self, struct_size: int, flags: int = 0) -> None:
        super().__init__(struct_size, TYPED_ABI_VERSION, flags, 0)


class Id128(ctypes.Structure):
    """Exact sixteen-byte semantic identifier; all zeroes mean absent."""

    _fields_ = [("_value", ctypes.c_uint8 * 16)]

    def __init__(self, value: bytes = bytes(16)) -> None:
        if len(value) != 16:
            raise ValueError("Id128 requires exactly 16 bytes")
        super().__init__((ctypes.c_uint8 * 16).from_buffer_copy(value))

    @property
    def bytes(self) -> bytes:
        """Copy the identifier bytes."""
        return bytes(self._value)


class Digest256(ctypes.Structure):
    """Exact thirty-two-byte evidence-context digest."""

    _fields_ = [("_value", ctypes.c_uint8 * 32)]

    def __init__(self, value: bytes = bytes(32)) -> None:
        if len(value) != 32:
            raise ValueError("Digest256 requires exactly 32 bytes")
        super().__init__((ctypes.c_uint8 * 32).from_buffer_copy(value))

    @property
    def bytes(self) -> bytes:
        """Copy the digest bytes."""
        return bytes(self._value)


class WfstDescriptor(ctypes.Structure):
    """Replay-critical tapes, algebra, snapshot, and evidence identity."""

    _fields_ = [
        ("header", AbiV2Header),
        ("input_tape", Id128),
        ("output_tape", Id128),
        ("algebra", Id128),
        ("snapshot", Id128),
        ("context", Digest256),
    ]

    def __init__(
        self,
        *,
        input_tape: Id128 | None = None,
        output_tape: Id128 | None = None,
        algebra: Id128 | None = None,
        snapshot: Id128 | None = None,
        context: Digest256 | None = None,
        flags: DescriptorFlag = DescriptorFlag.NONE,
    ) -> None:
        super().__init__(
            AbiV2Header(ctypes.sizeof(type(self)), flags),
            Id128() if input_tape is None else input_tape,
            Id128() if output_tape is None else output_tape,
            Id128() if algebra is None else algebra,
            Id128() if snapshot is None else snapshot,
            Digest256() if context is None else context,
        )


class Budget(ctypes.Structure):
    """Optional state, arc, byte, and abstract-work limits."""

    _fields_ = [
        ("header", AbiV2Header),
        ("max_states", ctypes.c_uint64),
        ("max_arcs", ctypes.c_uint64),
        ("max_bytes", ctypes.c_uint64),
        ("max_work", ctypes.c_uint64),
        ("reserved", ctypes.c_uint64 * 2),
    ]

    def __init__(
        self,
        *,
        max_states: int = 0,
        max_arcs: int = 0,
        max_bytes: int = 0,
        max_work: int = 0,
    ) -> None:
        values = (max_states, max_arcs, max_bytes, max_work)
        if any(isinstance(value, bool) or not 0 <= value < 2**64 for value in values):
            raise ValueError("budget limits must be unsigned 64-bit integers")
        flags = BudgetFlag.NONE
        for value, flag in zip(
            values,
            (BudgetFlag.STATES, BudgetFlag.ARCS, BudgetFlag.BYTES, BudgetFlag.WORK),
            strict=True,
        ):
            if value:
                flags |= flag
        super().__init__(
            AbiV2Header(ctypes.sizeof(type(self)), flags),
            *values,
            (ctypes.c_uint64 * 2)(),
        )


class Outcome(ctypes.Structure):
    """Orthogonal semantics, termination, evidence, and work counters."""

    _fields_ = [
        ("header", AbiV2Header),
        ("precision", ctypes.c_uint32),
        ("completeness", ctypes.c_uint32),
        ("applicability", ctypes.c_uint32),
        ("termination", ctypes.c_uint32),
        ("evidence", ctypes.c_uint32),
        ("reserved0", ctypes.c_uint32),
        ("states", ctypes.c_uint64),
        ("arcs", ctypes.c_uint64),
        ("bytes", ctypes.c_uint64),
        ("work", ctypes.c_uint64),
        ("limitations", ctypes.c_uint64),
        ("reserved1", ctypes.c_uint64),
    ]

    def __init__(
        self,
        *,
        precision: Precision,
        completeness: Completeness,
        applicability: Applicability,
        termination: Termination,
        evidence: EvidenceState,
        states: int = 0,
        arcs: int = 0,
        bytes_used: int = 0,
        work: int = 0,
        limitations: int = 0,
    ) -> None:
        counters = (states, arcs, bytes_used, work, limitations)
        if any(isinstance(value, bool) or not 0 <= value < 2**64 for value in counters):
            raise ValueError("outcome counters must be unsigned 64-bit integers")
        super().__init__(
            AbiV2Header(ctypes.sizeof(type(self))),
            Precision(precision),
            Completeness(completeness),
            Applicability(applicability),
            Termination(termination),
            EvidenceState(evidence),
            0,
            *counters,
            0,
        )


class NativeError(RuntimeError):
    """Typed lling-llang failure with a copied native diagnostic."""

    def __init__(self, status: int | Status, operation: str, message: str) -> None:
        super().__init__(f"{operation} failed: {message}")
        try:
            self.status: Status | int = Status(status)
        except ValueError:
            self.status = int(status)
        self.operation = operation


def _library_names() -> tuple[str, ...]:
    system = platform.system()
    if system == "Windows":
        return ("lling_llang.dll",)
    if system == "Darwin":
        return ("liblling_llang.dylib",)
    return ("liblling_llang.so",)


def _load_library() -> ctypes.CDLL:
    candidates: list[str] = []
    if explicit := os.environ.get("LLING_LLANG_LIBRARY"):
        candidates.append(explicit)
    package = Path(__file__).resolve().parent
    candidates.extend(str(package / "native" / name) for name in _library_names())
    if discovered := ctypes.util.find_library("lling_llang"):
        candidates.append(discovered)
    candidates.extend(_library_names())
    failures: list[str] = []
    for candidate in candidates:
        try:
            return ctypes.CDLL(candidate)
        except OSError as error:
            failures.append(f"{candidate}: {error}")
    raise ImportError(
        "could not load lling-llang; set LLING_LLANG_LIBRARY\n" + "\n".join(failures)
    )


lib: Any = _load_library()


def _bind(
    name: str,
    arguments: list[Any],
    result: object = ctypes.c_uint32,
) -> None:
    function = getattr(lib, name)
    function.argtypes = arguments
    function.restype = result


_bind("lling_abi_version", [])
_bind("lling_api_revision", [])
_bind("lling_last_error_message", [], ctypes.c_char_p)

_bind("lling_wfst_builder_new", [ctypes.POINTER(ctypes.c_void_p)])
_bind("lling_wfst_builder_free", [ctypes.c_void_p], None)
_bind("lling_wfst_builder_reserve_states", [ctypes.c_void_p, ctypes.c_size_t])
_bind(
    "lling_wfst_builder_add_state",
    [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)],
)
_bind("lling_wfst_builder_set_start", [ctypes.c_void_p, ctypes.c_uint32])
_bind(
    "lling_wfst_builder_set_final",
    [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_double],
)
_bind("lling_wfst_builder_clear_final", [ctypes.c_void_p, ctypes.c_uint32])
_bind(
    "lling_wfst_builder_add_arc",
    [
        ctypes.c_void_p,
        ctypes.c_uint32,
        ctypes.c_uint64,
        ctypes.c_uint8,
        ctypes.c_uint64,
        ctypes.c_uint8,
        ctypes.c_uint32,
        ctypes.c_double,
    ],
)
_bind(
    "lling_wfst_builder_build",
    [ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)],
)
_bind("lling_wfst_free", [ctypes.c_void_p], None)
_bind("lling_wfst_import", [VtResource, ctypes.POINTER(ctypes.c_void_p)])
_bind(
    "lling_wfst_import_ref",
    [ctypes.POINTER(VtResource), ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_compose",
    [VtResource, VtResource, ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_compose_refs",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(VtResource),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind("lling_wfst_resource", [ctypes.c_void_p, ctypes.POINTER(VtResource)])
_bind("lling_resource_release", [VtResource], None)

_bind(
    "lling_semiring_open", [ctypes.POINTER(VtResource), ctypes.POINTER(ctypes.c_void_p)]
)
_bind("lling_semiring_free", [ctypes.c_void_p], None)
_bind("lling_semiring_weight_free", [ctypes.c_void_p], None)
_bind("lling_semiring_properties", [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint64)])
for _name in ("lling_semiring_zero", "lling_semiring_one"):
    _bind(_name, [ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)])
_bind(
    "lling_semiring_weight_clone",
    [ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)],
)
for _name in ("lling_semiring_plus", "lling_semiring_times"):
    _bind(
        _name,
        [
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.POINTER(ctypes.c_void_p),
        ],
    )
_bind(
    "lling_semiring_equal",
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint8)],
)
_bind(
    "lling_semiring_approx_equal",
    [
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_double,
        ctypes.POINTER(ctypes.c_uint8),
    ],
)
_bind(
    "lling_semiring_natural_order",
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int32)],
)
for _name in ("lling_semiring_divide", "lling_semiring_left_divide"):
    _bind(
        _name,
        [
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.POINTER(ctypes.c_void_p),
            ctypes.POINTER(ctypes.c_uint8),
        ],
    )
_bind(
    "lling_semiring_star",
    [
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_void_p),
        ctypes.POINTER(ctypes.c_uint8),
    ],
)
for _name in ("lling_semiring_numerical_value", "lling_semiring_to_probability"):
    _bind(
        _name,
        [ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_double)],
    )
_bind(
    "lling_semiring_quantize",
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_double, ctypes.POINTER(ctypes.c_int64)],
)
_bind(
    "lling_semiring_closure_bound",
    [ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_uint8)],
)
_bind(
    "lling_semiring_stable_bytes",
    [
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_size_t),
        ctypes.POINTER(ctypes.c_size_t),
    ],
)
_bind(
    "lling_semiring_validate_laws",
    [
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_void_p),
        ctypes.c_size_t,
        ctypes.c_double,
    ],
)

_bind(
    "lling_lattice_open", [ctypes.POINTER(VtResource), ctypes.POINTER(ctypes.c_void_p)]
)
_bind("lling_lattice_free", [ctypes.c_void_p], None)
_bind("lling_lattice_domain_id", [ctypes.c_void_p, ctypes.c_void_p])
_bind("lling_lattice_flags", [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint64)])
for _name in ("lling_lattice_join", "lling_lattice_meet"):
    _bind(
        _name,
        [ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)],
    )
_bind(
    "lling_lattice_equal",
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint8)],
)
for _name in ("lling_lattice_stable_bytes", "lling_lattice_diagnostic"):
    _bind(
        _name,
        [
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_size_t,
            ctypes.POINTER(ctypes.c_size_t),
            ctypes.POINTER(ctypes.c_size_t),
        ],
    )
for _name in ("lling_lattice_join_many", "lling_lattice_meet_many"):
    _bind(
        _name,
        [
            ctypes.c_void_p,
            ctypes.POINTER(ctypes.c_void_p),
            ctypes.c_size_t,
            ctypes.POINTER(ctypes.c_void_p),
        ],
    )
_bind(
    "lling_lattice_validate_laws",
    [ctypes.POINTER(ctypes.c_void_p), ctypes.c_size_t],
)

_bind(
    "lling_abi_v2_validate_header",
    [ctypes.POINTER(AbiV2Header), ctypes.c_uint32, ctypes.c_uint64],
)
_bind(
    "lling_abi_v2_validate_descriptor",
    [ctypes.POINTER(WfstDescriptor), ctypes.POINTER(ctypes.c_uint8)],
)
_bind("lling_abi_v2_validate_budget", [ctypes.POINTER(Budget)])
_bind(
    "lling_abi_v2_validate_outcome",
    [
        ctypes.POINTER(Outcome),
        ctypes.c_uint8,
        ctypes.c_uint8,
        ctypes.POINTER(ctypes.c_uint8),
    ],
)
_bind(
    "lling_abi_v2_identity_matches",
    [
        ctypes.POINTER(WfstDescriptor),
        ctypes.POINTER(WfstDescriptor),
        ctypes.POINTER(ctypes.c_uint8),
    ],
)
_bind("lling_cancellation_v2_new", [ctypes.POINTER(ctypes.c_void_p)])
_bind("lling_cancellation_v2_request", [ctypes.c_void_p, ctypes.c_uint32])
_bind(
    "lling_cancellation_v2_reason",
    [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)],
)
_bind("lling_cancellation_v2_free", [ctypes.POINTER(ctypes.c_void_p)])


def last_error_message() -> str:
    """Copy the current thread's native diagnostic before another ABI call."""
    raw = lib.lling_last_error_message()
    return raw.decode("utf-8", "replace") if raw else "native operation failed"


def check(status: int, operation: str) -> None:
    """Raise a typed exception for any non-success lling status."""
    if status != Status.OK:
        raise NativeError(status, operation, last_error_message())


def native_resource(resource: NativeResource | VtResource) -> VtResource:
    """Copy a borrowed two-word resource from a compatible Python facade."""
    raw = resource if isinstance(resource, VtResource) else resource.native_resource
    if not raw.context or not raw.vtable:
        raise NativeError(Status.CLOSED, "resource", "resource is closed")
    return VtResource(raw.context, raw.vtable)


def abi_version() -> int:
    """Return the loaded native ABI version."""
    return int(lib.lling_abi_version())


def api_revision() -> int:
    """Return the loaded additive API revision."""
    return int(lib.lling_api_revision())


if abi_version() != ABI_VERSION:
    raise ImportError(
        f"lling-llang native ABI {abi_version()} does not match {ABI_VERSION}"
    )
if api_revision() < API_REVISION:
    raise ImportError(
        f"lling-llang native API revision {api_revision()} is older than {API_REVISION}"
    )

_EXPECTED_LAYOUTS = {
    AbiV2Header: 24,
    Id128: 16,
    Digest256: 32,
    WfstDescriptor: 120,
    Budget: 72,
    Outcome: 96,
}
for _structure, _expected in _EXPECTED_LAYOUTS.items():
    if ctypes.sizeof(_structure) != _expected:
        raise ImportError(
            f"{_structure.__name__} layout is {ctypes.sizeof(_structure)}, expected {_expected}"
        )
