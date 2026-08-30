//! Stable rule identities and bounded, stack-safe WPDS proof witnesses.
//!
//! The saturation hot path may continue to use dense process-local rule IDs.
//! This module owns the portability boundary: external 128-bit rule keys are
//! sealed into an immutable bijection, encoded canonically, and used by flat
//! proof witnesses whose premise edges always point backward.

use std::fmt;
use std::sync::OnceLock;

const RULE_MAP_MAGIC: &[u8; 8] = b"LLWPRM01";
const WITNESS_MAGIC: &[u8; 8] = b"LLWPWT01";
const CHECKSUM_BYTES: usize = 32;

/// Exact caller-owned rule identity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct PortableRuleKey([u8; 16]);

impl PortableRuleKey {
    /// Construct a key without shortening or hashing its bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Return the exact portable representation.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    const fn ordered_word(self) -> u128 {
        u128::from_be_bytes(self.0)
    }
}

/// Typed rejection at the untrusted portability boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReplayRejection {
    /// Two dense positions were assigned the same external identity.
    DuplicateExternalRuleKey,
    /// A rule map cannot be represented by the versioned codec.
    TooManyRules,
    /// An encoded count or length overflowed its representation.
    LengthOverflow,
    /// The byte stream does not follow the canonical wire grammar.
    MalformedEncoding,
    /// The stream belongs to another codec or version.
    UnsupportedVersion,
    /// The encoded payload has trailing or otherwise non-canonical bytes.
    NonCanonicalEncoding,
    /// The payload checksum does not match its bytes.
    ChecksumMismatch,
    /// The input exceeds its explicit byte budget.
    ByteBudgetExceeded,
    /// The input exceeds its explicit proof-node budget.
    NodeBudgetExceeded,
    /// The input exceeds its explicit premise-edge budget.
    EdgeBudgetExceeded,
    /// A proof inference names a rule outside the sealed snapshot.
    UnknownExternalRuleKey,
    /// A premise does not precede its conclusion in the flat proof order.
    PremiseNotEarlier,
}

impl fmt::Display for ReplayRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::DuplicateExternalRuleKey => "duplicate external WPDS rule key",
            Self::TooManyRules => "too many WPDS rules for the portable codec",
            Self::LengthOverflow => "portable WPDS length overflow",
            Self::MalformedEncoding => "malformed portable WPDS encoding",
            Self::UnsupportedVersion => "unsupported portable WPDS codec version",
            Self::NonCanonicalEncoding => "non-canonical portable WPDS encoding",
            Self::ChecksumMismatch => "portable WPDS checksum mismatch",
            Self::ByteBudgetExceeded => "portable WPDS byte budget exceeded",
            Self::NodeBudgetExceeded => "portable WPDS node budget exceeded",
            Self::EdgeBudgetExceeded => "portable WPDS edge budget exceeded",
            Self::UnknownExternalRuleKey => "portable witness names an unknown WPDS rule key",
            Self::PremiseNotEarlier => "portable witness premise does not precede its conclusion",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ReplayRejection {}

/// Immutable external-key/dense-ID bijection for one rule snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortableRuleMap {
    external_for: Vec<PortableRuleKey>,
    // The key is converted once to a big-endian word, so each lookup compares
    // the full 128-bit identity without repeatedly walking byte slices.
    ordered_index: Vec<(u128, usize)>,
    snapshot_digest: [u8; 32],
}

impl PortableRuleMap {
    /// Seal keys in dense-rule order while building a deterministic radix index.
    pub fn seal(external_for: Vec<PortableRuleKey>) -> Result<Self, ReplayRejection> {
        if external_for.len() > u32::MAX as usize {
            return Err(ReplayRejection::TooManyRules);
        }

        let mut ordered = external_for
            .iter()
            .copied()
            .enumerate()
            .map(|(dense, key)| (key, dense))
            .collect::<Vec<_>>();
        radix_sort_rule_keys(&mut ordered);
        if ordered.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(ReplayRejection::DuplicateExternalRuleKey);
        }

