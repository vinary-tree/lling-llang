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
//! Status pins are exact: the `BindingError -> LlingLlangStatus` mapping is part
//! of the ABI contract (`bindings/api.json`), so each arm asserts the precise
//! status, and where the composition layer surfaces the same defect through
//! the raw wire the expected `VtStatus::…::to_raw()` value is pinned too.
//!
//! Formal-model correspondence (invariant registry owned by the coordinator):
//! - `// INVARIANT-HOOK: LLING-BRIDGE-4` — values outside the advertised
//!   carrier are rejected at every ABI ingestion path: import AND lazy
//!   composition expansion surface ProviderError, never a silent NaN weight.
//! - `// INVARIANT-HOOK: LLING-BRIDGE-2` — composition requires equal unit and
//!   weight domains, while import preserves every supported scalar domain.
#![cfg(feature = "ffi")]

mod support;

use lling_llang::ffi::{
    lling_last_error_message, lling_resource_release, lling_wfst_closure, lling_wfst_closure_plus,
    lling_wfst_compose, lling_wfst_concat, lling_wfst_free, lling_wfst_import,
    lling_wfst_project_input, lling_wfst_project_output, lling_wfst_resource, lling_wfst_reverse,
    lling_wfst_union, LlingBudgetV2, LlingLlangStatus, LlingWfst, LLING_ABI_V2,
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

#[test]
fn budgeted_rational_and_unary_imports_reject_malformed_pages_without_leaks() {
    type Unary =
        extern "C" fn(VtResource, *const LlingBudgetV2, *mut *mut LlingWfst) -> LlingLlangStatus;
    type Binary = extern "C" fn(
        VtResource,
        VtResource,
        *const LlingBudgetV2,
        *mut *mut LlingWfst,
    ) -> LlingLlangStatus;
    let malformed = TestWfst::new(
        chain_states(&[('a', 'x')], 1.0, 0.0),
        0,
        TestWfstConfig::default().with_misbehavior(Misbehavior::OvershootWritten),
    );
    let clean = clean_provider();
    let malformed_metrics = malformed.metrics();
    let clean_metrics = clean.metrics();
    let mut budget = LlingBudgetV2::default();
    budget.header.struct_size = std::mem::size_of::<LlingBudgetV2>() as u32;
    budget.header.abi_version = LLING_ABI_V2;
    let sentinel = ptr::dangling_mut::<LlingWfst>();

    for (name, operation) in [
        ("union", lling_wfst_union as Binary),
        ("concat", lling_wfst_concat as Binary),
    ] {
        for (first, second) in [
            (malformed.as_raw(), clean.as_raw()),
            (clean.as_raw(), malformed.as_raw()),
        ] {
            let mut output = sentinel;
            assert_eq!(
                operation(first, second, &budget, &mut output),
                LlingLlangStatus::ProviderError,
                "{name} must reject a malformed page in either operand"
            );
            assert_eq!(output, sentinel, "{name} must not publish a result");
        }
    }
    for (name, operation) in [
        ("project input", lling_wfst_project_input as Unary),
        ("project output", lling_wfst_project_output as Unary),
        ("reverse", lling_wfst_reverse as Unary),
        ("closure", lling_wfst_closure as Unary),
        ("closure plus", lling_wfst_closure_plus as Unary),
    ] {
        let mut output = sentinel;
        assert_eq!(
            operation(malformed.as_raw(), &budget, &mut output),
            LlingLlangStatus::ProviderError,
            "{name} must reject a malformed page"
        );
        assert_eq!(output, sentinel, "{name} must not publish a result");
    }

    assert!(malformed_metrics.snapshots() > 0);
    assert!(clean_metrics.snapshots() > 0);
    drop(malformed);
    drop(clean);
    assert_eq!(malformed_metrics.balance(), 0, "malformed provider leak");
    assert_eq!(clean_metrics.balance(), 0, "valid operand leak");
}

/// Assert that importing `resource` fails with `expected`, leaving the
/// out-pointer untouched, and that the error message mentions `fragment`.
fn assert_import_rejected(resource: VtResource, expected: LlingLlangStatus, fragment: &str) {
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

fn assert_import_accepted(resource: VtResource) {
    let mut out: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_import(resource, &mut out),
        LlingLlangStatus::Ok,
        "import must succeed (error: {})",
        last_error()
    );
    assert!(!out.is_null(), "successful import must write a handle");
    unsafe { lling_wfst_free(out) };
}

#[test]
fn null_resources_report_null_pointer() {
    let mut out: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_import(VtResource::NULL, &mut out),
        LlingLlangStatus::NullPointer
    );
    assert!(out.is_null());

    let clean = clean_provider();
    assert_eq!(
        lling_wfst_compose(VtResource::NULL, clean.as_raw(), &mut out),
        LlingLlangStatus::NullPointer
    );
    assert!(out.is_null());
    assert_eq!(
        lling_wfst_compose(clean.as_raw(), VtResource::NULL, &mut out),
        LlingLlangStatus::NullPointer
    );
    assert!(out.is_null());
    // A half-null resource (context without vtable) is the same class.
    let half = VtResource {
        context: clean.as_raw().context,
        vtable: ptr::null(),
    };
    assert_eq!(
        lling_wfst_import(half, &mut out),
        LlingLlangStatus::NullPointer
    );

    let metrics = clean.metrics();
    drop(clean);
    assert_eq!(metrics.balance(), 0, "no retain may leak from rejections");
}

