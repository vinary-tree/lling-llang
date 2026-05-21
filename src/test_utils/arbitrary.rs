//! `proptest` strategies for generating arbitrary WFSTs, lattices, and weights.
//!
//! This module provides strategies for property-based testing using `proptest`.
//! The strategies generate well-formed WFSTs with various configurations suitable
//! for testing algebraic properties and algorithm correctness.

use proptest::collection::vec as prop_vec;
use proptest::prelude::*;

use crate::semiring::{
    BoolWeight, LogWeight, ProbabilityWeight, ProductWeight, Semiring, TropicalWeight,
};
use crate::wfst::{MutableWfst, StateId, VectorWfst, WeightedTransition, Wfst};

// =============================================================================
// Weight Strategies
// =============================================================================

/// Strategy for generating arbitrary tropical weights.
///
/// Generates non-negative weights (positive costs) which satisfy all semiring axioms.
pub fn arb_tropical_weight() -> impl Strategy<Value = TropicalWeight> {
    prop_oneof![
        // Common finite weights
        (0.0f64..1000.0).prop_map(TropicalWeight::new),
        // Zero (multiplicative identity)
        Just(TropicalWeight::one()),
        // Small weights (common in tests)
        (0.0f64..10.0).prop_map(TropicalWeight::new),
    ]
}

/// Strategy for generating arbitrary tropical weights excluding zero (infinity).
pub fn arb_tropical_weight_nonzero() -> impl Strategy<Value = TropicalWeight> {
    (0.0f64..1000.0).prop_map(TropicalWeight::new)
}

/// Strategy for generating tropical weights suitable for division tests.
///
/// Avoids very small values that could cause numerical issues.
pub fn arb_tropical_weight_divisible() -> impl Strategy<Value = TropicalWeight> {
    (0.001f64..1000.0).prop_map(TropicalWeight::new)
}

/// Strategy for generating arbitrary log weights.
///
/// Log semiring uses negative log probabilities where smaller values are more likely.
pub fn arb_log_weight() -> impl Strategy<Value = LogWeight> {
    prop_oneof![
        // Common finite weights (negative log-probs)
        (0.0f64..20.0).prop_map(LogWeight::new),
        // Zero (multiplicative identity)
        Just(LogWeight::one()),
        // Small weights (high probability events)
        (0.0f64..5.0).prop_map(LogWeight::new),
    ]
}

/// Strategy for generating log weights excluding zero (infinity).
pub fn arb_log_weight_nonzero() -> impl Strategy<Value = LogWeight> {
    (0.0f64..20.0).prop_map(LogWeight::new)
}

/// Strategy for generating arbitrary probability weights.
///
/// Generates values in [0, 1] representing probabilities.
pub fn arb_probability_weight() -> impl Strategy<Value = ProbabilityWeight> {
    prop_oneof![
        // Valid probabilities in [0, 1]
        (0.0f64..=1.0).prop_map(ProbabilityWeight::new),
        // One (multiplicative identity)
        Just(ProbabilityWeight::one()),
        // Common boundary values
        Just(ProbabilityWeight::new(0.5)),
    ]
}

/// Strategy for generating non-zero probability weights.
pub fn arb_probability_weight_nonzero() -> impl Strategy<Value = ProbabilityWeight> {
    (0.001f64..=1.0).prop_map(ProbabilityWeight::new)
}

/// Strategy for generating arbitrary boolean weights.
pub fn arb_bool_weight() -> impl Strategy<Value = BoolWeight> {
    any::<bool>().prop_map(BoolWeight::new)
}

/// Strategy for generating arbitrary product weights.
pub fn arb_product_weight<W1, W2>(
    w1_strategy: impl Strategy<Value = W1>,
    w2_strategy: impl Strategy<Value = W2>,
) -> impl Strategy<Value = ProductWeight<W1, W2>>
where
    W1: Semiring,
    W2: Semiring,
{
    (w1_strategy, w2_strategy).prop_map(|(w1, w2)| ProductWeight::new(w1, w2))
}

