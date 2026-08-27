//! Differential correspondence between the ABI's lazy composition
//! (`lling_wfst_compose` -> `CompositionResource`) and the in-repo EAGER
//! composition oracle (`materialize(compose(a, b))` from
//! `src/composition/{fst_fst,materialize}.rs` — the algorithm behind the
//! composition integration tests).
//!
//! For arbitrary pairs of small acyclic Unicode/tropical WFSTs (the
//! `test-utils` strategy `arb_acyclic_wfst_tropical`, labels squashed onto a
//! two-letter alphabet for match density), the fully traversed ABI
//! composition must agree with the oracle:
//! 1. as accepted-path multisets — every `(input string, output string,
//!    weight)` triple, with exact f64 weights (both sides perform the
//!    identical addition sequences);
//! 2. as shortest-distance maps — the min accepted weight per enumerated
//!    input string; and
//! 3. structurally — identical canonical (BFS discovery order) forms, since
//!    both algorithms emit product arcs in the same case order
//!    (left-epsilon, right-epsilon, paired-epsilon, non-epsilon match) over the
//!    same sequencing filter.
//!
//! Epsilon handling is pinned extensionally: the example pins cover
//! single-sided leading/trailing epsilons and the canonical simultaneous move
//! for double-sided epsilons, all with hand-computed weights.
//!
//! Formal-model correspondence (invariant registry owned by the coordinator):
//! - `// INVARIANT-HOOK: LLING-COMP-1` — lazy ABI composition ≡ eager
//!   native composition on every observable: paths, shortest distances,
//!   and canonical structure.
#![cfg(all(feature = "ffi", feature = "test-utils"))]

mod support;

use lling_llang::bindings::{import_tropical_wfst, OwnedWfstResource};
use lling_llang::composition::{compose, materialize};
use lling_llang::ffi::{
    lling_resource_release, lling_wfst_compose, lling_wfst_free, lling_wfst_resource, LlingStatus,
    LlingWfst,
};
use lling_llang::semiring::TropicalWeight;
use lling_llang::test_utils::arb_acyclic_wfst_tropical;
use lling_llang::wfst::{MutableWfst, StateId, VectorWfst, Wfst};
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::ptr;
use support::interop_wfst::canonical_of_vector;
use vinary_tree_interop::VtResource;

/// Squash an arbitrary generated label onto {a, b} so left outputs actually
/// meet right inputs often enough to produce non-trivial products.
fn squash_label(label: char) -> char {
    if u32::from(label) & 1 == 0 {
        'a'
    } else {
        'b'
    }
}

/// Rebuild `wfst` with every label squashed via [`squash_label`].
fn squash_alphabet(wfst: &VectorWfst<char, TropicalWeight>) -> VectorWfst<char, TropicalWeight> {
    let mut result: VectorWfst<char, TropicalWeight> = VectorWfst::with_capacity(wfst.num_states());
    for _ in 0..wfst.num_states() {
        result.add_state();
    }
    result.set_start(wfst.start());
    for state in 0..wfst.num_states() as StateId {
        if wfst.is_final(state) {
            result.set_final(state, wfst.final_weight(state));
        }
        for transition in wfst.transitions(state) {
            result.add_arc(
                state,
                transition.input.map(squash_label),
                transition.output.map(squash_label),
                transition.to,
                transition.weight,
            );
        }
    }
    result
}

/// One accepted path: input string, output string, exact tropical weight.
type AcceptedPath = (String, String, f64);

/// Enumerate every accepting path of an ACYCLIC machine by DFS, spending at
/// most `budget` node visits. Returns `None` when the budget is exhausted
/// (the property discards such pathological cases instead of timing out).
fn accepting_paths(
    wfst: &VectorWfst<char, TropicalWeight>,
    budget: &mut usize,
) -> Option<Vec<AcceptedPath>> {
    let mut paths = Vec::new();
    let mut stack: Vec<(StateId, String, String, f64)> =
        vec![(wfst.start(), String::new(), String::new(), 0.0)];
    while let Some((state, input, output, weight)) = stack.pop() {
        *budget = budget.checked_sub(1)?;
        if *budget == 0 {
            return None;
        }
        if wfst.is_final(state) {
            paths.push((
                input.clone(),
                output.clone(),
                weight + wfst.final_weight(state).value(),
            ));
        }
        for transition in wfst.transitions(state) {
            let mut next_input = input.clone();
            if let Some(label) = transition.input {
                next_input.push(label);
            }
            let mut next_output = output.clone();
            if let Some(label) = transition.output {
                next_output.push(label);
            }
            stack.push((
                transition.to,
                next_input,
                next_output,
                weight + transition.weight.value(),
            ));
        }
    }
    Some(paths)
}

