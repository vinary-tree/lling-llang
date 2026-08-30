#![cfg(feature = "ffi")]

//! Required-red refinement properties extracted from the typed ABI v2 model.
//!
//! This test deliberately names the production v2 surface before its
//! implementation. The formal tranche records the compile failure as the red
//! gate; the following implementation tranche must make these same properties
//! pass without weakening or disabling them.

use lling_llang::ffi::{
    abi_v2_authoritative_exact, abi_v2_identity_matches, abi_v2_typed_evidence_allowed,
    lling_abi_v2_identity_matches, lling_abi_v2_validate_budget, lling_abi_v2_validate_descriptor,
    lling_abi_v2_validate_header, lling_abi_v2_validate_outcome, lling_cancellation_v2_free,
    lling_cancellation_v2_new, lling_cancellation_v2_reason, lling_cancellation_v2_request,
    validate_abi_v2_header, validate_budget_v2, validate_descriptor_v2, validate_outcome_v2,
    LlingAbiV2Header, LlingApplicabilityV2, LlingBudgetV2, LlingCancellationReasonV2,
    LlingCancellationV2, LlingCompletenessV2, LlingDigest256, LlingEvidenceStateV2, LlingId128,
    LlingOutcomeV2, LlingPrecisionV2, LlingStatus, LlingTerminationV2, LlingWfstDescriptorV2,
    LLING_ABI_V2, LLING_BUDGET_ARCS, LLING_BUDGET_BYTES, LLING_BUDGET_STATES, LLING_BUDGET_WORK,
    LLING_DESCRIPTOR_CONTEXT_PRESENT, LLING_DESCRIPTOR_SIGNATURE_KNOWN,
    LLING_DESCRIPTOR_SNAPSHOT_PRESENT,
};
use proptest::prelude::*;
use std::mem::{align_of, size_of};
use std::ptr;

fn header(size: usize, flags: u64) -> LlingAbiV2Header {
    LlingAbiV2Header {
        struct_size: u32::try_from(size).expect("ABI structures fit u32"),
        abi_version: LLING_ABI_V2,
        flags,
        reserved: 0,
    }
}

fn typed_descriptor(seed: u8) -> LlingWfstDescriptorV2 {
    LlingWfstDescriptorV2 {
        header: header(
            size_of::<LlingWfstDescriptorV2>(),
            LLING_DESCRIPTOR_SIGNATURE_KNOWN
                | LLING_DESCRIPTOR_SNAPSHOT_PRESENT
                | LLING_DESCRIPTOR_CONTEXT_PRESENT,
        ),
        input_tape: LlingId128 { bytes: [seed; 16] },
        output_tape: LlingId128 {
            bytes: [seed.wrapping_add(1); 16],
        },
        algebra: LlingId128 {
            bytes: [seed.wrapping_add(2); 16],
        },
        snapshot: LlingId128 {
            bytes: [seed.wrapping_add(3); 16],
        },
        context: LlingDigest256 {
            bytes: [seed.wrapping_add(4); 32],
        },
    }
}

// INVARIANT-HOOK: LLING-ABI2-POD-1
#[test]
fn v2_metadata_layouts_are_fixed_and_pointer_free() {
    assert_eq!(size_of::<LlingAbiV2Header>(), 24);
    assert_eq!(size_of::<LlingId128>(), 16);
    assert_eq!(align_of::<LlingId128>(), 1);
    assert_eq!(size_of::<LlingDigest256>(), 32);
    assert_eq!(align_of::<LlingDigest256>(), 1);
    assert_eq!(size_of::<LlingWfstDescriptorV2>(), 120);
    assert_eq!(size_of::<LlingBudgetV2>(), 72);
    assert_eq!(size_of::<LlingOutcomeV2>(), 96);
}

