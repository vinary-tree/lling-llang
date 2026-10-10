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
API_REVISION = 14
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
    NON_CONVERGENT = 8
    UNSUPPORTED = 9


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


class PathPoll(IntEnum):
    """Bounded path-cursor result; truncation never means exact completion."""

    PATH = 1
    PENDING = 2
    EXHAUSTED = 3
    TRUNCATED = 4
    CANCELLED = 5


class GraphPoll(IntEnum):
    """Incremental graph capture distinguishes exact completion from cancellation."""

    PENDING = 1
    COMPLETE = 2
    CANCELLED = 3


class DistancePoll(IntEnum):
    """Bounded native distance-analysis outcomes."""

    PENDING = 1
    COMPLETE = 2
    CANCELLED = 3


class RankedPoll(IntEnum):
    """Bounded best-first enumeration; truncation is not exhaustion."""

    PATH = 1
    PENDING = 2
    EXHAUSTED = 3
    TRUNCATED = 4
    CANCELLED = 5


class SamplePoll(IntEnum):
    """Seeded native sampling outcomes; count/depth caps are not exhaustion."""

    PATH = 1
    PENDING = 2
    EXHAUSTED = 3
    TRUNCATED = 4
    CANCELLED = 5


class SampleStrategy(IntEnum):
    """Uniform accepting paths or semiring-mass-proportional paths."""

    UNIFORM = 1
    PROPORTIONAL = 2


class PathConfig(ctypes.Structure):
    """Versioned, explicit limits for a snapshot-pinned path traversal."""

    _fields_ = [
        ("struct_size", ctypes.c_uint32),
        ("version", ctypes.c_uint32),
        ("max_states", ctypes.c_uint64),
        ("max_arcs", ctypes.c_uint64),
        ("max_work", ctypes.c_uint64),
        ("work_per_call", ctypes.c_uint64),
        ("max_depth", ctypes.c_uint64),
        ("max_paths", ctypes.c_uint64),
    ]


class PathArc(ctypes.Structure):
    """Exact imported ``VtWfstArc`` C layout carried by one path step."""

    _fields_ = [
        ("input_label", ctypes.c_uint64),
        ("output_label", ctypes.c_uint64),
        ("target_state", ctypes.c_uint64),
        ("weight", ctypes.c_double),
        ("has_input", ctypes.c_uint8),
        ("has_output", ctypes.c_uint8),
        ("reserved", ctypes.c_uint8 * 6),
    ]


class PathStep(ctypes.Structure):
    """Source state and original scalar arc of one accepting path."""

    _fields_ = [("from_state", ctypes.c_uint64), ("arc", PathArc)]


class GraphConfig(ctypes.Structure):
    """Versioned reachable-graph capture limits."""

    _fields_ = [
        ("struct_size", ctypes.c_uint32),
        ("version", ctypes.c_uint32),
        ("max_states", ctypes.c_uint64),
        ("max_arcs", ctypes.c_uint64),
        ("max_work", ctypes.c_uint64),
        ("work_per_call", ctypes.c_uint64),
    ]


class GraphArc(ctypes.Structure):
    """Original scalar arc and deterministic local target state ID."""

    _fields_ = [("target_local", ctypes.c_uint64), ("arc", PathArc)]


class GraphDistanceConfig(ctypes.Structure):
    """Versioned native graph-analysis work limits."""

    _fields_ = [
        ("struct_size", ctypes.c_uint32),
        ("version", ctypes.c_uint32),
        ("max_work", ctypes.c_uint64),
        ("work_per_call", ctypes.c_uint64),
    ]


class RankedPathConfig(ctypes.Structure):
    """Versioned native ranking work, depth, count, and frontier limits."""

    _fields_ = [
        ("struct_size", ctypes.c_uint32),
        ("version", ctypes.c_uint32),
        ("max_work", ctypes.c_uint64),
        ("work_per_call", ctypes.c_uint64),
        ("max_depth", ctypes.c_uint64),
        ("max_paths", ctypes.c_uint64),
        ("max_frontier", ctypes.c_uint64),
    ]


class SamplePathConfig(ctypes.Structure):
    """Versioned exact-distribution sampler limits and stable seed."""

    _fields_ = [
        ("struct_size", ctypes.c_uint32),
        ("version", ctypes.c_uint32),
        ("max_work", ctypes.c_uint64),
        ("work_per_call", ctypes.c_uint64),
        ("max_depth", ctypes.c_uint64),
        ("max_samples", ctypes.c_uint64),
        ("strategy", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32),
        ("seed", ctypes.c_uint64),
    ]


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
_bind("lling_llang_api_revision", [])
_bind("lling_last_error_message", [], ctypes.c_char_p)

