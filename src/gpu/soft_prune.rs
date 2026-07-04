//! Soft pruning for efficient GPU token management.
//!
//! This module provides soft pruning abstractions that avoid expensive memory
//! deallocation on GPU by marking tokens as pruned rather than removing them.
//!
//! ## Problem
//!
//! During beam search decoding, many tokens are pruned. On GPU, memory
//! deallocation is expensive and causes fragmentation. Additionally, removing
//! elements from vectors requires shifting all subsequent elements.
//!
//! ## Solution: Soft Pruning
//!
//! Instead of removing pruned tokens:
//! 1. Mark them as "soft-pruned" by zeroing their out-arc degree
//! 2. Load balancer safely ignores zero-degree tokens
//! 3. Periodically compact remaining tokens (batch operation)
//!
//! ```text
//! procedure SOFT_PRUNE(token, threshold):
//!     if token.cost > threshold:
//!         token.out_degree = 0  // Mark as pruned
//!     // Token remains in memory but is ignored
//!
//! procedure COMPACT(tokens):
//!     surviving = []
//!     for token in tokens:
//!         if token.out_degree > 0:
//!             surviving.push(token)
//!     tokens = surviving
//! ```
//!
//! ## Benefits
//!
//! - **No deallocation**: Avoids expensive GPU memory operations
//! - **No shifting**: Elements stay in place until compaction
//! - **Batch compaction**: Amortizes compaction cost over many prunes
//! - **Load balancer compatible**: Zero-degree tokens automatically skipped
//!
//! ## References
//!
//! - Braun et al., "GPU-Accelerated Viterbi Exact Lattice Decoder for Batched Online and Offline
//!   Speech Recognition" (ICASSP 2020, arXiv:1910.10032)
//! - Chen et al., "A GPU-based WFST Decoder with Exact Lattice Generation" (Interspeech 2018)

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// A soft-prunable token that can be marked as pruned without deallocation.
#[derive(Debug)]
pub struct SoftToken<T> {
    /// The token data.
    data: T,
    /// Number of outgoing arcs (0 = pruned).
    out_degree: AtomicU32,
    /// Whether this token is active (not pruned).
    active: AtomicBool,
    /// Frame index when this token was created.
    frame: u32,
    /// Cost of this token.
    cost: f32,
}

impl<T> SoftToken<T> {
    /// Create a new soft token.
    pub fn new(data: T, out_degree: u32, frame: u32, cost: f32) -> Self {
        Self {
            data,
            out_degree: AtomicU32::new(out_degree),
            active: AtomicBool::new(true),
            frame,
            cost,
        }
    }

    /// Get the token data.
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Get mutable access to token data.
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.data
    }

    /// Get the out-degree.
    pub fn out_degree(&self) -> u32 {
        self.out_degree.load(Ordering::Acquire)
    }

    /// Set the out-degree.
    pub fn set_out_degree(&self, degree: u32) {
        self.out_degree.store(degree, Ordering::Release);
    }

    /// Check if the token is active (not pruned).
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Check if the token is pruned.
    pub fn is_pruned(&self) -> bool {
        !self.is_active() || self.out_degree() == 0
    }

    /// Get the frame index.
    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// Get the cost.
    pub fn cost(&self) -> f32 {
        self.cost
    }

    /// Soft-prune this token by zeroing out-degree.
    pub fn soft_prune(&self) {
        self.out_degree.store(0, Ordering::Release);
        self.active.store(false, Ordering::Release);
    }

    /// Check if this token should be pruned based on threshold.
    pub fn should_prune(&self, threshold: f32) -> bool {
        self.cost > threshold
    }

    /// Soft-prune if cost exceeds threshold.
    ///
    /// Returns `true` if the token was pruned.
    pub fn prune_if_above(&self, threshold: f32) -> bool {
        if self.should_prune(threshold) {
            self.soft_prune();
            true
        } else {
            false
        }
    }
}

impl<T: Clone> Clone for SoftToken<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            out_degree: AtomicU32::new(self.out_degree.load(Ordering::Relaxed)),
            active: AtomicBool::new(self.active.load(Ordering::Relaxed)),
            frame: self.frame,
            cost: self.cost,
        }
    }
}

/// Configuration for soft pruning.
#[derive(Clone, Copy, Debug)]
pub struct SoftPruneConfig {
    /// Beam width (cost threshold relative to best).
    pub beam: f32,
    /// Maximum number of active tokens.
    pub max_active: usize,
    /// Compaction threshold (compact when this fraction is pruned).
    pub compact_threshold: f32,
    /// Minimum tokens before considering compaction.
    pub min_tokens_for_compact: usize,
}

impl SoftPruneConfig {
    /// Create a new configuration.
    pub fn new(beam: f32, max_active: usize) -> Self {
        Self {
            beam,
            max_active,
            compact_threshold: 0.5,
            min_tokens_for_compact: 1000,
        }
    }

    /// Create with custom compaction settings.
    pub fn with_compaction(
        beam: f32,
        max_active: usize,
        compact_threshold: f32,
        min_tokens_for_compact: usize,
    ) -> Self {
        Self {
            beam,
            max_active,
            compact_threshold,
            min_tokens_for_compact,
        }
    }
}

impl Default for SoftPruneConfig {
    fn default() -> Self {
        Self::new(16.0, 10000)
    }
}