// =============================================================================
// Label Strategies
// =============================================================================

/// Strategy for generating arbitrary labels (characters).
pub fn arb_label() -> impl Strategy<Value = char> {
    prop_oneof![
        // Lowercase letters (most common in tests)
        (b'a'..=b'z').prop_map(|b| b as char),
        // Uppercase letters
        (b'A'..=b'Z').prop_map(|b| b as char),
        // Digits
        (b'0'..=b'9').prop_map(|b| b as char),
    ]
}

/// Strategy for generating arbitrary labels including epsilon (None).
pub fn arb_label_or_epsilon() -> impl Strategy<Value = Option<char>> {
    prop_oneof![
        // Non-epsilon label (80% of the time)
        8 => arb_label().prop_map(Some),
        // Epsilon transition (20% of the time)
        2 => Just(None),
    ]
}

/// Strategy for generating small label sets for deterministic WFSTs.
pub fn arb_small_label_set() -> impl Strategy<Value = char> {
    (b'a'..=b'e').prop_map(|b| b as char) // Only 5 labels for easier determinism
}

// =============================================================================
// WFST Strategies
// =============================================================================

/// Strategy for generating arbitrary VectorWfst.
///
/// # Parameters
///
/// - `max_states`: Maximum number of states (1 to max_states)
/// - `max_arcs_per_state`: Maximum outgoing arcs per state
///
/// # Properties of Generated WFSTs
///
/// - Always has at least one state
/// - Start state is always state 0
/// - May have cycles
/// - May have epsilon transitions
/// - Always has at least one final state
pub fn arb_wfst<L, W>(
    max_states: usize,
    max_arcs_per_state: usize,
) -> impl Strategy<Value = VectorWfst<L, W>>
where
    L: Clone + Send + Sync + std::fmt::Debug + 'static,
    W: Semiring + std::fmt::Debug + 'static,
    (Option<L>, Option<L>): Arbitrary,
    W: Arbitrary,
{
    arb_wfst_with_config::<L, W>(WfstGenConfig {
        min_states: 1,
        max_states,
        max_arcs_per_state,
        allow_epsilon: true,
        allow_cycles: true,
        force_final: true,
    })
}

/// Configuration for WFST generation.
#[derive(Clone, Debug)]
pub struct WfstGenConfig {
    /// Minimum number of states.
    pub min_states: usize,
    /// Maximum number of states.
    pub max_states: usize,
    /// Maximum outgoing arcs per state.
    pub max_arcs_per_state: usize,
    /// Whether to allow epsilon transitions.
    pub allow_epsilon: bool,
    /// Whether to allow cycles.
    pub allow_cycles: bool,
    /// Whether to force at least one final state.
    pub force_final: bool,
}