        let ordered_index = ordered
            .into_iter()
            .map(|(key, dense)| (key.ordered_word(), dense))
            .collect();
        let snapshot_digest = rule_snapshot_digest(&external_for);
        Ok(Self {
            external_for,
            ordered_index,
            snapshot_digest,
        })
    }

    /// Number of rules in the sealed snapshot.
    #[must_use]
    pub fn len(&self) -> usize {
        self.external_for.len()
    }

    /// Whether the sealed snapshot contains no rules.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.external_for.is_empty()
    }

    /// Resolve a dense process-local position to its portable identity.
    #[must_use]
    pub fn external_for(&self, dense: usize) -> Option<PortableRuleKey> {
        self.external_for.get(dense).copied()
    }

    /// Resolve a portable identity to its dense process-local position.
    #[must_use]
    pub fn dense_for(&self, external: PortableRuleKey) -> Option<usize> {
        let needle = external.ordered_word();
        self.ordered_index
            .binary_search_by_key(&needle, |&(key, _)| key)
            .ok()
            .map(|index| self.ordered_index[index].1)
    }

    /// Digest of the ordered external-key tape and codec domain.
    #[must_use]
    pub const fn snapshot_digest(&self) -> [u8; 32] {
        self.snapshot_digest
    }

    /// Encode the sealed key tape in canonical dense order.
    pub fn encode_flat(&self) -> Result<Vec<u8>, ReplayRejection> {
        let count = u32::try_from(self.len()).map_err(|_| ReplayRejection::TooManyRules)?;
        let payload_bytes = 12usize
            .checked_add(
                self.len()
                    .checked_mul(16)
                    .ok_or(ReplayRejection::LengthOverflow)?,
            )
            .ok_or(ReplayRejection::LengthOverflow)?;
        let capacity = payload_bytes
            .checked_add(CHECKSUM_BYTES)
            .ok_or(ReplayRejection::LengthOverflow)?;
        let mut bytes = Vec::with_capacity(capacity);
        bytes.extend_from_slice(RULE_MAP_MAGIC);
        bytes.extend_from_slice(&count.to_le_bytes());
        for key in &self.external_for {
            bytes.extend_from_slice(key.as_bytes());
        }
        append_checksum(&mut bytes);
        Ok(bytes)
    }

    /// Decode and re-seal a canonical key tape.
    pub fn decode_flat(bytes: &[u8]) -> Result<Self, ReplayRejection> {
        let payload = checked_payload(bytes)?;
        if payload.get(..8) != Some(RULE_MAP_MAGIC) {
            return Err(ReplayRejection::UnsupportedVersion);
        }
        let count = read_u32_at(payload, 8)? as usize;
        let expected = 12usize
            .checked_add(
                count
                    .checked_mul(16)
                    .ok_or(ReplayRejection::LengthOverflow)?,
            )
            .ok_or(ReplayRejection::LengthOverflow)?;
        if payload.len() != expected {
            return Err(ReplayRejection::NonCanonicalEncoding);
        }
        let mut keys = Vec::with_capacity(count);
        let mut cursor = 12usize;
        for _ in 0..count {
            let end = cursor
                .checked_add(16)
                .ok_or(ReplayRejection::LengthOverflow)?;
            let raw: [u8; 16] = payload
                .get(cursor..end)
                .ok_or(ReplayRejection::MalformedEncoding)?
                .try_into()
                .map_err(|_| ReplayRejection::MalformedEncoding)?;
            keys.push(PortableRuleKey::from_bytes(raw));
            cursor = end;
        }
        let map = Self::seal(keys)?;
        if map.encode_flat()?.as_slice() != bytes {
            return Err(ReplayRejection::NonCanonicalEncoding);
        }
        Ok(map)
    }
}

