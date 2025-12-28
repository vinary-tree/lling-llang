//! Token recombination with uint64 packing.
//!
//! This module provides efficient token recombination using uint64 packing,
//! enabling atomic operations without precision loss.
//!
//! ## Problem
//!
//! During Viterbi decoding, multiple tokens may reach the same state. We need to:
//! 1. Keep only the best token (lowest cost)
//! 2. Handle concurrent updates from parallel threads
//! 3. Preserve full precision for costs
//!
//! ## Solution: uint64 Packing
//!
//! Pack cost and arc ID into a single 64-bit value:
//!
//! ```text
//! |<------ 32 bits ------>|<------ 32 bits ------>|
//! |     cost (f32)        |      arc_id (u32)     |
//! ```
//!
//! The cost is stored in the high bits so that atomic min operations
//! naturally select the lowest-cost token.
//!
//! ## Algorithm
//!
//! ```text
//! procedure RECOMBINE(cost, arc_id, state):
//!     old_packed = state_to_token[state]
//!     new_packed = pack(cost, arc_id)
//!     result = atomic_min(state_to_token[state], new_packed)
//!     if result > new_packed:
//!         // This token won, store in per-arc buffer
//!         per_arc_buffer[arc_id] = token
//! ```
//!
//! ## Benefits
//!
//! - **No precision loss**: Full 32-bit float precision preserved
//! - **Lock-free**: Uses atomic min operation
//! - **No write conflicts**: Per-arc buffer eliminates contention
//!
//! ## References
//!
//! - Chen et al., "GPU-based WFST Decoder with Exact Lattice Generation" (2018)

use std::sync::atomic::{AtomicU64, Ordering};

/// A token packed into 64 bits for atomic operations.
///
/// Layout: [cost: 32 bits (high)] [arc_id: 32 bits (low)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedToken(u64);

impl PackedToken {
    /// Create a packed token with no value (infinity cost).
    pub const EMPTY: PackedToken = PackedToken(u64::MAX);

    /// Create a new packed token.
    pub fn new(cost: f32, arc_id: u32) -> Self {
        Self(pack_cost_arc(cost, arc_id))
    }

    /// Get the cost.
    pub fn cost(self) -> f32 {
        // Handle EMPTY case specially - u64::MAX unpacks to a finite value
        if self.0 == u64::MAX {
            return f32::INFINITY;
        }
        let (cost, _) = unpack_cost_arc(self.0);
        cost
    }

    /// Get the arc ID.
    pub fn arc_id(self) -> u32 {
        let (_, arc_id) = unpack_cost_arc(self.0);
        arc_id
    }

    /// Get the raw packed value.
    pub fn raw(self) -> u64 {
        self.0
    }

    /// Create from raw packed value.
    pub fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Check if this is an empty token.
    pub fn is_empty(self) -> bool {
        self.0 == u64::MAX
    }

    /// Check if this token is better (lower cost) than another.
    pub fn is_better_than(self, other: PackedToken) -> bool {
        self.0 < other.0
    }
}

impl Default for PackedToken {
    fn default() -> Self {
        Self::EMPTY
    }
}

/// Pack a cost and arc ID into a u64.
///
/// The cost is placed in the high 32 bits so that atomic min
/// operations naturally select lower costs.
///
/// # Arguments
///
/// * `cost` - The token cost (log probability, lower is better)
/// * `arc_id` - The arc identifier
///
/// # Returns
///
/// A packed u64 value.
///
/// # Note
///
/// For negative costs, we need to handle the sign bit carefully.
/// We use a transformation that preserves ordering:
/// - Positive floats: flip sign bit (0x80000000 XOR)
/// - Negative floats: flip all bits (NOT)
pub fn pack_cost_arc(cost: f32, arc_id: u32) -> u64 {
    let cost_bits = cost.to_bits();

    // Transform to preserve ordering under integer comparison
    // Positive floats: XOR with 0x80000000 to make them > negative
    // Negative floats: XOR with 0xFFFFFFFF to flip ordering
    let ordered_bits = if (cost_bits as i32) >= 0 {
        cost_bits ^ 0x8000_0000
    } else {
        !cost_bits
    };

    ((ordered_bits as u64) << 32) | (arc_id as u64)
}

/// Unpack a cost and arc ID from a u64.
///
/// # Arguments
///
/// * `packed` - The packed value
///
/// # Returns
///
/// A tuple of (cost, arc_id).
pub fn unpack_cost_arc(packed: u64) -> (f32, u32) {
    let ordered_bits = (packed >> 32) as u32;
    let arc_id = packed as u32;

    // Reverse the transformation
    let cost_bits = if (ordered_bits as i32) >= 0 {
        !ordered_bits
    } else {
        ordered_bits ^ 0x8000_0000
    };

    (f32::from_bits(cost_bits), arc_id)
}