// INVARIANT-HOOK: LLING-ABI2-HDR-1..2
proptest! {
    #[test]
    fn v2_header_accepts_only_additive_known_prefixes(extra in 0u16..4096) {
        let required = size_of::<LlingWfstDescriptorV2>();
        let known = LLING_DESCRIPTOR_SIGNATURE_KNOWN
            | LLING_DESCRIPTOR_SNAPSHOT_PRESENT
            | LLING_DESCRIPTOR_CONTEXT_PRESENT;
        let valid = header(required + usize::from(extra), known);
        prop_assert!(validate_abi_v2_header(&valid, required, known));

        let mut short = valid;
        short.struct_size = u32::try_from(required - 1).unwrap();
        prop_assert!(!validate_abi_v2_header(&short, required, known));
        let mut wrong_version = valid;
        wrong_version.abi_version = LLING_ABI_V2 + 1;
        prop_assert!(!validate_abi_v2_header(&wrong_version, required, known));
        let mut unknown = valid;
        unknown.flags |= 1 << 63;
        prop_assert!(!validate_abi_v2_header(&unknown, required, known));
        let mut reserved = valid;
        reserved.reserved = 1;
        prop_assert!(!validate_abi_v2_header(&reserved, required, known));
    }
}

// INVARIANT-HOOK: LLING-ABI2-RAW-1
#[test]
fn v2_raw_axes_reject_unknown_values() {
    assert!(LlingPrecisionV2::from_raw(99).is_none());
    assert!(LlingCompletenessV2::from_raw(99).is_none());
    assert!(LlingApplicabilityV2::from_raw(99).is_none());
    assert!(LlingTerminationV2::from_raw(99).is_none());
    assert!(LlingEvidenceStateV2::from_raw(99).is_none());
}

// INVARIANT-HOOK: LLING-ABI2-RAW-1
proptest! {
    #[test]
    fn v2_raw_axes_accept_exactly_their_wire_domains(
        precision in any::<u32>(),
        completeness in any::<u32>(),
        applicability in any::<u32>(),
        termination in any::<u32>(),
        evidence in any::<u32>(),
    ) {
        prop_assert_eq!(
            LlingPrecisionV2::from_raw(precision).is_some(),
            matches!(precision, 1..=3)
        );
        prop_assert_eq!(
            LlingCompletenessV2::from_raw(completeness).is_some(),
            matches!(completeness, 1..=2)
        );
        prop_assert_eq!(
            LlingApplicabilityV2::from_raw(applicability).is_some(),
            matches!(applicability, 1..=3)
        );
        prop_assert_eq!(
            LlingTerminationV2::from_raw(termination).is_some(),
            matches!(termination, 1..=4)
        );
        prop_assert_eq!(
            LlingEvidenceStateV2::from_raw(evidence).is_some(),
            matches!(evidence, 0..=4)
        );
    }
}

// INVARIANT-HOOK: LLING-ABI2-SIG-1
#[test]
fn v2_typed_descriptors_are_canonical() {
    let descriptor = typed_descriptor(10);
    assert!(validate_descriptor_v2(&descriptor));
    assert!(abi_v2_typed_evidence_allowed(&descriptor));

    let mut absent_signature_field = descriptor;
    absent_signature_field.algebra = LlingId128::default();
    assert!(!validate_descriptor_v2(&absent_signature_field));

    let mut inactive_snapshot = descriptor;
    inactive_snapshot.header.flags &= !LLING_DESCRIPTOR_SNAPSHOT_PRESENT;
    assert!(!validate_descriptor_v2(&inactive_snapshot));
}

// INVARIANT-HOOK: LLING-ABI2-OPAQUE-1
#[test]
fn v2_opaque_v1_never_yields_typed_evidence() {
    let descriptor = LlingWfstDescriptorV2 {
        header: header(size_of::<LlingWfstDescriptorV2>(), 0),
        ..LlingWfstDescriptorV2::default()
    };
    assert!(validate_descriptor_v2(&descriptor));
    assert!(!abi_v2_typed_evidence_allowed(&descriptor));
}

// INVARIANT-HOOK: LLING-ABI2-ID-1
#[test]
fn v2_evidence_replay_binds_signature_snapshot_and_context() {
    let expected = typed_descriptor(20);
    let mut observed = expected;
    assert!(abi_v2_identity_matches(&expected, &observed));
    observed.input_tape.bytes[0] ^= 1;
    assert!(!abi_v2_identity_matches(&expected, &observed));
    observed = expected;
    observed.output_tape.bytes[7] ^= 1;
    assert!(!abi_v2_identity_matches(&expected, &observed));
    observed = expected;
    observed.algebra.bytes[15] ^= 1;
    assert!(!abi_v2_identity_matches(&expected, &observed));
    observed = expected;
    observed.snapshot.bytes[0] ^= 1;
    assert!(!abi_v2_identity_matches(&expected, &observed));
    observed = expected;
    observed.context.bytes[31] ^= 1;
    assert!(!abi_v2_identity_matches(&expected, &observed));
}

