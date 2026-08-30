//! Required-red properties extracted from the WPDS portability formal model.
//!
//! These properties were extracted before the production portability module
//! existed. They remain the executable refinement gate for every implementation
//! change and must never be weakened or conditionally disabled.

use std::collections::BTreeSet;

use lling_llang::pushdown::wpds::portability::{
    PortableCancellation, PortableDecodeLimits, PortableProofNode, PortableReplayChecks,
    PortableReplayIdentity, PortableRuleKey, PortableRuleMap, PortableWitness, PortableWorkShape,
    ReplayRejection,
};
use proptest::prelude::*;

fn key(value: u128) -> PortableRuleKey {
    PortableRuleKey::from_bytes(value.to_be_bytes())
}

fn identity(seed: u8) -> PortableReplayIdentity {
    PortableReplayIdentity {
        rule_snapshot: [seed; 32],
        context: [seed.wrapping_add(1); 32],
        query: [seed.wrapping_add(2); 32],
        semantics: [seed.wrapping_add(3); 32],
        codec_profile: [seed.wrapping_add(4); 32],
    }
}

fn valid_checks() -> PortableReplayChecks {
    PortableReplayChecks {
        well_formed: true,
        checksum_valid: true,
        within_budget: true,
        witness_valid: true,
        cancellation_reason: None,
    }
}

