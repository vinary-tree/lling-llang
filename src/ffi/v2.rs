//! Typed, additive ABI-v2 metadata and validation rules.
//!
//! The public structures in this module are pointer-free `repr(C)` prefixes.
//! Their `struct_size` field admits a larger future tail, while every known
//! field, flag, discriminant, and reserved word remains canonical.

use core::mem::size_of;
use core::sync::atomic::{AtomicU32, Ordering};

/// Version carried by typed ABI-v2 metadata headers.
pub const LLING_ABI_V2: u32 = 2;

/// The descriptor carries distinct input-tape, output-tape, and algebra IDs.
pub const LLING_DESCRIPTOR_SIGNATURE_KNOWN: u64 = 1 << 0;
/// The descriptor carries an immutable snapshot ID.
pub const LLING_DESCRIPTOR_SNAPSHOT_PRESENT: u64 = 1 << 1;
/// The descriptor carries a domain-neutral evidence-context digest.
pub const LLING_DESCRIPTOR_CONTEXT_PRESENT: u64 = 1 << 2;

/// The state-count budget is active.
pub const LLING_BUDGET_STATES: u64 = 1 << 0;
/// The arc-count budget is active.
pub const LLING_BUDGET_ARCS: u64 = 1 << 1;
/// The byte-count budget is active.
pub const LLING_BUDGET_BYTES: u64 = 1 << 2;
/// The abstract-work budget is active.
pub const LLING_BUDGET_WORK: u64 = 1 << 3;

const KNOWN_DESCRIPTOR_FLAGS: u64 = LLING_DESCRIPTOR_SIGNATURE_KNOWN
    | LLING_DESCRIPTOR_SNAPSHOT_PRESENT
    | LLING_DESCRIPTOR_CONTEXT_PRESENT;
const KNOWN_BUDGET_FLAGS: u64 =
    LLING_BUDGET_STATES | LLING_BUDGET_ARCS | LLING_BUDGET_BYTES | LLING_BUDGET_WORK;

/// Common additive prefix for every typed ABI-v2 metadata structure.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LlingAbiV2Header {
    /// Bytes available at this pointer, including this prefix.
    pub struct_size: u32,
    /// Must equal [`LLING_ABI_V2`].
    pub abi_version: u32,
    /// Structure-specific flags; unknown bits are rejected.
    pub flags: u64,
    /// Must be zero.
    pub reserved: u64,
}

/// Fixed-width semantic identifier. All-zero bytes denote absence.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LlingId128 {
    /// Canonical identifier bytes.
    pub bytes: [u8; 16],
}

impl LlingId128 {
    fn is_zero(self) -> bool {
        self.bytes == [0; 16]
    }
}

/// Fixed-width digest. All-zero bytes denote absence.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LlingDigest256 {
    /// Canonical digest bytes.
    pub bytes: [u8; 32],
}

impl LlingDigest256 {
    fn is_zero(self) -> bool {
        self.bytes == [0; 32]
    }
}

/// Typed identity metadata for a WFST or optimizer value.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LlingWfstDescriptorV2 {
    /// Additive metadata prefix.
    pub header: LlingAbiV2Header,
    /// Input-tape semantic domain.
    pub input_tape: LlingId128,
    /// Output-tape semantic domain.
    pub output_tape: LlingId128,
    /// Weight/algebra semantic domain.
    pub algebra: LlingId128,
    /// Immutable producer snapshot.
    pub snapshot: LlingId128,
    /// Domain-neutral assumptions and evidence context.
    pub context: LlingDigest256,
}

/// Canonical resource limits for a bounded operation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LlingBudgetV2 {
    /// Additive metadata prefix; flags select active limits.
    pub header: LlingAbiV2Header,
    /// Maximum number of states when [`LLING_BUDGET_STATES`] is active.
    pub max_states: u64,
    /// Maximum number of arcs when [`LLING_BUDGET_ARCS`] is active.
    pub max_arcs: u64,
    /// Maximum resident or allocated bytes when [`LLING_BUDGET_BYTES`] is active.
    pub max_bytes: u64,
    /// Maximum abstract work units when [`LLING_BUDGET_WORK`] is active.
    pub max_work: u64,
    /// Must be all zero.
    pub reserved: [u64; 2],
}