_bind("lling_wfst_builder_new", [ctypes.POINTER(ctypes.c_void_p)])
_bind(
    "lling_wfst_builder_new_for_domains",
    [ctypes.c_uint32, ctypes.c_uint32, ctypes.POINTER(ctypes.c_void_p)],
)
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
_bind(
    "lling_wfst_acceptor_intersect",
    [VtResource, VtResource, ctypes.POINTER(Budget), ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_acceptor_intersect_refs",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(VtResource),
        ctypes.POINTER(Budget),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_wfst_project_input",
    [VtResource, ctypes.POINTER(Budget), ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_project_output",
    [VtResource, ctypes.POINTER(Budget), ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_reverse",
    [VtResource, ctypes.POINTER(Budget), ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_project_input_ref",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(Budget),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_wfst_project_output_ref",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(Budget),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_wfst_reverse_ref",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(Budget),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_wfst_determinize",
    [VtResource, ctypes.POINTER(Budget), ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_minimize",
    [VtResource, ctypes.POINTER(Budget), ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_remove_epsilon",
    [VtResource, ctypes.POINTER(Budget), ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_connect",
    [VtResource, ctypes.POINTER(Budget), ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_determinize_ref",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(Budget),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_wfst_minimize_ref",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(Budget),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_wfst_remove_epsilon_ref",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(Budget),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_wfst_connect_ref",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(Budget),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_wfst_union",
    [VtResource, VtResource, ctypes.POINTER(Budget), ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_concat",
    [VtResource, VtResource, ctypes.POINTER(Budget), ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_closure",
    [VtResource, ctypes.POINTER(Budget), ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_closure_plus",
    [VtResource, ctypes.POINTER(Budget), ctypes.POINTER(ctypes.c_void_p)],
)
_bind(
    "lling_wfst_union_refs",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(VtResource),
        ctypes.POINTER(Budget),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_wfst_concat_refs",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(VtResource),
        ctypes.POINTER(Budget),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_wfst_closure_ref",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(Budget),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_wfst_closure_plus_ref",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(Budget),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind("lling_wfst_resource", [ctypes.c_void_p, ctypes.POINTER(VtResource)])
_bind("lling_resource_release", [VtResource], None)
_bind(
    "lling_path_cursor_open",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(PathConfig),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_path_cursor_next",
    [
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_uint32),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind("lling_path_cursor_free", [ctypes.c_void_p], None)
_bind(
    "lling_path_info",
    [
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_uint64),
        ctypes.POINTER(ctypes.c_double),
        ctypes.POINTER(ctypes.c_size_t),
    ],
)
_bind(
    "lling_path_steps",
    [
        ctypes.c_void_p,
        ctypes.c_size_t,
        ctypes.POINTER(PathStep),
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_size_t),
        ctypes.POINTER(ctypes.c_size_t),
    ],
)
_bind("lling_path_free", [ctypes.c_void_p], None)
_bind(
    "lling_graph_cursor_open",
    [
        ctypes.POINTER(VtResource),
        ctypes.POINTER(GraphConfig),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_graph_cursor_next",
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)],
)
_bind(
    "lling_graph_cursor_take",
    [ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)],
)
_bind("lling_graph_cursor_free", [ctypes.c_void_p], None)
_bind(
    "lling_graph_info",
    [
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_uint32),
        ctypes.POINTER(ctypes.c_uint32),
        ctypes.POINTER(ctypes.c_uint64),
        ctypes.POINTER(ctypes.c_size_t),
        ctypes.POINTER(ctypes.c_size_t),
    ],
)
_bind(
    "lling_graph_state",
    [
        ctypes.c_void_p,
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_uint64),
        ctypes.POINTER(ctypes.c_uint8),
        ctypes.POINTER(ctypes.c_double),
        ctypes.POINTER(ctypes.c_size_t),
    ],
)
_bind(
    "lling_graph_arcs",
    [
        ctypes.c_void_p,
        ctypes.c_size_t,
        ctypes.c_size_t,
        ctypes.POINTER(GraphArc),
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_size_t),
        ctypes.POINTER(ctypes.c_size_t),
    ],
)
_bind("lling_graph_free", [ctypes.c_void_p], None)
_bind(
    "lling_graph_distance_open",
    [
        ctypes.c_void_p,
        ctypes.POINTER(GraphDistanceConfig),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_graph_distance_next",
    [ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)],
)
_bind(
    "lling_graph_distance_take",
    [ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)],
)
_bind("lling_graph_distance_cursor_free", [ctypes.c_void_p], None)
_bind(
    "lling_graph_distance_info",
    [ctypes.c_void_p, ctypes.POINTER(ctypes.c_double), ctypes.POINTER(ctypes.c_size_t)],
)
_bind(
    "lling_graph_distance_page",
    [
        ctypes.c_void_p,
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_double),
        ctypes.POINTER(ctypes.c_double),
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_size_t),
        ctypes.POINTER(ctypes.c_size_t),
    ],
)
_bind("lling_graph_distance_free", [ctypes.c_void_p], None)
_bind(
    "lling_graph_posterior_arcs",
    [
        ctypes.c_void_p,
        ctypes.c_size_t,
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_double),
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_size_t),
        ctypes.POINTER(ctypes.c_size_t),
    ],
)
_bind(
    "lling_graph_posterior_final",
    [ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_double)],
)
_bind(
    "lling_ranked_path_cursor_open",
    [
        ctypes.c_void_p,
        ctypes.POINTER(RankedPathConfig),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_ranked_path_cursor_next",
    [
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_uint32),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind("lling_ranked_path_cursor_free", [ctypes.c_void_p], None)
_bind(
    "lling_sample_path_cursor_open",
    [
        ctypes.c_void_p,
        ctypes.POINTER(SamplePathConfig),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind(
    "lling_sample_path_cursor_next",
    [
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_uint32),
        ctypes.POINTER(ctypes.c_void_p),
    ],
)
_bind("lling_sample_path_cursor_free", [ctypes.c_void_p], None)

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
    "lling_semiring_diagnostic",
    [
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_size_t),
        ctypes.POINTER(ctypes.c_size_t),
    ],
)
for _name in ("lling_semiring_plus_many", "lling_semiring_times_many"):
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

_bind("lling_symbolic_predicate_constant", [ctypes.c_uint32, ctypes.c_int64, ctypes.c_int64, ctypes.c_uint8, ctypes.POINTER(ctypes.c_void_p)])
_bind("lling_symbolic_interval_range", [ctypes.c_int64, ctypes.c_int64, ctypes.c_int64, ctypes.c_int64, ctypes.POINTER(ctypes.c_void_p)])
_bind("lling_symbolic_char_range", [ctypes.c_uint32, ctypes.c_uint32, ctypes.POINTER(ctypes.c_void_p)])
_bind("lling_symbolic_predicate_binary", [ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)])
_bind("lling_symbolic_predicate_not", [ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)])
_bind("lling_symbolic_predicate_evaluate", [ctypes.c_void_p, ctypes.c_int64, ctypes.POINTER(ctypes.c_uint8)])
_bind("lling_symbolic_predicate_witness", [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint8), ctypes.POINTER(ctypes.c_int64)])
_bind("lling_symbolic_predicate_relation", [ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint8)])
_bind("lling_symbolic_predicate_free", [ctypes.c_void_p], None)
_bind("lling_symbolic_automaton_new", [ctypes.c_uint32, ctypes.c_int64, ctypes.c_int64, ctypes.POINTER(ctypes.c_void_p)])
_bind("lling_symbolic_automaton_add_state", [ctypes.c_void_p, ctypes.c_uint8, ctypes.POINTER(ctypes.c_uint64)])
_bind("lling_symbolic_automaton_set_initial", [ctypes.c_void_p, ctypes.c_uint64])
_bind("lling_symbolic_automaton_add_transition", [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint64, ctypes.c_void_p])
_bind("lling_symbolic_automaton_accepts", [ctypes.c_void_p, ctypes.POINTER(ctypes.c_int64), ctypes.c_size_t, ctypes.POINTER(ctypes.c_uint8)])
_bind("lling_symbolic_automaton_is_empty", [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint8)])
_bind("lling_symbolic_automaton_free", [ctypes.c_void_p], None)
_bind("lling_symbolic_transducer_new", [ctypes.c_uint32, ctypes.c_int64, ctypes.c_int64, ctypes.POINTER(ctypes.c_void_p)])
_bind("lling_symbolic_transducer_add_state", [ctypes.c_void_p, ctypes.c_uint8, ctypes.POINTER(ctypes.c_uint64)])
_bind("lling_symbolic_transducer_set_initial", [ctypes.c_void_p, ctypes.c_uint64])
_bind("lling_symbolic_transducer_add_transition", [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_uint32, ctypes.POINTER(ctypes.c_int64), ctypes.c_size_t])
_bind("lling_symbolic_transducer_transduce", [ctypes.c_void_p, ctypes.POINTER(ctypes.c_int64), ctypes.c_size_t, ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)])
_bind("lling_symbolic_transduction_count", [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint64)])
_bind("lling_symbolic_transduction_output", [ctypes.c_void_p, ctypes.c_uint64, ctypes.POINTER(ctypes.c_int64), ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)])
_bind("lling_symbolic_transducer_free", [ctypes.c_void_p], None)
_bind("lling_symbolic_transduction_free", [ctypes.c_void_p], None)
_bind("lling_symbolic_transducer_compose_transduce", [ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int64), ctypes.c_size_t, ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)])

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
    return int(lib.lling_llang_api_revision())


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