/// Exact identity required before a decoded computation may publish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortableReplayIdentity {
    /// Ordered rule snapshot and normalized rules.
    pub rule_snapshot: [u8; 32],
    /// Caller-defined execution context.
    pub context: [u8; 32],
    /// Query and saturation direction.
    pub query: [u8; 32],
    /// Weight and inference semantics.
    pub semantics: [u8; 32],
    /// Wire version and canonicalization profile.
    pub codec_profile: [u8; 32],
}

impl PortableReplayIdentity {
    /// Admit publication only when every identity and validation fact agrees.
    #[must_use]
    pub fn admits(&self, observed: &Self, checks: PortableReplayChecks) -> bool {
        self == observed
            && checks.well_formed
            && checks.checksum_valid
            && checks.within_budget
            && checks.witness_valid
            && checks.cancellation_reason.is_none()
    }
}

/// Validation facts gathered before replay publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortableReplayChecks {
    /// The payload is syntactically canonical.
    pub well_formed: bool,
    /// The complete payload checksum is valid.
    pub checksum_valid: bool,
    /// All caller-declared resource budgets were respected.
    pub within_budget: bool,
    /// Every rule inference and premise edge was validated.
    pub witness_valid: bool,
    /// First cancellation reason, if cancellation was requested.
    pub cancellation_reason: Option<u32>,
}

/// Atomic, first-writer-sticky cancellation state.
#[derive(Debug, Default)]
pub struct PortableCancellation {
    reason: OnceLock<u32>,
}

impl PortableCancellation {
    /// Create an uncancelled request state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            reason: OnceLock::new(),
        }
    }

    /// Request cancellation. Later requests cannot replace the first reason.
    pub fn request(&self, reason: u32) {
        let _ = self.reason.set(reason);
    }

    /// Return the first reason, including reason zero.
    #[must_use]
    pub fn reason(&self) -> Option<u32> {
        self.reason.get().copied()
    }

    /// Whether cancellation has been requested.
    #[must_use]
    pub fn is_requested(&self) -> bool {
        self.reason.get().is_some()
    }
}

/// Independent limits for untrusted witness decoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortableDecodeLimits {
    /// Maximum complete encoded length, including checksum.
    pub max_bytes: usize,
    /// Maximum number of proof nodes.
    pub max_nodes: usize,
    /// Maximum aggregate number of premise edges.
    pub max_edges: usize,
}

/// One node in premises-first flat proof order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortableProofNode {
    /// Caller-supplied input fact.
    Input,
    /// Inference by an external rule key and earlier premise indices.
    Rule {
        /// Stable rule identity; never a process-local dense ID.
        rule: PortableRuleKey,
        /// Indices strictly smaller than this node's index.
        premises: Vec<u32>,
    },
}

impl PortableProofNode {
    /// Construct an input fact.
    #[must_use]
    pub const fn input() -> Self {
        Self::Input
    }

    /// Construct a rule inference. [`PortableWitness::from_nodes`] validates it.
    #[must_use]
    pub fn rule(rule: PortableRuleKey, premises: Vec<u32>) -> Self {
        Self::Rule { rule, premises }
    }

    /// Stable rule identity for an inference node.
    #[must_use]
    pub const fn rule_key(&self) -> Option<PortableRuleKey> {
        match self {
            Self::Input => None,
            Self::Rule { rule, .. } => Some(*rule),
        }
    }

    /// Premise indices; input facts have no premises.
    #[must_use]
    pub fn premises(&self) -> &[u32] {
        match self {
            Self::Input => &[],
            Self::Rule { premises, .. } => premises,
        }
    }
}

/// Resource usage observed during a bounded flat decode.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PortableDecodeUsage {
    /// Complete encoded bytes consumed.
    pub bytes: usize,
    /// Proof nodes decoded.
    pub nodes: usize,
    /// Premise edges decoded.
    pub edges: usize,
    /// Positive-width cursor advances.
    pub positive_steps: usize,
}

/// Validated, flat proof DAG with stack-safe destruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortableWitness {
    nodes: Vec<PortableProofNode>,
    decode_usage: PortableDecodeUsage,
}