fn outcome(
    precision: LlingPrecisionV2,
    completeness: LlingCompletenessV2,
    applicability: LlingApplicabilityV2,
    termination: LlingTerminationV2,
    evidence: LlingEvidenceStateV2,
) -> LlingOutcomeV2 {
    LlingOutcomeV2 {
        header: header(size_of::<LlingOutcomeV2>(), 0),
        precision: precision.to_raw(),
        completeness: completeness.to_raw(),
        applicability: applicability.to_raw(),
        termination: termination.to_raw(),
        evidence: evidence.to_raw(),
        ..LlingOutcomeV2::default()
    }
}

// INVARIANT-HOOK: LLING-ABI2-AXIS-1
// INVARIANT-HOOK: LLING-ABI2-AUTH-1
#[test]
fn v2_outcome_axes_do_not_self_promote() {
    let exact_incomplete = outcome(
        LlingPrecisionV2::Exact,
        LlingCompletenessV2::Incomplete,
        LlingApplicabilityV2::Applicable,
        LlingTerminationV2::Succeeded,
        LlingEvidenceStateV2::Candidate,
    );
    assert!(validate_outcome_v2(&exact_incomplete, true, true));
    assert!(!abi_v2_authoritative_exact(&exact_incomplete, true));

    let approximate_complete = outcome(
        LlingPrecisionV2::Approximate,
        LlingCompletenessV2::Complete,
        LlingApplicabilityV2::Applicable,
        LlingTerminationV2::Succeeded,
        LlingEvidenceStateV2::Candidate,
    );
    assert!(validate_outcome_v2(&approximate_complete, true, true));
    assert!(!abi_v2_authoritative_exact(&approximate_complete, true));

    let authoritative = outcome(
        LlingPrecisionV2::Exact,
        LlingCompletenessV2::Complete,
        LlingApplicabilityV2::Applicable,
        LlingTerminationV2::Succeeded,
        LlingEvidenceStateV2::Verified,
    );
    assert!(validate_outcome_v2(&authoritative, true, true));
    assert!(abi_v2_authoritative_exact(&authoritative, true));
    assert!(!validate_outcome_v2(&authoritative, true, false));
}

// INVARIANT-HOOK: LLING-ABI2-TERM-1..2
#[test]
fn v2_cancelled_and_budget_outcomes_never_publish() {
    for termination in [
        LlingTerminationV2::Cancelled,
        LlingTerminationV2::BudgetExhausted,
    ] {
        let incomplete = outcome(
            LlingPrecisionV2::Unknown,
            LlingCompletenessV2::Incomplete,
            LlingApplicabilityV2::Applicable,
            termination,
            LlingEvidenceStateV2::None,
        );
        assert!(validate_outcome_v2(&incomplete, false, false));
        assert!(!validate_outcome_v2(&incomplete, true, false));
        assert!(!validate_outcome_v2(&incomplete, false, true));
    }
}

// INVARIANT-HOOK: LLING-ABI2-BUDGET-1
#[test]
fn v2_budget_flags_and_values_are_canonical() {
    let all = LLING_BUDGET_STATES | LLING_BUDGET_ARCS | LLING_BUDGET_BYTES | LLING_BUDGET_WORK;
    let valid = LlingBudgetV2 {
        header: header(size_of::<LlingBudgetV2>(), all),
        max_states: 1,
        max_arcs: 2,
        max_bytes: 3,
        max_work: 4,
        reserved: [0; 2],
    };
    assert!(validate_budget_v2(&valid));
    let mut inactive_nonzero = valid;
    inactive_nonzero.header.flags &= !LLING_BUDGET_STATES;
    assert!(!validate_budget_v2(&inactive_nonzero));
    let mut active_zero = valid;
    active_zero.max_states = 0;
    assert!(!validate_budget_v2(&active_zero));
}