/// Packer for converting between floats and ordered integers.
///
/// This utility handles the bit manipulation needed to make float
/// comparison work correctly with integer atomic operations.
#[derive(Clone, Copy, Debug, Default)]
pub struct TokenPacker;

impl TokenPacker {
    /// Create a new token packer.
    pub fn new() -> Self {
        Self
    }

    /// Pack a cost and arc ID.
    pub fn pack(&self, cost: f32, arc_id: u32) -> u64 {
        pack_cost_arc(cost, arc_id)
    }

    /// Unpack a cost and arc ID.
    pub fn unpack(&self, packed: u64) -> (f32, u32) {
        unpack_cost_arc(packed)
    }

    /// Create a packed token.
    pub fn create_token(&self, cost: f32, arc_id: u32) -> PackedToken {
        PackedToken::new(cost, arc_id)
    }
}

/// Buffer for token recombination.
///
/// This structure maintains the best token reaching each state,
/// using atomic operations for thread-safe updates.
#[derive(Debug)]
pub struct RecombinationBuffer {
    /// Packed tokens indexed by state ID.
    state_tokens: Vec<AtomicU64>,
    /// Per-arc token storage (for winning tokens).
    per_arc_tokens: Vec<AtomicU64>,
    /// Number of states.
    num_states: usize,
    /// Number of arcs.
    num_arcs: usize,
}

impl RecombinationBuffer {
    /// Create a new recombination buffer.
    ///
    /// # Arguments
    ///
    /// * `num_states` - Number of states in the WFST
    /// * `num_arcs` - Number of arcs in the WFST
    pub fn new(num_states: usize, num_arcs: usize) -> Self {
        Self {
            state_tokens: (0..num_states)
                .map(|_| AtomicU64::new(u64::MAX))
                .collect(),
            per_arc_tokens: (0..num_arcs)
                .map(|_| AtomicU64::new(u64::MAX))
                .collect(),
            num_states,
            num_arcs,
        }
    }

    /// Reset the buffer for a new frame.
    pub fn reset(&self) {
        for token in &self.state_tokens {
            token.store(u64::MAX, Ordering::Relaxed);
        }
        // Per-arc tokens don't need reset (overwritten when used)
    }

    /// Attempt to recombine a token at a state.
    ///
    /// # Arguments
    ///
    /// * `state` - The destination state
    /// * `cost` - The token cost
    /// * `arc_id` - The arc used to reach this state
    ///
    /// # Returns
    ///
    /// `true` if this token won (was better than existing), `false` otherwise.
    pub fn recombine(&self, state: usize, cost: f32, arc_id: u32) -> bool {
        let packed = pack_cost_arc(cost, arc_id);
        let old = self.state_tokens[state].fetch_min(packed, Ordering::AcqRel);

        if old > packed {
            // This token won, store in per-arc buffer
            self.per_arc_tokens[arc_id as usize].store(packed, Ordering::Release);
            true
        } else {
            false
        }
    }

    /// Get the best token for a state.
    pub fn get_token(&self, state: usize) -> Option<PackedToken> {
        let packed = self.state_tokens[state].load(Ordering::Acquire);
        if packed == u64::MAX {
            None
        } else {
            Some(PackedToken::from_raw(packed))
        }
    }

    /// Get the token stored for an arc (if it won).
    pub fn get_arc_token(&self, arc_id: u32) -> Option<PackedToken> {
        let packed = self.per_arc_tokens[arc_id as usize].load(Ordering::Acquire);
        if packed == u64::MAX {
            None
        } else {
            Some(PackedToken::from_raw(packed))
        }
    }

    /// Collect all surviving tokens (best token per state).
    pub fn collect_survivors(&self) -> Vec<(usize, PackedToken)> {
        self.state_tokens
            .iter()
            .enumerate()
            .filter_map(|(state, atomic)| {
                let packed = atomic.load(Ordering::Acquire);
                if packed == u64::MAX {
                    None
                } else {
                    Some((state, PackedToken::from_raw(packed)))
                }
            })
            .collect()
    }

    /// Get the number of active states.
    pub fn num_active(&self) -> usize {
        self.state_tokens
            .iter()
            .filter(|t| t.load(Ordering::Relaxed) != u64::MAX)
            .count()
    }

