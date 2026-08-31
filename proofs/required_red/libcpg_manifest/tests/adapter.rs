use proptest::prelude::*;
use vinary_libcpg_adapter::{
    AdapterDependencyBoundary, FactRuleRelation, LoweringCertificate,
    LoweringScenario,
};

fn adapter_contract() {
    let _ = FactRuleRelation::empty_for_test();
    let _ = LoweringCertificate::empty_for_test();
    let _ = AdapterDependencyBoundary::canonical();
}

macro_rules! adapter_property {
    ($name:ident) => {
        proptest! {
            #[test]
            fn $name(seed in any::<u64>()) {
                adapter_contract();
                let scenario = LoweringScenario::from_test_seed(seed);
                prop_assert!(scenario.$name());
            }
        }
    };
}

adapter_property!(prop_e7_mf_coq_lowering_is_adapter_owned);
adapter_property!(prop_e7_mf_coq_generic_rule_identity_is_lling_llang_owned);
adapter_property!(prop_e7_mf_coq_certified_lowering_preserves_every_relation_pair);
adapter_property!(prop_e7_mf_coq_certified_lowering_has_no_provenance_orphans);
adapter_property!(prop_e7_mf_coq_fact_rule_lowering_is_not_forced_functional);
adapter_property!(prop_e7_mf_coq_libcpg_has_no_lling_llang_dependency);
adapter_property!(prop_e7_mf_coq_lling_llang_has_no_libcpg_dependency);
adapter_property!(prop_e7_mf_coq_adapter_is_the_only_composition_boundary);
adapter_property!(prop_e7_mf_coq_runtime_envelope_does_not_reverse_core_dependencies);
adapter_property!(prop_e7_mf_tla_every_lowered_rule_has_source_fact);
adapter_property!(prop_e7_mf_tla_many_to_many_lowering_is_preserved);
adapter_property!(prop_e7_mf_smt_lowering_no_provenance_orphan);
adapter_property!(prop_e7_mf_smt_many_to_many_preserved);
adapter_property!(prop_e7_mf_smt_core_dependency_independence);
adapter_property!(prop_e7_mf_smt_valid_many_to_many_witness);
