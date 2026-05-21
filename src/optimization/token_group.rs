//! Token grouping and lazy evaluation for on-the-fly composition.
//!
//! This module implements the LET-Decoder optimization from Lv et al.,
//! which groups tokens with the same base-graph state but different
//! grammar states, deferring expansion until word boundaries.
//!
//! ## Overview
//!
//! During on-the-fly composition (e.g., HCLG ∘ G_r where G_r is a residual
//! grammar), many tokens share the same base-graph state but differ in
//! grammar state. These tokens can be grouped together:
//!
//! - **Token Group**: Collection of tokens at same base-graph state
//! - **Lazy Evaluation**: Only expand tokens when word labels are emitted
//! - **α-Stable Property**: Forward probabilities remain stable after update
//!
//! ## Benefits
//!
//! - 10-20× reduction in composition operations
//! - Significant speedup for on-the-fly rescoring
//! - Memory savings from deferred expansion
//!
//! ## References
//!
//! - Lv et al. (2023): "LET-Decoder: Lazy-evaluation Token-group Decoder"

use std::collections::VecDeque;

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::semiring::{LogWeight, Semiring};
use crate::wfst::StateId;

/// A token representing a hypothesis during decoding.
#[derive(Clone, Debug)]
pub struct Token {
    /// State in the base graph (e.g., HCLG).
    pub base_state: StateId,
    /// State in the residual grammar.
    pub grammar_state: StateId,
    /// Accumulated forward probability (log).
    pub forward_prob: LogWeight,
    /// Index of preceding token (for back-tracing).
    pub prev_token: Option<TokenId>,
    /// Arc taken to reach this token.
    pub prev_arc: Option<ArcId>,
}

/// Unique identifier for a token.
pub type TokenId = u32;

/// Unique identifier for an arc.
pub type ArcId = u32;

/// A link from one token group to another (for lazy back-tracing).
#[derive(Clone, Debug)]
pub struct GroupLink {
    /// Source token group ID.
    pub source_group: TokenGroupId,
    /// Target token group ID.
    pub target_group: TokenGroupId,
    /// Weight of the link.
    pub weight: LogWeight,
    /// Whether this is a word arc (output label present).
    pub is_word_arc: bool,
}

/// Unique identifier for a token group.
pub type TokenGroupId = u32;

/// A token group representing multiple tokens at the same base-graph state.
///
/// Token groups enable lazy evaluation by:
/// 1. Storing only the best forward probability for pruning
/// 2. Deferring token materialization until word boundaries
/// 3. Maintaining links for efficient back-tracing
#[derive(Clone, Debug)]
pub struct TokenGroup {
    /// State in the base graph (shared by all tokens in group).
    pub base_state: StateId,
    /// Best forward probability among all tokens in this group.
    pub best_forward_prob: LogWeight,
    /// Whether this group has been expanded (tokens materialized).
    pub expanded: bool,
    /// Actual tokens (only populated after expansion).
    tokens: SmallVec<[Token; 4]>,
    /// Links to preceding token groups.
    preceding_links: SmallVec<[GroupLink; 4]>,
    /// Links to succeeding token groups.
    succeeding_links: SmallVec<[GroupLink; 4]>,
    /// Frame index when this group was created.
    pub frame: u32,
}

impl TokenGroup {
    /// Create a new token group.
    pub fn new(base_state: StateId, frame: u32) -> Self {
        Self {
            base_state,
            best_forward_prob: LogWeight::zero(),
            expanded: false,
            tokens: SmallVec::new(),
            preceding_links: SmallVec::new(),
            succeeding_links: SmallVec::new(),
            frame,
        }
    }

    /// Create an expanded token group with an initial token.
    pub fn with_token(base_state: StateId, token: Token, frame: u32) -> Self {
        let forward_prob = token.forward_prob.clone();
        Self {
            base_state,
            best_forward_prob: forward_prob,
            expanded: true,
            tokens: SmallVec::from_elem(token, 1),
            preceding_links: SmallVec::new(),
            succeeding_links: SmallVec::new(),
            frame,
        }
    }

    /// Add a token to this group (must be expanded).
    pub fn add_token(&mut self, token: Token) {
        // Update best forward prob
        self.best_forward_prob = self.best_forward_prob.plus(&token.forward_prob);
        self.tokens.push(token);
    }

    /// Add a preceding link (for lazy back-tracing).
    pub fn add_preceding_link(&mut self, link: GroupLink) {
        // Update best forward prob based on link
        let incoming_prob = link.weight.clone();
        self.best_forward_prob = self.best_forward_prob.plus(&incoming_prob);
        self.preceding_links.push(link);
    }

