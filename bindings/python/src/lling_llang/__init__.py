"""Build and compose weighted automata with Python-defined algebra and graphs.

``lling_llang`` combines a native Unicode/tropical WFST engine with the stable
Vinary Tree resource ABI. Python applications may build graphs eagerly, lazily
compose any compatible resource, or implement custom WFST, lattice, and
semiring providers using the re-exported protocols from
``vinary_tree_interop``.

Every owning type is a context manager. A provider may be closed immediately
after successful import because the returned native adapter owns an independent
retain. Dynamic lattice and semiring handles remain bound to their creating
thread; immutable scalar-WFST resources follow their advertised capability
flags.
"""

from vinary_tree_interop import (
    DivisibleSemiringProvider,
    DomainId,
    LatticeFlag,
    LatticeOperand,
    LatticeOptions,
    LatticeProvider,
    LatticeResource,
    NativeResource,
    NumericSemiringProvider,
    ProviderStatusError,
    ScalarWfstArc,
    ScalarWfstResource,
    ScalarWfstSnapshot,
    ScalarWfstState,
    ScalarWfstStateInfo,
    SemiringFlag,
    SemiringOptions,
    SemiringOrder,
    SemiringProperty,
    SemiringProvider,
    SemiringResource,
    StarSemiringProvider,
    UnitDomain,
    VtResource,
    WeightDomain,
    WfstFlag,
)

from ._abi import (
    ABI_VERSION,
    API_REVISION,
    TYPED_ABI_VERSION,
    AbiV2Header,
    Applicability,
    Budget,
    BudgetFlag,
    CancellationReason,
    Completeness,
    DescriptorFlag,
    Digest256,
    EvidenceState,
    Id128,
    NativeError,
    Outcome,
    Precision,
    Status,
    Termination,
    WfstDescriptor,
    abi_version,
    api_revision,
)
from ._algebra import (
    LatticeValue,
    SemiringContext,
    SemiringWeight,
    validate_lattice_laws,
)
from ._control import (
    Cancellation,
    authoritative_exact,
    identity_matches,
    typed_evidence_allowed,
    validate_budget,
    validate_header,
)
from ._wfst import Wfst, WfstBuilder, compose, import_wfst

__version__ = "4.0.0rc6"

__all__ = [
    "ABI_VERSION",
    "API_REVISION",
    "TYPED_ABI_VERSION",
    "AbiV2Header",
    "Applicability",
    "Budget",
    "BudgetFlag",
    "Cancellation",
    "CancellationReason",
    "Completeness",
    "DescriptorFlag",
    "Digest256",
    "DivisibleSemiringProvider",
    "DomainId",
    "EvidenceState",
    "Id128",
    "LatticeFlag",
    "LatticeOperand",
    "LatticeOptions",
    "LatticeProvider",
    "LatticeResource",
    "LatticeValue",
    "NativeError",
    "NativeResource",
    "NumericSemiringProvider",
    "Outcome",
    "Precision",
    "ProviderStatusError",
    "ScalarWfstArc",
    "ScalarWfstResource",
    "ScalarWfstSnapshot",
    "ScalarWfstState",
    "ScalarWfstStateInfo",
    "SemiringContext",
    "SemiringFlag",
    "SemiringOptions",
    "SemiringOrder",
    "SemiringProperty",
    "SemiringProvider",
    "SemiringResource",
    "SemiringWeight",
    "StarSemiringProvider",
    "Status",
    "Termination",
    "UnitDomain",
    "VtResource",
    "WeightDomain",
    "Wfst",
    "WfstBuilder",
    "WfstDescriptor",
    "WfstFlag",
    "__version__",
    "abi_version",
    "api_revision",
    "authoritative_exact",
    "compose",
    "identity_matches",
    "import_wfst",
    "typed_evidence_allowed",
    "validate_budget",
    "validate_header",
    "validate_lattice_laws",
]