proptest! {
    #[test]
    fn prop_rule_map_rejects_duplicates_and_is_bijective(
        raw in prop::collection::vec(any::<u128>(), 0..64),
    ) {
        let keys: Vec<_> = raw.iter().copied().map(key).collect();
        let unique: BTreeSet<_> = keys.iter().copied().collect();
        let result = PortableRuleMap::seal(keys.clone());
        if unique.len() != keys.len() {
            prop_assert_eq!(result, Err(ReplayRejection::DuplicateExternalRuleKey));
            return Ok(());
        }

        let map = result.expect("a unique rule-key tape must seal");
        prop_assert_eq!(map.len(), keys.len());
        for (dense, external) in keys.into_iter().enumerate() {
            prop_assert_eq!(map.external_for(dense), Some(external));
            prop_assert_eq!(map.dense_for(external), Some(dense));
        }
    }

    #[test]
    fn prop_same_snapshot_keeps_dense_ids_stable(
        raw in prop::collection::btree_set(any::<u128>(), 0..64),
    ) {
        let keys: Vec<_> = raw.into_iter().map(key).collect();
        let first = PortableRuleMap::seal(keys.clone()).unwrap();
        let second = PortableRuleMap::seal(keys.clone()).unwrap();
        prop_assert_eq!(first.snapshot_digest(), second.snapshot_digest());
        for external in keys {
            prop_assert_eq!(first.dense_for(external), second.dense_for(external));
        }
    }

    #[test]
    fn prop_replay_requires_all_five_identity_fields(
        seed in any::<u8>(),
        field in 0usize..5,
    ) {
        let expected = identity(seed);
        let mut observed = expected;
        match field {
            0 => observed.rule_snapshot[0] ^= 1,
            1 => observed.context[0] ^= 1,
            2 => observed.query[0] ^= 1,
            3 => observed.semantics[0] ^= 1,
            4 => observed.codec_profile[0] ^= 1,
            _ => unreachable!(),
        }
        prop_assert!(!expected.admits(&observed, valid_checks()));
        prop_assert!(expected.admits(&expected, valid_checks()));
    }

    #[test]
    fn prop_rejected_inputs_never_publish(
        seed in any::<u8>(),
        malformed in any::<bool>(),
        bad_checksum in any::<bool>(),
        over_budget in any::<bool>(),
        cancelled in any::<bool>(),
        invalid_witness in any::<bool>(),
    ) {
        prop_assume!(malformed || bad_checksum || over_budget || cancelled || invalid_witness);
        let expected = identity(seed);
        let checks = PortableReplayChecks {
            well_formed: !malformed,
            checksum_valid: !bad_checksum,
            within_budget: !over_budget,
            witness_valid: !invalid_witness,
            cancellation_reason: cancelled.then_some(1),
        };
        prop_assert!(!expected.admits(&expected, checks));
    }

    #[test]
    fn prop_cancellation_reason_is_first_writer_sticky(
        first in any::<u32>(),
        second in any::<u32>(),
    ) {
        let cancellation = PortableCancellation::new();
        cancellation.request(first);
        cancellation.request(second);
        prop_assert_eq!(cancellation.reason(), Some(first));
    }

    #[test]
    fn prop_flat_decoder_is_positive_bounded_and_overflow_safe(
        bytes in prop::collection::vec(any::<u8>(), 0..4096),
        max_nodes in 0usize..256,
        max_edges in 0usize..512,
    ) {
        let limits = PortableDecodeLimits {
            max_bytes: bytes.len(),
            max_nodes,
            max_edges,
        };
        let outcome = PortableWitness::decode_flat(&bytes, limits);
        if let Ok(witness) = outcome {
            let usage = witness.decode_usage();
            prop_assert_eq!(usage.bytes, bytes.len());
            prop_assert!(usage.nodes <= max_nodes);
            prop_assert!(usage.edges <= max_edges);
            prop_assert!(usage.positive_steps <= bytes.len());
        }
    }

    #[test]
    fn prop_portable_witness_uses_known_keys_and_earlier_premises(
        raw in prop::collection::btree_set(any::<u128>(), 1..32),
        choose_unknown in any::<bool>(),
    ) {
        let keys: Vec<_> = raw.into_iter().map(key).collect();
        let map = PortableRuleMap::seal(keys.clone()).unwrap();
        let mut unknown_value = 0_u128;
        while map.dense_for(key(unknown_value)).is_some() {
            unknown_value += 1;
        }
        let rule = if choose_unknown { key(unknown_value) } else { keys[0] };
        let nodes = vec![
            PortableProofNode::input(),
            PortableProofNode::rule(rule, vec![0]),
        ];
        let result = PortableWitness::from_nodes(&map, nodes);
        prop_assert_eq!(result.is_ok(), map.dense_for(rule).is_some());
    }

    #[test]
    fn prop_portable_codec_is_deterministic(
        raw in prop::collection::btree_set(any::<u128>(), 0..64),
    ) {
        let keys: Vec<_> = raw.into_iter().map(key).collect();
        let map = PortableRuleMap::seal(keys).unwrap();
        let first = map.encode_flat().unwrap();
        let second = map.encode_flat().unwrap();
        prop_assert_eq!(&first, &second);
        prop_assert_eq!(PortableRuleMap::decode_flat(&first).unwrap(), map);
    }

    #[test]
    fn prop_declared_work_models_are_linear(
        rules in 0usize..4096,
        transitions in 0usize..4096,
        nodes in 0usize..4096,
        edges in 0usize..8192,
        pending in 0usize..4096,
        bytes in 0usize..65536,
    ) {
        let shape = PortableWorkShape {
            rules,
            transitions,
            proof_nodes: nodes,
            premise_edges: edges,
            pending_deltas: pending,
            encoded_bytes: bytes,
        };
        prop_assert_eq!(shape.radix_map_work(), 16 * rules);
        prop_assert_eq!(shape.replay_work(), nodes + edges + rules);
        prop_assert_eq!(shape.codec_work(), bytes + nodes + edges + rules);
        prop_assert_eq!(
            shape.explicit_heap_items(),
            rules + transitions + nodes + edges + pending,
        );
    }
}

#[test]
fn prop_iterative_release_is_stack_safe_for_deep_flat_witnesses() {
    let map = PortableRuleMap::seal(vec![key(1)]).unwrap();
    let mut nodes = Vec::with_capacity(100_000);
    nodes.push(PortableProofNode::input());
    for index in 1..100_000_u32 {
        nodes.push(PortableProofNode::rule(key(1), vec![index - 1]));
    }
    let witness = PortableWitness::from_nodes(&map, nodes).unwrap();
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || drop(witness))
        .unwrap()
        .join()
        .expect("flat witness release must not consume logical-depth stack");
}