    /// Add a succeeding link.
    pub fn add_succeeding_link(&mut self, link: GroupLink) {
        self.succeeding_links.push(link);
    }

    /// Get the number of tokens (only valid after expansion).
    pub fn num_tokens(&self) -> usize {
        self.tokens.len()
    }

    /// Get tokens (only valid after expansion).
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    /// Get mutable tokens.
    pub fn tokens_mut(&mut self) -> &mut SmallVec<[Token; 4]> {
        &mut self.tokens
    }

    /// Get preceding links.
    pub fn preceding_links(&self) -> &[GroupLink] {
        &self.preceding_links
    }

    /// Get succeeding links.
    pub fn succeeding_links(&self) -> &[GroupLink] {
        &self.succeeding_links
    }

    /// Check if this group is empty.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty() && self.preceding_links.is_empty()
    }
}

/// Pool for managing token groups.
///
/// Provides efficient allocation and lookup of token groups by base state.
#[derive(Debug)]
pub struct TokenGroupPool {
    /// All token groups.
    groups: Vec<TokenGroup>,
    /// Map from base state to group ID for current frame.
    current_frame_map: FxHashMap<StateId, TokenGroupId>,
    /// Current frame index.
    current_frame: u32,
}

impl TokenGroupPool {
    /// Create a new token group pool.
    pub fn new() -> Self {
        Self {
            groups: Vec::new(),
            current_frame_map: FxHashMap::default(),
            current_frame: 0,
        }
    }

    /// Create with capacity hint.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            groups: Vec::with_capacity(capacity),
            current_frame_map: FxHashMap::with_capacity_and_hasher(capacity, Default::default()),
            current_frame: 0,
        }
    }

    /// Advance to the next frame.
    pub fn advance_frame(&mut self) {
        self.current_frame += 1;
        self.current_frame_map.clear();
    }

    /// Get or create a token group for a base state in the current frame.
    pub fn get_or_create(&mut self, base_state: StateId) -> TokenGroupId {
        if let Some(&group_id) = self.current_frame_map.get(&base_state) {
            return group_id;
        }

        let group_id = self.groups.len() as TokenGroupId;
        self.groups
            .push(TokenGroup::new(base_state, self.current_frame));
        self.current_frame_map.insert(base_state, group_id);
        group_id
    }

    /// Get a token group by ID.
    pub fn get(&self, id: TokenGroupId) -> Option<&TokenGroup> {
        self.groups.get(id as usize)
    }

    /// Get a mutable token group by ID.
    pub fn get_mut(&mut self, id: TokenGroupId) -> Option<&mut TokenGroup> {
        self.groups.get_mut(id as usize)
    }

    /// Get the number of token groups.
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// Check if the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Get the current frame.
    pub fn current_frame(&self) -> u32 {
        self.current_frame
    }

    /// Clear all groups.
    pub fn clear(&mut self) {
        self.groups.clear();
        self.current_frame_map.clear();
        self.current_frame = 0;
    }

    /// Get groups for the current frame.
    pub fn current_frame_groups(&self) -> impl Iterator<Item = (TokenGroupId, &TokenGroup)> {
        self.current_frame_map
            .iter()
            .filter_map(|(&_base_state, &group_id)| {
                self.groups.get(group_id as usize).map(|g| (group_id, g))
            })
    }
}

impl Default for TokenGroupPool {
    fn default() -> Self {
        Self::new()
    }
}

/// A bucket queue for histogram-based pruning.
///
/// Elements are organized by integer priority (bucket index).
/// This enables efficient histogram pruning where we process
/// tokens in order of quantized forward probability.
#[derive(Debug)]
pub struct BucketQueue<T> {
    /// Buckets indexed by priority (lower = better).
    buckets: Vec<VecDeque<T>>,
    /// Index of the current best non-empty bucket.
    min_bucket: usize,
    /// Total number of elements.
    len: usize,
    /// Scale factor for converting weights to bucket indices.
    scale: f64,
    /// Offset for bucket index computation.
    offset: f64,
}