    /// Get buffer statistics.
    pub fn stats(&self) -> RecombinationStats {
        let active_states = self.num_active();
        RecombinationStats {
            num_states: self.num_states,
            num_arcs: self.num_arcs,
            active_states,
            recombination_ratio: if active_states > 0 {
                1.0 - (active_states as f64 / self.num_states as f64)
            } else {
                0.0
            },
        }
    }
}

/// Statistics about recombination.
#[derive(Clone, Debug)]
pub struct RecombinationStats {
    /// Total number of states.
    pub num_states: usize,
    /// Total number of arcs.
    pub num_arcs: usize,
    /// Number of active states (with tokens).
    pub active_states: usize,
    /// Recombination ratio (fraction of states combined).
    pub recombination_ratio: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_unpack_positive() {
        let cost = 1.5f32;
        let arc_id = 42u32;

        let packed = pack_cost_arc(cost, arc_id);
        let (unpacked_cost, unpacked_arc) = unpack_cost_arc(packed);

        assert!((unpacked_cost - cost).abs() < 1e-6);
        assert_eq!(unpacked_arc, arc_id);
    }

    #[test]
    fn test_pack_unpack_negative() {
        let cost = -2.5f32;
        let arc_id = 100u32;

        let packed = pack_cost_arc(cost, arc_id);
        let (unpacked_cost, unpacked_arc) = unpack_cost_arc(packed);

        assert!((unpacked_cost - cost).abs() < 1e-6);
        assert_eq!(unpacked_arc, arc_id);
    }

    #[test]
    fn test_pack_unpack_zero() {
        let cost = 0.0f32;
        let arc_id = 0u32;

        let packed = pack_cost_arc(cost, arc_id);
        let (unpacked_cost, unpacked_arc) = unpack_cost_arc(packed);

        assert!((unpacked_cost - cost).abs() < 1e-6);
        assert_eq!(unpacked_arc, arc_id);
    }

    #[test]
    fn test_ordering_positive_costs() {
        // Lower costs should have lower packed values
        let packed1 = pack_cost_arc(1.0, 0);
        let packed2 = pack_cost_arc(2.0, 0);
        let packed3 = pack_cost_arc(3.0, 0);

        assert!(packed1 < packed2);
        assert!(packed2 < packed3);
    }

    #[test]
    fn test_ordering_negative_costs() {
        // Lower (more negative) costs should have lower packed values
        let packed1 = pack_cost_arc(-3.0, 0);
        let packed2 = pack_cost_arc(-2.0, 0);
        let packed3 = pack_cost_arc(-1.0, 0);

        assert!(packed1 < packed2);
        assert!(packed2 < packed3);
    }

    #[test]
    fn test_ordering_mixed_costs() {
        // Negative costs are lower than positive
        let packed_neg = pack_cost_arc(-1.0, 0);
        let packed_zero = pack_cost_arc(0.0, 0);
        let packed_pos = pack_cost_arc(1.0, 0);

        assert!(packed_neg < packed_zero);
        assert!(packed_zero < packed_pos);
    }

    #[test]
    fn test_packed_token() {
        let token = PackedToken::new(1.5, 42);

        assert!((token.cost() - 1.5).abs() < 1e-6);
        assert_eq!(token.arc_id(), 42);
        assert!(!token.is_empty());
    }

    #[test]
    fn test_packed_token_empty() {
        let token = PackedToken::EMPTY;

        assert!(token.is_empty());
        assert!(token.cost().is_infinite());
    }

    #[test]
    fn test_packed_token_comparison() {
        let better = PackedToken::new(1.0, 1);
        let worse = PackedToken::new(2.0, 2);

        assert!(better.is_better_than(worse));
        assert!(!worse.is_better_than(better));
    }

    #[test]
    fn test_token_packer() {
        let packer = TokenPacker::new();

        let packed = packer.pack(1.5, 42);
        let (cost, arc_id) = packer.unpack(packed);

        assert!((cost - 1.5).abs() < 1e-6);
        assert_eq!(arc_id, 42);
    }

    #[test]
    fn test_recombination_buffer() {
        let buffer = RecombinationBuffer::new(10, 100);

        // Recombine several tokens to the same state
        assert!(buffer.recombine(5, 2.0, 10));
        assert!(!buffer.recombine(5, 3.0, 20)); // worse, should fail
        assert!(buffer.recombine(5, 1.0, 30)); // better, should succeed

        // Check the best token
        let token = buffer.get_token(5).expect("should have token");
        assert!((token.cost() - 1.0).abs() < 1e-6);
        assert_eq!(token.arc_id(), 30);
    }

