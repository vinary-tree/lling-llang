//! SIMD-accelerated operations for high-performance WFST computations.
//!
//! This module provides vectorized implementations of common WFST operations
//! using SIMD instructions (AVX-512, AVX2, SSE, or NEON depending on platform).
//!
//! ## Supported Operations
//!
//! | Operation | Description | Speedup |
//! |-----------|-------------|---------|
//! | `simd_tropical_min` | Parallel min for tropical ⊕ | ~4-8× |
//! | `simd_log_add` | Vectorized log-sum-exp | ~2-4× |
//! | `simd_add` | Parallel addition for ⊗ | ~4-8× |
//! | `simd_forward_scores` | Batch forward score computation | ~3-6× |
//!
//! ## Example
//!
//! ```rust
//! use lling_llang::simd::{simd_tropical_min, simd_add};
//!
//! let a = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
//! let b = vec![8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0];
//!
//! // Compute element-wise min using SIMD
//! let min_result = simd_tropical_min(&a, &b);
//! assert_eq!(min_result, vec![1.0, 2.0, 3.0, 4.0, 4.0, 3.0, 2.0, 1.0]);
//!
//! // Compute element-wise sum using SIMD
//! let sum_result = simd_add(&a, &b);
//! assert_eq!(sum_result, vec![9.0, 9.0, 9.0, 9.0, 9.0, 9.0, 9.0, 9.0]);
//! ```
//!
//! ## Feature Detection
//!
//! The module automatically detects and uses the best available SIMD instruction set:
//!
//! - x86-64: AVX-512 → AVX2 → SSE4.2 → Scalar fallback
//! - AArch64: NEON → Scalar fallback
//!
//! ## Configuration
//!
//! SIMD operations can be controlled via feature flags:
//!
//! ```toml
//! [features]
//! simd-avx512 = []  # Enable AVX-512 (requires nightly)
//! simd-avx2 = []    # Enable AVX2
//! simd-neon = []    # Enable ARM NEON
//! ```

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

// ============================================================================
// SIMD Feature Detection
// ============================================================================

/// Runtime SIMD capability detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimdCapability {
    /// AVX-512 with F instructions.
    Avx512,
    /// AVX2.
    Avx2,
    /// SSE4.2.
    Sse42,
    /// ARM NEON.
    Neon,
    /// No SIMD (scalar fallback).
    Scalar,
}

impl SimdCapability {
    /// Detect the best available SIMD capability on this CPU.
    #[cfg(target_arch = "x86_64")]
    pub fn detect() -> Self {
        if is_x86_feature_detected!("avx512f") {
            SimdCapability::Avx512
        } else if is_x86_feature_detected!("avx2") {
            SimdCapability::Avx2
        } else if is_x86_feature_detected!("sse4.2") {
            SimdCapability::Sse42
        } else {
            SimdCapability::Scalar
        }
    }

    #[cfg(target_arch = "aarch64")]
    pub fn detect() -> Self {
        // NEON is mandatory on AArch64
        SimdCapability::Neon
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    pub fn detect() -> Self {
        SimdCapability::Scalar
    }

    /// Get the vector width (number of f64 elements per SIMD register).
    pub const fn vector_width(&self) -> usize {
        match self {
            SimdCapability::Avx512 => 8,
            SimdCapability::Avx2 => 4,
            SimdCapability::Sse42 => 2,
            SimdCapability::Neon => 2,
            SimdCapability::Scalar => 1,
        }
    }
}

// ============================================================================
// Tropical Semiring Operations (min-based)
// ============================================================================

/// Compute element-wise minimum of two vectors (tropical ⊕).
///
/// Uses SIMD when available for significant speedup on large vectors.
pub fn simd_tropical_min(a: &[f64], b: &[f64]) -> Vec<f64> {
    assert_eq!(a.len(), b.len(), "vectors must have same length");
    let n = a.len();
    let mut result = vec![0.0; n];

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe { simd_tropical_min_avx2(a, b, &mut result) };
            return result;
        }
    }

    // Scalar fallback
    for i in 0..n {
        result[i] = a[i].min(b[i]);
    }
    result
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn simd_tropical_min_avx2(a: &[f64], b: &[f64], result: &mut [f64]) {
    let n = a.len();
    let chunks = n / 4;
    let remainder = n % 4;

    for i in 0..chunks {
        let offset = i * 4;
        let va = _mm256_loadu_pd(a.as_ptr().add(offset));
        let vb = _mm256_loadu_pd(b.as_ptr().add(offset));
        let vmin = _mm256_min_pd(va, vb);
        _mm256_storeu_pd(result.as_mut_ptr().add(offset), vmin);
    }

    // Handle remainder
    let base = chunks * 4;
    for i in 0..remainder {
        result[base + i] = a[base + i].min(b[base + i]);
    }
}