/// Buffer for soft-prunable tokens with automatic compaction.
#[derive(Debug)]
pub struct SoftPruneBuffer<T> {
    /// Tokens in the buffer.
    tokens: Vec<SoftToken<T>>,
    /// Number of active (non-pruned) tokens.
    active_count: AtomicUsize,
    /// Configuration.
    config: SoftPruneConfig,
    /// Current best cost (for beam pruning).
    best_cost: f32,
}

impl<T> SoftPruneBuffer<T> {
    /// Create a new soft prune buffer.
    pub fn new(config: SoftPruneConfig) -> Self {
        Self {
            tokens: Vec::new(),
            active_count: AtomicUsize::new(0),
            config,
            best_cost: f32::INFINITY,
        }
    }

    /// Create with initial capacity.
    pub fn with_capacity(config: SoftPruneConfig, capacity: usize) -> Self {
        Self {
            tokens: Vec::with_capacity(capacity),
            active_count: AtomicUsize::new(0),
            config,
            best_cost: f32::INFINITY,
        }
    }

    /// Get the configuration.
    pub fn config(&self) -> &SoftPruneConfig {
        &self.config
    }

    /// Get the number of active tokens.
    pub fn active_count(&self) -> usize {
        self.active_count.load(Ordering::Acquire)
    }

    /// Get the total number of tokens (including pruned).
    pub fn total_count(&self) -> usize {
        self.tokens.len()
    }

    /// Get the pruned token count.
    ///
    /// Note: This counts pruned tokens by scanning, not from cached atomic count,
    /// to ensure accuracy when tokens are pruned directly via `SoftToken::soft_prune()`.
    pub fn pruned_count(&self) -> usize {
        self.tokens.iter().filter(|t| t.is_pruned()).count()
    }

    /// Get the actual active count by scanning tokens.
    ///
    /// More accurate than `active_count()` when tokens are pruned directly.
    pub fn actual_active_count(&self) -> usize {
        self.tokens.iter().filter(|t| t.is_active()).count()
    }

    /// Get the current best cost.
    pub fn best_cost(&self) -> f32 {
        self.best_cost
    }

    /// Get the current beam threshold.
    pub fn threshold(&self) -> f32 {
        self.best_cost + self.config.beam
    }

    /// Check if compaction is needed.
    pub fn needs_compaction(&self) -> bool {
        let total = self.total_count();
        if total < self.config.min_tokens_for_compact {
            return false;
        }

        let pruned_ratio = self.pruned_count() as f32 / total as f32;
        pruned_ratio >= self.config.compact_threshold
    }

    /// Push a new token to the buffer.
    ///
    /// Returns the token index, or `None` if pruned immediately.
    pub fn push(&mut self, token: SoftToken<T>) -> Option<usize> {
        // Update best cost
        if token.cost() < self.best_cost {
            self.best_cost = token.cost();
        }

        // Check if should be pruned immediately
        if token.should_prune(self.threshold()) {
            return None;
        }

        let index = self.tokens.len();
        self.tokens.push(token);
        self.active_count.fetch_add(1, Ordering::AcqRel);
        Some(index)
    }

    /// Get a token by index.
    pub fn get(&self, index: usize) -> Option<&SoftToken<T>> {
        self.tokens.get(index)
    }

    /// Iterate over active tokens.
    pub fn active_tokens(&self) -> impl Iterator<Item = (usize, &SoftToken<T>)> {
        self.tokens
            .iter()
            .enumerate()
            .filter(|(_, t)| t.is_active())
    }

    /// Apply beam pruning to all tokens.
    ///
    /// Returns the number of tokens pruned.
    pub fn apply_beam_pruning(&self) -> usize {
        let threshold = self.threshold();
        let mut pruned = 0;

        for token in &self.tokens {
            if token.is_active() && token.prune_if_above(threshold) {
                self.active_count.fetch_sub(1, Ordering::AcqRel);
                pruned += 1;
            }
        }

        pruned
    }

    /// Update best cost and apply beam pruning.
    pub fn update_best_and_prune(&mut self, new_best: f32) -> usize {
        if new_best < self.best_cost {
            self.best_cost = new_best;
        }
        self.apply_beam_pruning()
    }

    /// Clear all tokens.
    pub fn clear(&mut self) {
        self.tokens.clear();
        self.active_count.store(0, Ordering::Release);
        self.best_cost = f32::INFINITY;
    }

    /// Reset for a new frame.
    pub fn reset_for_frame(&mut self) {
        self.clear();
    }
}

impl<T: Clone> SoftPruneBuffer<T> {
    /// Compact the buffer, removing pruned tokens.
    ///
    /// Returns the number of tokens removed.
    pub fn compact(&mut self) -> usize {
        let original_len = self.tokens.len();

        // Retain only active tokens
        self.tokens.retain(|t| t.is_active());

        let removed = original_len - self.tokens.len();

        // Update active count to match actual count
        self.active_count
            .store(self.tokens.len(), Ordering::Release);

        removed
    }

    /// Compact if needed based on configuration.
    ///
    /// Returns the number of tokens removed, or 0 if no compaction occurred.
    pub fn compact_if_needed(&mut self) -> usize {
        if self.needs_compaction() {
            self.compact()
        } else {
            0
        }
    }

    /// Extract surviving tokens, consuming the buffer.
    pub fn into_survivors(self) -> Vec<T> {
        self.tokens
            .into_iter()
            .filter(|t| t.is_active())
            .map(|t| t.data)
            .collect()
    }
}

