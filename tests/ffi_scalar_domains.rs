//! Exhaustive scalar-domain conformance for the project-owned C ABI.
//!
//! Every one of the 3 label domains times 7 built-in weight domains crosses
//! builder, immutable resource, import, and lazy composition boundaries. The
//! assertions pin domain metadata and domain-specific multiplication, not only
//! successful handle creation.
#![cfg(feature = "ffi")]

use lling_llang::ffi::{
    lling_resource_release, lling_wfst_builder_add_arc, lling_wfst_builder_add_state,
    lling_wfst_builder_build, lling_wfst_builder_free, lling_wfst_builder_new_for_domains,
    lling_wfst_builder_set_final, lling_wfst_builder_set_start, lling_wfst_compose,
    lling_wfst_free, lling_wfst_import, lling_wfst_resource, LlingLlangStatus, LlingWfst,
    LlingWfstBuilder,
};
use std::ptr;
use vinary_tree_interop::{
    VtResource, VtStatus, VtUnitDomain, VtWeightDomain, VtWfstArc, VtWfstVTable,
    VT_WFST_INTERFACE_ID, VT_WFST_INTERFACE_VERSION,
};

struct BuiltWfst {
    builder: *mut LlingWfstBuilder,
    wfst: *mut LlingWfst,
    resource: VtResource,
}

impl Drop for BuiltWfst {
    fn drop(&mut self) {
        lling_resource_release(self.resource);
        unsafe {
            lling_wfst_free(self.wfst);
            lling_wfst_builder_free(self.builder);
        }
    }
}

fn build_chain(
    unit: VtUnitDomain,
    weight: VtWeightDomain,
    label: u64,
    arc_weight: f64,
    final_weight: f64,
) -> BuiltWfst {
    let mut builder = ptr::null_mut();
    assert_eq!(
        lling_wfst_builder_new_for_domains(unit as u32, weight as u32, &mut builder),
        LlingLlangStatus::Ok
    );
    let mut first = u32::MAX;
    let mut second = u32::MAX;
    assert_eq!(
        lling_wfst_builder_add_state(builder, &mut first),
        LlingLlangStatus::Ok
    );
    assert_eq!(
        lling_wfst_builder_add_state(builder, &mut second),
        LlingLlangStatus::Ok
    );
    assert_eq!(
        lling_wfst_builder_set_start(builder, first),
        LlingLlangStatus::Ok
    );
    assert_eq!(
        lling_wfst_builder_set_final(builder, second, final_weight),
        LlingLlangStatus::Ok
    );
    assert_eq!(
        lling_wfst_builder_add_arc(builder, first, label, 1, label, 1, second, arc_weight,),
        LlingLlangStatus::Ok
    );
    let mut wfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_builder_build(builder, &mut wfst),
        LlingLlangStatus::Ok
    );
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { lling_wfst_resource(wfst, &mut resource) },
        LlingLlangStatus::Ok
    );
    BuiltWfst {
        builder,
        wfst,
        resource,
    }
}

unsafe fn table(resource: VtResource) -> &'static VtWfstVTable {
    let mut interface = ptr::null();
    assert_eq!(
        unsafe {
            (*resource.vtable)
                .query_interface
                .expect("resource query callback")(
                resource.context,
                &VT_WFST_INTERFACE_ID,
                VT_WFST_INTERFACE_VERSION,
                &mut interface,
            )
        },
        VtStatus::Ok.to_raw()
    );
    unsafe { &*interface.cast::<VtWfstVTable>() }
}

unsafe fn only_arc(resource: VtResource, state: u64) -> VtWfstArc {
    let table = unsafe { table(resource) };
    let mut arc = VtWfstArc::default();
    let mut written = 0;
    let mut total = 0;
    assert_eq!(
        unsafe {
            table.state_arcs.expect("state_arcs callback")(
                resource.context,
                state,
                0,
                &mut arc,
                1,
                &mut written,
                &mut total,
            )
        },
        VtStatus::Ok.to_raw()
    );
    assert_eq!((written, total), (1, 1));
    arc
}