macro_rules! raw_axis {
    ($(#[$metadata:meta])* $name:ident { $($variant:ident = $raw:literal),+ $(,)? }) => {
        $(#[$metadata])*
        #[repr(u32)]
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum $name {
            $(
                #[doc = concat!("Wire value `", stringify!($raw), "`.")]
                $variant = $raw,
            )+
        }

        impl $name {
            /// Decode a raw wire discriminant without constructing an invalid enum.
            pub const fn from_raw(raw: u32) -> Option<Self> {
                match raw {
                    $($raw => Some(Self::$variant),)+
                    _ => None,
                }
            }

            /// Return the stable wire discriminant.
            pub const fn to_raw(self) -> u32 {
                self as u32
            }
        }
    };
}

raw_axis!(
    /// Whether the result preserves the denotation exactly.
    LlingPrecisionV2 {
    Exact = 1,
    Approximate = 2,
    Unknown = 3,
});
raw_axis!(
    /// Whether the operation completed its declared search or transformation.
    LlingCompletenessV2 {
    Complete = 1,
    Incomplete = 2,
});
raw_axis!(
    /// Whether the operation applies to the supplied semantic domains.
    LlingApplicabilityV2 {
    Applicable = 1,
    Unsupported = 2,
    Unknown = 3,
});
raw_axis!(
    /// How the operation terminated, independently of semantic precision.
    LlingTerminationV2 {
    Succeeded = 1,
    Cancelled = 2,
    BudgetExhausted = 3,
    Failed = 4,
});
raw_axis!(
    /// Validation state of the evidence associated with a result.
    LlingEvidenceStateV2 {
    None = 0,
    Candidate = 1,
    Verified = 2,
    Stale = 3,
    Invalid = 4,
});
raw_axis!(
    /// Stable reason recorded by a cooperative-cancellation handle.
    LlingCancellationReasonV2 {
    Requested = 1,
    Deadline = 2,
    Budget = 3,
    Source = 4,
});

/// Orthogonal semantic and operational result axes.
///
/// Raw `u32` fields deliberately precede enum decoding so malformed foreign
/// discriminants can be rejected without invoking Rust enum validity rules.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LlingOutcomeV2 {
    /// Additive metadata prefix; no flags are currently defined.
    pub header: LlingAbiV2Header,
    /// Raw [`LlingPrecisionV2`] value.
    pub precision: u32,
    /// Raw [`LlingCompletenessV2`] value.
    pub completeness: u32,
    /// Raw [`LlingApplicabilityV2`] value.
    pub applicability: u32,
    /// Raw [`LlingTerminationV2`] value.
    pub termination: u32,
    /// Raw [`LlingEvidenceStateV2`] value.
    pub evidence: u32,
    /// Must be zero.
    pub reserved0: u32,
    /// States consumed or produced.
    pub states: u64,
    /// Arcs consumed or produced.
    pub arcs: u64,
    /// Bytes consumed or produced.
    pub bytes: u64,
    /// Abstract work units consumed.
    pub work: u64,
    /// Domain-neutral limitation bits defined by the producing operation.
    pub limitations: u64,
    /// Must be zero.
    pub reserved1: u64,
}

/// Thread-safe cooperative-cancellation handle.
///
/// Zero means live; the first nonzero [`LlingCancellationReasonV2`] is sticky.
pub struct LlingCancellationV2 {
    pub(super) reason: AtomicU32,
}

impl LlingCancellationV2 {
    pub(super) fn new() -> Self {
        Self {
            reason: AtomicU32::new(0),
        }
    }

    pub(super) fn request(&self, reason: LlingCancellationReasonV2) {
        let _ =
            self.reason
                .compare_exchange(0, reason.to_raw(), Ordering::AcqRel, Ordering::Acquire);
    }

    pub(super) fn reason(&self) -> u32 {
        self.reason.load(Ordering::Acquire)
    }
}

/// Validate a typed ABI-v2 header against a required known prefix.
pub fn validate_abi_v2_header(
    header: &LlingAbiV2Header,
    required_size: usize,
    known_flags: u64,
) -> bool {
    let Ok(required_size) = u32::try_from(required_size) else {
        return false;
    };
    header.struct_size >= required_size
        && header.abi_version == LLING_ABI_V2
        && header.flags & !known_flags == 0
        && header.reserved == 0
}

/// Validate canonical presence flags and identities in a WFST descriptor.
pub fn validate_descriptor_v2(descriptor: &LlingWfstDescriptorV2) -> bool {
    if !validate_abi_v2_header(
        &descriptor.header,
        size_of::<LlingWfstDescriptorV2>(),
        KNOWN_DESCRIPTOR_FLAGS,
    ) {
        return false;
    }

    let signature_present = descriptor.header.flags & LLING_DESCRIPTOR_SIGNATURE_KNOWN != 0;
    let signature_is_zero = descriptor.input_tape.is_zero()
        && descriptor.output_tape.is_zero()
        && descriptor.algebra.is_zero();
    let signature_is_complete = !descriptor.input_tape.is_zero()
        && !descriptor.output_tape.is_zero()
        && !descriptor.algebra.is_zero();
    let snapshot_present = descriptor.header.flags & LLING_DESCRIPTOR_SNAPSHOT_PRESENT != 0;
    let context_present = descriptor.header.flags & LLING_DESCRIPTOR_CONTEXT_PRESENT != 0;

    (if signature_present {
        signature_is_complete
    } else {
        signature_is_zero
    }) && snapshot_present == !descriptor.snapshot.is_zero()
        && context_present == !descriptor.context.is_zero()
}

/// Whether a descriptor carries enough canonical identity for typed evidence.
pub fn abi_v2_typed_evidence_allowed(descriptor: &LlingWfstDescriptorV2) -> bool {
    validate_descriptor_v2(descriptor)
        && descriptor.header.flags & KNOWN_DESCRIPTOR_FLAGS == KNOWN_DESCRIPTOR_FLAGS
}

/// Compare the signature, immutable snapshot, and evidence context required for replay.
pub fn abi_v2_identity_matches(
    expected: &LlingWfstDescriptorV2,
    observed: &LlingWfstDescriptorV2,
) -> bool {
    abi_v2_typed_evidence_allowed(expected)
        && abi_v2_typed_evidence_allowed(observed)
        && expected.input_tape == observed.input_tape
        && expected.output_tape == observed.output_tape
        && expected.algebra == observed.algebra
        && expected.snapshot == observed.snapshot
        && expected.context == observed.context
}

fn canonical_limit(enabled: bool, value: u64) -> bool {
    if enabled {
        value > 0
    } else {
        value == 0
    }
}

/// Validate a canonical bounded-operation budget.
pub fn validate_budget_v2(budget: &LlingBudgetV2) -> bool {
    validate_abi_v2_header(
        &budget.header,
        size_of::<LlingBudgetV2>(),
        KNOWN_BUDGET_FLAGS,
    ) && canonical_limit(
        budget.header.flags & LLING_BUDGET_STATES != 0,
        budget.max_states,
    ) && canonical_limit(
        budget.header.flags & LLING_BUDGET_ARCS != 0,
        budget.max_arcs,
    ) && canonical_limit(
        budget.header.flags & LLING_BUDGET_BYTES != 0,
        budget.max_bytes,
    ) && canonical_limit(
        budget.header.flags & LLING_BUDGET_WORK != 0,
        budget.max_work,
    ) && budget.reserved == [0; 2]
}

/// Validate orthogonal outcome axes and handle-publication relationships.
pub fn validate_outcome_v2(
    outcome: &LlingOutcomeV2,
    resource_present: bool,
    evidence_present: bool,
) -> bool {
    if !validate_abi_v2_header(&outcome.header, size_of::<LlingOutcomeV2>(), 0)
        || outcome.reserved0 != 0
        || outcome.reserved1 != 0
    {
        return false;
    }
    let (
        Some(_precision),
        Some(completeness),
        Some(applicability),
        Some(termination),
        Some(evidence),
    ) = (
        LlingPrecisionV2::from_raw(outcome.precision),
        LlingCompletenessV2::from_raw(outcome.completeness),
        LlingApplicabilityV2::from_raw(outcome.applicability),
        LlingTerminationV2::from_raw(outcome.termination),
        LlingEvidenceStateV2::from_raw(outcome.evidence),
    )
    else {
        return false;
    };

    let terminal_non_success = matches!(
        termination,
        LlingTerminationV2::Cancelled
            | LlingTerminationV2::BudgetExhausted
            | LlingTerminationV2::Failed
    );
    if terminal_non_success && (resource_present || evidence_present) {
        return false;
    }
    if evidence_present
        && (!resource_present
            || !matches!(
                evidence,
                LlingEvidenceStateV2::Candidate | LlingEvidenceStateV2::Verified
            ))
    {
        return false;
    }
    if resource_present
        && (termination != LlingTerminationV2::Succeeded
            || applicability != LlingApplicabilityV2::Applicable)
    {
        return false;
    }
    if matches!(
        termination,
        LlingTerminationV2::Cancelled | LlingTerminationV2::BudgetExhausted
    ) && completeness != LlingCompletenessV2::Incomplete
    {
        return false;
    }
    evidence != LlingEvidenceStateV2::Verified || evidence_present
}

/// Whether an outcome can carry an authoritative exact claim.
pub fn abi_v2_authoritative_exact(outcome: &LlingOutcomeV2, evidence_live: bool) -> bool {
    evidence_live
        && validate_outcome_v2(outcome, true, true)
        && LlingPrecisionV2::from_raw(outcome.precision) == Some(LlingPrecisionV2::Exact)
        && LlingCompletenessV2::from_raw(outcome.completeness)
            == Some(LlingCompletenessV2::Complete)
        && LlingApplicabilityV2::from_raw(outcome.applicability)
            == Some(LlingApplicabilityV2::Applicable)
        && LlingTerminationV2::from_raw(outcome.termination) == Some(LlingTerminationV2::Succeeded)
        && LlingEvidenceStateV2::from_raw(outcome.evidence) == Some(LlingEvidenceStateV2::Verified)
}