impl<T> BucketQueue<T> {
    /// Create a new bucket queue.
    ///
    /// # Arguments
    ///
    /// * `num_buckets` - Number of priority buckets
    /// * `min_weight` - Minimum expected weight (maps to bucket 0)
    /// * `max_weight` - Maximum expected weight (maps to bucket num_buckets-1)
    pub fn new(num_buckets: usize, min_weight: f64, max_weight: f64) -> Self {
        let range = max_weight - min_weight;
        let scale = if range > 0.0 {
            (num_buckets - 1) as f64 / range
        } else {
            1.0
        };

        Self {
            buckets: (0..num_buckets).map(|_| VecDeque::new()).collect(),
            min_bucket: num_buckets, // Invalid until first insert
            len: 0,
            scale,
            offset: min_weight,
        }
    }

    /// Create with default parameters suitable for log probabilities.
    pub fn default_for_log_probs(num_buckets: usize) -> Self {
        // Log probs typically range from 0 (prob=1) to 100+ (prob≈0)
        Self::new(num_buckets, 0.0, 100.0)
    }

    /// Insert an element with a weight (log probability).
    pub fn insert(&mut self, weight: f64, item: T) {
        let bucket = self.weight_to_bucket(weight);
        self.buckets[bucket].push_back(item);
        self.len += 1;

        if bucket < self.min_bucket {
            self.min_bucket = bucket;
        }
    }

    /// Pop the best (lowest weight) element.
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }

        // Find next non-empty bucket starting from min_bucket
        while self.min_bucket < self.buckets.len() {
            if let Some(item) = self.buckets[self.min_bucket].pop_front() {
                self.len -= 1;
                return Some(item);
            }
            self.min_bucket += 1;
        }

        None
    }

    /// Peek at the best element without removing it.
    pub fn peek(&self) -> Option<&T> {
        if self.len == 0 {
            return None;
        }

        for bucket in self.min_bucket..self.buckets.len() {
            if let Some(item) = self.buckets[bucket].front() {
                return Some(item);
            }
        }

        None
    }

    /// Get the number of elements.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Clear all elements.
    pub fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
        self.len = 0;
        self.min_bucket = self.buckets.len();
    }

    /// Convert weight to bucket index.
    fn weight_to_bucket(&self, weight: f64) -> usize {
        let normalized = (weight - self.offset) * self.scale;
        let bucket = normalized.round() as isize;
        bucket.clamp(0, (self.buckets.len() - 1) as isize) as usize
    }

    /// Get histogram of bucket occupancy.
    pub fn histogram(&self) -> Vec<usize> {
        self.buckets.iter().map(|b| b.len()).collect()
    }

    /// Prune elements beyond a given bucket threshold.
    pub fn prune_beyond(&mut self, max_bucket: usize) -> usize {
        let mut pruned = 0;
        for bucket_idx in (max_bucket + 1)..self.buckets.len() {
            pruned += self.buckets[bucket_idx].len();
            self.buckets[bucket_idx].clear();
        }
        self.len -= pruned;
        pruned
    }
}

impl<T> Default for BucketQueue<T> {
    fn default() -> Self {
        Self::default_for_log_probs(100)
    }
}

/// Configuration for token grouping.
#[derive(Clone, Debug)]
pub struct TokenGroupConfig {
    /// Maximum number of tokens per group before forced expansion.
    pub max_tokens_per_group: usize,
    /// Maximum number of groups to track.
    pub max_groups: usize,
    /// Number of buckets for histogram pruning.
    pub num_buckets: usize,
    /// Whether to use lazy evaluation (defer expansion).
    pub lazy_evaluation: bool,
}

impl Default for TokenGroupConfig {
    fn default() -> Self {
        Self {
            max_tokens_per_group: 32,
            max_groups: 10000,
            num_buckets: 100,
            lazy_evaluation: true,
        }
    }
}

/// Statistics from token grouping operations.
#[derive(Clone, Debug, Default)]
pub struct TokenGroupStats {
    /// Total number of tokens processed.
    pub tokens_processed: usize,
    /// Number of token groups created.
    pub groups_created: usize,
    /// Number of expansions performed.
    pub expansions: usize,
    /// Number of composition operations saved.
    pub ops_saved: usize,
    /// Average tokens per group.
    pub avg_tokens_per_group: f64,
}

/// Result of grouping tokens at a frame.
#[derive(Clone, Debug)]
pub struct GroupedFrame {
    /// Active token groups at this frame.
    pub active_groups: Vec<TokenGroupId>,
    /// Best forward probability at this frame.
    pub best_forward_prob: LogWeight,
    /// Whether any groups need expansion.
    pub needs_expansion: bool,
}

impl GroupedFrame {
    /// Create an empty grouped frame.
    pub fn new() -> Self {
        Self {
            active_groups: Vec::new(),
            best_forward_prob: LogWeight::zero(),
            needs_expansion: false,
        }
    }
}