    #[test]
    fn test_recombination_buffer_reset() {
        let buffer = RecombinationBuffer::new(10, 100);

        buffer.recombine(0, 1.0, 0);
        buffer.recombine(1, 1.0, 1);
        assert_eq!(buffer.num_active(), 2);

        buffer.reset();
        assert_eq!(buffer.num_active(), 0);
    }

    #[test]
    fn test_collect_survivors() {
        let buffer = RecombinationBuffer::new(5, 10);

        buffer.recombine(0, 1.0, 0);
        buffer.recombine(2, 2.0, 1);
        buffer.recombine(4, 3.0, 2);

        let survivors = buffer.collect_survivors();
        assert_eq!(survivors.len(), 3);

        // Check states
        let states: Vec<_> = survivors.iter().map(|(s, _)| *s).collect();
        assert!(states.contains(&0));
        assert!(states.contains(&2));
        assert!(states.contains(&4));
    }

    #[test]
    fn test_recombination_stats() {
        let buffer = RecombinationBuffer::new(100, 500);

        buffer.recombine(0, 1.0, 0);
        buffer.recombine(50, 1.0, 1);

        let stats = buffer.stats();
        assert_eq!(stats.num_states, 100);
        assert_eq!(stats.active_states, 2);
        assert!(stats.recombination_ratio > 0.9);
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        // =====================================================================
        // Pack/Unpack Roundtrip Properties
        // =====================================================================

        /// Pack-unpack is lossless for any finite f32 cost and u32 arc_id.
        #[test]
        fn pack_unpack_roundtrip(cost in -1e10f32..1e10, arc_id in 0u32..u32::MAX) {
            let packed = pack_cost_arc(cost, arc_id);
            let (unpacked_cost, unpacked_arc) = unpack_cost_arc(packed);

            prop_assert!((unpacked_cost - cost).abs() < 1e-6,
                "Cost mismatch: {} vs {}", cost, unpacked_cost);
            prop_assert_eq!(unpacked_arc, arc_id);
        }

        /// PackedToken preserves cost and arc_id.
        #[test]
        fn packed_token_roundtrip(cost in -1e6f32..1e6, arc_id in 0u32..1_000_000) {
            let token = PackedToken::new(cost, arc_id);

            prop_assert!((token.cost() - cost).abs() < 1e-5,
                "Token cost mismatch: {} vs {}", cost, token.cost());
            prop_assert_eq!(token.arc_id(), arc_id);
        }

        // =====================================================================
        // Ordering Properties
        // =====================================================================

        /// Lower costs produce lower packed values (preserves ordering).
        #[test]
        fn pack_preserves_ordering(cost1 in -1e6f32..1e6, cost2 in -1e6f32..1e6) {
            let packed1 = pack_cost_arc(cost1, 0);
            let packed2 = pack_cost_arc(cost2, 0);

            if cost1 < cost2 {
                prop_assert!(packed1 < packed2,
                    "Ordering violated: {} < {} but {} >= {}",
                    cost1, cost2, packed1, packed2);
            } else if cost1 > cost2 {
                prop_assert!(packed1 > packed2,
                    "Ordering violated: {} > {} but {} <= {}",
                    cost1, cost2, packed1, packed2);
            }
        }

        /// is_better_than matches cost comparison.
        #[test]
        fn is_better_than_matches_cost(cost1 in 0.0f32..1e6, cost2 in 0.0f32..1e6, arc1 in 0u32..1000, arc2 in 0u32..1000) {
            let token1 = PackedToken::new(cost1, arc1);
            let token2 = PackedToken::new(cost2, arc2);

            if cost1 < cost2 {
                prop_assert!(token1.is_better_than(token2));
            } else if cost1 > cost2 {
                prop_assert!(token2.is_better_than(token1));
            }
        }

        // =====================================================================
        // Negative Cost Ordering
        // =====================================================================

        /// Negative costs order correctly (more negative = lower packed value).
        #[test]
        fn negative_cost_ordering(a in -1e6f32..-0.001, b in -1e6f32..-0.001) {
            let packed_a = pack_cost_arc(a, 0);
            let packed_b = pack_cost_arc(b, 0);

            if a < b {
                prop_assert!(packed_a < packed_b,
                    "Negative ordering failed: {} < {} but packed {} >= {}",
                    a, b, packed_a, packed_b);
            }
        }

        /// Mixed positive and negative costs order correctly.
        #[test]
        fn mixed_sign_ordering(neg in -1e6f32..-0.001, pos in 0.001f32..1e6) {
            let packed_neg = pack_cost_arc(neg, 0);
            let packed_pos = pack_cost_arc(pos, 0);

            prop_assert!(packed_neg < packed_pos,
                "Mixed ordering failed: {} should be < {} but packed {} >= {}",
                neg, pos, packed_neg, packed_pos);
        }

        // =====================================================================
        // TokenPacker Properties
        // =====================================================================

        /// TokenPacker pack/unpack matches direct functions.
        #[test]
        fn token_packer_consistent(cost in -1e6f32..1e6, arc_id in 0u32..1_000_000) {
            let packer = TokenPacker::new();

            let packed_direct = pack_cost_arc(cost, arc_id);
            let packed_packer = packer.pack(cost, arc_id);
            prop_assert_eq!(packed_direct, packed_packer);

            let (cost_direct, arc_direct) = unpack_cost_arc(packed_direct);
            let (cost_packer, arc_packer) = packer.unpack(packed_packer);
            prop_assert!((cost_direct - cost_packer).abs() < 1e-10);
            prop_assert_eq!(arc_direct, arc_packer);
        }

        // =====================================================================
        // RecombinationBuffer Properties
        // =====================================================================

        /// Recombine keeps the best token for each state.
        #[test]
        fn recombine_keeps_best(
            costs in proptest::collection::vec(0.1f32..100.0, 1..10),
            state in 0usize..50
        ) {
            let buffer = RecombinationBuffer::new(100, 100);
            let mut best_cost = f32::INFINITY;
            let mut best_arc = 0u32;

            for (i, &cost) in costs.iter().enumerate() {
                let arc_id = i as u32;
                buffer.recombine(state, cost, arc_id);
                if cost < best_cost {
                    best_cost = cost;
                    best_arc = arc_id;
                }
            }

            let token = buffer.get_token(state).expect("should have token");
            prop_assert!((token.cost() - best_cost).abs() < 1e-5,
                "Best cost mismatch: expected {}, got {}", best_cost, token.cost());
            prop_assert_eq!(token.arc_id(), best_arc,
                "Best arc mismatch: expected {}, got {}", best_arc, token.arc_id());
        }

        /// Reset clears all tokens.
        #[test]
        fn recombine_reset_clears(num_states in 5usize..20) {
            let buffer = RecombinationBuffer::new(num_states, 100);

            // Add some tokens
            for s in 0..num_states {
                buffer.recombine(s, s as f32, s as u32);
            }
            prop_assert_eq!(buffer.num_active(), num_states);

            buffer.reset();
            prop_assert_eq!(buffer.num_active(), 0);

            // Check all states are empty
            for s in 0..num_states {
                prop_assert!(buffer.get_token(s).is_none());
            }
        }

        /// collect_survivors returns exactly the active tokens.
        #[test]
        fn collect_survivors_accurate(active_states in proptest::collection::vec(0usize..50, 1..20)) {
            let buffer = RecombinationBuffer::new(100, 100);

            // Add tokens to specific states
            let unique_states: std::collections::HashSet<_> = active_states.iter().cloned().collect();
            for &s in &unique_states {
                buffer.recombine(s, s as f32, s as u32);
            }

            let survivors = buffer.collect_survivors();
            prop_assert_eq!(survivors.len(), unique_states.len());

            let survivor_states: std::collections::HashSet<_> = survivors.iter().map(|(s, _)| *s).collect();
            prop_assert_eq!(survivor_states, unique_states);
        }

        // =====================================================================
        // PackedToken Special Values
        // =====================================================================

        /// EMPTY token has infinity cost.
        #[test]
        fn empty_token_is_infinite(_ in 0..1) {
            let empty = PackedToken::EMPTY;
            prop_assert!(empty.is_empty());
            prop_assert!(empty.cost().is_infinite());
        }

        /// Non-empty tokens are better than EMPTY.
        #[test]
        fn finite_better_than_empty(cost in -1e6f32..1e6, arc_id in 0u32..1000) {
            let token = PackedToken::new(cost, arc_id);
            let empty = PackedToken::EMPTY;

            prop_assert!(token.is_better_than(empty));
            prop_assert!(!empty.is_better_than(token));
        }

        // =====================================================================
        // Stats Properties
        // =====================================================================

        /// Stats reflect accurate counts.
        #[test]
        fn stats_accurate(num_active in 1usize..50, num_states in 50usize..100) {
            let buffer = RecombinationBuffer::new(num_states, 200);

            for s in 0..num_active {
                buffer.recombine(s, s as f32, s as u32);
            }

            let stats = buffer.stats();
            prop_assert_eq!(stats.num_states, num_states);
            prop_assert_eq!(stats.active_states, num_active);

            let expected_ratio = 1.0 - (num_active as f64 / num_states as f64);
            prop_assert!((stats.recombination_ratio - expected_ratio).abs() < 1e-10);
        }
    }
}