/// Sort a path multiset into canonical comparison order.
fn sorted_paths(mut paths: Vec<AcceptedPath>) -> Vec<AcceptedPath> {
    paths.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.total_cmp(&right.2))
    });
    paths
}

/// Shortest accepted distance per input string.
fn shortest_distances(paths: &[AcceptedPath]) -> BTreeMap<&str, f64> {
    let mut distances: BTreeMap<&str, f64> = BTreeMap::new();
    for (input, _, weight) in paths {
        distances
            .entry(input.as_str())
            .and_modify(|best| *best = best.min(*weight))
            .or_insert(*weight);
    }
    distances
}

/// Compose `left` and `right` through the C ABI and fully traverse the lazy
/// product back into an eager native graph.
fn abi_composition(
    left: &VectorWfst<char, TropicalWeight>,
    right: &VectorWfst<char, TropicalWeight>,
) -> VectorWfst<char, TropicalWeight> {
    let left_resource = OwnedWfstResource::from_wfst(left.clone());
    let right_resource = OwnedWfstResource::from_wfst(right.clone());
    let mut composed: *mut LlingWfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_compose(
            left_resource.as_raw(),
            right_resource.as_raw(),
            &mut composed
        ),
        LlingStatus::Ok
    );
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { lling_wfst_resource(composed, &mut resource) },
        LlingStatus::Ok
    );
    let materialized =
        import_tropical_wfst(resource).expect("fully traversing the lazy product must succeed");
    lling_resource_release(resource);
    unsafe { lling_wfst_free(composed) };
    materialized
}

/// The eager oracle: `materialize(compose(a, b))`.
fn oracle_composition(
    left: &VectorWfst<char, TropicalWeight>,
    right: &VectorWfst<char, TropicalWeight>,
) -> VectorWfst<char, TropicalWeight> {
    materialize(compose(left.clone(), right.clone()))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// INVARIANT-HOOK: LLING-COMP-1 — for arbitrary acyclic pairs, the fully
    /// traversed lazy ABI composition equals the eager oracle on accepted
    /// paths (multisets, exact weights), shortest distances per input, and
    /// canonical structure.
    #[test]
    fn lazy_abi_composition_equals_eager_oracle(
        raw_left in arb_acyclic_wfst_tropical(4, 2),
        raw_right in arb_acyclic_wfst_tropical(4, 2),
    ) {
        let left = squash_alphabet(&raw_left);
        let right = squash_alphabet(&raw_right);

        let oracle = oracle_composition(&left, &right);
        let abi = abi_composition(&left, &right);

        // 3. Structural correspondence: identical canonical forms.
        prop_assert_eq!(
            canonical_of_vector(&abi),
            canonical_of_vector(&oracle),
            "lazy and eager products must be structurally identical"
        );

        // 1. Accepted-path multisets with exact weights.
        let mut budget = 100_000usize;
        let oracle_paths = accepting_paths(&oracle, &mut budget);
        let abi_paths = accepting_paths(&abi, &mut budget);
        prop_assume!(oracle_paths.is_some() && abi_paths.is_some());
        let oracle_paths =
            sorted_paths(oracle_paths.expect("assumed present"));
        let abi_paths = sorted_paths(abi_paths.expect("assumed present"));
        prop_assert_eq!(&abi_paths, &oracle_paths, "accepted-path multisets must agree");

        // 2. Shortest distance per enumerated input string.
        prop_assert_eq!(
            shortest_distances(&abi_paths),
            shortest_distances(&oracle_paths),
            "per-input shortest distances must agree"
        );
    }
}

/// Hand-computable pin: a trailing LEFT-side epsilon after the match.
///
/// left:  0 -a:x/1-> 1 -b:ε/1-> 2, final(2)=0.25
/// right: 0 -x:y/0.5-> 1,          final(1)=0.5
///
/// One accepting path: match a:x⊗x:y (None filter), then the left b:ε move
/// (allowed from None, enters Eps1), accept at (2, 1):
/// input "ab", output "y", weight = (1+0.5) + 1 + (0.25+0.5) = 3.25.
#[test]
fn epsilon_pin_trailing_left_epsilon_is_accepted() {
    let mut left: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let l0 = left.add_state();
    let l1 = left.add_state();
    let l2 = left.add_state();
    left.set_start(l0);
    left.set_final(l2, TropicalWeight::new(0.25));
    left.add_arc(l0, Some('a'), Some('x'), l1, TropicalWeight::new(1.0));
    left.add_arc(l1, Some('b'), None, l2, TropicalWeight::new(1.0));

    let mut right: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let r0 = right.add_state();
    let r1 = right.add_state();
    right.set_start(r0);
    right.set_final(r1, TropicalWeight::new(0.5));
    right.add_arc(r0, Some('x'), Some('y'), r1, TropicalWeight::new(0.5));

    let expected = vec![("ab".to_string(), "y".to_string(), 3.25)];
    let mut budget = 10_000usize;
    let oracle_paths = sorted_paths(
        accepting_paths(&oracle_composition(&left, &right), &mut budget)
            .expect("tiny fixture fits the budget"),
    );
    let abi_paths = sorted_paths(
        accepting_paths(&abi_composition(&left, &right), &mut budget)
            .expect("tiny fixture fits the budget"),
    );
    assert_eq!(
        oracle_paths, expected,
        "oracle must match the hand computation"
    );
    assert_eq!(abi_paths, expected, "ABI must match the hand computation");
}