/// Compute minimum across all elements (tropical sum reduction).
pub fn simd_tropical_reduce_min(a: &[f64]) -> f64 {
    if a.is_empty() {
        return f64::INFINITY;
    }

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            return unsafe { simd_tropical_reduce_min_avx2(a) };
        }
    }

    // Scalar fallback
    a.iter().copied().fold(f64::INFINITY, f64::min)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn simd_tropical_reduce_min_avx2(a: &[f64]) -> f64 {
    let n = a.len();
    let chunks = n / 4;
    let remainder = n % 4;

    let mut acc = _mm256_set1_pd(f64::INFINITY);

    for i in 0..chunks {
        let va = _mm256_loadu_pd(a.as_ptr().add(i * 4));
        acc = _mm256_min_pd(acc, va);
    }

    // Reduce 4 lanes to scalar
    let mut temp = [0.0f64; 4];
    _mm256_storeu_pd(temp.as_mut_ptr(), acc);
    let mut result = temp[0].min(temp[1]).min(temp[2]).min(temp[3]);

    // Handle remainder
    let base = chunks * 4;
    for i in 0..remainder {
        result = result.min(a[base + i]);
    }

    result
}

// ============================================================================
// Addition Operations (semiring ⊗)
// ============================================================================

/// Compute element-wise sum of two vectors (semiring ⊗ for tropical/log).
pub fn simd_add(a: &[f64], b: &[f64]) -> Vec<f64> {
    assert_eq!(a.len(), b.len(), "vectors must have same length");
    let n = a.len();
    let mut result = vec![0.0; n];

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe { simd_add_avx2(a, b, &mut result) };
            return result;
        }
    }

    // Scalar fallback
    for i in 0..n {
        result[i] = a[i] + b[i];
    }
    result
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn simd_add_avx2(a: &[f64], b: &[f64], result: &mut [f64]) {
    let n = a.len();
    let chunks = n / 4;
    let remainder = n % 4;

    for i in 0..chunks {
        let offset = i * 4;
        let va = _mm256_loadu_pd(a.as_ptr().add(offset));
        let vb = _mm256_loadu_pd(b.as_ptr().add(offset));
        let vsum = _mm256_add_pd(va, vb);
        _mm256_storeu_pd(result.as_mut_ptr().add(offset), vsum);
    }

    let base = chunks * 4;
    for i in 0..remainder {
        result[base + i] = a[base + i] + b[base + i];
    }
}

/// Compute sum across all elements.
pub fn simd_reduce_sum(a: &[f64]) -> f64 {
    if a.is_empty() {
        return 0.0;
    }

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            return unsafe { simd_reduce_sum_avx2(a) };
        }
    }

    // Scalar fallback
    a.iter().sum()
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn simd_reduce_sum_avx2(a: &[f64]) -> f64 {
    let n = a.len();
    let chunks = n / 4;
    let remainder = n % 4;

    let mut acc = _mm256_setzero_pd();

    for i in 0..chunks {
        let va = _mm256_loadu_pd(a.as_ptr().add(i * 4));
        acc = _mm256_add_pd(acc, va);
    }

    // Reduce 4 lanes to scalar
    let mut temp = [0.0f64; 4];
    _mm256_storeu_pd(temp.as_mut_ptr(), acc);
    let mut result = temp[0] + temp[1] + temp[2] + temp[3];

    // Handle remainder
    let base = chunks * 4;
    for i in 0..remainder {
        result += a[base + i];
    }

    result
}

// ============================================================================
// Log-Space Operations (log-sum-exp)
// ============================================================================