// INVARIANT-HOOK: LLING-ABI2-BUDGET-1
proptest! {
    #[test]
    fn v2_budget_validation_equals_the_flag_value_bijection(
        flags in any::<u64>(),
        values in any::<[u64; 4]>(),
    ) {
        let budget = LlingBudgetV2 {
            header: header(size_of::<LlingBudgetV2>(), flags),
            max_states: values[0],
            max_arcs: values[1],
            max_bytes: values[2],
            max_work: values[3],
            reserved: [0; 2],
        };
        let expected = flags & !0b1111 == 0
            && (0..4).all(|index| {
                let enabled = flags & (1 << index) != 0;
                if enabled { values[index] > 0 } else { values[index] == 0 }
            });
        prop_assert_eq!(validate_budget_v2(&budget), expected);
    }
}

// INVARIANT-HOOK: LLING-ABI2-HDR-1..2
// INVARIANT-HOOK: LLING-ABI2-RAW-1
#[test]
fn v2_c_validators_reject_null_misaligned_malformed_and_non_boolean_inputs() {
    let descriptor = typed_descriptor(31);
    let budget = LlingBudgetV2 {
        header: header(size_of::<LlingBudgetV2>(), LLING_BUDGET_STATES),
        max_states: 8,
        ..LlingBudgetV2::default()
    };
    let result = outcome(
        LlingPrecisionV2::Exact,
        LlingCompletenessV2::Complete,
        LlingApplicabilityV2::Applicable,
        LlingTerminationV2::Succeeded,
        LlingEvidenceStateV2::Verified,
    );
    let mut answer = 0xff;

    assert_eq!(
        lling_abi_v2_validate_header(&descriptor.header, 120, 0b111),
        LlingStatus::Ok
    );
    assert_eq!(
        lling_abi_v2_validate_descriptor(&descriptor, &mut answer),
        LlingStatus::Ok
    );
    assert_eq!(answer, 1);
    assert_eq!(lling_abi_v2_validate_budget(&budget), LlingStatus::Ok);
    assert_eq!(
        lling_abi_v2_validate_outcome(&result, 1, 1, &mut answer),
        LlingStatus::Ok
    );
    assert_eq!(answer, 1);

    assert_eq!(
        lling_abi_v2_validate_header(ptr::null(), 24, 0),
        LlingStatus::NullPointer
    );
    assert_eq!(
        lling_abi_v2_validate_descriptor(ptr::null(), &mut answer),
        LlingStatus::NullPointer
    );
    assert_eq!(
        lling_abi_v2_validate_descriptor(&descriptor, ptr::null_mut()),
        LlingStatus::NullPointer
    );
    answer = 0xa5;
    assert_eq!(
        lling_abi_v2_validate_outcome(&result, 2, 0, &mut answer),
        LlingStatus::InvalidArgument
    );
    assert_eq!(answer, 0xa5);

    #[repr(align(8))]
    struct AlignedDescriptorBytes([u8; 121]);
    let storage = AlignedDescriptorBytes([0; 121]);
    let misaligned = unsafe { storage.0.as_ptr().add(1) }.cast::<LlingWfstDescriptorV2>();
    assert_eq!(
        lling_abi_v2_validate_descriptor(misaligned, &mut answer),
        LlingStatus::InvalidArgument
    );
}

// INVARIANT-HOOK: LLING-ABI2-ID-1
#[test]
fn v2_c_identity_comparison_is_total_and_does_not_write_on_failure() {
    let expected = typed_descriptor(44);
    let mut observed = expected;
    let mut answer = 0xff;
    assert_eq!(
        lling_abi_v2_identity_matches(&expected, &observed, &mut answer),
        LlingStatus::Ok
    );
    assert_eq!(answer, 1);

    observed.context.bytes[0] ^= 1;
    assert_eq!(
        lling_abi_v2_identity_matches(&expected, &observed, &mut answer),
        LlingStatus::Ok
    );
    assert_eq!(answer, 0);

    answer = 0xa5;
    assert_eq!(
        lling_abi_v2_identity_matches(ptr::null(), &observed, &mut answer),
        LlingStatus::NullPointer
    );
    assert_eq!(answer, 0xa5);
}