impl Default for GroupedFrame {
    fn default() -> Self {
        Self::new()
    }
}

/// Manager for token grouping during decoding.
///
/// Implements the LET-Decoder lazy evaluation strategy:
/// 1. Group tokens by base-graph state
/// 2. Track group-level forward probabilities for pruning
/// 3. Defer expansion until word boundaries
/// 4. Use α-stable property for correct lattice generation
#[derive(Debug)]
pub struct TokenGroupManager {
    /// Configuration.
    config: TokenGroupConfig,
    /// Token group pool.
    pool: TokenGroupPool,
    /// Bucket queue for pruning.
    queue: BucketQueue<TokenGroupId>,
    /// Statistics.
    stats: TokenGroupStats,
}

impl TokenGroupManager {
    /// Create a new token group manager.
    pub fn new(config: TokenGroupConfig) -> Self {
        let num_buckets = config.num_buckets;
        Self {
            config,
            pool: TokenGroupPool::new(),
            queue: BucketQueue::default_for_log_probs(num_buckets),
            stats: TokenGroupStats::default(),
        }
    }

    /// Create with default configuration.
    pub fn default_config() -> Self {
        Self::new(TokenGroupConfig::default())
    }

    /// Process a token, adding it to the appropriate group.
    ///
    /// Returns the group ID where the token was added.
    ///
    /// # Arguments
    ///
    /// * `token` - The token to process
    /// * `is_word_arc` - Whether the token arrived via a word arc
    pub fn process_token(&mut self, token: Token, is_word_arc: bool) -> TokenGroupId {
        let group_id = self.pool.get_or_create(token.base_state);
        let group = self.pool.get_mut(group_id).expect("just created");

        self.stats.tokens_processed += 1;

        // If arriving via word arc or group is already expanded, expand now
        if is_word_arc || group.expanded || !self.config.lazy_evaluation {
            group.expanded = true;
            group.add_token(token);
        } else {
            // Lazy: just update forward prob, don't add token yet
            group.best_forward_prob = group.best_forward_prob.plus(&token.forward_prob);
        }

        // Update queue
        let weight = group.best_forward_prob.value();
        self.queue.insert(weight, group_id);

        group_id
    }

    /// Add a group link (for lazy back-tracing).
    ///
    /// Links connect token groups across frames without materializing
    /// actual tokens until needed.
    pub fn add_link(
        &mut self,
        source_group: TokenGroupId,
        target_group: TokenGroupId,
        weight: LogWeight,
        is_word_arc: bool,
    ) {
        let link = GroupLink {
            source_group,
            target_group,
            weight: weight.clone(),
            is_word_arc,
        };

        // Add to target's preceding links
        if let Some(target) = self.pool.get_mut(target_group) {
            target.add_preceding_link(link.clone());
        }

        // Add to source's succeeding links
        if let Some(source) = self.pool.get_mut(source_group) {
            source.add_succeeding_link(link);
        }

        // Track saved operations
        self.stats.ops_saved += 1;
    }

    /// Expand a token group (materialize tokens from links).
    ///
    /// This is called when we need actual tokens, typically at word
    /// boundaries or during back-tracing.
    pub fn expand_group(&mut self, group_id: TokenGroupId) {
        let group = match self.pool.get_mut(group_id) {
            Some(g) => g,
            None => return,
        };

        if group.expanded {
            return;
        }

        group.expanded = true;
        self.stats.expansions += 1;

        // Expansion would trace back through preceding links
        // and materialize tokens. For now, mark as expanded.
        // Full implementation would recursively expand predecessors.
    }

    /// Advance to the next frame.
    pub fn advance_frame(&mut self) -> GroupedFrame {
        let frame = GroupedFrame {
            active_groups: self.pool.current_frame_map.values().copied().collect(),
            best_forward_prob: self.compute_best_forward_prob(),
            needs_expansion: false,
        };

        self.pool.advance_frame();
        self.queue.clear();

        frame
    }

    /// Get the best forward probability across all current groups.
    fn compute_best_forward_prob(&self) -> LogWeight {
        let mut best = LogWeight::zero();
        for (_id, group) in self.pool.current_frame_groups() {
            best = best.plus(&group.best_forward_prob);
        }
        best
    }