/// Statistics about soft pruning operations.
#[derive(Clone, Debug, Default)]
pub struct SoftPruneStats {
    /// Total tokens processed.
    pub total_tokens: usize,
    /// Tokens pruned by beam.
    pub beam_pruned: usize,
    /// Tokens pruned by max-active limit.
    pub limit_pruned: usize,
    /// Compaction operations performed.
    pub compactions: usize,
    /// Tokens removed by compaction.
    pub compacted_tokens: usize,
}

impl SoftPruneStats {
    /// Create new stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the total pruned count.
    pub fn total_pruned(&self) -> usize {
        self.beam_pruned + self.limit_pruned
    }

    /// Get the prune ratio.
    pub fn prune_ratio(&self) -> f64 {
        if self.total_tokens == 0 {
            0.0
        } else {
            self.total_pruned() as f64 / self.total_tokens as f64
        }
    }

    /// Get the compaction efficiency.
    pub fn compaction_efficiency(&self) -> f64 {
        if self.compactions == 0 {
            0.0
        } else {
            self.compacted_tokens as f64 / self.compactions as f64
        }
    }

    /// Record a beam prune.
    pub fn record_beam_prune(&mut self, count: usize) {
        self.beam_pruned += count;
    }

    /// Record a limit prune.
    pub fn record_limit_prune(&mut self, count: usize) {
        self.limit_pruned += count;
    }

    /// Record a compaction.
    pub fn record_compaction(&mut self, tokens_removed: usize) {
        self.compactions += 1;
        self.compacted_tokens += tokens_removed;
    }

    /// Record tokens processed.
    pub fn record_tokens(&mut self, count: usize) {
        self.total_tokens += count;
    }

    /// Merge stats from another instance.
    pub fn merge(&mut self, other: &SoftPruneStats) {
        self.total_tokens += other.total_tokens;
        self.beam_pruned += other.beam_pruned;
        self.limit_pruned += other.limit_pruned;
        self.compactions += other.compactions;
        self.compacted_tokens += other.compacted_tokens;
    }
}

/// Histogram-based adaptive beam for soft pruning.
///
/// Uses a histogram to quickly find the beam threshold that keeps
/// approximately `max_active` tokens.
#[derive(Debug)]
pub struct AdaptiveBeam {
    /// Number of histogram buckets.
    num_buckets: usize,
    /// Bucket counts.
    buckets: Vec<AtomicUsize>,
    /// Minimum cost seen.
    min_cost: f32,
    /// Maximum cost seen.
    max_cost: f32,
    /// Target number of active tokens.
    target_active: usize,
}

impl AdaptiveBeam {
    /// Create a new adaptive beam.
    ///
    /// A histogram needs at least one bucket, so a degenerate `num_buckets == 0`
    /// request is clamped up to `1`. Without the clamp,
    /// [`AdaptiveBeam::add_with_range`] would underflow `num_buckets - 1` and
    /// index an empty bucket vector, and [`AdaptiveBeam::compute_threshold`]
    /// would divide by zero.
    pub fn new(num_buckets: usize, target_active: usize) -> Self {
        let num_buckets = num_buckets.max(1);
        Self {
            num_buckets,
            buckets: (0..num_buckets).map(|_| AtomicUsize::new(0)).collect(),
            min_cost: f32::INFINITY,
            max_cost: f32::NEG_INFINITY,
            target_active,
        }
    }

    /// Reset the histogram.
    pub fn reset(&mut self) {
        for bucket in &self.buckets {
            bucket.store(0, Ordering::Relaxed);
        }
        self.min_cost = f32::INFINITY;
        self.max_cost = f32::NEG_INFINITY;
    }

    /// Add a cost to the histogram.
    pub fn add(&mut self, cost: f32) {
        if cost < self.min_cost {
            self.min_cost = cost;
        }
        if cost > self.max_cost {
            self.max_cost = cost;
        }

        // Bucket assignment is the second pass; this pass establishes the range.
    }

    /// Set the cost range for threshold computation.
    pub fn set_range(&mut self, min_cost: f32, max_cost: f32) {
        self.min_cost = min_cost;
        self.max_cost = max_cost;
    }

    /// Add a cost with known range.
    pub fn add_with_range(&self, cost: f32, range_min: f32, range_max: f32) {
        if range_max <= range_min {
            return;
        }

        let normalized = (cost - range_min) / (range_max - range_min);
        let bucket = ((normalized * self.num_buckets as f32) as usize).min(self.num_buckets - 1);
        self.buckets[bucket].fetch_add(1, Ordering::Relaxed);
    }

    /// Compute the adaptive threshold.
    ///
    /// Returns the cost threshold that keeps approximately `target_active` tokens.
    pub fn compute_threshold(&self) -> f32 {
        if self.max_cost <= self.min_cost {
            return f32::INFINITY;
        }

        let mut cumulative = 0;
        let bucket_width = (self.max_cost - self.min_cost) / self.num_buckets as f32;

        for (i, bucket) in self.buckets.iter().enumerate() {
            cumulative += bucket.load(Ordering::Relaxed);
            if cumulative >= self.target_active {
                // Return the upper bound of this bucket
                return self.min_cost + (i + 1) as f32 * bucket_width;
            }
        }

        // All tokens fit within target
        f32::INFINITY
    }

    /// Get the total count in the histogram.
    pub fn total_count(&self) -> usize {
        self.buckets.iter().map(|b| b.load(Ordering::Relaxed)).sum()
    }
}