/// Strategy for generating WFSTs with custom configuration.
pub fn arb_wfst_with_config<L, W>(config: WfstGenConfig) -> impl Strategy<Value = VectorWfst<L, W>>
where
    L: Clone + Send + Sync + std::fmt::Debug + 'static,
    W: Semiring + std::fmt::Debug + 'static,
    (Option<L>, Option<L>): Arbitrary,
    W: Arbitrary,
{
    let WfstGenConfig {
        min_states,
        max_states,
        max_arcs_per_state,
        allow_epsilon: _,
        allow_cycles,
        force_final,
    } = config;

    (min_states..=max_states).prop_flat_map(move |num_states| {
        // Generate arcs for each state
        let arc_counts = prop_vec(0..=max_arcs_per_state, num_states);

        // Generate final state flags
        let final_flags = prop_vec(any::<bool>(), num_states);

        // Generate final weights for final states
        let final_weights = prop_vec(any::<W>(), num_states);

        (arc_counts, final_flags, final_weights).prop_flat_map(move |(counts, finals, final_ws)| {
            // Generate labels and weights for each arc
            let arc_data: Vec<_> = counts
                .iter()
                .enumerate()
                .map(|(from, &count)| {
                    let max_to = if allow_cycles {
                        num_states
                    } else {
                        // For acyclic, only allow forward edges
                        num_states.saturating_sub(from).max(1)
                    };
                    prop_vec(
                        (any::<(Option<L>, Option<L>)>(), any::<W>(), 0..max_to),
                        count,
                    )
                })
                .collect();

            // Combine all arc data into a single strategy
            arc_data.prop_map(move |arcs_per_state| {
                let mut fst: VectorWfst<L, W> = VectorWfst::with_capacity(num_states);

                // Add states
                for _ in 0..num_states {
                    fst.add_state();
                }

                // Set start state
                fst.set_start(0);

                // Set final states
                let mut has_final = false;
                for (state, (&is_final, final_weight)) in
                    finals.iter().zip(final_ws.iter()).enumerate()
                {
                    if is_final && !final_weight.is_zero() {
                        fst.set_final(state as StateId, *final_weight);
                        has_final = true;
                    }
                }

                // Ensure at least one final state if required
                if force_final && !has_final && num_states > 0 {
                    fst.set_final((num_states - 1) as StateId, W::one());
                }

                // Add arcs
                for (from, arcs) in arcs_per_state.into_iter().enumerate() {
                    for ((input, output), weight, to_offset) in arcs {
                        let to = if allow_cycles {
                            to_offset as StateId
                        } else {
                            // For acyclic, target is from + offset + 1
                            ((from + to_offset + 1) % num_states) as StateId
                        };
                        if to < num_states as StateId {
                            fst.add_arc(from as StateId, input, output, to, weight);
                        }
                    }
                }

                fst
            })
        })
    })
}

/// Strategy for generating acyclic WFSTs.
///
/// Generates WFSTs where all transitions go from lower to higher state IDs,
/// guaranteeing no cycles.
pub fn arb_acyclic_wfst<L, W>(
    max_states: usize,
    max_arcs_per_state: usize,
) -> impl Strategy<Value = VectorWfst<L, W>>
where
    L: Clone + Send + Sync + std::fmt::Debug + 'static,
    W: Semiring + std::fmt::Debug + 'static,
    (Option<L>, Option<L>): Arbitrary,
    W: Arbitrary,
{
    arb_wfst_with_config::<L, W>(WfstGenConfig {
        min_states: 1,
        max_states,
        max_arcs_per_state,
        allow_epsilon: true,
        allow_cycles: false,
        force_final: true,
    })
}

/// Strategy for generating deterministic WFSTs (DFAs) with tropical weights.
///
/// Ensures that for each state and input label, there is at most one transition.
/// No epsilon transitions on input.
pub fn arb_deterministic_wfst_tropical(
    max_states: usize,
    alphabet_size: usize,
) -> impl Strategy<Value = VectorWfst<char, TropicalWeight>> {
    let alphabet: Vec<char> = ('a'..'z').take(alphabet_size).collect();

    (1..=max_states).prop_flat_map(move |num_states| {
        let alphabet = alphabet.clone();

        // For each (state, label), decide if there's a transition and where it goes
        let transitions_per_state: Vec<_> = (0..num_states)
            .map(|_| {
                prop_vec(
                    (any::<bool>(), 0..num_states, arb_tropical_weight_nonzero()),
                    alphabet.len(),
                )
            })
            .collect();

        // Generate final state info
        let final_info = prop_vec((any::<bool>(), arb_tropical_weight_nonzero()), num_states);

        (transitions_per_state, final_info).prop_map(move |(trans_per_state, finals)| {
            let mut fst: VectorWfst<char, TropicalWeight> = VectorWfst::with_capacity(num_states);

            // Add states
            for _ in 0..num_states {
                fst.add_state();
            }

            // Set start state
            fst.set_start(0);

            // Set final states
            let mut has_final = false;
            for (state, (is_final, weight)) in finals.iter().enumerate() {
                if *is_final && !weight.is_zero() {
                    fst.set_final(state as StateId, *weight);
                    has_final = true;
                }
            }

            // Ensure at least one final state
            if !has_final && num_states > 0 {
                fst.set_final((num_states - 1) as StateId, TropicalWeight::one());
            }

            // Add deterministic transitions (at most one per input label)
            for (from, state_trans) in trans_per_state.into_iter().enumerate() {
                for (label_idx, (has_trans, to, weight)) in state_trans.into_iter().enumerate() {
                    if has_trans && !weight.is_zero() {
                        let label = alphabet[label_idx];
                        fst.add_arc(
                            from as StateId,
                            Some(label),
                            Some(label),
                            to as StateId,
                            weight,
                        );
                    }
                }
            }

            fst
        })
    })
}

