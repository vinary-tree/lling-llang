use proptest::prelude::*;
use vinary_analysis_graph::{
    ClaimStrength, DialectConformance, EpistemicAxes, JsonlBuilder, RelationNode, RoleEdge,
};

fn graph_contract() {
    let _ = EpistemicAxes::unknown();
    let _ = DialectConformance::neutral();
    let _ = JsonlBuilder::bounded(1);
}

proptest! {
    #[test]
    fn prop_graph_epistemic_axes_are_orthogonal(rank in 0_u8..4) {
        graph_contract();
        let axes = EpistemicAxes::from_strength_rank(rank);
        prop_assert_eq!(axes.with_completion(axes.completion()).strength(), axes.strength());
    }
    #[test]
    fn prop_relation_lowering_preserves_every_role(roles in proptest::collection::vec((any::<u32>(), any::<u32>()), 1..32)) {
        graph_contract();
        let edges: Vec<_> = roles.iter().map(|(role, target)| RoleEdge::new(*role, *target)).collect();
        let relation = RelationNode::new(1, edges.clone()).expect("non-empty relation roles");
        prop_assert_eq!(relation.lower_binary_edges(), edges);
    }
    #[test]
    fn prop_empty_neutral_dialect_requires_no_application_fields(fields in proptest::collection::vec(any::<u32>(), 0..32)) {
        graph_contract();
        prop_assert!(DialectConformance::neutral().accepts_fields(&fields));
    }
    #[test]
    fn prop_stale_graph_patch_is_atomic(base in any::<u64>(), stale in any::<u64>()) {
        graph_contract();
        prop_assume!(base != stale);
        let graph = EpistemicAxes::test_graph(base);
        let before = graph.clone();
        prop_assert!(graph.apply_test_patch(stale).is_err());
        prop_assert_eq!(graph, before);
    }
    #[test]
    fn prop_projection_never_strengthens(source in 0_u8..4, requested in 0_u8..4) {
        graph_contract();
        let projected = ClaimStrength::from_rank(source).project(ClaimStrength::from_rank(requested));
        prop_assert!(projected.rank() <= source);
    }
    #[test]
    fn prop_jsonl_limit_exhaustion_is_not_completion(records in 2_usize..256) {
        graph_contract();
        prop_assert!(!JsonlBuilder::bounded(1).ingest_test_records(records).is_complete());
    }
    #[test]
    fn prop_projection_never_strengthens_lifecycle(source in 0_u8..4, requested in 0_u8..4) {
        graph_contract();
        prop_assert!(ClaimStrength::from_rank(source).project(ClaimStrength::from_rank(requested)).rank() <= source);
    }
    #[test]
    fn prop_patch_commit_requires_matching_base(base in any::<u64>(), patch in any::<u64>()) {
        graph_contract();
        prop_assert_eq!(EpistemicAxes::test_graph(base).apply_test_patch(patch).is_ok(), base == patch);
    }
    #[test]
    fn prop_e9_nf_smt_projection_nonstrengthening(source in 0_u8..4, requested in 0_u8..4) {
        graph_contract();
        prop_assert!(ClaimStrength::from_rank(source).project(ClaimStrength::from_rank(requested)).rank() <= source);
    }
    #[test]
    fn prop_e9_nf_smt_patch_base_gate(base in any::<u64>(), patch in any::<u64>()) {
        graph_contract();
        prop_assert_eq!(EpistemicAxes::test_graph(base).apply_test_patch(patch).is_ok(), base == patch);
    }
}