/// Manager for soft pruning across multiple frames.
#[derive(Debug)]
pub struct SoftPruneManager<T> {
    /// Current frame buffer.
    current: SoftPruneBuffer<T>,
    /// Previous frame buffer (for swapping).
    previous: SoftPruneBuffer<T>,
    /// Current frame index.
    frame: u32,
    /// Adaptive beam for histogram pruning.
    adaptive_beam: AdaptiveBeam,
    /// Statistics.
    stats: SoftPruneStats,
}

impl<T> SoftPruneManager<T> {
    /// Create a new soft prune manager.
    pub fn new(config: SoftPruneConfig) -> Self {
        Self {
            current: SoftPruneBuffer::with_capacity(config, config.max_active),
            previous: SoftPruneBuffer::with_capacity(config, config.max_active),
            frame: 0,
            adaptive_beam: AdaptiveBeam::new(100, config.max_active),
            stats: SoftPruneStats::new(),
        }
    }

    /// Get the current frame.
    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// Get the current buffer.
    pub fn current(&self) -> &SoftPruneBuffer<T> {
        &self.current
    }

    /// Get mutable access to the current buffer.
    pub fn current_mut(&mut self) -> &mut SoftPruneBuffer<T> {
        &mut self.current
    }

    /// Get the previous buffer.
    pub fn previous(&self) -> &SoftPruneBuffer<T> {
        &self.previous
    }

    /// Get statistics.
    pub fn stats(&self) -> &SoftPruneStats {
        &self.stats
    }

    /// Add a token to the current frame.
    pub fn add_token(&mut self, data: T, out_degree: u32, cost: f32) -> Option<usize> {
        let token = SoftToken::new(data, out_degree, self.frame, cost);
        self.stats.record_tokens(1);
        self.current.push(token)
    }

    /// Apply beam pruning to current frame.
    pub fn apply_pruning(&mut self) -> usize {
        let pruned = self.current.apply_beam_pruning();
        self.stats.record_beam_prune(pruned);
        pruned
    }
}

impl<T: Clone> SoftPruneManager<T> {
    /// Advance to the next frame.
    pub fn advance_frame(&mut self) {
        // Compact current if needed
        let compacted = self.current.compact_if_needed();
        if compacted > 0 {
            self.stats.record_compaction(compacted);
        }

        // Swap buffers
        std::mem::swap(&mut self.current, &mut self.previous);

        // Reset current for new frame
        self.current.reset_for_frame();
        self.adaptive_beam.reset();
        self.frame += 1;
    }

