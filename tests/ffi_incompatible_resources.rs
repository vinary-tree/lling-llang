//! Rejection matrix for `lling_wfst_import` / `lling_wfst_compose` over
//! incompatible, malformed, or misbehaving foreign resources.
//!
//! Every fixture is an in-repo provider from `tests/support/interop_wfst.rs`
//! (no duallity dependency, per the family placement rule): a minimal
//! `vt.dictionary.v1` resource, scalar-WFST providers with wrong weight/unit
//! domains, and providers whose models carry the exact payloads the F1
//! finding weaponized (-inf and NaN weights), plus call-level protocol lies
//! (overshooting `out_written`, unstable `out_total`, injected raw statuses
//! including values outside the published `VtStatus` range).
//!
//! Status pins are exact: the `BindingError -> LlingStatus` mapping is part
//! of the ABI contract (`bindings/api.json`), so each arm asserts the precise
//! status, and where the composition layer surfaces the same defect through
//! the raw wire the expected `VtStatus::…::to_raw()` value is pinned too.
//!
//! Formal-model correspondence (invariant registry owned by the coordinator):
//! - `// INVARIANT-HOOK: LLING-BRIDGE-4` — non-tropical weights (NaN, -inf)
//!   are rejected at every ABI ingestion path: import AND lazy composition
//!   expansion surface ProviderError, never a silent NaN weight (the F1
//!   regression shape at the composition layer).
//! - `// INVARIANT-HOOK: LLING-BRIDGE-2` — the weight-domain handshake:
//!   resources advertising any scalar domain other than TropicalF64 are
//!   refused before a single weight crosses the bridge.
#![cfg(feature = "ffi")]

mod support;

use lling_llang::ffi::{
    lling_last_error_message, lling_resource_release, lling_wfst_compose, lling_wfst_free,
    lling_wfst_import, lling_wfst_resource, LlingStatus, LlingWfst,
};
use std::ffi::CStr;
use std::ptr;
use support::interop_wfst::{
    chain_states, discover_scalar_wfst, Misbehavior, TestArc, TestDictionaryResource, TestState,
    TestWfst, TestWfstConfig,
};
use vinary_tree_interop::{VtResource, VtStatus, VtUnitDomain, VtWeightDomain, VtWfstArc};

/// Copy this thread's last ABI error message into owned storage.
fn last_error() -> String {
    unsafe { CStr::from_ptr(lling_last_error_message()) }
        .to_string_lossy()
        .into_owned()
}

/// A well-formed one-arc tropical provider: `0 -a:x/1-> 1`, final(1)=0.
fn clean_provider() -> TestWfst {
    TestWfst::tropical(chain_states(&[('a', 'x')], 1.0, 0.0), 0)
}

/// Assert that importing `resource` fails with `expected`, leaving the
/// out-pointer untouched, and that the error message mentions `fragment`.
fn assert_import_rejected(resource: VtResource, expected: LlingStatus, fragment: &str) {
    let mut out: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_import(resource, &mut out),
        expected,
        "import must fail with {expected:?} (error: {})",
        last_error()
    );
    assert!(out.is_null(), "failed import must not write a handle");
    let message = last_error();
    assert!(
        message.contains(fragment),
        "error message {message:?} must mention {fragment:?}"
    );
}

#[test]
fn null_resources_report_null_pointer() {
    let mut out: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_import(VtResource::NULL, &mut out),
        LlingStatus::NullPointer
    );
    assert!(out.is_null());

    let clean = clean_provider();
    assert_eq!(
        lling_wfst_compose(VtResource::NULL, clean.as_raw(), &mut out),
        LlingStatus::NullPointer
    );
    assert!(out.is_null());
    assert_eq!(
        lling_wfst_compose(clean.as_raw(), VtResource::NULL, &mut out),
        LlingStatus::NullPointer
    );
    assert!(out.is_null());
    // A half-null resource (context without vtable) is the same class.
    let half = VtResource {
        context: clean.as_raw().context,
        vtable: ptr::null(),
    };
    assert_eq!(lling_wfst_import(half, &mut out), LlingStatus::NullPointer);

    let metrics = clean.metrics();
    drop(clean);
    assert_eq!(metrics.balance(), 0, "no retain may leak from rejections");
}

