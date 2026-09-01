use proptest::prelude::*;
use libcpg::{
    CacheCompatibility, DenseFactIndex, DurableFactKey, ExtractionCoverage,
    ExtractorManifest, FeatureHistory, HistoricalFeatureId, ManifestScenario,
    PortableFactSnapshot, SourceFactEvidence,
};

fn manifest_contract() {
    let _ = ExtractorManifest::zeroed_for_test();
    let _ = DenseFactIndex::<DurableFactKey>::empty_for_test();
    let _ = FeatureHistory::<HistoricalFeatureId>::empty_for_test();
    let _ = PortableFactSnapshot::empty_for_test();
    let _ = SourceFactEvidence::zeroed_for_test();
    let _ = CacheCompatibility::Unknown;
    let _ = ExtractionCoverage::Incomplete;
}

macro_rules! manifest_property {
    ($name:ident) => {
        proptest! {
            #[test]
            fn $name(seed in any::<u64>()) {
                manifest_contract();
                let scenario = ManifestScenario::from_test_seed(seed);
                prop_assert!(scenario.$name());
            }
        }
    };
}

manifest_property!(prop_e7_mf_coq_extraction_dimensions_are_libcpg_owned);
manifest_property!(prop_e7_mf_coq_runtime_dimensions_are_runtime_owned);
manifest_property!(prop_e7_mf_coq_manifest_compatibility_reflexive);
manifest_property!(prop_e7_mf_coq_manifest_compatibility_symmetric);
manifest_property!(prop_e7_mf_coq_manifest_compatibility_transitive);
manifest_property!(prop_e7_mf_coq_repository_mismatch_invalidates);
manifest_property!(prop_e7_mf_coq_parser_mismatch_invalidates);
manifest_property!(prop_e7_mf_coq_grammar_mismatch_invalidates);
manifest_property!(prop_e7_mf_coq_extractor_mismatch_invalidates);
manifest_property!(prop_e7_mf_coq_query_mismatch_invalidates);
manifest_property!(prop_e7_mf_coq_feature_revision_mismatch_invalidates);
manifest_property!(prop_e7_mf_coq_schema_mismatch_invalidates);
manifest_property!(prop_e7_mf_coq_source_identity_mismatch_invalidates);
manifest_property!(prop_e7_mf_coq_source_revision_mismatch_invalidates);
manifest_property!(prop_e7_mf_coq_configuration_mismatch_invalidates);
manifest_property!(prop_e7_mf_coq_cache_reuse_requires_exact_manifest);
manifest_property!(prop_e7_mf_coq_incomplete_entry_is_not_reusable);
manifest_property!(prop_e7_mf_coq_unknown_compatibility_never_reuses);
manifest_property!(prop_e7_mf_coq_incompatible_manifest_never_reuses);
manifest_property!(prop_e7_mf_coq_reuse_requires_compatible_complete);
manifest_property!(prop_e7_mf_coq_rename_preserves_durable_identity);
manifest_property!(prop_e7_mf_coq_source_display_rename_preserves_fact_key);
manifest_property!(prop_e7_mf_coq_every_active_fact_has_dense_id);
manifest_property!(prop_e7_mf_coq_durable_to_dense_to_durable);
manifest_property!(prop_e7_mf_coq_dense_to_durable_to_dense);
manifest_property!(prop_e7_mf_coq_durable_keys_map_injectively);
manifest_property!(prop_e7_mf_coq_dense_ids_have_no_orphans);
manifest_property!(prop_e7_mf_coq_feature_rename_preserves_identity);
manifest_property!(prop_e7_mf_coq_feature_rename_preserves_semantics);
manifest_property!(prop_e7_mf_coq_feature_rename_preserves_status);
manifest_property!(prop_e7_mf_coq_tombstones_are_absorbing);
manifest_property!(prop_e7_mf_coq_historical_feature_ids_are_never_reused);
manifest_property!(prop_e7_mf_coq_tombstoned_feature_cannot_reactivate);
manifest_property!(prop_e7_mf_coq_exact_source_range_is_ordered);
manifest_property!(prop_e7_mf_coq_exact_source_range_is_bounded);
manifest_property!(prop_e7_mf_coq_exact_source_range_length_is_bounded);
manifest_property!(prop_e7_mf_coq_source_mismatch_rejects_evidence);
manifest_property!(prop_e7_mf_coq_source_revision_mismatch_rejects_evidence);
manifest_property!(prop_e7_mf_coq_out_of_bounds_range_rejects_evidence);
manifest_property!(prop_e7_mf_coq_incomplete_nonobservation_is_unknown);
manifest_property!(prop_e7_mf_coq_incomplete_extraction_never_establishes_absence);
manifest_property!(prop_e7_mf_coq_absence_requires_complete_extraction);
manifest_property!(prop_e7_mf_coq_contains_key_correct);
manifest_property!(prop_e7_mf_coq_contains_key_permutation);
manifest_property!(prop_e7_mf_coq_canonical_export_is_insertion_order_invariant);
manifest_property!(prop_e7_mf_coq_canonical_export_is_sound);
manifest_property!(prop_e7_mf_coq_canonical_export_is_complete_over_universe);
manifest_property!(prop_e7_mf_coq_canonical_export_has_no_duplicate_keys);
manifest_property!(prop_e7_mf_coq_canonical_export_bytes_are_deterministic);
manifest_property!(prop_e7_mf_coq_export_step_consumes_one);
manifest_property!(prop_e7_mf_coq_arbitrary_export_run_has_linear_work);
manifest_property!(prop_e7_mf_coq_arbitrary_export_run_is_bounded);
manifest_property!(prop_e7_mf_coq_arbitrary_export_run_has_constant_native_stack);
manifest_property!(prop_e7_mf_coq_export_step_increments_work_once);
manifest_property!(prop_e7_mf_tla_type_ok);
manifest_property!(prop_e7_mf_tla_ownership_is_split);
manifest_property!(prop_e7_mf_tla_rename_preserves_durable_identity);
manifest_property!(prop_e7_mf_tla_semantic_mismatch_invalidates);
manifest_property!(prop_e7_mf_tla_source_revision_mismatch_invalidates);
manifest_property!(prop_e7_mf_tla_tombstone_reactivation_never_accepted);
manifest_property!(prop_e7_mf_tla_tombstoned_features_remain_inactive);
manifest_property!(prop_e7_mf_tla_historical_feature_id_never_reused);
manifest_property!(prop_e7_mf_tla_dense_forward_reverse_correspondence);
manifest_property!(prop_e7_mf_tla_dense_indices_have_no_orphans);
manifest_property!(prop_e7_mf_tla_inactive_facts_have_no_dense_index);
manifest_property!(prop_e7_mf_tla_cache_reuse_requires_manifest_compatibility);
manifest_property!(prop_e7_mf_tla_cache_reuse_requires_complete_extraction);
manifest_property!(prop_e7_mf_tla_unknown_compatibility_never_reuses);
manifest_property!(prop_e7_mf_tla_exact_source_range_required_for_reuse);
manifest_property!(prop_e7_mf_tla_deterministic_export_is_canonical);
manifest_property!(prop_e7_mf_tla_insertion_permutation_does_not_change_export);
manifest_property!(prop_e7_mf_tla_incomplete_never_establishes_absence);
manifest_property!(prop_e7_mf_tla_incomplete_never_produces_accepted_outcome);
manifest_property!(prop_e7_mf_tla_core_dependency_direction_is_independent);
manifest_property!(prop_e7_mf_tla_native_stack_bound_is_input_independent);
manifest_property!(prop_e7_mf_tla_export_work_is_linear);
manifest_property!(prop_e7_mf_tla_terminal_outcome_is_classified);
manifest_property!(prop_e7_mf_tla_eventually_terminal);
manifest_property!(prop_e7_mf_smt_rename_preserves_durable_id);
manifest_property!(prop_e7_mf_smt_parser_mismatch_no_reuse);
manifest_property!(prop_e7_mf_smt_grammar_mismatch_no_reuse);
manifest_property!(prop_e7_mf_smt_feature_revision_mismatch_no_reuse);
manifest_property!(prop_e7_mf_smt_source_revision_mismatch_no_reuse);
manifest_property!(prop_e7_mf_smt_configuration_mismatch_no_reuse);
manifest_property!(prop_e7_mf_smt_unknown_compatibility_no_reuse);
manifest_property!(prop_e7_mf_smt_incomplete_no_reuse);
manifest_property!(prop_e7_mf_smt_invalid_range_no_reuse);
manifest_property!(prop_e7_mf_smt_tombstone_reactivation_no_reuse);
manifest_property!(prop_e7_mf_smt_durable_dense_roundtrip);
manifest_property!(prop_e7_mf_smt_dense_injectivity);
manifest_property!(prop_e7_mf_smt_dense_no_orphans);
manifest_property!(prop_e7_mf_smt_tombstone_not_active);
manifest_property!(prop_e7_mf_smt_historical_id_not_reused);
manifest_property!(prop_e7_mf_smt_deterministic_canonical_export);
manifest_property!(prop_e7_mf_smt_incomplete_no_absence);
manifest_property!(prop_e7_mf_smt_linear_work_bound);
manifest_property!(prop_e7_mf_smt_constant_native_stack);
manifest_property!(prop_e7_mf_smt_valid_cache_witness);
