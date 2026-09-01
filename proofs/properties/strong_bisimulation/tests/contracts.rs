//! Properties extracted from the formally verified strong-bisimulation model.

use lling_llang::symbolic::bisimulation::{CertifiedBisimulation, Lts};
use proptest::prelude::*;

type Edge = (usize, u32, usize);

fn oracle(num_states: usize, edges: &[Edge], colors: &[usize]) -> Vec<bool> {
    let mut relation = vec![false; num_states * num_states];
    for left in 0..num_states {
        for right in 0..num_states {
            relation[left * num_states + right] = colors[left] == colors[right];
        }
    }
    loop {
        let mut next = relation.clone();
        for left in 0..num_states {
            for right in 0..num_states {
                let index = left * num_states + right;
                if relation[index]
                    && (!transfers(
                        num_states,
                        edges,
                        &relation,
                        left,
                        right,
                    ) || !transfers(
                        num_states,
                        edges,
                        &relation,
                        right,
                        left,
                    ))
                {
                    next[index] = false;
                }
            }
        }
        if next == relation {
            return relation;
        }
        relation = next;
    }
}

fn transfers(
    num_states: usize,
    edges: &[Edge],
    relation: &[bool],
    left: usize,
    right: usize,
) -> bool {
    edges
        .iter()
        .filter(|(source, _, _)| *source == left)
        .all(|(_, label, left_target)| {
            edges.iter().any(|(source, right_label, right_target)| {
                *source == right
                    && label == right_label
                    && relation[left_target * num_states + right_target]
            })
        })
}

fn relation_from_blocks(blocks: &[usize]) -> Vec<bool> {
    let mut relation = Vec::with_capacity(blocks.len() * blocks.len());
    for left in blocks {
        for right in blocks {
            relation.push(left == right);
        }
    }
    relation
}