#[test]
fn dictionary_resource_is_incompatible() {
    let dictionary = TestDictionaryResource::new();
    assert_import_rejected(
        dictionary.as_raw(),
        LlingStatus::IncompatibleResource,
        "no scalar WFST interface",
    );

    // Both composition operands are validated.
    let clean = clean_provider();
    let mut out: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_compose(dictionary.as_raw(), clean.as_raw(), &mut out),
        LlingStatus::IncompatibleResource
    );
    assert!(out.is_null());
    assert_eq!(
        lling_wfst_compose(clean.as_raw(), dictionary.as_raw(), &mut out),
        LlingStatus::IncompatibleResource
    );
    assert!(out.is_null());

    let metrics = clean.metrics();
    drop(clean);
    assert_eq!(metrics.balance(), 0, "no retain may leak from rejections");
}

// INVARIANT-HOOK: LLING-BRIDGE-2 — the weight-domain handshake refuses every
// non-tropical scalar domain before any weight is ingested.
#[test]
fn wrong_weight_domain_is_incompatible() {
    for domain in [
        VtWeightDomain::LogF64,
        VtWeightDomain::ProbabilityF64,
        VtWeightDomain::ArcticF64,
        VtWeightDomain::SignedTropicalF64,
        VtWeightDomain::CountF64,
        VtWeightDomain::BooleanF64,
    ] {
        let provider = TestWfst::new(
            chain_states(&[('a', 'x')], 1.0, 0.0),
            0,
            TestWfstConfig::default().with_weight_domain(domain),
        );
        assert_import_rejected(
            provider.as_raw(),
            LlingStatus::IncompatibleResource,
            "expected tropical",
        );

        let clean = clean_provider();
        let mut out: *mut LlingWfst = ptr::null_mut();
        assert_eq!(
            lling_wfst_compose(provider.as_raw(), clean.as_raw(), &mut out),
            LlingStatus::IncompatibleResource,
            "compose must refuse a {domain:?} left operand"
        );
        assert_eq!(
            lling_wfst_compose(clean.as_raw(), provider.as_raw(), &mut out),
            LlingStatus::IncompatibleResource,
            "compose must refuse a {domain:?} right operand"
        );

        let metrics = provider.metrics();
        drop(provider);
        assert_eq!(metrics.balance(), 0, "domain rejection must not leak");
    }
}

#[test]
fn wrong_unit_domain_is_incompatible() {
    for domain in [VtUnitDomain::Byte, VtUnitDomain::U64] {
        let provider = TestWfst::new(
            chain_states(&[('a', 'x')], 1.0, 0.0),
            0,
            TestWfstConfig::default().with_unit_domain(domain),
        );
        assert_import_rejected(
            provider.as_raw(),
            LlingStatus::IncompatibleResource,
            "Unicode scalar",
        );
    }
}

// INVARIANT-HOOK: LLING-BRIDGE-4 — NaN and -inf (the F1 shape) are rejected
// as provider errors on the import path, at every weight position.
#[test]
fn non_tropical_weights_reject_at_import() {
    // Arc-weight poison in both invalid shapes.
    for poison in [f64::NEG_INFINITY, f64::NAN] {
        let states = vec![
            TestState::interior(vec![TestArc {
                weight: poison,
                ..TestArc::pair('a', 'x', 1, 0.0)
            }]),
            TestState::accepting(0.0, Vec::new()),
        ];
        let provider = TestWfst::tropical(states, 0);
        assert_import_rejected(
            provider.as_raw(),
            LlingStatus::ProviderError,
            "invalid arc fields",
        );
        let metrics = provider.metrics();
        drop(provider);
        assert_eq!(metrics.balance(), 0, "weight rejection must not leak");
    }

    // Final-weight poison in both invalid shapes.
    for poison in [f64::NEG_INFINITY, f64::NAN] {
        let states = vec![
            TestState::interior(vec![TestArc::pair('a', 'x', 1, 1.0)]),
            TestState::accepting(poison, Vec::new()),
        ];
        let provider = TestWfst::tropical(states, 0);
        assert_import_rejected(
            provider.as_raw(),
            LlingStatus::ProviderError,
            "invalid state_info fields",
        );
    }
}