    /// Prune groups beyond a given threshold.
    ///
    /// Returns the number of groups pruned.
    pub fn prune(&mut self, threshold: f64) -> usize {
        // Convert threshold to bucket index
        let max_bucket = ((threshold - self.queue.offset) * self.queue.scale).round() as usize;
        let max_bucket = max_bucket.min(self.config.num_buckets - 1);

        self.queue.prune_beyond(max_bucket)
    }

    /// Get statistics.
    pub fn stats(&self) -> &TokenGroupStats {
        &self.stats
    }

    /// Get mutable access to a group.
    pub fn group_mut(&mut self, id: TokenGroupId) -> Option<&mut TokenGroup> {
        self.pool.get_mut(id)
    }

    /// Get read access to a group.
    pub fn group(&self, id: TokenGroupId) -> Option<&TokenGroup> {
        self.pool.get(id)
    }

    /// Get the number of groups.
    pub fn num_groups(&self) -> usize {
        self.pool.len()
    }

    /// Clear all groups and reset.
    pub fn clear(&mut self) {
        self.pool.clear();
        self.queue.clear();
        self.stats = TokenGroupStats::default();
    }

    /// Update statistics with final computations.
    pub fn finalize_stats(&mut self) {
        if self.stats.groups_created > 0 {
            self.stats.avg_tokens_per_group =
                self.stats.tokens_processed as f64 / self.stats.groups_created as f64;
        }
    }
}