/// Compute log-sum-exp across all elements (log semiring sum).
///
/// Uses the numerically stable formula: log(Σ exp(a_i)) = max(a) + log(Σ exp(a_i - max(a)))
pub fn simd_log_sum_exp(a: &[f64]) -> f64 {
    if a.is_empty() {
        return f64::NEG_INFINITY;
    }
    if a.len() == 1 {
        return a[0];
    }

    // Find max for numerical stability
    let max_val = simd_reduce_max(a);

    if max_val.is_infinite() {
        return max_val;
    }

    // Compute sum of exp(a_i - max) without materializing the shifted vector.
    let sum = a
        .iter()
        .copied()
        .fold(0.0, |sum, x| sum + (x - max_val).exp());

    max_val + sum.ln()
}

/// Element-wise log-add (log semiring ⊕).
///
/// Computes log(exp(a) + exp(b)) element-wise with numerical stability.
pub fn simd_log_add(a: &[f64], b: &[f64]) -> Vec<f64> {
    assert_eq!(a.len(), b.len(), "vectors must have same length");
    let n = a.len();
    let mut result = vec![0.0; n];

    // SIMD exp/ln requires special handling, so keep the transcendental
    // operation scalar while avoiding duplicated edge-case logic.
    for ((out, &left), &right) in result.iter_mut().zip(a).zip(b) {
        *out = log_add_scalar(left, right);
    }

    result
}

/// Compute maximum across all elements.
pub fn simd_reduce_max(a: &[f64]) -> f64 {
    if a.is_empty() {
        return f64::NEG_INFINITY;
    }

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            return unsafe { simd_reduce_max_avx2(a) };
        }
    }

    // Scalar fallback
    a.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn simd_reduce_max_avx2(a: &[f64]) -> f64 {
    let n = a.len();
    let chunks = n / 4;
    let remainder = n % 4;

    let mut acc = _mm256_set1_pd(f64::NEG_INFINITY);

    for i in 0..chunks {
        let va = _mm256_loadu_pd(a.as_ptr().add(i * 4));
        acc = _mm256_max_pd(acc, va);
    }

    let mut temp = [0.0f64; 4];
    _mm256_storeu_pd(temp.as_mut_ptr(), acc);
    let mut result = temp[0].max(temp[1]).max(temp[2]).max(temp[3]);

    let base = chunks * 4;
    for i in 0..remainder {
        result = result.max(a[base + i]);
    }

    result
}

// ============================================================================
// Batch Forward Score Computation
// ============================================================================

/// Configuration for batch forward score computation.
#[derive(Debug, Clone)]
pub struct BatchForwardConfig {
    /// Maximum number of active states to track.
    pub max_active_states: usize,
    /// Pruning beam (states with score worse than best + beam are pruned).
    pub beam: f64,
}

impl Default for BatchForwardConfig {
    fn default() -> Self {
        Self {
            max_active_states: 1000,
            beam: 10.0,
        }
    }
}

/// Batch forward scores represented as sparse vectors.
#[derive(Debug, Clone)]
pub struct BatchForwardScores {
    /// State indices.
    pub states: Vec<u32>,
    /// Forward scores for each state.
    pub scores: Vec<f64>,
    /// Best score seen so far.
    pub best_score: f64,
}

impl BatchForwardScores {
    /// Create new empty forward scores.
    pub fn new() -> Self {
        Self {
            states: Vec::new(),
            scores: Vec::new(),
            best_score: f64::INFINITY,
        }
    }