/// Hand-computable pin: a leading RIGHT-side epsilon before the match.
///
/// left:  0 -a:x/1-> 1,             final(1)=0
/// right: 0 -ε:c/0.5-> 1 -x:y/0.5-> 2, final(2)=0
///
/// One accepting path: the right ε:c move (enters Eps2), then the match
/// a:x⊗x:y (allowed from Eps2, resets to None), accept at (1, 2):
/// input "a", output "cy", weight = 0.5 + (1+0.5) + 0 = 2.0.
#[test]
fn epsilon_pin_leading_right_epsilon_is_accepted() {
    let mut left: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let l0 = left.add_state();
    let l1 = left.add_state();
    left.set_start(l0);
    left.set_final(l1, TropicalWeight::new(0.0));
    left.add_arc(l0, Some('a'), Some('x'), l1, TropicalWeight::new(1.0));

    let mut right: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let r0 = right.add_state();
    let r1 = right.add_state();
    let r2 = right.add_state();
    right.set_start(r0);
    right.set_final(r2, TropicalWeight::new(0.0));
    right.add_arc(r0, None, Some('c'), r1, TropicalWeight::new(0.5));
    right.add_arc(r1, Some('x'), Some('y'), r2, TropicalWeight::new(0.5));

    let expected = vec![("a".to_string(), "cy".to_string(), 2.0)];
    let mut budget = 10_000usize;
    let oracle_paths = sorted_paths(
        accepting_paths(&oracle_composition(&left, &right), &mut budget)
            .expect("tiny fixture fits the budget"),
    );
    let abi_paths = sorted_paths(
        accepting_paths(&abi_composition(&left, &right), &mut budget)
            .expect("tiny fixture fits the budget"),
    );
    assert_eq!(
        oracle_paths, expected,
        "oracle must match the hand computation"
    );
    assert_eq!(abi_paths, expected, "ABI must match the hand computation");
}

/// Pin of the sequencing filter's DOUBLE-SIDED epsilon behavior. The paired
/// epsilon move advances both machines simultaneously, representing the valid
/// composition once without duplicating left-first and right-first schedules.
///
/// left:  0 -a:x/1-> 1 -b:ε/1-> 2, final(2)=0
/// right: 0 -x:y/1-> 1 -ε:z/1-> 2, final(2)=0
#[test]
fn epsilon_pin_double_sided_trailing_epsilons_compose_identically() {
    let mut left: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let l0 = left.add_state();
    let l1 = left.add_state();
    let l2 = left.add_state();
    left.set_start(l0);
    left.set_final(l2, TropicalWeight::new(0.0));
    left.add_arc(l0, Some('a'), Some('x'), l1, TropicalWeight::new(1.0));
    left.add_arc(l1, Some('b'), None, l2, TropicalWeight::new(1.0));

    let mut right: VectorWfst<char, TropicalWeight> = VectorWfst::new();
    let r0 = right.add_state();
    let r1 = right.add_state();
    let r2 = right.add_state();
    right.set_start(r0);
    right.set_final(r2, TropicalWeight::new(0.0));
    right.add_arc(r0, Some('x'), Some('y'), r1, TropicalWeight::new(1.0));
    right.add_arc(r1, None, Some('z'), r2, TropicalWeight::new(1.0));

    let mut budget = 10_000usize;
    let oracle_paths = accepting_paths(&oracle_composition(&left, &right), &mut budget)
        .expect("tiny fixture fits the budget");
    let abi_paths = accepting_paths(&abi_composition(&left, &right), &mut budget)
        .expect("tiny fixture fits the budget");
    let expected = vec![("ab".to_string(), "yz".to_string(), 4.0)];
    assert_eq!(sorted_paths(oracle_paths), expected);
    assert_eq!(sorted_paths(abi_paths), expected);
    assert_eq!(
        canonical_of_vector(&abi_composition(&left, &right)),
        canonical_of_vector(&oracle_composition(&left, &right))
    );
}