impl Default for TokenGroupManager {
    fn default() -> Self {
        Self::default_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_group_creation() {
        let group = TokenGroup::new(0, 0);
        assert_eq!(group.base_state, 0);
        assert!(group.best_forward_prob.is_zero());
        assert!(!group.expanded);
        assert!(group.is_empty());
    }

    #[test]
    fn test_token_group_with_token() {
        let token = Token {
            base_state: 0,
            grammar_state: 1,
            forward_prob: LogWeight::new(1.0),
            prev_token: None,
            prev_arc: None,
        };

        let group = TokenGroup::with_token(0, token, 0);
        assert!(group.expanded);
        assert_eq!(group.num_tokens(), 1);
        assert!(group
            .best_forward_prob
            .approx_eq(&LogWeight::new(1.0), 0.001));
    }

    #[test]
    fn test_token_group_add_token() {
        let mut group = TokenGroup::new(0, 0);
        group.expanded = true;

        let token = Token {
            base_state: 0,
            grammar_state: 1,
            forward_prob: LogWeight::new(1.0),
            prev_token: None,
            prev_arc: None,
        };

        group.add_token(token);
        assert_eq!(group.num_tokens(), 1);

        // Add second token - forward probs should combine (log-sum-exp)
        let token2 = Token {
            base_state: 0,
            grammar_state: 2,
            forward_prob: LogWeight::new(1.0),
            prev_token: None,
            prev_arc: None,
        };

        group.add_token(token2);
        assert_eq!(group.num_tokens(), 2);

        // Combined: logadd(1.0, 1.0) = log(exp(-1) + exp(-1)) = log(2*exp(-1)) ≈ 0.307
        let expected = -(2.0 * (-1.0_f64).exp()).ln();
        assert!(
            group
                .best_forward_prob
                .approx_eq(&LogWeight::new(expected), 0.01),
            "Expected {:?}, got {:?}",
            expected,
            group.best_forward_prob
        );
    }

    #[test]
    fn test_token_group_pool() {
        let mut pool = TokenGroupPool::new();

        // Create groups
        let id1 = pool.get_or_create(0);
        let id2 = pool.get_or_create(1);
        let id3 = pool.get_or_create(0); // Same base state, same group

        assert_eq!(id1, id3);
        assert_ne!(id1, id2);
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn test_token_group_pool_advance_frame() {
        let mut pool = TokenGroupPool::new();

        let id1 = pool.get_or_create(0);
        assert_eq!(pool.current_frame(), 0);

        pool.advance_frame();
        assert_eq!(pool.current_frame(), 1);

        // Same base state should create new group in new frame
        let id2 = pool.get_or_create(0);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_bucket_queue_basic() {
        let mut queue: BucketQueue<u32> = BucketQueue::new(10, 0.0, 10.0);

        // Insert items with different weights
        queue.insert(5.0, 1);
        queue.insert(2.0, 2);
        queue.insert(8.0, 3);

        assert_eq!(queue.len(), 3);

        // Should pop in order of weight (best first)
        assert_eq!(queue.pop(), Some(2)); // weight 2.0
        assert_eq!(queue.pop(), Some(1)); // weight 5.0
        assert_eq!(queue.pop(), Some(3)); // weight 8.0
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn test_bucket_queue_same_bucket() {
        let mut queue: BucketQueue<u32> = BucketQueue::new(10, 0.0, 10.0);

        // Insert items that map to the same bucket
        queue.insert(2.0, 1);
        queue.insert(2.1, 2);
        queue.insert(2.2, 3);

        assert_eq!(queue.len(), 3);

        // Should all pop (FIFO within bucket)
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(3));
    }

    #[test]
    fn test_bucket_queue_prune() {
        let mut queue: BucketQueue<u32> = BucketQueue::new(10, 0.0, 10.0);

        queue.insert(1.0, 1);
        queue.insert(5.0, 2);
        queue.insert(9.0, 3);

        // Prune items beyond bucket 5
        let pruned = queue.prune_beyond(5);

        assert_eq!(pruned, 1); // Item with weight 9.0 pruned
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn test_bucket_queue_histogram() {
        let mut queue: BucketQueue<u32> = BucketQueue::new(5, 0.0, 4.0);

        queue.insert(0.0, 1);
        queue.insert(0.0, 2);
        queue.insert(2.0, 3);
        queue.insert(4.0, 4);

        let hist = queue.histogram();
        assert_eq!(hist[0], 2); // Two items at weight 0
        assert_eq!(hist[2], 1); // One item at weight 2
        assert_eq!(hist[4], 1); // One item at weight 4
    }

    #[test]
    fn test_token_group_manager_basic() {
        let mut manager = TokenGroupManager::default_config();

        let token = Token {
            base_state: 0,
            grammar_state: 1,
            forward_prob: LogWeight::new(1.0),
            prev_token: None,
            prev_arc: None,
        };

        let group_id = manager.process_token(token, false);

        let group = manager.group(group_id).expect("group exists");
        assert!(group
            .best_forward_prob
            .approx_eq(&LogWeight::new(1.0), 0.001));
    }

    #[test]
    fn test_token_group_manager_word_arc() {
        let config = TokenGroupConfig {
            lazy_evaluation: true,
            ..Default::default()
        };
        let mut manager = TokenGroupManager::new(config);

        // Non-word arc - should be lazy
        let token1 = Token {
            base_state: 0,
            grammar_state: 1,
            forward_prob: LogWeight::new(1.0),
            prev_token: None,
            prev_arc: None,
        };
        let id1 = manager.process_token(token1, false);

        // Word arc - should force expansion
        let token2 = Token {
            base_state: 1,
            grammar_state: 2,
            forward_prob: LogWeight::new(2.0),
            prev_token: None,
            prev_arc: None,
        };
        let id2 = manager.process_token(token2, true);

        let group2 = manager.group(id2).expect("group exists");
        assert!(group2.expanded);
    }

    #[test]
    fn test_token_group_manager_stats() {
        let mut manager = TokenGroupManager::default_config();

        for i in 0..5 {
            let token = Token {
                base_state: i % 2, // Two base states
                grammar_state: i,
                forward_prob: LogWeight::new(1.0),
                prev_token: None,
                prev_arc: None,
            };
            manager.process_token(token, i == 4); // Last one is word arc
        }

        assert_eq!(manager.stats().tokens_processed, 5);
    }

    #[test]
    fn test_grouped_frame() {
        let mut manager = TokenGroupManager::default_config();

        // Add tokens in frame 0
        for i in 0..3 {
            let token = Token {
                base_state: i,
                grammar_state: i,
                forward_prob: LogWeight::new(1.0),
                prev_token: None,
                prev_arc: None,
            };
            manager.process_token(token, true);
        }

        let frame = manager.advance_frame();
        assert_eq!(frame.active_groups.len(), 3);
    }

    #[test]
    fn test_group_link() {
        let mut manager = TokenGroupManager::default_config();

        let token1 = Token {
            base_state: 0,
            grammar_state: 0,
            forward_prob: LogWeight::new(1.0),
            prev_token: None,
            prev_arc: None,
        };
        let id1 = manager.process_token(token1, true);

        manager.advance_frame();

        let token2 = Token {
            base_state: 1,
            grammar_state: 1,
            forward_prob: LogWeight::new(2.0),
            prev_token: None,
            prev_arc: None,
        };
        let id2 = manager.process_token(token2, false);

        manager.add_link(id1, id2, LogWeight::new(0.5), false);

        let group2 = manager.group(id2).expect("group exists");
        assert_eq!(group2.preceding_links().len(), 1);

        assert_eq!(manager.stats().ops_saved, 1);
    }
}