fn compute(num_states: usize, edges: Vec<Edge>, colors: Vec<usize>) -> CertifiedBisimulation {
    CertifiedBisimulation::compute(&Lts::new(num_states, edges), &colors)
        .expect("generated LTS is valid")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn prop_exhaustive_small_system_matches_independent_fixed_point(
        num_states in 1_usize..5,
        raw_edges in proptest::collection::vec((0_usize..4, 0_u32..3, 0_usize..4), 0..20),
        raw_colors in proptest::collection::vec(0_usize..3, 1..5),
    ) {
        let edges: Vec<_> = raw_edges.into_iter()
            .filter(|(source, _, target)| *source < num_states && *target < num_states)
            .collect();
        let colors: Vec<_> = (0..num_states)
            .map(|state| raw_colors[state % raw_colors.len()])
            .collect();
        let result = compute(num_states, edges.clone(), colors.clone());
        prop_assert_eq!(
            relation_from_blocks(result.blocks()),
            oracle(num_states, &edges, &colors),
        );
    }

    #[test]
    fn prop_malformed_source_is_rejected(
        num_states in 1_usize..64,
        label in any::<u32>(),
        target in 0_usize..64,
    ) {
        let target = target % num_states;
        let lts = Lts::new(num_states, vec![(num_states, label, target)]);
        prop_assert!(CertifiedBisimulation::compute(&lts, &vec![0; num_states]).is_err());
    }

    #[test]
    fn prop_malformed_target_is_rejected(
        num_states in 1_usize..64,
        label in any::<u32>(),
        source in 0_usize..64,
    ) {
        let source = source % num_states;
        let lts = Lts::new(num_states, vec![(source, label, num_states)]);
        prop_assert!(CertifiedBisimulation::compute(&lts, &vec![0; num_states]).is_err());
    }

    #[test]
    fn prop_color_vector_length_is_total(
        num_states in 1_usize..64,
        wrong_length in 0_usize..64,
    ) {
        prop_assume!(num_states != wrong_length);
        let lts = Lts::new(num_states, vec![]);
        prop_assert!(CertifiedBisimulation::compute(&lts, &vec![0; wrong_length]).is_err());
    }

    #[test]
    fn prop_transition_permutation_preserves_canonical_relation(
        num_states in 1_usize..16,
        raw_edges in proptest::collection::vec((0_usize..16, 0_u32..4, 0_usize..16), 0..64),
    ) {
        let edges: Vec<_> = raw_edges.into_iter()
            .filter(|(source, _, target)| *source < num_states && *target < num_states)
            .collect();
        let mut reverse = edges.clone();
        reverse.reverse();
        let colors = vec![0; num_states];
        let forward = compute(num_states, edges, colors.clone());
        let backward = compute(num_states, reverse, colors);
        prop_assert_eq!(forward.relation_matrix(), backward.relation_matrix());
    }

    #[test]
    fn prop_duplicate_transitions_preserve_canonical_relation(
        num_states in 1_usize..16,
        raw_edges in proptest::collection::vec((0_usize..16, 0_u32..4, 0_usize..16), 0..64),
    ) {
        let edges: Vec<_> = raw_edges.into_iter()
            .filter(|(source, _, target)| *source < num_states && *target < num_states)
            .collect();
        let mut duplicates = edges.clone();
        duplicates.extend(edges.iter().copied());
        let colors = vec![0; num_states];
        let unique = compute(num_states, edges, colors.clone());
        let repeated = compute(num_states, duplicates, colors);
        prop_assert_eq!(unique.relation_matrix(), repeated.relation_matrix());
    }

    #[test]
    fn prop_injective_label_relabeling_preserves_relation(
        num_states in 1_usize..16,
        raw_edges in proptest::collection::vec((0_usize..16, 0_u32..4, 0_usize..16), 0..64),
    ) {
        let edges: Vec<_> = raw_edges.into_iter()
            .filter(|(source, _, target)| *source < num_states && *target < num_states)
            .collect();
        let relabeled: Vec<_> = edges.iter()
            .map(|(source, label, target)| (*source, label * 2 + 17, *target))
            .collect();
        let colors = vec![0; num_states];
        let original = compute(num_states, edges, colors.clone());
        let renamed = compute(num_states, relabeled, colors);
        prop_assert_eq!(original.relation_matrix(), renamed.relation_matrix());
    }

    #[test]
    fn prop_certificate_replay_reconstructs_exact_partition(
        num_states in 1_usize..16,
        raw_edges in proptest::collection::vec((0_usize..16, 0_u32..4, 0_usize..16), 0..64),
    ) {
        let edges: Vec<_> = raw_edges.into_iter()
            .filter(|(source, _, target)| *source < num_states && *target < num_states)
            .collect();
        let colors = vec![0; num_states];
        let lts = Lts::new(num_states, edges);
        let result = CertifiedBisimulation::compute(&lts, &colors).unwrap();
        let replayed = result.certificate().replay(&lts, &colors).unwrap();
        prop_assert_eq!(replayed, result.blocks());
    }

    #[test]
    fn prop_non_equivalent_pair_has_sound_distinguishing_witness(
        num_states in 2_usize..16,
        raw_edges in proptest::collection::vec((0_usize..16, 0_u32..4, 0_usize..16), 0..64),
        left in 0_usize..16,
        right in 0_usize..16,
    ) {
        let edges: Vec<_> = raw_edges.into_iter()
            .filter(|(source, _, target)| *source < num_states && *target < num_states)
            .collect();
        let colors = vec![0; num_states];
        let lts = Lts::new(num_states, edges);
        let result = CertifiedBisimulation::compute(&lts, &colors).unwrap();
        let left = left % num_states;
        let right = right % num_states;
        prop_assume!(result.blocks()[left] != result.blocks()[right]);
        let witness = result.witness(left, right).expect("separated pair needs witness");
        prop_assert_ne!(
            witness.evaluate(&lts, &colors, left).unwrap(),
            witness.evaluate(&lts, &colors, right).unwrap(),
        );
    }

    #[test]
    fn prop_adversarial_discrete_partition_has_no_whole_rescan(
        num_states in 1_usize..4096,
    ) {
        let colors: Vec<_> = (0..num_states).collect();
        let result = compute(num_states, vec![], colors);
        prop_assert_eq!(result.resources().whole_partition_rescans(), 0);
        prop_assert_eq!(result.resources().maximum_native_frames(), 1);
    }

    #[test]
    fn prop_resource_account_respects_quasilinear_work_and_linear_heap(
        num_states in 1_usize..1024,
        raw_edges in proptest::collection::vec((0_usize..1024, 0_u32..8, 0_usize..1024), 0..4096),
    ) {
        let edges: Vec<_> = raw_edges.into_iter()
            .filter(|(source, _, target)| *source < num_states && *target < num_states)
            .collect();
        let edge_count = edges.len();
        let result = compute(num_states, edges, vec![0; num_states]);
        let logarithm =
            usize::BITS as usize - 1 - num_states.leading_zeros() as usize;
        prop_assert!(result.resources().charged_work()
            <= (num_states + edge_count) * logarithm);
        prop_assert!(result.resources().core_heap_cells()
            <= 12 * (num_states + edge_count) + 16);
    }
}

#[test]
fn prop_empty_lts_is_valid_and_canonical() {
    let result = CertifiedBisimulation::compute(&Lts::new(0, vec![]), &[])
        .expect("the empty LTS has a unique empty bisimulation");
    assert!(result.blocks().is_empty());
    assert!(result.relation_matrix().is_empty());
    assert_eq!(result.resources().maximum_native_frames(), 1);
}

#[test]
fn prop_deep_chain_is_stack_safe_on_small_native_stack() {
    const STATES: usize = 100_000;
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let edges: Vec<_> = (0..STATES - 1)
                .map(|source| (source, 0, source + 1))
                .collect();
            let result = compute(STATES, edges, vec![0; STATES]);
            assert_eq!(result.resources().maximum_native_frames(), 1);
        })
        .expect("spawn small-stack test thread")
        .join()
        .expect("certified bisimulation must not overflow the native stack");
}
