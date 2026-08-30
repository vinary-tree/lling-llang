//! Bit-precise bounded model of portable WPDS maps and flat replay machines.

const MAX_RULES: usize = 3;
const MAX_PREMISES: usize = 2;
const MAX_RELEASE_SLOTS: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExternalRuleKey {
    value: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DenseRuleMap {
    entries: [Option<ExternalRuleKey>; MAX_RULES],
    len: u8,
}

impl DenseRuleMap {
    fn external_for(&self, dense: u8) -> Option<ExternalRuleKey> {
        if dense < self.len {
            self.entries[usize::from(dense)]
        } else {
            None
        }
    }

    fn dense_for(&self, key: ExternalRuleKey) -> Option<u8> {
        let mut dense = 0_u8;
        while dense < self.len {
            if self.entries[usize::from(dense)] == Some(key) {
                return Some(dense);
            }
            dense += 1;
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MapError {
    DuplicateExternalKey,
    TooManyRules,
}

fn seal_rule_map(keys: [ExternalRuleKey; MAX_RULES], len: u8) -> Result<DenseRuleMap, MapError> {
    if usize::from(len) > MAX_RULES {
        return Err(MapError::TooManyRules);
    }

    let mut map = DenseRuleMap {
        entries: [None; MAX_RULES],
        len: 0,
    };
    let mut index = 0_usize;
    while index < usize::from(len) {
        let mut earlier = 0_usize;
        while earlier < index {
            if keys[earlier] == keys[index] {
                return Err(MapError::DuplicateExternalKey);
            }
            earlier += 1;
        }
        map.entries[index] = Some(keys[index]);
        map.len += 1;
        index += 1;
    }
    Ok(map)
}

#[kani::proof]
#[kani::unwind(4)]
fn accepted_rule_maps_are_bijective_and_duplicates_are_rejected() {
    let keys = [
        ExternalRuleKey { value: kani::any() },
        ExternalRuleKey { value: kani::any() },
        ExternalRuleKey { value: kani::any() },
    ];
    let len: u8 = kani::any();
    kani::assume(usize::from(len) <= MAX_RULES);

    match seal_rule_map(keys, len) {
        Ok(map) => {
            assert_eq!(map.len, len);
            let mut dense = 0_u8;
            while dense < len {
                let key = keys[usize::from(dense)];
                assert_eq!(map.external_for(dense), Some(key));
                assert_eq!(map.dense_for(key), Some(dense));
                let mut other = dense + 1;
                while other < len {
                    assert_ne!(key, keys[usize::from(other)]);
                    other += 1;
                }
                dense += 1;
            }
        }
        Err(MapError::DuplicateExternalKey) => {
            let mut duplicate_exists = false;
            let mut right = 0_usize;
            while right < usize::from(len) {
                let mut left = 0_usize;
                while left < right {
                    duplicate_exists |= keys[left] == keys[right];
                    left += 1;
                }
                right += 1;
            }
            assert!(duplicate_exists);
        }
        Err(MapError::TooManyRules) => unreachable!(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReplayIdentity {
    rule_snapshot: u8,
    context: u8,
    query: u8,
    semantics: u8,
    codec_profile: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReplayChecks {
    well_formed: bool,
    checksum_valid: bool,
    within_budget: bool,
    witness_valid: bool,
    cancellation_reason: u8,
}

fn replay_admitted(
    expected: ReplayIdentity,
    observed: ReplayIdentity,
    checks: ReplayChecks,
) -> bool {
    expected == observed
        && checks.well_formed
        && checks.checksum_valid
        && checks.within_budget
        && checks.witness_valid
        && checks.cancellation_reason == 0
}

#[kani::proof]
fn replay_admission_requires_the_complete_identity_and_all_checks() {
    let expected = ReplayIdentity {
        rule_snapshot: kani::any(),
        context: kani::any(),
        query: kani::any(),
        semantics: kani::any(),
        codec_profile: kani::any(),
    };
    let observed = ReplayIdentity {
        rule_snapshot: kani::any(),
        context: kani::any(),
        query: kani::any(),
        semantics: kani::any(),
        codec_profile: kani::any(),
    };
    let checks = ReplayChecks {
        well_formed: kani::any(),
        checksum_valid: kani::any(),
        within_budget: kani::any(),
        witness_valid: kani::any(),
        cancellation_reason: kani::any(),
    };
    let expected_result = expected == observed
        && checks.well_formed
        && checks.checksum_valid
        && checks.within_budget
        && checks.witness_valid
        && checks.cancellation_reason == 0;
    assert_eq!(replay_admitted(expected, observed, checks), expected_result);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DecoderState {
    cursor: u16,
    nodes: u16,
    edges: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DecodeBudgets {
    bytes: u16,
    nodes: u16,
    edges: u16,
}

fn checked_decode_step(
    state: DecoderState,
    width: u16,
    added_nodes: u16,
    added_edges: u16,
    budgets: DecodeBudgets,
) -> Option<DecoderState> {
    if width == 0 {
        return None;
    }
    let next = DecoderState {
        cursor: state.cursor.checked_add(width)?,
        nodes: state.nodes.checked_add(added_nodes)?,
        edges: state.edges.checked_add(added_edges)?,
    };
    if next.cursor > budgets.bytes || next.nodes > budgets.nodes || next.edges > budgets.edges {
        None
    } else {
        Some(next)
    }
}

#[kani::proof]
fn accepted_decoder_steps_are_positive_bounded_and_overflow_free() {
    let state = DecoderState {
        cursor: kani::any(),
        nodes: kani::any(),
        edges: kani::any(),
    };
    let budgets = DecodeBudgets {
        bytes: kani::any(),
        nodes: kani::any(),
        edges: kani::any(),
    };
    let width: u16 = kani::any();
    let added_nodes: u16 = kani::any();
    let added_edges: u16 = kani::any();

    if let Some(next) = checked_decode_step(state, width, added_nodes, added_edges, budgets) {
        assert!(width > 0);
        assert!(next.cursor > state.cursor);
        assert_eq!(next.cursor, state.cursor + width);
        assert_eq!(next.nodes, state.nodes + added_nodes);
        assert_eq!(next.edges, state.edges + added_edges);
        assert!(next.cursor <= budgets.bytes);
        assert!(next.nodes <= budgets.nodes);
        assert!(next.edges <= budgets.edges);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PortableProofNode {
    has_rule: bool,
    rule_key: ExternalRuleKey,
    premise_count: u8,
    premises: [u8; MAX_PREMISES],
}

fn proof_node_valid(map: DenseRuleMap, node_index: u8, node: PortableProofNode) -> bool {
    if node.has_rule && map.dense_for(node.rule_key).is_none() {
        return false;
    }
    if usize::from(node.premise_count) > MAX_PREMISES {
        return false;
    }
    let mut premise = 0_usize;
    while premise < usize::from(node.premise_count) {
        if node.premises[premise] >= node_index {
            return false;
        }
        premise += 1;
    }
    true
}

#[kani::proof]
#[kani::unwind(4)]
fn accepted_proof_nodes_use_known_keys_and_earlier_premises() {
    let keys = [
        ExternalRuleKey { value: kani::any() },
        ExternalRuleKey { value: kani::any() },
        ExternalRuleKey { value: kani::any() },
    ];
    let len: u8 = kani::any();
    kani::assume(usize::from(len) <= MAX_RULES);
    let Ok(map) = seal_rule_map(keys, len) else {
        return;
    };
    let node_index: u8 = kani::any();
    let node = PortableProofNode {
        has_rule: kani::any(),
        rule_key: ExternalRuleKey { value: kani::any() },
        premise_count: kani::any(),
        premises: kani::any(),
    };

    if proof_node_valid(map, node_index, node) {
        if node.has_rule {
            assert!(map.dense_for(node.rule_key).is_some());
        }
        assert!(usize::from(node.premise_count) <= MAX_PREMISES);
        let mut premise = 0_usize;
        while premise < usize::from(node.premise_count) {
            assert!(node.premises[premise] < node_index);
            premise += 1;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReleaseMachine {
    live: [bool; MAX_RELEASE_SLOTS],
    cursor: u8,
    len: u8,
}

fn release_step(machine: &mut ReleaseMachine) -> bool {
    if machine.cursor >= machine.len {
        return false;
    }
    let slot = usize::from(machine.cursor);
    machine.live[slot] = false;
    machine.cursor += 1;
    true
}

#[kani::proof]
#[kani::unwind(5)]
fn bounded_release_machine_is_iterative_and_single_pass() {
    let len: u8 = kani::any();
    kani::assume(usize::from(len) <= MAX_RELEASE_SLOTS);
    let mut machine = ReleaseMachine {
        live: [true; MAX_RELEASE_SLOTS],
        cursor: 0,
        len,
    };
    let mut steps = 0_usize;
    while steps < MAX_RELEASE_SLOTS {
        let _ = release_step(&mut machine);
        steps += 1;
    }
    assert_eq!(machine.cursor, len);
    let mut released = 0_usize;
    while released < usize::from(len) {
        assert!(!machine.live[released]);
        released += 1;
    }
    let complete = machine;
    assert!(!release_step(&mut machine));
    assert_eq!(machine, complete);
}

#[kani::proof]
fn rejected_replay_inputs_never_publish() {
    let identity = ReplayIdentity {
        rule_snapshot: kani::any(),
        context: kani::any(),
        query: kani::any(),
        semantics: kani::any(),
        codec_profile: kani::any(),
    };
    let observed = ReplayIdentity {
        rule_snapshot: kani::any(),
        context: kani::any(),
        query: kani::any(),
        semantics: kani::any(),
        codec_profile: kani::any(),
    };
    let checks = ReplayChecks {
        well_formed: kani::any(),
        checksum_valid: kani::any(),
        within_budget: kani::any(),
        witness_valid: kani::any(),
        cancellation_reason: kani::any(),
    };
    let rejected = identity != observed
        || !checks.well_formed
        || !checks.checksum_valid
        || !checks.within_budget
        || !checks.witness_valid
        || checks.cancellation_reason != 0;
    if rejected {
        assert!(!replay_admitted(identity, observed, checks));
    }
}