#[test]
fn dictionary_resource_is_incompatible() {
    let dictionary = TestDictionaryResource::new();
    assert_import_rejected(
        dictionary.as_raw(),
        LlingLlangStatus::IncompatibleResource,
        "no scalar WFST interface",
    );

    // Both composition operands are validated.
    let clean = clean_provider();
    let mut out: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_compose(dictionary.as_raw(), clean.as_raw(), &mut out),
        LlingLlangStatus::IncompatibleResource
    );
    assert!(out.is_null());
    assert_eq!(
        lling_wfst_compose(clean.as_raw(), dictionary.as_raw(), &mut out),
        LlingLlangStatus::IncompatibleResource
    );
    assert!(out.is_null());

    let metrics = clean.metrics();
    drop(clean);
    assert_eq!(metrics.balance(), 0, "no retain may leak from rejections");
}

// INVARIANT-HOOK: LLING-BRIDGE-2 — import preserves every scalar domain, but
// composition rejects operands whose weight domains differ.
#[test]
fn every_weight_domain_imports_but_mismatched_composition_is_incompatible() {
    for domain in [
        VtWeightDomain::LogF64,
        VtWeightDomain::ProbabilityF64,
        VtWeightDomain::ArcticF64,
        VtWeightDomain::SignedTropicalF64,
        VtWeightDomain::CountF64,
        VtWeightDomain::BooleanF64,
    ] {
        let mut states = chain_states(&[('a', 'x')], 1.0, 0.0);
        states[0].final_weight = match domain {
            VtWeightDomain::TropicalF64
            | VtWeightDomain::LogF64
            | VtWeightDomain::SignedTropicalF64 => f64::INFINITY,
            VtWeightDomain::ArcticF64 => f64::NEG_INFINITY,
            VtWeightDomain::ProbabilityF64
            | VtWeightDomain::CountF64
            | VtWeightDomain::BooleanF64 => 0.0,
        };
        let provider = TestWfst::new(
            states,
            0,
            TestWfstConfig::default().with_weight_domain(domain),
        );
        assert_import_accepted(provider.as_raw());

        let clean = clean_provider();
        let mut out: *mut LlingWfst = ptr::null_mut();
        assert_eq!(
            lling_wfst_compose(provider.as_raw(), clean.as_raw(), &mut out),
            LlingLlangStatus::IncompatibleResource,
            "compose must refuse mismatched {domain:?}/tropical operands"
        );
        assert_eq!(
            lling_wfst_compose(clean.as_raw(), provider.as_raw(), &mut out),
            LlingLlangStatus::IncompatibleResource,
            "compose must refuse mismatched tropical/{domain:?} operands"
        );

        let metrics = provider.metrics();
        drop(provider);
        assert_eq!(metrics.balance(), 0, "domain rejection must not leak");
    }
}

#[test]
fn every_unit_domain_imports_but_mismatched_composition_is_incompatible() {
    for domain in [VtUnitDomain::Byte, VtUnitDomain::U64] {
        let provider = TestWfst::new(
            chain_states(&[('a', 'x')], 1.0, 0.0),
            0,
            TestWfstConfig::default().with_unit_domain(domain),
        );
        assert_import_accepted(provider.as_raw());
        let clean = clean_provider();
        let mut out: *mut LlingWfst = ptr::null_mut();
        assert_eq!(
            lling_wfst_compose(provider.as_raw(), clean.as_raw(), &mut out),
            LlingLlangStatus::IncompatibleResource
        );
        assert!(out.is_null());
        assert!(last_error().contains("label domain"));
    }
}