unsafe fn final_weight(resource: VtResource, state: u64) -> f64 {
    let table = unsafe { table(resource) };
    let mut valid = 0;
    let mut finality = 0;
    let mut weight = f64::NAN;
    assert_eq!(
        unsafe {
            table.state_info.expect("state_info callback")(
                resource.context,
                state,
                &mut valid,
                &mut finality,
                &mut weight,
            )
        },
        VtStatus::Ok.to_raw()
    );
    assert_eq!((valid, finality), (1, 1));
    weight
}

const UNITS: [(VtUnitDomain, u64); 3] = [
    (VtUnitDomain::Byte, u8::MAX as u64),
    (VtUnitDomain::UnicodeScalar, 'λ' as u64),
    (VtUnitDomain::U64, u64::MAX),
];

const WEIGHTS: [(VtWeightDomain, f64, f64); 7] = [
    (VtWeightDomain::TropicalF64, -2.0, 3.0),
    (VtWeightDomain::LogF64, 2.0, 3.0),
    (VtWeightDomain::ProbabilityF64, 0.5, 0.25),
    (VtWeightDomain::ArcticF64, -2.0, 3.0),
    (VtWeightDomain::SignedTropicalF64, -2.0, 3.0),
    (VtWeightDomain::CountF64, 3.0, 4.0),
    (VtWeightDomain::BooleanF64, 1.0, 0.0),
];

#[test]
fn all_twenty_one_domain_pairs_build_and_import_without_metadata_loss() {
    for (unit, label) in UNITS {
        for (weight, arc_weight, expected_final) in WEIGHTS {
            let original = build_chain(unit, weight, label, arc_weight, expected_final);
            let original_table = unsafe { table(original.resource) };
            assert_eq!(
                (original_table.unit_domain, original_table.weight_domain),
                (unit, weight)
            );
            let arc = unsafe { only_arc(original.resource, 0) };
            assert_eq!(
                (arc.input_label, arc.output_label, arc.weight),
                (label, label, arc_weight)
            );
            assert_eq!(
                unsafe { final_weight(original.resource, 1) },
                expected_final
            );

            let mut imported = ptr::null_mut();
            assert_eq!(
                lling_wfst_import(original.resource, &mut imported),
                LlingLlangStatus::Ok
            );
            let mut imported_resource = VtResource::NULL;
            assert_eq!(
                unsafe { lling_wfst_resource(imported, &mut imported_resource) },
                LlingLlangStatus::Ok
            );
            let imported_table = unsafe { table(imported_resource) };
            assert_eq!(
                (imported_table.unit_domain, imported_table.weight_domain),
                (unit, weight)
            );
            assert_eq!(unsafe { only_arc(imported_resource, 0) }.weight, arc_weight);
            lling_resource_release(imported_resource);
            unsafe { lling_wfst_free(imported) };
        }
    }
}

#[test]
fn composition_uses_each_declared_semirings_multiplication() {
    let cases = [
        (VtWeightDomain::TropicalF64, 2.0, 3.0, 4.0, 5.0, 5.0, 9.0),
        (VtWeightDomain::LogF64, 2.0, 3.0, 4.0, 5.0, 5.0, 9.0),
        (
            VtWeightDomain::ProbabilityF64,
            0.5,
            0.25,
            0.4,
            0.5,
            0.125,
            0.2,
        ),
        (VtWeightDomain::ArcticF64, -2.0, 3.0, -2.0, 1.0, 1.0, -1.0),
        (
            VtWeightDomain::SignedTropicalF64,
            -2.0,
            3.0,
            -4.0,
            5.0,
            1.0,
            1.0,
        ),
        (VtWeightDomain::CountF64, 3.0, 4.0, 5.0, 6.0, 12.0, 30.0),
        (VtWeightDomain::BooleanF64, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0),
    ];
    for (domain, left_arc, right_arc, left_final, right_final, arc_product, final_product) in cases
    {
        let left = build_chain(VtUnitDomain::U64, domain, 9, left_arc, left_final);
        let right = build_chain(VtUnitDomain::U64, domain, 9, right_arc, right_final);
        let mut composed = ptr::null_mut();
        assert_eq!(
            lling_wfst_compose(left.resource, right.resource, &mut composed),
            LlingLlangStatus::Ok
        );
        let mut resource = VtResource::NULL;
        assert_eq!(
            unsafe { lling_wfst_resource(composed, &mut resource) },
            LlingLlangStatus::Ok
        );
        let arc = unsafe { only_arc(resource, 0) };
        assert_eq!(arc.weight, arc_product, "{domain:?} arc multiplication");
        assert_eq!(
            unsafe { final_weight(resource, arc.target_state) },
            final_product,
            "{domain:?} final multiplication"
        );
        lling_resource_release(resource);
        unsafe { lling_wfst_free(composed) };
    }
}