impl PortableWitness {
    /// Validate rule membership and premises-first order.
    pub fn from_nodes(
        rule_map: &PortableRuleMap,
        nodes: Vec<PortableProofNode>,
    ) -> Result<Self, ReplayRejection> {
        validate_nodes(Some(rule_map), &nodes)?;
        let edges = nodes.iter().fold(0usize, |total, node| {
            total.saturating_add(node.premises().len())
        });
        Ok(Self {
            decode_usage: PortableDecodeUsage {
                bytes: 0,
                nodes: nodes.len(),
                edges,
                positive_steps: 0,
            },
            nodes,
        })
    }

    /// Validate decoded external rule keys against a particular sealed snapshot.
    pub fn validate_for(&self, rule_map: &PortableRuleMap) -> Result<(), ReplayRejection> {
        validate_nodes(Some(rule_map), &self.nodes)
    }

    /// Flat proof nodes in premises-first order.
    #[must_use]
    pub fn nodes(&self) -> &[PortableProofNode] {
        &self.nodes
    }

    /// Usage of the successful decode that created this witness.
    #[must_use]
    pub const fn decode_usage(&self) -> PortableDecodeUsage {
        self.decode_usage
    }

    /// Encode the flat proof DAG canonically with a BLAKE3 checksum.
    pub fn encode_flat(&self) -> Result<Vec<u8>, ReplayRejection> {
        let count =
            u32::try_from(self.nodes.len()).map_err(|_| ReplayRejection::NodeBudgetExceeded)?;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(WITNESS_MAGIC);
        bytes.extend_from_slice(&count.to_le_bytes());
        for node in &self.nodes {
            match node {
                PortableProofNode::Input => bytes.push(0),
                PortableProofNode::Rule { rule, premises } => {
                    let premise_count = u32::try_from(premises.len())
                        .map_err(|_| ReplayRejection::EdgeBudgetExceeded)?;
                    bytes.push(1);
                    bytes.extend_from_slice(rule.as_bytes());
                    bytes.extend_from_slice(&premise_count.to_le_bytes());
                    for premise in premises {
                        bytes.extend_from_slice(&premise.to_le_bytes());
                    }
                }
            }
        }
        append_checksum(&mut bytes);
        Ok(bytes)
    }

    /// Decode a canonical flat proof DAG under explicit hard limits.
    pub fn decode_flat(
        bytes: &[u8],
        limits: PortableDecodeLimits,
    ) -> Result<Self, ReplayRejection> {
        if bytes.len() > limits.max_bytes {
            return Err(ReplayRejection::ByteBudgetExceeded);
        }
        let payload = checked_payload(bytes)?;
        let mut decoder = Decoder::new(payload);
        if decoder.take(8)? != WITNESS_MAGIC {
            return Err(ReplayRejection::UnsupportedVersion);
        }
        let count = decoder.u32()? as usize;
        if count > limits.max_nodes {
            return Err(ReplayRejection::NodeBudgetExceeded);
        }
        let mut nodes = Vec::with_capacity(count);
        let mut edge_count = 0usize;
        for index in 0..count {
            match decoder.byte()? {
                0 => nodes.push(PortableProofNode::Input),
                1 => {
                    let rule = PortableRuleKey::from_bytes(
                        decoder
                            .take(16)?
                            .try_into()
                            .map_err(|_| ReplayRejection::MalformedEncoding)?,
                    );
                    let premise_count = decoder.u32()? as usize;
                    edge_count = edge_count
                        .checked_add(premise_count)
                        .ok_or(ReplayRejection::LengthOverflow)?;
                    if edge_count > limits.max_edges {
                        return Err(ReplayRejection::EdgeBudgetExceeded);
                    }
                    let premise_bytes = premise_count
                        .checked_mul(4)
                        .ok_or(ReplayRejection::LengthOverflow)?;
                    let raw_premises = decoder.take(premise_bytes)?;
                    let mut premises = Vec::with_capacity(premise_count);
                    for raw in raw_premises.chunks_exact(4) {
                        let premise = u32::from_le_bytes(
                            raw.try_into()
                                .map_err(|_| ReplayRejection::MalformedEncoding)?,
                        );
                        if premise as usize >= index {
                            return Err(ReplayRejection::PremiseNotEarlier);
                        }
                        premises.push(premise);
                    }
                    nodes.push(PortableProofNode::Rule { rule, premises });
                }
                _ => return Err(ReplayRejection::MalformedEncoding),
            }
        }
        if !decoder.is_finished() {
            return Err(ReplayRejection::NonCanonicalEncoding);
        }
        validate_nodes(None, &nodes)?;
        Ok(Self {
            decode_usage: PortableDecodeUsage {
                bytes: bytes.len(),
                nodes: nodes.len(),
                edges: edge_count,
                positive_steps: decoder.positive_steps,
            },
            nodes,
        })
    }
}