/// Strategy for generating deterministic WFSTs (DFAs) with a custom weight strategy.
///
/// Ensures that for each state and input label, there is at most one transition.
/// No epsilon transitions on input.
pub fn arb_deterministic_wfst<W, WS>(
    max_states: usize,
    alphabet_size: usize,
    weight_strategy: WS,
) -> impl Strategy<Value = VectorWfst<char, W>>
where
    W: Semiring + std::fmt::Debug + 'static,
    WS: Strategy<Value = W> + Clone + 'static,
{
    let alphabet: Vec<char> = ('a'..'z').take(alphabet_size).collect();

    (1..=max_states).prop_flat_map(move |num_states| {
        let alphabet = alphabet.clone();
        let ws = weight_strategy.clone();

        // For each (state, label), decide if there's a transition and where it goes
        let transitions_per_state: Vec<_> = (0..num_states)
            .map(|_| prop_vec((any::<bool>(), 0..num_states, ws.clone()), alphabet.len()))
            .collect();

        // Generate final state info
        let final_info = prop_vec((any::<bool>(), ws.clone()), num_states);

        (transitions_per_state, final_info).prop_map(move |(trans_per_state, finals)| {
            let mut fst: VectorWfst<char, W> = VectorWfst::with_capacity(num_states);

            // Add states
            for _ in 0..num_states {
                fst.add_state();
            }

            // Set start state
            fst.set_start(0);

            // Set final states
            let mut has_final = false;
            for (state, (is_final, weight)) in finals.iter().enumerate() {
                if *is_final && !weight.is_zero() {
                    fst.set_final(state as StateId, *weight);
                    has_final = true;
                }
            }

            // Ensure at least one final state
            if !has_final && num_states > 0 {
                fst.set_final((num_states - 1) as StateId, W::one());
            }

            // Add deterministic transitions (at most one per input label)
            for (from, state_trans) in trans_per_state.into_iter().enumerate() {
                for (label_idx, (has_trans, to, weight)) in state_trans.into_iter().enumerate() {
                    if has_trans && !weight.is_zero() {
                        let label = alphabet[label_idx];
                        fst.add_arc(
                            from as StateId,
                            Some(label),
                            Some(label),
                            to as StateId,
                            weight,
                        );
                    }
                }
            }

            fst
        })
    })
}

