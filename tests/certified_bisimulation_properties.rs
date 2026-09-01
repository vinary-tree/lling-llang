//! Production properties for optimal certified strong bisimulation.

use lling_llang::symbolic::bisimulation::{CertifiedBisimulation, Lts};
use proptest::prelude::*;

type Edge = (usize, u32, usize);

fn valid_edges(num_states: usize, raw: Vec<Edge>) -> Vec<Edge> {
    raw.into_iter()
        .filter(|(source, _, target)| *source < num_states && *target < num_states)
        .collect()
}

fn colors(num_states: usize, raw: &[usize]) -> Vec<usize> {
    (0..num_states)
        .map(|state| raw[state % raw.len()])
        .collect()
}

fn floor_log2(value: usize) -> usize {
    usize::BITS as usize - 1 - value.leading_zeros() as usize
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn every_certificate_layer_and_characteristic_formula_is_exact(
        num_states in 1_usize..13,
        raw_edges in proptest::collection::vec((0_usize..12, any::<u32>(), 0_usize..12), 0..96),
        raw_colors in proptest::collection::vec(0_usize..4, 1..13),
    ) {
        let edges = valid_edges(num_states, raw_edges);
        let colors = colors(num_states, &raw_colors);
        let lts = Lts::new(num_states, edges.clone());
        let result = CertifiedBisimulation::compute(&lts, &colors).unwrap();

        prop_assert_eq!(
            result.certificate().replay(&lts, &colors).unwrap(),
            result.blocks(),
        );

        for left in 0..num_states {
            if let Some(right) = (0..num_states)
                .find(|&right| result.blocks()[right] != result.blocks()[left])
            {
                let witness = result.try_witness(left, right).unwrap().unwrap();
                for state in 0..num_states {
                    prop_assert_eq!(
                        witness.evaluate(&lts, &colors, state).unwrap(),
                        result.blocks()[state] == result.blocks()[left],
                    );
                }
            }
        }

        let mut reversed = edges.clone();
        reversed.reverse();
        let reversed = CertifiedBisimulation::compute(
            &Lts::new(num_states, reversed),
            &colors,
        )
        .unwrap();
        prop_assert_eq!(result.blocks(), reversed.blocks());
        prop_assert_eq!(result.certificate(), reversed.certificate());

        let mut duplicated = edges.clone();
        duplicated.extend_from_slice(&edges);
        let duplicated = CertifiedBisimulation::compute(
            &Lts::new(num_states, duplicated),
            &colors,
        )
        .unwrap();
        prop_assert_eq!(result.blocks(), duplicated.blocks());
        prop_assert_eq!(result.certificate(), duplicated.certificate());

        let resources = result.resources();
        let logarithm = floor_log2(num_states);
        prop_assert!(
            resources
                .state_charge_counts()
                .iter()
                .all(|&charges| charges <= logarithm)
        );
        prop_assert!(
            resources
                .transition_charge_counts()
                .iter()
                .all(|&charges| charges <= logarithm)
        );
        let canonical_transitions = resources.transition_charge_counts().len();
        prop_assert!(
            resources.witness_dag_cells()
                <= 16 * (num_states + canonical_transitions) + 16
        );
    }
}