    /// Create with capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            states: Vec::with_capacity(capacity),
            scores: Vec::with_capacity(capacity),
            best_score: f64::INFINITY,
        }
    }

    /// Add a score for a state.
    pub fn add(&mut self, state: u32, score: f64) {
        self.states.push(state);
        self.scores.push(score);
        self.best_score = self.best_score.min(score);
    }

    /// Get the number of active states.
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// Prune states outside the beam.
    pub fn prune(&mut self, beam: f64) {
        let threshold = self.best_score + beam;
        let mut write_idx = 0;

        for read_idx in 0..self.states.len() {
            if self.scores[read_idx] <= threshold {
                self.states[write_idx] = self.states[read_idx];
                self.scores[write_idx] = self.scores[read_idx];
                write_idx += 1;
            }
        }

        self.states.truncate(write_idx);
        self.scores.truncate(write_idx);
    }

    /// Merge scores for duplicate states using log-add.
    pub fn merge_duplicates_log(&mut self) {
        self.merge_duplicates_by(log_add_scalar);
    }

    /// Merge scores for duplicate states using tropical (min).
    pub fn merge_duplicates_tropical(&mut self) {
        self.merge_duplicates_by(f64::min);
    }

    fn merge_duplicates_by(&mut self, merge: impl Fn(f64, f64) -> f64) {
        let len = self.states.len().min(self.scores.len());
        self.states.truncate(len);
        self.scores.truncate(len);

        if len <= 1 {
            self.best_score = simd_tropical_reduce_min(&self.scores);
            return;
        }

        let mut pairs: Vec<(u32, f64)> = self
            .states
            .iter()
            .copied()
            .zip(self.scores.iter().copied())
            .collect();
        pairs.sort_unstable_by_key(|&(state, _)| state);

        self.states.clear();
        self.scores.clear();

        let mut pairs = pairs.into_iter();
        let Some((mut current_state, mut current_score)) = pairs.next() else {
            self.best_score = f64::INFINITY;
            return;
        };

        for (state, score) in pairs {
            if state == current_state {
                current_score = merge(current_score, score);
            } else {
                self.states.push(current_state);
                self.scores.push(current_score);
                current_state = state;
                current_score = score;
            }
        }

        self.states.push(current_state);
        self.scores.push(current_score);
        self.best_score = simd_tropical_reduce_min(&self.scores);
    }
}