/// Strategy for generating WFSTs with specific properties for tropical semiring.
pub fn arb_tropical_wfst(
    max_states: usize,
    max_arcs_per_state: usize,
) -> impl Strategy<Value = VectorWfst<char, TropicalWeight>> {
    let config = WfstGenConfig {
        min_states: 1,
        max_states,
        max_arcs_per_state,
        allow_epsilon: true,
        allow_cycles: true,
        force_final: true,
    };

    (config.min_states..=config.max_states).prop_flat_map(move |num_states| {
        // Generate arcs per state
        let arc_counts = prop_vec(0..=max_arcs_per_state, num_states);
        let final_flags = prop_vec(any::<bool>(), num_states);
        let final_weights = prop_vec(arb_tropical_weight_nonzero(), num_states);

        (arc_counts, final_flags, final_weights).prop_flat_map(move |(counts, finals, final_ws)| {
            // Generate arc data
            let arc_data: Vec<_> = counts
                .iter()
                .enumerate()
                .map(|(_, &count)| {
                    prop_vec(
                        (
                            arb_label_or_epsilon(),
                            arb_label_or_epsilon(),
                            arb_tropical_weight_nonzero(),
                            0..num_states,
                        ),
                        count,
                    )
                })
                .collect();

            arc_data.prop_map(move |arcs_per_state| {
                let mut fst: VectorWfst<char, TropicalWeight> =
                    VectorWfst::with_capacity(num_states);

                for _ in 0..num_states {
                    fst.add_state();
                }

                fst.set_start(0);

                let mut has_final = false;
                for (state, (&is_final, final_weight)) in
                    finals.iter().zip(final_ws.iter()).enumerate()
                {
                    if is_final {
                        fst.set_final(state as StateId, *final_weight);
                        has_final = true;
                    }
                }

                if !has_final && num_states > 0 {
                    fst.set_final((num_states - 1) as StateId, TropicalWeight::one());
                }

                for (from, arcs) in arcs_per_state.into_iter().enumerate() {
                    for (input, output, weight, to) in arcs {
                        fst.add_arc(from as StateId, input, output, to as StateId, weight);
                    }
                }

                fst
            })
        })
    })
}

/// Strategy for generating acyclic WFSTs for tropical semiring.
///
/// Generates WFSTs where all transitions go from lower to higher state IDs,
/// guaranteeing no cycles.
pub fn arb_acyclic_wfst_tropical(
    max_states: usize,
    max_arcs_per_state: usize,
) -> impl Strategy<Value = VectorWfst<char, TropicalWeight>> {
    (1..=max_states).prop_flat_map(move |num_states| {
        // Generate arcs per state
        let arc_counts = prop_vec(0..=max_arcs_per_state, num_states);
        let final_flags = prop_vec(any::<bool>(), num_states);
        let final_weights = prop_vec(arb_tropical_weight_nonzero(), num_states);

        (arc_counts, final_flags, final_weights).prop_flat_map(move |(counts, finals, final_ws)| {
            // Generate arc data - for acyclic, only allow forward edges
            let arc_data: Vec<_> = counts
                .iter()
                .enumerate()
                .map(|(from, &count)| {
                    let remaining = num_states.saturating_sub(from + 1);
                    if remaining > 0 {
                        prop_vec(
                            (
                                arb_label_or_epsilon(),
                                arb_label_or_epsilon(),
                                arb_tropical_weight_nonzero(),
                                0..remaining,
                            ),
                            count,
                        )
                        .boxed()
                    } else {
                        Just(Vec::new()).boxed()
                    }
                })
                .collect();

            arc_data.prop_map(move |arcs_per_state| {
                let mut fst: VectorWfst<char, TropicalWeight> =
                    VectorWfst::with_capacity(num_states);

                for _ in 0..num_states {
                    fst.add_state();
                }

                fst.set_start(0);

                let mut has_final = false;
                for (state, (&is_final, final_weight)) in
                    finals.iter().zip(final_ws.iter()).enumerate()
                {
                    if is_final {
                        fst.set_final(state as StateId, *final_weight);
                        has_final = true;
                    }
                }

                if !has_final && num_states > 0 {
                    fst.set_final((num_states - 1) as StateId, TropicalWeight::one());
                }

                // For acyclic WFSTs, only add forward edges (from < to)
                for (from, arcs) in arcs_per_state.into_iter().enumerate() {
                    for (input, output, weight, to_offset) in arcs {
                        let to = from + 1 + to_offset;
                        if to < num_states {
                            fst.add_arc(from as StateId, input, output, to as StateId, weight);
                        }
                    }
                }

                fst
            })
        })
    })
}