    /// Get surviving tokens from the current frame.
    pub fn survivors(&self) -> Vec<T> {
        self.current
            .tokens
            .iter()
            .filter(|t| t.is_active())
            .map(|t| t.data.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soft_token_creation() {
        let token = SoftToken::new(42, 5, 0, 1.5);

        assert_eq!(*token.data(), 42);
        assert_eq!(token.out_degree(), 5);
        assert_eq!(token.frame(), 0);
        assert!((token.cost() - 1.5).abs() < 1e-6);
        assert!(token.is_active());
        assert!(!token.is_pruned());
    }

    #[test]
    fn test_soft_token_pruning() {
        let token = SoftToken::new(42, 5, 0, 1.5);

        assert!(token.is_active());
        token.soft_prune();
        assert!(!token.is_active());
        assert!(token.is_pruned());
        assert_eq!(token.out_degree(), 0);
    }

    #[test]
    fn test_soft_token_threshold_pruning() {
        let token = SoftToken::new(42, 5, 0, 10.0);

        assert!(!token.prune_if_above(15.0)); // Cost 10 < threshold 15
        assert!(token.is_active());

        assert!(token.prune_if_above(5.0)); // Cost 10 > threshold 5
        assert!(!token.is_active());
    }

    #[test]
    fn test_soft_prune_config() {
        let config = SoftPruneConfig::new(10.0, 5000);

        assert!((config.beam - 10.0).abs() < 1e-6);
        assert_eq!(config.max_active, 5000);
    }

    #[test]
    fn test_soft_prune_buffer() {
        let config = SoftPruneConfig::new(10.0, 100);
        let mut buffer = SoftPruneBuffer::new(config);

        // Add tokens
        let idx1 = buffer.push(SoftToken::new(1, 3, 0, 1.0));
        let idx2 = buffer.push(SoftToken::new(2, 2, 0, 2.0));
        let idx3 = buffer.push(SoftToken::new(3, 1, 0, 15.0)); // Should be pruned (cost > beam)

        assert!(idx1.is_some());
        assert!(idx2.is_some());
        assert!(idx3.is_none()); // Pruned immediately

        assert_eq!(buffer.active_count(), 2);
        assert_eq!(buffer.total_count(), 2);
    }

    #[test]
    fn test_soft_prune_buffer_beam_pruning() {
        let config = SoftPruneConfig::new(5.0, 100);
        let mut buffer = SoftPruneBuffer::new(config);

        buffer.push(SoftToken::new(1, 3, 0, 1.0));
        buffer.push(SoftToken::new(2, 2, 0, 3.0));
        buffer.push(SoftToken::new(3, 1, 0, 4.0));

        assert_eq!(buffer.active_count(), 3);

        // Update best cost and prune
        let pruned = buffer.update_best_and_prune(0.5);
        // New threshold = 0.5 + 5.0 = 5.5, so token with cost 1.0, 3.0, 4.0 should survive
        assert_eq!(pruned, 0);
        assert_eq!(buffer.active_count(), 3);
    }

    #[test]
    fn test_soft_prune_buffer_compact() {
        let config = SoftPruneConfig::new(10.0, 100);
        let mut buffer = SoftPruneBuffer::new(config);

        buffer.push(SoftToken::new(1, 3, 0, 1.0));
        buffer.push(SoftToken::new(2, 2, 0, 2.0));
        buffer.push(SoftToken::new(3, 1, 0, 3.0));

        // Manually prune one token
        buffer
            .get(1)
            .expect("gpu/soft_prune.rs: required value was None/Err")
            .soft_prune();

        assert_eq!(buffer.total_count(), 3);
        let removed = buffer.compact();
        assert_eq!(removed, 1);
        assert_eq!(buffer.total_count(), 2);
    }

    #[test]
    fn test_soft_prune_stats() {
        let mut stats = SoftPruneStats::new();

        stats.record_tokens(100);
        stats.record_beam_prune(20);
        stats.record_limit_prune(10);
        stats.record_compaction(15);

        assert_eq!(stats.total_tokens, 100);
        assert_eq!(stats.total_pruned(), 30);
        assert!((stats.prune_ratio() - 0.3).abs() < 1e-6);
        assert_eq!(stats.compactions, 1);
        assert_eq!(stats.compacted_tokens, 15);
    }

    #[test]
    fn test_adaptive_beam() {
        let mut beam = AdaptiveBeam::new(10, 50);

        // Set the range before computing threshold
        beam.set_range(0.0, 100.0);

        // Add costs with known range
        for i in 0..100 {
            beam.add_with_range(i as f32, 0.0, 100.0);
        }

        let threshold = beam.compute_threshold();
        // Should be around 50 to keep ~50 tokens
        assert!(threshold > 40.0 && threshold < 60.0);
    }

    #[test]
    fn test_adaptive_beam_zero_buckets_is_clamped_and_safe() {
        // A degenerate zero-bucket request must not underflow `num_buckets - 1`
        // or index an empty bucket vector; new() clamps it to a single bucket.
        let beam = AdaptiveBeam::new(0, 4);
        // Pre-fix, each of these panicked (usize underflow / OOB on empty Vec).
        beam.add_with_range(1.0, 0.0, 2.0);
        beam.add_with_range(0.5, 0.0, 2.0);
        // compute_threshold divides by num_buckets; must not divide by zero.
        let _ = beam.compute_threshold();
    }

    #[test]
    fn test_soft_prune_manager() {
        let config = SoftPruneConfig::new(10.0, 100);
        let mut manager = SoftPruneManager::new(config);

        // Add tokens
        manager.add_token(1, 3, 1.0);
        manager.add_token(2, 2, 2.0);
        manager.add_token(3, 1, 3.0);

        assert_eq!(manager.current().active_count(), 3);
        assert_eq!(manager.frame(), 0);

        // Advance frame
        manager.advance_frame();
        assert_eq!(manager.frame(), 1);
        assert_eq!(manager.current().active_count(), 0);
        assert_eq!(manager.previous().active_count(), 3);
    }

    #[test]
    fn test_soft_prune_manager_survivors() {
        let config = SoftPruneConfig::new(10.0, 100);
        let mut manager = SoftPruneManager::new(config);

        manager.add_token(1, 3, 1.0);
        manager.add_token(2, 2, 2.0);

        // Prune one
        manager
            .current()
            .get(0)
            .expect("gpu/soft_prune.rs: required value was None/Err")
            .soft_prune();

        let survivors = manager.survivors();
        assert_eq!(survivors.len(), 1);
        assert_eq!(survivors[0], 2);
    }

    #[test]
    fn test_needs_compaction() {
        let config = SoftPruneConfig::with_compaction(10.0, 100, 0.5, 4);
        let mut buffer = SoftPruneBuffer::new(config);

        // Add 10 tokens
        for i in 0..10 {
            buffer.push(SoftToken::new(i, 3, 0, i as f32));
        }

        assert!(!buffer.needs_compaction()); // 0% pruned

        // Prune 6 tokens (60%)
        for i in 0..6 {
            buffer
                .get(i)
                .expect("gpu/soft_prune.rs: required value was None/Err")
                .soft_prune();
        }

        assert!(buffer.needs_compaction()); // >50% pruned
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // =========================================================================
    // SoftToken Properties
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Newly created SoftToken is always active and not pruned.
        #[test]
        fn soft_token_new_is_active(
            data in any::<i32>(),
            out_degree in 1u32..100,
            frame in 0u32..1000,
            cost in -1000.0f32..1000.0
        ) {
            let token = SoftToken::new(data, out_degree, frame, cost);
            prop_assert!(token.is_active());
            prop_assert!(!token.is_pruned());
            prop_assert_eq!(*token.data(), data);
            prop_assert_eq!(token.out_degree(), out_degree);
            prop_assert_eq!(token.frame(), frame);
            prop_assert!((token.cost() - cost).abs() < 1e-6);
        }

        /// soft_prune always makes token inactive and pruned.
        #[test]
        fn soft_prune_makes_inactive(
            data in any::<i32>(),
            out_degree in 1u32..100,
            frame in 0u32..1000,
            cost in -1000.0f32..1000.0
        ) {
            let token = SoftToken::new(data, out_degree, frame, cost);
            token.soft_prune();
            prop_assert!(!token.is_active());
            prop_assert!(token.is_pruned());
            prop_assert_eq!(token.out_degree(), 0);
        }

        /// prune_if_above prunes only when cost > threshold.
        #[test]
        fn prune_if_above_correct(
            cost in 0.0f32..100.0,
            threshold in 0.0f32..100.0
        ) {
            let token = SoftToken::new(42, 5, 0, cost);
            let was_pruned = token.prune_if_above(threshold);

            if cost > threshold {
                prop_assert!(was_pruned);
                prop_assert!(!token.is_active());
            } else {
                prop_assert!(!was_pruned);
                prop_assert!(token.is_active());
            }
        }

        /// Clone preserves all SoftToken fields.
        #[test]
        fn soft_token_clone_preserves_fields(
            data in any::<i32>(),
            out_degree in 0u32..100,
            frame in 0u32..1000,
            cost in -1000.0f32..1000.0
        ) {
            let token = SoftToken::new(data, out_degree, frame, cost);
            let cloned = token.clone();

            prop_assert_eq!(*cloned.data(), data);
            prop_assert_eq!(cloned.out_degree(), out_degree);
            prop_assert_eq!(cloned.frame(), frame);
            prop_assert!((cloned.cost() - cost).abs() < 1e-6);
            prop_assert_eq!(cloned.is_active(), token.is_active());
        }

        /// Token with out_degree=0 is considered pruned.
        #[test]
        fn zero_out_degree_is_pruned(
            data in any::<i32>(),
            frame in 0u32..1000,
            cost in -1000.0f32..1000.0
        ) {
            let token = SoftToken::new(data, 0, frame, cost);
            prop_assert!(token.is_pruned());
        }
    }

    // =========================================================================
    // SoftPruneConfig Properties
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// SoftPruneConfig preserves all settings.
        #[test]
        fn config_preserves_settings(
            beam in 0.1f32..100.0,
            max_active in 1usize..10000
        ) {
            let config = SoftPruneConfig::new(beam, max_active);
            prop_assert!((config.beam - beam).abs() < 1e-6);
            prop_assert_eq!(config.max_active, max_active);
        }

        /// Custom compaction settings are preserved.
        #[test]
        fn config_custom_compaction(
            beam in 0.1f32..100.0,
            max_active in 1usize..10000,
            compact_threshold in 0.0f32..1.0,
            min_tokens in 0usize..1000
        ) {
            let config = SoftPruneConfig::with_compaction(
                beam,
                max_active,
                compact_threshold,
                min_tokens,
            );
            prop_assert!((config.beam - beam).abs() < 1e-6);
            prop_assert_eq!(config.max_active, max_active);
            prop_assert!((config.compact_threshold - compact_threshold).abs() < 1e-6);
            prop_assert_eq!(config.min_tokens_for_compact, min_tokens);
        }
    }

    // =========================================================================
    // SoftPruneBuffer Properties
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Push increases active count for tokens within threshold.
        #[test]
        fn buffer_push_increases_active_count(
            num_tokens in 1usize..20,
            beam in 10.0f32..100.0
        ) {
            let config = SoftPruneConfig::new(beam, 1000);
            let mut buffer = SoftPruneBuffer::new(config);

            // Add tokens with costs well below the beam
            for i in 0..num_tokens {
                buffer.push(SoftToken::new(i as i32, 5, 0, i as f32 * 0.1));
            }

            prop_assert_eq!(buffer.active_count(), num_tokens);
            prop_assert_eq!(buffer.total_count(), num_tokens);
        }

        /// Tokens exceeding threshold are rejected on push.
        #[test]
        fn buffer_rejects_high_cost_tokens(
            base_cost in 0.0f32..10.0,
            beam in 1.0f32..5.0
        ) {
            let config = SoftPruneConfig::new(beam, 1000);
            let mut buffer = SoftPruneBuffer::new(config);

            // Add first token to set best cost
            let idx1 = buffer.push(SoftToken::new(1, 5, 0, base_cost));
            prop_assert!(idx1.is_some());

            // Add token that exceeds threshold
            let high_cost = base_cost + beam + 1.0;
            let idx2 = buffer.push(SoftToken::new(2, 5, 0, high_cost));
            prop_assert!(idx2.is_none());

            prop_assert_eq!(buffer.active_count(), 1);
        }

        /// Compact removes exactly the pruned tokens.
        #[test]
        fn buffer_compact_removes_pruned(
            num_tokens in 5usize..20,
            num_to_prune in 1usize..5
        ) {
            let config = SoftPruneConfig::new(100.0, 1000);
            let mut buffer = SoftPruneBuffer::<i32>::new(config);

            // Add tokens
            for i in 0..num_tokens {
                buffer.push(SoftToken::new(i as i32, 5, 0, i as f32));
            }

            // Prune some tokens
            let actual_prune = num_to_prune.min(num_tokens);
            for i in 0..actual_prune {
                if let Some(token) = buffer.get(i) {
                    token.soft_prune();
                }
            }

            let removed = buffer.compact();
            prop_assert_eq!(removed, actual_prune);
            prop_assert_eq!(buffer.total_count(), num_tokens - actual_prune);
        }

        /// into_survivors returns only active tokens.
        #[test]
        fn buffer_into_survivors_only_active(
            num_tokens in 2usize..10,
            prune_indices in proptest::collection::vec(0usize..10, 0..5)
        ) {
            let config = SoftPruneConfig::new(100.0, 1000);
            let mut buffer = SoftPruneBuffer::new(config);

            for i in 0..num_tokens {
                buffer.push(SoftToken::new(i as i32, 5, 0, i as f32 * 0.5));
            }

            // Prune tokens at specified indices
            let mut pruned_count = 0;
            for &idx in &prune_indices {
                if idx < num_tokens {
                    if let Some(token) = buffer.get(idx) {
                        if token.is_active() {
                            token.soft_prune();
                            pruned_count += 1;
                        }
                    }
                }
            }

            let survivors = buffer.into_survivors();
            prop_assert_eq!(survivors.len(), num_tokens - pruned_count);
        }

        /// best_cost tracks minimum cost seen.
        #[test]
        fn buffer_tracks_best_cost(costs in proptest::collection::vec(0.0f32..100.0, 1..20)) {
            let config = SoftPruneConfig::new(200.0, 1000);
            let mut buffer = SoftPruneBuffer::new(config);

            for (i, &cost) in costs.iter().enumerate() {
                buffer.push(SoftToken::new(i as i32, 5, 0, cost));
            }

            let expected_best = costs.iter().cloned().fold(f32::INFINITY, f32::min);
            prop_assert!((buffer.best_cost() - expected_best).abs() < 1e-6);
        }

        /// Clear resets buffer state.
        #[test]
        fn buffer_clear_resets(num_tokens in 1usize..20) {
            let config = SoftPruneConfig::new(100.0, 1000);
            let mut buffer = SoftPruneBuffer::new(config);

            for i in 0..num_tokens {
                buffer.push(SoftToken::new(i as i32, 5, 0, i as f32));
            }

            buffer.clear();

            prop_assert_eq!(buffer.active_count(), 0);
            prop_assert_eq!(buffer.total_count(), 0);
            prop_assert!(buffer.best_cost().is_infinite());
        }
    }