// INVARIANT-HOOK: LLING-ABI2-TERM-1
// INVARIANT-HOOK: LLING-ABI2-OWN-1
#[test]
fn v2_cancellation_is_sticky_queryable_and_single_release() {
    let mut cancellation: *mut LlingCancellationV2 = ptr::null_mut();
    assert_eq!(
        lling_cancellation_v2_new(&mut cancellation),
        LlingStatus::Ok
    );
    assert!(!cancellation.is_null());

    let mut reason = u32::MAX;
    assert_eq!(
        lling_cancellation_v2_reason(cancellation, &mut reason),
        LlingStatus::Ok
    );
    assert_eq!(reason, 0);
    assert_eq!(
        lling_cancellation_v2_request(cancellation, LlingCancellationReasonV2::Deadline.to_raw()),
        LlingStatus::Ok
    );
    assert_eq!(
        lling_cancellation_v2_request(cancellation, LlingCancellationReasonV2::Source.to_raw()),
        LlingStatus::Ok
    );
    assert_eq!(
        lling_cancellation_v2_reason(cancellation, &mut reason),
        LlingStatus::Ok
    );
    assert_eq!(reason, LlingCancellationReasonV2::Deadline.to_raw());

    assert_eq!(
        lling_cancellation_v2_free(&mut cancellation),
        LlingStatus::Ok
    );
    assert!(cancellation.is_null());
    assert_eq!(
        lling_cancellation_v2_free(&mut cancellation),
        LlingStatus::Closed
    );
    assert_eq!(
        lling_cancellation_v2_free(ptr::null_mut()),
        LlingStatus::NullPointer
    );
}

// INVARIANT-HOOK: LLING-ABI2-RAW-1
#[test]
fn v2_cancellation_rejects_unknown_reasons_without_changing_state() {
    let mut cancellation: *mut LlingCancellationV2 = ptr::null_mut();
    assert_eq!(
        lling_cancellation_v2_new(&mut cancellation),
        LlingStatus::Ok
    );
    assert_eq!(
        lling_cancellation_v2_request(cancellation, 99),
        LlingStatus::InvalidArgument
    );
    let mut reason = u32::MAX;
    assert_eq!(
        lling_cancellation_v2_reason(cancellation, &mut reason),
        LlingStatus::Ok
    );
    assert_eq!(reason, 0);
    assert_eq!(
        lling_cancellation_v2_free(&mut cancellation),
        LlingStatus::Ok
    );
}

// INVARIANT-HOOK: LLING-ABI2-TERM-1
#[test]
fn v2_concurrent_cancellation_preserves_one_first_reason() {
    let mut cancellation: *mut LlingCancellationV2 = ptr::null_mut();
    assert_eq!(
        lling_cancellation_v2_new(&mut cancellation),
        LlingStatus::Ok
    );
    let address = cancellation as usize;
    let threads: Vec<_> = [
        LlingCancellationReasonV2::Requested,
        LlingCancellationReasonV2::Deadline,
        LlingCancellationReasonV2::Budget,
        LlingCancellationReasonV2::Source,
    ]
    .into_iter()
    .map(|reason| {
        std::thread::spawn(move || {
            let cancellation = address as *const LlingCancellationV2;
            lling_cancellation_v2_request(cancellation, reason.to_raw())
        })
    })
    .collect();
    for thread in threads {
        assert_eq!(
            thread.join().expect("request thread did not panic"),
            LlingStatus::Ok
        );
    }

    let mut first = 0;
    assert_eq!(
        lling_cancellation_v2_reason(cancellation, &mut first),
        LlingStatus::Ok
    );
    assert!(LlingCancellationReasonV2::from_raw(first).is_some());
    assert_eq!(
        lling_cancellation_v2_request(cancellation, LlingCancellationReasonV2::Requested.to_raw(),),
        LlingStatus::Ok
    );
    let mut after = 0;
    assert_eq!(
        lling_cancellation_v2_reason(cancellation, &mut after),
        LlingStatus::Ok
    );
    assert_eq!(after, first);
    assert_eq!(
        lling_cancellation_v2_free(&mut cancellation),
        LlingStatus::Ok
    );
}

#[test]
fn v2_cancellation_constructor_never_overwrites_a_live_slot() {
    let mut cancellation = std::ptr::NonNull::<LlingCancellationV2>::dangling().as_ptr();
    let original = cancellation;
    assert_eq!(
        lling_cancellation_v2_new(&mut cancellation),
        LlingStatus::InvalidArgument
    );
    assert_eq!(cancellation, original);
}