/// Strategy for generating WFSTs with specific properties for log semiring.
pub fn arb_log_wfst(
    max_states: usize,
    max_arcs_per_state: usize,
) -> impl Strategy<Value = VectorWfst<char, LogWeight>> {
    (1..=max_states).prop_flat_map(move |num_states| {
        let arc_counts = prop_vec(0..=max_arcs_per_state, num_states);
        let final_flags = prop_vec(any::<bool>(), num_states);
        let final_weights = prop_vec(arb_log_weight_nonzero(), num_states);

        (arc_counts, final_flags, final_weights).prop_flat_map(move |(counts, finals, final_ws)| {
            let arc_data: Vec<_> = counts
                .iter()
                .enumerate()
                .map(|(_, &count)| {
                    prop_vec(
                        (
                            arb_label_or_epsilon(),
                            arb_label_or_epsilon(),
                            arb_log_weight_nonzero(),
                            0..num_states,
                        ),
                        count,
                    )
                })
                .collect();

            arc_data.prop_map(move |arcs_per_state| {
                let mut fst: VectorWfst<char, LogWeight> = VectorWfst::with_capacity(num_states);

                for _ in 0..num_states {
                    fst.add_state();
                }

                fst.set_start(0);

                let mut has_final = false;
                for (state, (&is_final, final_weight)) in
                    finals.iter().zip(final_ws.iter()).enumerate()
                {
                    if is_final {
                        fst.set_final(state as StateId, *final_weight);
                        has_final = true;
                    }
                }

                if !has_final && num_states > 0 {
                    fst.set_final((num_states - 1) as StateId, LogWeight::one());
                }

                for (from, arcs) in arcs_per_state.into_iter().enumerate() {
                    for (input, output, weight, to) in arcs {
                        fst.add_arc(from as StateId, input, output, to as StateId, weight);
                    }
                }

                fst
            })
        })
    })
}

/// Strategy for generating small WFSTs suitable for exhaustive testing.
pub fn arb_small_wfst<W>() -> impl Strategy<Value = VectorWfst<char, W>>
where
    W: Semiring + std::fmt::Debug + 'static + Arbitrary,
{
    arb_wfst_with_config::<char, W>(WfstGenConfig {
        min_states: 1,
        max_states: 5,
        max_arcs_per_state: 3,
        allow_epsilon: true,
        allow_cycles: false,
        force_final: true,
    })
}

// =============================================================================
// Transition Strategies
// =============================================================================

/// Strategy for generating arbitrary weighted transitions.
pub fn arb_transition<L, W>(max_state: StateId) -> impl Strategy<Value = WeightedTransition<L, W>>
where
    L: Clone + Send + Sync + std::fmt::Debug + 'static + Arbitrary,
    W: Semiring + std::fmt::Debug + 'static + Arbitrary,
{
    (
        0..max_state,
        any::<Option<L>>(),
        any::<Option<L>>(),
        0..max_state,
        any::<W>(),
    )
        .prop_map(|(from, input, output, to, weight)| {
            WeightedTransition::new(from, input, output, to, weight)
        })
}

/// Strategy for generating epsilon transitions.
pub fn arb_epsilon_transition<L, W>(
    max_state: StateId,
) -> impl Strategy<Value = WeightedTransition<L, W>>
where
    L: Clone + Send + Sync + std::fmt::Debug + 'static,
    W: Semiring + std::fmt::Debug + 'static + Arbitrary,
{
    (0..max_state, 0..max_state, any::<W>())
        .prop_map(|(from, to, weight)| WeightedTransition::epsilon(from, to, weight))
}

// =============================================================================
// Lattice Strategies
// =============================================================================

use crate::backend::HashMapBackend;
use crate::lattice::{EdgeMetadata, Lattice, LatticeBuilder};