/// Representation-level work and explicit heap-space model.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PortableWorkShape {
    /// Rules in the sealed snapshot.
    pub rules: usize,
    /// WPDS transitions retained by the native engine.
    pub transitions: usize,
    /// Flat proof nodes.
    pub proof_nodes: usize,
    /// Flat premise edges.
    pub premise_edges: usize,
    /// Pending saturation deltas.
    pub pending_deltas: usize,
    /// Complete encoded bytes.
    pub encoded_bytes: usize,
}

impl PortableWorkShape {
    /// Sixteen stable-radix passes over fixed-width rule keys.
    #[must_use]
    pub fn radix_map_work(self) -> usize {
        self.rules.saturating_mul(16)
    }

    /// Flat replay plus rule-map validation.
    #[must_use]
    pub fn replay_work(self) -> usize {
        self.proof_nodes
            .saturating_add(self.premise_edges)
            .saturating_add(self.rules)
    }

    /// Flat codec, proof, and rule-map work.
    #[must_use]
    pub fn codec_work(self) -> usize {
        self.encoded_bytes
            .saturating_add(self.proof_nodes)
            .saturating_add(self.premise_edges)
            .saturating_add(self.rules)
    }

    /// Explicit heap items; logical depth never becomes native call depth.
    #[must_use]
    pub fn explicit_heap_items(self) -> usize {
        self.rules
            .saturating_add(self.transitions)
            .saturating_add(self.proof_nodes)
            .saturating_add(self.premise_edges)
            .saturating_add(self.pending_deltas)
    }
}

fn radix_sort_rule_keys(values: &mut Vec<(PortableRuleKey, usize)>) {
    if values.len() < 2 {
        return;
    }
    let mut scratch = vec![(PortableRuleKey::default(), 0); values.len()];
    for byte_index in (0..16).rev() {
        let mut offsets = [0usize; 256];
        for (key, _) in values.iter() {
            offsets[key.0[byte_index] as usize] += 1;
        }
        let mut total = 0usize;
        for offset in &mut offsets {
            let count = *offset;
            *offset = total;
            total += count;
        }
        for value in values.iter().copied() {
            let bucket = value.0 .0[byte_index] as usize;
            scratch[offsets[bucket]] = value;
            offsets[bucket] += 1;
        }
        std::mem::swap(values, &mut scratch);
    }
}