#[test]
fn composition_reports_scalar_carrier_overflow_as_limit_exceeded() {
    for (domain, left_weight, right_weight) in [
        (VtWeightDomain::ProbabilityF64, f64::MAX, 2.0),
        (VtWeightDomain::CountF64, 9_007_199_254_740_992.0, 2.0),
    ] {
        let left = build_chain(VtUnitDomain::U64, domain, 9, left_weight, 1.0);
        let right = build_chain(VtUnitDomain::U64, domain, 9, right_weight, 1.0);
        let mut composed = ptr::null_mut();
        assert_eq!(
            lling_wfst_compose(left.resource, right.resource, &mut composed),
            LlingLlangStatus::Ok
        );
        let mut resource = VtResource::NULL;
        assert_eq!(
            unsafe { lling_wfst_resource(composed, &mut resource) },
            LlingLlangStatus::Ok
        );

        let table = unsafe { table(resource) };
        let mut arc = VtWfstArc::default();
        let mut written = 0;
        let mut total = 0;
        assert_eq!(
            unsafe {
                table.state_arcs.expect("state_arcs callback")(
                    resource.context,
                    0,
                    0,
                    &mut arc,
                    1,
                    &mut written,
                    &mut total,
                )
            },
            VtStatus::LimitExceeded.to_raw(),
            "{domain:?} overflow must not be collapsed into ProviderError"
        );

        lling_resource_release(resource);
        unsafe { lling_wfst_free(composed) };
    }
}

#[test]
fn constructors_reject_unknown_discriminants_and_domain_invalid_values() {
    for (unit, weight) in [(0, 1), (4, 1), (2, 0), (2, 8)] {
        let mut builder = ptr::null_mut();
        assert_eq!(
            lling_wfst_builder_new_for_domains(unit, weight, &mut builder),
            LlingLlangStatus::InvalidArgument
        );
        assert!(builder.is_null());
    }

    for (domain, invalid) in [
        (VtWeightDomain::TropicalF64, f64::NEG_INFINITY),
        (VtWeightDomain::LogF64, f64::NAN),
        (VtWeightDomain::ProbabilityF64, -1.0),
        (VtWeightDomain::ArcticF64, f64::INFINITY),
        (VtWeightDomain::SignedTropicalF64, f64::NEG_INFINITY),
        (VtWeightDomain::CountF64, 0.5),
        (VtWeightDomain::BooleanF64, 0.5),
    ] {
        let mut builder = ptr::null_mut();
        assert_eq!(
            lling_wfst_builder_new_for_domains(3, domain as u32, &mut builder),
            LlingLlangStatus::Ok
        );
        let mut state = 0;
        assert_eq!(
            lling_wfst_builder_add_state(builder, &mut state),
            LlingLlangStatus::Ok
        );
        assert_eq!(
            lling_wfst_builder_set_final(builder, state, invalid),
            LlingLlangStatus::InvalidArgument,
            "{domain:?} must reject {invalid:?}"
        );
        unsafe { lling_wfst_builder_free(builder) };
    }

    let mut open_byte = ptr::null_mut();
    assert_eq!(
        lling_wfst_builder_new_for_domains(1, 1, &mut open_byte),
        LlingLlangStatus::Ok
    );
    let mut state = 0;
    assert_eq!(
        lling_wfst_builder_add_state(open_byte, &mut state),
        LlingLlangStatus::Ok
    );
    assert_eq!(
        lling_wfst_builder_add_arc(open_byte, state, 256, 1, 0, 0, state, 0.0),
        LlingLlangStatus::InvalidArgument
    );
    unsafe { lling_wfst_builder_free(open_byte) };
}