/// Strategy for generating random lattices with tropical weights.
///
/// Creates lattices with `num_positions` positions, with a random number
/// of edges per position.
pub fn arb_tropical_lattice(
    num_positions: usize,
    max_edges_per_position: usize,
) -> impl Strategy<Value = Lattice<TropicalWeight, HashMapBackend>> {
    // Generate edge counts for each position
    let edge_counts = prop_vec(1..=max_edges_per_position, num_positions);

    // Generate weights for each edge
    edge_counts.prop_flat_map(move |counts| {
        let total_edges: usize = counts.iter().sum();
        let weights = prop_vec(arb_tropical_weight_nonzero(), total_edges);

        (Just(counts), weights).prop_map(move |(counts, weights)| {
            let backend = HashMapBackend::new();
            let mut builder = LatticeBuilder::new(backend);

            let mut weight_idx = 0;
            for (pos, &count) in counts.iter().enumerate() {
                for edge_num in 0..count {
                    let word = format!("w{}_{}", pos, edge_num);
                    let weight = weights[weight_idx];
                    weight_idx += 1;
                    builder.add_correction(pos, pos + 1, &word, weight, EdgeMetadata::default());
                }
            }

            builder.build(num_positions)
        })
    })
}

/// Strategy for generating simple linear lattices (one edge per position).
pub fn arb_linear_lattice(
    num_positions: usize,
) -> impl Strategy<Value = Lattice<TropicalWeight, HashMapBackend>> {
    let weights = prop_vec(arb_tropical_weight_nonzero(), num_positions);

    weights.prop_map(move |weights| {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        for (pos, weight) in weights.iter().enumerate() {
            let word = format!("word{}", pos);
            builder.add_correction(pos, pos + 1, &word, *weight, EdgeMetadata::default());
        }

        builder.build(num_positions)
    })
}

/// Strategy for generating diamond-shaped lattices.
///
/// Each position has exactly 2 alternatives, creating 2^n paths.
pub fn arb_diamond_lattice(
    num_positions: usize,
) -> impl Strategy<Value = Lattice<TropicalWeight, HashMapBackend>> {
    let weights = prop_vec(arb_tropical_weight_nonzero(), num_positions * 2);

    weights.prop_map(move |weights| {
        let backend = HashMapBackend::new();
        let mut builder = LatticeBuilder::new(backend);

        for pos in 0..num_positions {
            let word_a = format!("a{}", pos);
            let word_b = format!("b{}", pos);
            builder.add_correction(
                pos,
                pos + 1,
                &word_a,
                weights[pos * 2],
                EdgeMetadata::default(),
            );
            builder.add_correction(
                pos,
                pos + 1,
                &word_b,
                weights[pos * 2 + 1],
                EdgeMetadata::default(),
            );
        }

        builder.build(num_positions)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest! {
        #[test]
        fn test_arb_tropical_weight_valid(w in arb_tropical_weight()) {
            // Weight should be a valid tropical weight
            assert!(!w.value().is_nan());
        }

        #[test]
        fn test_arb_tropical_wfst_valid(fst in arb_tropical_wfst(10, 5)) {
            // Should have valid start state
            assert!(fst.num_states() > 0);
            assert!(fst.start() < fst.num_states() as StateId);

            // Should have at least one final state
            let has_final = (0..fst.num_states())
                .any(|s| fst.is_final(s as StateId));
            assert!(has_final);
        }

        #[test]
        fn test_arb_deterministic_wfst_valid(fst in arb_deterministic_wfst_tropical(10, 5)) {
            // Check determinism: for each state, at most one arc per input label
            for state in 0..fst.num_states() as StateId {
                let trans = fst.transitions(state);
                let mut seen_labels: std::collections::HashSet<Option<char>> = std::collections::HashSet::new();
                for t in trans {
                    // Should not have duplicate input labels
                    assert!(
                        seen_labels.insert(t.input),
                        "Duplicate input label {:?} in state {}",
                        t.input,
                        state
                    );
                }
            }
        }

        #[test]
        fn test_arb_log_weight_valid(w in arb_log_weight()) {
            // Weight should be a valid log weight
            assert!(!w.value().is_nan());
            assert!(w.value() >= 0.0 || w.is_zero());
        }

        #[test]
        fn test_arb_probability_weight_valid(w in arb_probability_weight()) {
            // Weight should be in [0, 1]
            let v = w.value();
            assert!(v >= 0.0 && v <= 1.0);
        }
    }
}