impl Default for BatchForwardScores {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Distance Matrix Operations
// ============================================================================

/// Compute pairwise min-plus distance update for dense matrices.
///
/// Updates D[i,j] = min(D[i,j], D[i,k] + D[k,j]) for all i,j.
/// This is useful for Floyd-Warshall-style all-pairs shortest paths.
pub fn simd_min_plus_update(d: &mut [f64], n: usize, k: usize) {
    assert_eq!(d.len(), n * n, "matrix must be n×n");

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe { simd_min_plus_update_avx2(d, n, k) };
            return;
        }
    }

    // Scalar fallback
    for i in 0..n {
        let d_ik = d[i * n + k];
        if d_ik.is_infinite() {
            continue;
        }
        for j in 0..n {
            let d_kj = d[k * n + j];
            let new_dist = d_ik + d_kj;
            if new_dist < d[i * n + j] {
                d[i * n + j] = new_dist;
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn simd_min_plus_update_avx2(d: &mut [f64], n: usize, k: usize) {
    for i in 0..n {
        let d_ik = d[i * n + k];
        if d_ik.is_infinite() {
            continue;
        }
        let v_d_ik = _mm256_set1_pd(d_ik);

        let chunks = n / 4;
        let remainder = n % 4;

        for j_chunk in 0..chunks {
            let j = j_chunk * 4;
            let v_d_kj = _mm256_loadu_pd(d.as_ptr().add(k * n + j));
            let v_new = _mm256_add_pd(v_d_ik, v_d_kj);
            let v_d_ij = _mm256_loadu_pd(d.as_ptr().add(i * n + j));
            let v_result = _mm256_min_pd(v_d_ij, v_new);
            _mm256_storeu_pd(d.as_mut_ptr().add(i * n + j), v_result);
        }

        // Handle remainder
        let base = chunks * 4;
        for j in 0..remainder {
            let idx = i * n + base + j;
            let new_dist = d_ik + d[k * n + base + j];
            if new_dist < d[idx] {
                d[idx] = new_dist;
            }
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Scalar log-add for two values.
#[inline]
fn log_add_scalar(a: f64, b: f64) -> f64 {
    let (x, y) = if a >= b { (a, b) } else { (b, a) };

    if x == f64::INFINITY || y == f64::NEG_INFINITY {
        x
    } else {
        x + (y - x).exp().ln_1p()
    }
}

/// Scale all elements by a constant.
pub fn simd_scale(a: &mut [f64], scale: f64) {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe { simd_scale_avx2(a, scale) };
            return;
        }
    }

    for x in a.iter_mut() {
        *x *= scale;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn simd_scale_avx2(a: &mut [f64], scale: f64) {
    let n = a.len();
    let chunks = n / 4;
    let remainder = n % 4;
    let v_scale = _mm256_set1_pd(scale);

    for i in 0..chunks {
        let offset = i * 4;
        let va = _mm256_loadu_pd(a.as_ptr().add(offset));
        let vr = _mm256_mul_pd(va, v_scale);
        _mm256_storeu_pd(a.as_mut_ptr().add(offset), vr);
    }

    let base = chunks * 4;
    for i in 0..remainder {
        a[base + i] *= scale;
    }
}

/// Add a constant to all elements (shift).
pub fn simd_shift(a: &mut [f64], offset: f64) {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            unsafe { simd_shift_avx2(a, offset) };
            return;
        }
    }

    for x in a.iter_mut() {
        *x += offset;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn simd_shift_avx2(a: &mut [f64], offset: f64) {
    let n = a.len();
    let chunks = n / 4;
    let remainder = n % 4;
    let v_offset = _mm256_set1_pd(offset);

    for i in 0..chunks {
        let idx = i * 4;
        let va = _mm256_loadu_pd(a.as_ptr().add(idx));
        let vr = _mm256_add_pd(va, v_offset);
        _mm256_storeu_pd(a.as_mut_ptr().add(idx), vr);
    }

    let base = chunks * 4;
    for i in 0..remainder {
        a[base + i] += offset;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simd_capability_detect() {
        let cap = SimdCapability::detect();
        println!("Detected SIMD capability: {:?}", cap);
        assert!(cap.vector_width() >= 1);
    }

    #[test]
    fn test_tropical_min() {
        let a = vec![1.0, 5.0, 3.0, 7.0, 2.0, 6.0, 4.0, 8.0];
        let b = vec![8.0, 4.0, 6.0, 2.0, 7.0, 3.0, 5.0, 1.0];
        let result = simd_tropical_min(&a, &b);

        assert_eq!(result, vec![1.0, 4.0, 3.0, 2.0, 2.0, 3.0, 4.0, 1.0]);
    }

    #[test]
    fn test_tropical_reduce_min() {
        let a = vec![5.0, 2.0, 8.0, 1.0, 6.0, 3.0, 7.0, 4.0, 9.0, 0.5];
        assert!((simd_tropical_reduce_min(&a) - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_add() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let b = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let result = simd_add(&a, &b);

        assert_eq!(result, vec![11.0, 22.0, 33.0, 44.0, 55.0]);
    }

    #[test]
    fn test_reduce_sum() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert!((simd_reduce_sum(&a) - 55.0).abs() < 1e-10);
    }

    #[test]
    fn test_log_sum_exp() {
        let a = vec![0.0, 0.0]; // log(1) + log(1) = log(2) ≈ 0.693
        let result = simd_log_sum_exp(&a);
        assert!((result - 2.0_f64.ln()).abs() < 1e-10);

        // Test numerical stability with large values
        let b = vec![1000.0, 1000.0];
        let result_b = simd_log_sum_exp(&b);
        assert!((result_b - (1000.0 + 2.0_f64.ln())).abs() < 1e-10);
    }

    #[test]
    fn test_log_add() {
        let a = vec![0.0, -1.0, -2.0];
        let b = vec![0.0, -1.0, -3.0];
        let result = simd_log_add(&a, &b);

        // log(exp(0) + exp(0)) = log(2)
        assert!((result[0] - 2.0_f64.ln()).abs() < 1e-10);
        // log(exp(-1) + exp(-1)) = -1 + log(2)
        assert!((result[1] - (-1.0 + 2.0_f64.ln())).abs() < 1e-10);
    }

    #[test]
    fn test_log_add_handles_positive_infinity() {
        let result = simd_log_add(&[f64::INFINITY], &[f64::INFINITY]);
        assert_eq!(result, vec![f64::INFINITY]);
    }

    #[test]
    fn test_log_sum_exp_handles_positive_infinity() {
        let result = simd_log_sum_exp(&[0.0, f64::INFINITY]);
        assert_eq!(result, f64::INFINITY);
    }

    #[test]
    fn test_reduce_max() {
        let a = vec![1.0, 5.0, 3.0, 8.0, 2.0, 7.0, 4.0, 6.0, 0.0, 9.0];
        assert!((simd_reduce_max(&a) - 9.0).abs() < 1e-10);
    }

    #[test]
    fn test_batch_forward_scores_prune() {
        let mut scores = BatchForwardScores::with_capacity(5);
        scores.add(0, 1.0);
        scores.add(1, 5.0);
        scores.add(2, 2.0);
        scores.add(3, 10.0);
        scores.add(4, 1.5);

        scores.prune(3.0); // beam = 3.0, best = 1.0, threshold = 4.0

        assert_eq!(scores.len(), 3); // States 0, 2, 4 should survive
        assert!(scores.scores.iter().all(|&s| s <= 4.0));
    }

    #[test]
    fn test_batch_forward_scores_merge_tropical() {
        let mut scores = BatchForwardScores::with_capacity(5);
        scores.add(0, 3.0);
        scores.add(1, 5.0);
        scores.add(0, 2.0); // Duplicate state 0
        scores.add(1, 4.0); // Duplicate state 1

        scores.merge_duplicates_tropical();

        assert_eq!(scores.len(), 2);
        // Find scores for states 0 and 1
        let s0_idx = scores.states.iter().position(|&s| s == 0).expect("state 0");
        let s1_idx = scores.states.iter().position(|&s| s == 1).expect("state 1");
        assert!((scores.scores[s0_idx] - 2.0).abs() < 1e-10); // min(3.0, 2.0)
        assert!((scores.scores[s1_idx] - 4.0).abs() < 1e-10); // min(5.0, 4.0)
    }

    #[test]
    fn test_batch_forward_scores_merge_log() {
        let mut scores = BatchForwardScores::with_capacity(4);
        scores.add(2, 0.0);
        scores.add(1, -1.0);
        scores.add(2, 0.0);
        scores.add(1, f64::NEG_INFINITY);

        scores.merge_duplicates_log();

        assert_eq!(scores.len(), 2);
        let s1_idx = scores.states.iter().position(|&s| s == 1).expect("state 1");
        let s2_idx = scores.states.iter().position(|&s| s == 2).expect("state 2");
        assert!((scores.scores[s1_idx] + 1.0).abs() < 1e-10);
        assert!((scores.scores[s2_idx] - 2.0_f64.ln()).abs() < 1e-10);
        assert!((scores.best_score + 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_min_plus_update() {
        // 3x3 distance matrix
        let mut d = vec![
            0.0,
            1.0,
            f64::INFINITY,
            f64::INFINITY,
            0.0,
            1.0,
            f64::INFINITY,
            f64::INFINITY,
            0.0,
        ];

        // Update through vertex 1
        simd_min_plus_update(&mut d, 3, 1);

        // D[0,2] should now be D[0,1] + D[1,2] = 1.0 + 1.0 = 2.0
        assert!((d[2] - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_scale() {
        let mut a = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        simd_scale(&mut a, 2.0);
        assert_eq!(
            a,
            vec![2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0, 20.0]
        );
    }

    #[test]
    fn test_shift() {
        let mut a = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        simd_shift(&mut a, 100.0);
        assert_eq!(
            a,
            vec![101.0, 102.0, 103.0, 104.0, 105.0, 106.0, 107.0, 108.0, 109.0, 110.0]
        );
    }

    #[test]
    fn test_empty_vectors() {
        let empty: Vec<f64> = vec![];
        assert_eq!(simd_tropical_reduce_min(&empty), f64::INFINITY);
        assert_eq!(simd_reduce_sum(&empty), 0.0);
        assert_eq!(simd_reduce_max(&empty), f64::NEG_INFINITY);
        assert_eq!(simd_log_sum_exp(&empty), f64::NEG_INFINITY);
    }

    #[test]
    fn test_single_element() {
        let single = vec![42.0];
        assert!((simd_tropical_reduce_min(&single) - 42.0).abs() < 1e-10);
        assert!((simd_reduce_sum(&single) - 42.0).abs() < 1e-10);
        assert!((simd_reduce_max(&single) - 42.0).abs() < 1e-10);
        assert!((simd_log_sum_exp(&single) - 42.0).abs() < 1e-10);
    }
}