// INVARIANT-HOOK: LLING-BRIDGE-4 — NaN and -inf (the F1 shape) are rejected
// as provider errors on the import path, at every weight position.
#[test]
fn invalid_tropical_weights_reject_at_import() {
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
            LlingLlangStatus::ProviderError,
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
            LlingLlangStatus::ProviderError,
            "invalid state_info fields",
        );
    }
}

// INVARIANT-HOOK: LLING-BRIDGE-4 — the same poison at the LAZY COMPOSITION
// layer: compose succeeds (nothing expanded yet), and the poisoned product
// state then surfaces ProviderError through the raw wire — never a silent
// NaN arc weight (the F1 regression at the composition layer).
#[test]
fn invalid_tropical_weights_reject_during_composition_expansion() {
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
            LlingLlangStatus::Ok,
            "lazy compose must succeed before expansion"
        );
        let mut resource = VtResource::NULL;
        assert_eq!(
            unsafe { lling_wfst_resource(composed, &mut resource) },
            LlingLlangStatus::Ok
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

        // The provider violates its advertised Unicode domain. That is
        // malformed provider output rather than resource size exhaustion.
        let provider = TestWfst::tropical(states.clone(), 0);
        assert_import_rejected(
            provider.as_raw(),
            LlingLlangStatus::ProviderError,
            "declared domain",
        );

        // Lazy composition reports the same malformed-provider class through
        // the family status wire when expansion first reaches the bad arc.
        let poisoned = TestWfst::tropical(states, 0);
        let clean = clean_provider();
        let mut composed: *mut LlingWfst = ptr::null_mut();
        assert_eq!(
            lling_wfst_compose(poisoned.as_raw(), clean.as_raw(), &mut composed),
            LlingLlangStatus::Ok
        );
        let mut resource = VtResource::NULL;
        assert_eq!(
            unsafe { lling_wfst_resource(composed, &mut resource) },
            LlingLlangStatus::Ok
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
                "label {bad_label:#x} must surface as ProviderError on composition expansion"
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
        LlingLlangStatus::ProviderError,
        "invalid arc fields",
    );

    let poisoned = TestWfst::tropical(states, 0);
    let clean = clean_provider();
    let mut composed: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_compose(poisoned.as_raw(), clean.as_raw(), &mut composed),
        LlingLlangStatus::Ok
    );
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { lling_wfst_resource(composed, &mut resource) },
        LlingLlangStatus::Ok
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
        LlingLlangStatus::ProviderError,
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
        LlingLlangStatus::ProviderError,
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
        LlingLlangStatus::ProviderError,
        "out-of-range status",
    );

    let arcs_liar = TestWfst::new(
        chain_states(&[('a', 'x')], 1.0, 0.0),
        0,
        TestWfstConfig::default().with_misbehavior(Misbehavior::StateArcsStatus(4242)),
    );
    assert_import_rejected(
        arcs_liar.as_raw(),
        LlingLlangStatus::ProviderError,
        "out-of-range status",
    );
}

#[test]
fn in_range_provider_failures_are_forwarded() {
    // The direct project ABI has dedicated limit/closed outcomes; other
    // provider failures retain their detail in the error message. This differs
    // from the raw interop exporter, which can return every VtStatus unchanged.
    for (wire, direct) in [
        (VtStatus::End, LlingLlangStatus::ProviderError),
        (VtStatus::InvalidArgument, LlingLlangStatus::ProviderError),
        (VtStatus::NullPointer, LlingLlangStatus::ProviderError),
        (VtStatus::Unsupported, LlingLlangStatus::ProviderError),
        (VtStatus::IoError, LlingLlangStatus::ProviderError),
        (VtStatus::Closed, LlingLlangStatus::Closed),
        (VtStatus::LimitExceeded, LlingLlangStatus::LimitExceeded),
        (VtStatus::ProviderError, LlingLlangStatus::ProviderError),
        (VtStatus::BatchInUse, LlingLlangStatus::ProviderError),
    ] {
        for failure in [
            Misbehavior::StateInfoStatus(wire.to_raw()),
            Misbehavior::StateArcsStatus(wire.to_raw()),
        ] {
            let provider = TestWfst::new(
                chain_states(&[('a', 'x')], 1.0, 0.0),
                0,
                TestWfstConfig::default().with_misbehavior(failure),
            );
            assert_import_rejected(provider.as_raw(), direct, &format!("{wire:?}"));
        }
    }
}