    // =========================================================================
    // SoftPruneStats Properties
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// total_pruned = beam_pruned + limit_pruned.
        #[test]
        fn stats_total_pruned_correct(
            beam_pruned in 0usize..1000,
            limit_pruned in 0usize..1000
        ) {
            let mut stats = SoftPruneStats::new();
            stats.record_beam_prune(beam_pruned);
            stats.record_limit_prune(limit_pruned);

            prop_assert_eq!(stats.total_pruned(), beam_pruned + limit_pruned);
        }

        /// prune_ratio is in [0, 1] when total_tokens > 0.
        #[test]
        fn stats_prune_ratio_bounded(
            total_tokens in 1usize..1000,
            beam_pruned in 0usize..500,
            limit_pruned in 0usize..500
        ) {
            let mut stats = SoftPruneStats::new();
            stats.record_tokens(total_tokens);
            stats.record_beam_prune(beam_pruned.min(total_tokens));
            stats.record_limit_prune(limit_pruned.min(total_tokens - beam_pruned.min(total_tokens)));

            let ratio = stats.prune_ratio();
            prop_assert!(ratio >= 0.0);
            prop_assert!(ratio <= 1.0);
        }

        /// prune_ratio is 0 when no tokens processed.
        #[test]
        fn stats_prune_ratio_zero_tokens(
            beam_pruned in 0usize..100,
            limit_pruned in 0usize..100
        ) {
            let mut stats = SoftPruneStats::new();
            stats.record_beam_prune(beam_pruned);
            stats.record_limit_prune(limit_pruned);

            prop_assert!((stats.prune_ratio() - 0.0).abs() < 1e-10);
        }