// INVARIANT-HOOK: LLING-BRIDGE-4 — the same poison at the LAZY COMPOSITION
// layer: compose succeeds (nothing expanded yet), and the poisoned product
// state then surfaces ProviderError through the raw wire — never a silent
// NaN arc weight (the F1 regression at the composition layer).
#[test]
fn non_tropical_weights_reject_during_composition_expansion() {
    for poison in [f64::NEG_INFINITY, f64::NAN] {
        let states = vec![
            TestState::interior(vec![TestArc {
                weight: poison,
                ..TestArc::pair('a', 'x', 1, 0.0)
            }]),
            TestState::accepting(0.0, Vec::new()),
        ];
        let poisoned = TestWfst::tropical(states, 0);
        let clean = clean_provider();

        let mut composed: *mut LlingWfst = ptr::null_mut();
        assert_eq!(
            lling_wfst_compose(poisoned.as_raw(), clean.as_raw(), &mut composed),
            LlingStatus::Ok,
            "lazy compose must succeed before expansion"
        );
        let mut resource = VtResource::NULL;
        assert_eq!(
            unsafe { lling_wfst_resource(composed, &mut resource) },
            LlingStatus::Ok
        );

        unsafe {
            let table = &*discover_scalar_wfst(resource);
            let mut valid = 0;
            let mut is_final = 0;
            let mut final_weight = 0.0;
            assert_eq!(
                table.state_info.expect("state_info published")(
                    resource.context,
                    0,
                    &mut valid,
                    &mut is_final,
                    &mut final_weight,
                ),
                VtStatus::ProviderError.to_raw(),
                "expanding a poisoned product state must fail on the raw wire"
            );

            let mut arc = VtWfstArc::default();
            let mut written = 0;
            let mut total = 0;
            assert_eq!(
                table.state_arcs.expect("state_arcs published")(
                    resource.context,
                    0,
                    0,
                    &mut arc,
                    1,
                    &mut written,
                    &mut total,
                ),
                VtStatus::ProviderError.to_raw(),
                "poisoned expansion must never yield an arc page"
            );
            assert!(
                !arc.weight.is_nan(),
                "no NaN weight may ever be written to the caller's page"
            );
        }

        lling_resource_release(resource);
        unsafe { lling_wfst_free(composed) };
        let poisoned_metrics = poisoned.metrics();
        let clean_metrics = clean.metrics();
        drop(poisoned);
        drop(clean);
        assert_eq!(poisoned_metrics.balance(), 0);
        assert_eq!(clean_metrics.balance(), 0);
    }
}

#[test]
fn label_beyond_char_max_pins_exact_statuses() {
    // Three unrepresentable label shapes: beyond char::MAX, a surrogate,
    // and beyond u32 entirely.
    let beyond_char = u64::from(u32::from(char::MAX)) + 1;
    let surrogate = 0xD800_u64;
    let beyond_u32 = u64::from(u32::MAX) + 1;
    for bad_label in [beyond_char, surrogate, beyond_u32] {
        let states = vec![
            TestState::interior(vec![TestArc {
                input_label: bad_label,
                ..TestArc::pair('a', 'x', 1, 1.0)
            }]),
            TestState::accepting(0.0, Vec::new()),
        ];

        // Import DECODES labels into chars, so the failure is the native
        // representation limit: BindingError::RepresentationLimit ->
        // LLING_STATUS_LIMIT_EXCEEDED.
        let provider = TestWfst::tropical(states.clone(), 0);
        assert_import_rejected(
            provider.as_raw(),
            LlingStatus::LimitExceeded,
            "representation",
        );

        // The composition layer VALIDATES-and-forwards raw labels instead of
        // decoding them, so the same payload surfaces as invalid provider
        // output (ProviderError) when the product state expands. Pinned
        // asymmetry — a candidate for the family-wide F3 harmonization.
        let poisoned = TestWfst::tropical(states, 0);
        let clean = clean_provider();
        let mut composed: *mut LlingWfst = ptr::null_mut();
        assert_eq!(
            lling_wfst_compose(poisoned.as_raw(), clean.as_raw(), &mut composed),
            LlingStatus::Ok
        );
        let mut resource = VtResource::NULL;
        assert_eq!(
            unsafe { lling_wfst_resource(composed, &mut resource) },
            LlingStatus::Ok
        );
        unsafe {
            let table = &*discover_scalar_wfst(resource);
            let mut arc = VtWfstArc::default();
            let mut written = 0;
            let mut total = 0;
            assert_eq!(
                table.state_arcs.expect("state_arcs published")(
                    resource.context,
                    0,
                    0,
                    &mut arc,
                    1,
                    &mut written,
                    &mut total,
                ),
                VtStatus::ProviderError.to_raw(),
                "label {bad_label:#x} must fail composition expansion"
            );
        }
        lling_resource_release(resource);
        unsafe { lling_wfst_free(composed) };
    }
}