fn rule_snapshot_digest(keys: &[PortableRuleKey]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"lling-llang/wpds/rule-map/v1\0");
    hasher.update(&(keys.len() as u64).to_le_bytes());
    for key in keys {
        hasher.update(key.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn append_checksum(bytes: &mut Vec<u8>) {
    let checksum = blake3::hash(bytes);
    bytes.extend_from_slice(checksum.as_bytes());
}

fn checked_payload(bytes: &[u8]) -> Result<&[u8], ReplayRejection> {
    let payload_end = bytes
        .len()
        .checked_sub(CHECKSUM_BYTES)
        .ok_or(ReplayRejection::MalformedEncoding)?;
    let (payload, checksum) = bytes.split_at(payload_end);
    if blake3::hash(payload).as_bytes() != checksum {
        return Err(ReplayRejection::ChecksumMismatch);
    }
    Ok(payload)
}

fn read_u32_at(bytes: &[u8], offset: usize) -> Result<u32, ReplayRejection> {
    let end = offset
        .checked_add(4)
        .ok_or(ReplayRejection::LengthOverflow)?;
    let raw: [u8; 4] = bytes
        .get(offset..end)
        .ok_or(ReplayRejection::MalformedEncoding)?
        .try_into()
        .map_err(|_| ReplayRejection::MalformedEncoding)?;
    Ok(u32::from_le_bytes(raw))
}

fn validate_nodes(
    rule_map: Option<&PortableRuleMap>,
    nodes: &[PortableProofNode],
) -> Result<(), ReplayRejection> {
    for (index, node) in nodes.iter().enumerate() {
        if let PortableProofNode::Rule { rule, premises } = node {
            if rule_map.is_some_and(|map| map.dense_for(*rule).is_none()) {
                return Err(ReplayRejection::UnknownExternalRuleKey);
            }
            if premises.iter().any(|&premise| premise as usize >= index) {
                return Err(ReplayRejection::PremiseNotEarlier);
            }
        }
    }
    Ok(())
}

struct Decoder<'a> {
    bytes: &'a [u8],
    cursor: usize,
    positive_steps: usize,
}

impl<'a> Decoder<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            cursor: 0,
            positive_steps: 0,
        }
    }

    fn take(&mut self, width: usize) -> Result<&'a [u8], ReplayRejection> {
        if width == 0 {
            return Ok(&[]);
        }
        let end = self
            .cursor
            .checked_add(width)
            .ok_or(ReplayRejection::LengthOverflow)?;
        let value = self
            .bytes
            .get(self.cursor..end)
            .ok_or(ReplayRejection::MalformedEncoding)?;
        self.cursor = end;
        self.positive_steps = self.positive_steps.saturating_add(1);
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, ReplayRejection> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, ReplayRejection> {
        let raw: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| ReplayRejection::MalformedEncoding)?;
        Ok(u32::from_le_bytes(raw))
    }

    fn is_finished(&self) -> bool {
        self.cursor == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: u128) -> PortableRuleKey {
        PortableRuleKey::from_bytes(value.to_be_bytes())
    }

    #[test]
    fn rule_map_round_trip_preserves_dense_order() {
        let map = PortableRuleMap::seal(vec![key(9), key(1), key(4)]).unwrap();
        let bytes = map.encode_flat().unwrap();
        assert_eq!(PortableRuleMap::decode_flat(&bytes), Ok(map));
    }

    #[test]
    fn witness_round_trip_is_flat_and_bounded() {
        let map = PortableRuleMap::seal(vec![key(1)]).unwrap();
        let witness = PortableWitness::from_nodes(
            &map,
            vec![
                PortableProofNode::input(),
                PortableProofNode::rule(key(1), vec![0]),
            ],
        )
        .unwrap();
        let bytes = witness.encode_flat().unwrap();
        let decoded = PortableWitness::decode_flat(
            &bytes,
            PortableDecodeLimits {
                max_bytes: bytes.len(),
                max_nodes: 2,
                max_edges: 1,
            },
        )
        .unwrap();
        decoded.validate_for(&map).unwrap();
        assert_eq!(decoded.nodes(), witness.nodes());
    }

    #[test]
    fn malformed_witness_never_allocates_from_an_unchecked_count() {
        let mut bytes = WITNESS_MAGIC.to_vec();
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        append_checksum(&mut bytes);
        assert_eq!(
            PortableWitness::decode_flat(
                &bytes,
                PortableDecodeLimits {
                    max_bytes: bytes.len(),
                    max_nodes: 8,
                    max_edges: 8,
                },
            ),
            Err(ReplayRejection::NodeBudgetExceeded)
        );
    }
}