        /// Merge combines all stats correctly.
        #[test]
        fn stats_merge_correct(
            total1 in 0usize..500,
            beam1 in 0usize..250,
            limit1 in 0usize..250,
            compact1 in 0usize..100,
            total2 in 0usize..500,
            beam2 in 0usize..250,
            limit2 in 0usize..250,
            compact2 in 0usize..100
        ) {
            let mut stats1 = SoftPruneStats::new();
            stats1.record_tokens(total1);
            stats1.record_beam_prune(beam1);
            stats1.record_limit_prune(limit1);
            stats1.record_compaction(compact1);

            let mut stats2 = SoftPruneStats::new();
            stats2.record_tokens(total2);
            stats2.record_beam_prune(beam2);
            stats2.record_limit_prune(limit2);
            stats2.record_compaction(compact2);

            stats1.merge(&stats2);

            prop_assert_eq!(stats1.total_tokens, total1 + total2);
            prop_assert_eq!(stats1.beam_pruned, beam1 + beam2);
            prop_assert_eq!(stats1.limit_pruned, limit1 + limit2);
            prop_assert_eq!(stats1.compactions, 2);
            prop_assert_eq!(stats1.compacted_tokens, compact1 + compact2);
        }

        /// compaction_efficiency is 0 when no compactions.
        #[test]
        fn stats_compaction_efficiency_zero(compacted in 0usize..100) {
            let mut stats = SoftPruneStats::new();
            // Don't record any compactions, just set compacted_tokens manually
            stats.compacted_tokens = compacted;

            prop_assert!((stats.compaction_efficiency() - 0.0).abs() < 1e-10);
        }
    }

    // =========================================================================
    // AdaptiveBeam Properties
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Reset clears all bucket counts.
        #[test]
        fn adaptive_beam_reset_clears(
            num_buckets in 5usize..50,
            target in 10usize..100,
            num_adds in 1usize..50
        ) {
            let mut beam = AdaptiveBeam::new(num_buckets, target);
            beam.set_range(0.0, 100.0);

            for i in 0..num_adds {
                beam.add_with_range(i as f32, 0.0, 100.0);
            }

            beam.reset();

            prop_assert_eq!(beam.total_count(), 0);
            prop_assert!(beam.min_cost.is_infinite());
            prop_assert!(beam.max_cost.is_infinite() && beam.max_cost.is_sign_negative());
        }

        /// total_count matches number of adds.
        #[test]
        fn adaptive_beam_total_count_correct(
            num_buckets in 5usize..50,
            target in 10usize..100,
            costs in proptest::collection::vec(0.0f32..100.0, 1..50)
        ) {
            let beam = AdaptiveBeam::new(num_buckets, target);

            for &cost in &costs {
                beam.add_with_range(cost, 0.0, 100.0);
            }

            prop_assert_eq!(beam.total_count(), costs.len());
        }

        /// compute_threshold returns INFINITY when all tokens fit.
        #[test]
        fn adaptive_beam_threshold_infinity_when_fits(
            num_buckets in 5usize..20,
            num_costs in 1usize..10
        ) {
            // Target more than we'll add
            let target = num_costs + 10;
            let mut beam = AdaptiveBeam::new(num_buckets, target);
            beam.set_range(0.0, 100.0);

            for i in 0..num_costs {
                beam.add_with_range(i as f32 * 5.0, 0.0, 100.0);
            }

            let threshold = beam.compute_threshold();
            prop_assert!(threshold.is_infinite());
        }

        /// compute_threshold is between min_cost and max_cost when pruning needed.
        #[test]
        fn adaptive_beam_threshold_in_range(
            num_buckets in 10usize..50,
            costs in proptest::collection::vec(0.0f32..100.0, 20..100)
        ) {
            // Target less than half of what we add
            let target = costs.len() / 3;
            let mut beam = AdaptiveBeam::new(num_buckets, target);

            let min_cost = costs.iter().cloned().fold(f32::INFINITY, f32::min);
            let max_cost = costs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

            if max_cost > min_cost {
                beam.set_range(min_cost, max_cost);

                for &cost in &costs {
                    beam.add_with_range(cost, min_cost, max_cost);
                }

                let threshold = beam.compute_threshold();

                // Threshold should be between min and max (or infinity if all fit)
                if !threshold.is_infinite() {
                    prop_assert!(threshold >= min_cost);
                    prop_assert!(threshold <= max_cost + 1e-6);
                }
            }
        }
    }

    // =========================================================================
    // SoftPruneManager Properties
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// advance_frame increments frame counter.
        #[test]
        fn manager_advance_frame_increments(num_advances in 1usize..10) {
            let config = SoftPruneConfig::new(100.0, 1000);
            let mut manager = SoftPruneManager::<i32>::new(config);

            for i in 0..num_advances {
                prop_assert_eq!(manager.frame(), i as u32);
                manager.advance_frame();
            }

            prop_assert_eq!(manager.frame(), num_advances as u32);
        }

        /// Frame swap moves current to previous.
        #[test]
        fn manager_frame_swap(num_tokens in 1usize..10) {
            let config = SoftPruneConfig::new(100.0, 1000);
            let mut manager = SoftPruneManager::new(config);

            // Add tokens to current frame
            for i in 0..num_tokens {
                manager.add_token(i as i32, 5, i as f32);
            }

            let current_count_before = manager.current().active_count();
            manager.advance_frame();

            // Previous should have the tokens from before
            prop_assert_eq!(manager.previous().active_count(), current_count_before);
            // Current should be empty
            prop_assert_eq!(manager.current().active_count(), 0);
        }

        /// survivors only returns active tokens.
        #[test]
        fn manager_survivors_only_active(
            num_tokens in 2usize..10,
            prune_first in any::<bool>()
        ) {
            let config = SoftPruneConfig::new(100.0, 1000);
            let mut manager = SoftPruneManager::new(config);

            for i in 0..num_tokens {
                manager.add_token(i as i32, 5, i as f32);
            }

            let expected_survivors = if prune_first {
                manager.current().get(0).expect("gpu/soft_prune.rs: required value was None/Err").soft_prune();
                num_tokens - 1
            } else {
                num_tokens
            };

            let survivors = manager.survivors();
            prop_assert_eq!(survivors.len(), expected_survivors);
        }

        /// Stats track tokens added.
        #[test]
        fn manager_stats_track_tokens(num_tokens in 1usize..20) {
            let config = SoftPruneConfig::new(100.0, 1000);
            let mut manager = SoftPruneManager::new(config);

            for i in 0..num_tokens {
                manager.add_token(i as i32, 5, i as f32);
            }

            prop_assert_eq!(manager.stats().total_tokens, num_tokens);
        }

        /// apply_pruning updates stats.
        #[test]
        fn manager_apply_pruning_updates_stats(num_tokens in 5usize..20) {
            let beam = 5.0f32;
            let config = SoftPruneConfig::new(beam, 1000);
            let mut manager = SoftPruneManager::new(config);

            // Add tokens with increasing costs - some will be beyond beam
            for i in 0..num_tokens {
                manager.add_token(i as i32, 5, i as f32);
            }

            let pruned = manager.apply_pruning();
            prop_assert_eq!(manager.stats().beam_pruned, pruned);
        }
    }
}