#[test]
fn presence_flag_two_rejects_at_both_layers() {
    let states = vec![
        TestState::interior(vec![TestArc {
            has_input: 2,
            ..TestArc::pair('a', 'x', 1, 1.0)
        }]),
        TestState::accepting(0.0, Vec::new()),
    ];

    let provider = TestWfst::tropical(states.clone(), 0);
    assert_import_rejected(
        provider.as_raw(),
        LlingStatus::ProviderError,
        "invalid arc fields",
    );

    let poisoned = TestWfst::tropical(states, 0);
    let clean = clean_provider();
    let mut composed: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_compose(poisoned.as_raw(), clean.as_raw(), &mut composed),
        LlingStatus::Ok
    );
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { lling_wfst_resource(composed, &mut resource) },
        LlingStatus::Ok
    );
    unsafe {
        let table = &*discover_scalar_wfst(resource);
        let mut arc = VtWfstArc::default();
        let mut written = 0;
        let mut total = 0;
        assert_eq!(
            table.state_arcs.expect("state_arcs published")(
                resource.context,
                0,
                0,
                &mut arc,
                1,
                &mut written,
                &mut total,
            ),
            VtStatus::ProviderError.to_raw()
        );
    }
    lling_resource_release(resource);
    unsafe { lling_wfst_free(composed) };
}

#[test]
fn overshooting_out_written_is_rejected() {
    let provider = TestWfst::new(
        chain_states(&[('a', 'x')], 1.0, 0.0),
        0,
        TestWfstConfig::default().with_misbehavior(Misbehavior::OvershootWritten),
    );
    assert_import_rejected(
        provider.as_raw(),
        LlingStatus::ProviderError,
        "invalid arc page counts",
    );
}

#[test]
fn unstable_out_total_is_rejected() {
    let provider = TestWfst::new(
        chain_states(&[('a', 'x')], 1.0, 0.0),
        0,
        TestWfstConfig::default().with_misbehavior(Misbehavior::UnstableOutTotal),
    );
    assert_import_rejected(
        provider.as_raw(),
        LlingStatus::ProviderError,
        "invalid arc page counts",
    );
}

#[test]
fn out_of_range_raw_status_is_a_provider_error_never_ub() {
    // 4242 lies far outside the published VtStatus range: the consumer must
    // decode with VtStatus::from_raw and treat the garbage as a VALUE.
    let info_liar = TestWfst::new(
        chain_states(&[('a', 'x')], 1.0, 0.0),
        0,
        TestWfstConfig::default().with_misbehavior(Misbehavior::StateInfoStatus(4242)),
    );
    assert_import_rejected(
        info_liar.as_raw(),
        LlingStatus::ProviderError,
        "out-of-range status",
    );

    let arcs_liar = TestWfst::new(
        chain_states(&[('a', 'x')], 1.0, 0.0),
        0,
        TestWfstConfig::default().with_misbehavior(Misbehavior::StateArcsStatus(4242)),
    );
    assert_import_rejected(
        arcs_liar.as_raw(),
        LlingStatus::ProviderError,
        "out-of-range status",
    );
}

#[test]
fn in_range_provider_failures_are_forwarded() {
    let io_failure = TestWfst::new(
        chain_states(&[('a', 'x')], 1.0, 0.0),
        0,
        TestWfstConfig::default()
            .with_misbehavior(Misbehavior::StateInfoStatus(VtStatus::IoError.to_raw())),
    );
    assert_import_rejected(io_failure.as_raw(), LlingStatus::ProviderError, "IoError");

    let limit_failure = TestWfst::new(
        chain_states(&[('a', 'x')], 1.0, 0.0),
        0,
        TestWfstConfig::default().with_misbehavior(Misbehavior::StateArcsStatus(
            VtStatus::LimitExceeded.to_raw(),
        )),
    );
    assert_import_rejected(
        limit_failure.as_raw(),
        LlingStatus::ProviderError,
        "LimitExceeded",
    );
}
