//! Quantized semiring weights for memory-efficient WFSTs.
//!
//! This module provides quantized weight representations that store weights
//! in reduced precision (8-bit or 4-bit) for significant memory savings.
//!
//! ## Use Cases
//!
//! - Large-vocabulary language models where memory is constrained
//! - Mobile/embedded deployment with limited RAM
//! - Approximate computations where full precision is unnecessary
//! - Fast nearest-neighbor lookups using quantized distances
//!
//! ## Quantization Schemes
//!
//! | Type | Bits | Range | Use Case |
//! |------|------|-------|----------|
//! | `Quantized8Weight` | 8 | 256 levels | General purpose |
//! | `Quantized4Weight` | 4 | 16 levels | Extreme compression |
//!
//! ## Example
//!
//! ```rust
//! use lling_llang::semiring::{LogWeight, Semiring};
//! use lling_llang::semiring::quantized::{Quantized8Weight, QuantizationParams};
//!
//! // Create quantization parameters for log probabilities in [-10, 0]
//! let params = QuantizationParams::new(-10.0, 0.0);
//!
//! // Quantize a LogWeight
//! let weight = LogWeight::new(-3.5);
//! let quantized = Quantized8Weight::from_log_weight(weight, &params);
//!
//! // Dequantize back
//! let recovered = quantized.to_log_weight(&params);
//! assert!((recovered.value() - (-3.5)).abs() < 0.1); // Within quantization error
//! ```

use super::{LogWeight, TropicalWeight};
use std::fmt;

/// Parameters for quantization/dequantization.
///
/// Defines the mapping between continuous weight values and quantized integers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuantizationParams {
    /// Minimum value in the quantization range.
    pub min_val: f64,
    /// Maximum value in the quantization range.
    pub max_val: f64,
    /// Scale factor: (max_val - min_val) / num_levels.
    scale: f64,
    /// Inverse scale for dequantization.
    inv_scale: f64,
}

impl QuantizationParams {
    /// Create new quantization parameters for a given range.
    ///
    /// # Arguments
    ///
    /// * `min_val` - Minimum expected weight value
    /// * `max_val` - Maximum expected weight value
    ///
    /// # Panics
    ///
    /// Panics if `min_val >= max_val`.
    pub fn new(min_val: f64, max_val: f64) -> Self {
        assert!(
            min_val < max_val,
            "min_val must be less than max_val: {} >= {}",
            min_val,
            max_val
        );
        let range = max_val - min_val;
        Self {
            min_val,
            max_val,
            scale: 255.0 / range, // For 8-bit quantization
            inv_scale: range / 255.0,
        }
    }

    /// Create parameters from observed data by computing min/max.
    pub fn from_data(values: impl Iterator<Item = f64>) -> Option<Self> {
        let mut min_val = f64::INFINITY;
        let mut max_val = f64::NEG_INFINITY;
        let mut count = 0;

        for v in values {
            if v.is_finite() {
                min_val = min_val.min(v);
                max_val = max_val.max(v);
                count += 1;
            }
        }

        if count == 0 || min_val >= max_val {
            return None;
        }

        // Add small margin to avoid boundary issues
        let margin = (max_val - min_val) * 0.01;
        Some(Self::new(min_val - margin, max_val + margin))
    }

    /// Create parameters for log-space weights (typical range: -20 to 0).
    pub fn for_log_weights() -> Self {
        Self::new(-20.0, 0.0)
    }

    /// Create parameters for tropical weights (typical range: 0 to 20).
    pub fn for_tropical_weights() -> Self {
        Self::new(0.0, 20.0)
    }

    /// Get the quantization scale factor (for 8-bit).
    #[inline]
    pub fn scale_8bit(&self) -> f64 {
        self.scale
    }

    /// Get the quantization scale factor for 4-bit.
    #[inline]
    pub fn scale_4bit(&self) -> f64 {
        15.0 / (self.max_val - self.min_val)
    }

    /// Quantize a value to 8-bit.
    #[inline]
    pub fn quantize_8bit(&self, value: f64) -> u8 {
        if !value.is_finite() {
            return if value.is_sign_positive() { 255 } else { 0 };
        }
        let clamped = value.clamp(self.min_val, self.max_val);
        let normalized = (clamped - self.min_val) * self.scale;
        normalized.round() as u8
    }

    /// Dequantize an 8-bit value.
    #[inline]
    pub fn dequantize_8bit(&self, quantized: u8) -> f64 {
        self.min_val + (quantized as f64) * self.inv_scale
    }

    /// Quantize a value to 4-bit.
    #[inline]
    pub fn quantize_4bit(&self, value: f64) -> u8 {
        if !value.is_finite() {
            return if value.is_sign_positive() { 15 } else { 0 };
        }
        let clamped = value.clamp(self.min_val, self.max_val);
        let scale = self.scale_4bit();
        let normalized = (clamped - self.min_val) * scale;
        (normalized.round() as u8).min(15)
    }

    /// Dequantize a 4-bit value.
    #[inline]
    pub fn dequantize_4bit(&self, quantized: u8) -> f64 {
        let inv_scale = (self.max_val - self.min_val) / 15.0;
        self.min_val + ((quantized & 0x0F) as f64) * inv_scale
    }
}

// ============================================================================
// 8-bit Quantized Weight
// ============================================================================

/// 8-bit quantized weight providing 256 distinct levels.
///
/// Memory usage is 1 byte per weight compared to 8 bytes for f64,
/// giving 8× memory reduction.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Quantized8Weight {
    /// The quantized value (0-255).
    value: u8,
}

impl Quantized8Weight {
    /// Create a new quantized weight from a raw byte value.
    #[inline]
    pub const fn from_raw(value: u8) -> Self {
        Self { value }
    }

    /// Get the raw byte value.
    #[inline]
    pub const fn raw(&self) -> u8 {
        self.value
    }

    /// Create from a LogWeight using given quantization parameters.
    #[inline]
    pub fn from_log_weight(weight: LogWeight, params: &QuantizationParams) -> Self {
        Self {
            value: params.quantize_8bit(weight.value()),
        }
    }

    /// Convert back to LogWeight.
    #[inline]
    pub fn to_log_weight(&self, params: &QuantizationParams) -> LogWeight {
        LogWeight::new(params.dequantize_8bit(self.value))
    }

    /// Create from a TropicalWeight.
    #[inline]
    pub fn from_tropical_weight(weight: TropicalWeight, params: &QuantizationParams) -> Self {
        Self {
            value: params.quantize_8bit(weight.value()),
        }
    }

    /// Convert back to TropicalWeight.
    #[inline]
    pub fn to_tropical_weight(&self, params: &QuantizationParams) -> TropicalWeight {
        TropicalWeight::new(params.dequantize_8bit(self.value))
    }

    /// Zero value (represents minimum of range, typically infinity).
    pub const ZERO: Self = Self { value: 255 };

    /// One value (represents neutral element).
    pub const ONE: Self = Self { value: 0 };
}

impl fmt::Debug for Quantized8Weight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Q8({})", self.value)
    }
}

impl fmt::Display for Quantized8Weight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl Default for Quantized8Weight {
    fn default() -> Self {
        Self::ZERO
    }
}

// ============================================================================
// 4-bit Quantized Weight
// ============================================================================

/// 4-bit quantized weight providing 16 distinct levels.
///
/// Two weights can be packed into a single byte for extreme compression.
/// Useful when weight precision is less important than memory footprint.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Quantized4Weight {
    /// The quantized value (0-15).
    value: u8,
}

impl Quantized4Weight {
    /// Create a new quantized weight from a raw nibble value.
    #[inline]
    pub const fn from_raw(value: u8) -> Self {
        Self {
            value: value & 0x0F,
        }
    }

    /// Get the raw nibble value.
    #[inline]
    pub const fn raw(&self) -> u8 {
        self.value
    }

    /// Create from a LogWeight.
    #[inline]
    pub fn from_log_weight(weight: LogWeight, params: &QuantizationParams) -> Self {
        Self {
            value: params.quantize_4bit(weight.value()),
        }
    }

    /// Convert back to LogWeight.
    #[inline]
    pub fn to_log_weight(&self, params: &QuantizationParams) -> LogWeight {
        LogWeight::new(params.dequantize_4bit(self.value))
    }

    /// Create from a TropicalWeight.
    #[inline]
    pub fn from_tropical_weight(weight: TropicalWeight, params: &QuantizationParams) -> Self {
        Self {
            value: params.quantize_4bit(weight.value()),
        }
    }

    /// Convert back to TropicalWeight.
    #[inline]
    pub fn to_tropical_weight(&self, params: &QuantizationParams) -> TropicalWeight {
        TropicalWeight::new(params.dequantize_4bit(self.value))
    }

    /// Zero value (represents minimum of range).
    pub const ZERO: Self = Self { value: 15 };

    /// One value (represents neutral element).
    pub const ONE: Self = Self { value: 0 };
}

impl fmt::Debug for Quantized4Weight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Q4({})", self.value)
    }
}

impl fmt::Display for Quantized4Weight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl Default for Quantized4Weight {
    fn default() -> Self {
        Self::ZERO
    }
}

// ============================================================================
// Packed Weight Storage
// ============================================================================

/// Packed storage for 4-bit weights, storing two weights per byte.
///
/// This is useful for storing large arrays of weights with minimal memory.
#[derive(Debug, Clone)]
pub struct PackedWeights4 {
    /// Packed bytes, each containing two 4-bit weights.
    data: Vec<u8>,
    /// Number of weights stored.
    len: usize,
}

impl PackedWeights4 {
    /// Create a new packed weight array with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity((capacity + 1) / 2),
            len: 0,
        }
    }

    /// Create from a slice of 4-bit weights.
    pub fn from_weights(weights: &[Quantized4Weight]) -> Self {
        let mut packed = Self::with_capacity(weights.len());
        for w in weights {
            packed.push(*w);
        }
        packed
    }

    /// Push a weight onto the array.
    pub fn push(&mut self, weight: Quantized4Weight) {
        if self.len % 2 == 0 {
            self.data.push(weight.raw());
        } else {
            let last = self.data.last_mut().expect("non-empty after odd push");
            *last |= weight.raw() << 4;
        }
        self.len += 1;
    }

    /// Get the weight at the given index.
    #[inline]
    pub fn get(&self, index: usize) -> Option<Quantized4Weight> {
        if index >= self.len {
            return None;
        }
        let byte_idx = index / 2;
        let nibble = if index % 2 == 0 {
            self.data[byte_idx] & 0x0F
        } else {
            (self.data[byte_idx] >> 4) & 0x0F
        };
        Some(Quantized4Weight::from_raw(nibble))
    }

    /// Set the weight at the given index.
    pub fn set(&mut self, index: usize, weight: Quantized4Weight) {
        if index >= self.len {
            return;
        }
        let byte_idx = index / 2;
        if index % 2 == 0 {
            self.data[byte_idx] = (self.data[byte_idx] & 0xF0) | weight.raw();
        } else {
            self.data[byte_idx] = (self.data[byte_idx] & 0x0F) | (weight.raw() << 4);
        }
    }

    /// Get the number of weights stored.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get the memory size in bytes.
    #[inline]
    pub fn memory_size(&self) -> usize {
        self.data.len()
    }

    /// Iterate over the weights.
    pub fn iter(&self) -> impl Iterator<Item = Quantized4Weight> + '_ {
        (0..self.len).map(|i| self.get(i).expect("valid index"))
    }
}

// ============================================================================
// Quantization Statistics
// ============================================================================

/// Statistics about quantization error.
#[derive(Debug, Clone, Default)]
pub struct QuantizationStats {
    /// Total number of weights quantized.
    pub count: usize,
    /// Sum of absolute quantization errors.
    pub total_error: f64,
    /// Maximum absolute quantization error.
    pub max_error: f64,
    /// Sum of squared errors.
    pub sum_squared_error: f64,
}

impl QuantizationStats {
    /// Create new empty stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an observation of original and quantized values.
    pub fn add(&mut self, original: f64, quantized: f64) {
        let error = (original - quantized).abs();
        self.count += 1;
        self.total_error += error;
        self.max_error = self.max_error.max(error);
        self.sum_squared_error += error * error;
    }

    /// Get the mean absolute error.
    pub fn mean_error(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.total_error / self.count as f64
        }
    }

    /// Get the root mean squared error.
    pub fn rmse(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            (self.sum_squared_error / self.count as f64).sqrt()
        }
    }
}

// ============================================================================
// Batch Quantization
// ============================================================================

/// Quantize a batch of log weights to 8-bit.
pub fn quantize_log_weights_8bit(
    weights: &[LogWeight],
    params: &QuantizationParams,
) -> Vec<Quantized8Weight> {
    weights
        .iter()
        .map(|w| Quantized8Weight::from_log_weight(*w, params))
        .collect()
}

/// Dequantize a batch of 8-bit weights to log weights.
pub fn dequantize_8bit_to_log(
    weights: &[Quantized8Weight],
    params: &QuantizationParams,
) -> Vec<LogWeight> {
    weights.iter().map(|w| w.to_log_weight(params)).collect()
}

/// Quantize a batch of log weights to 4-bit.
pub fn quantize_log_weights_4bit(
    weights: &[LogWeight],
    params: &QuantizationParams,
) -> Vec<Quantized4Weight> {
    weights
        .iter()
        .map(|w| Quantized4Weight::from_log_weight(*w, params))
        .collect()
}

/// Quantize and pack log weights to 4-bit packed format.
pub fn quantize_and_pack_4bit(
    weights: &[LogWeight],
    params: &QuantizationParams,
) -> PackedWeights4 {
    let quantized: Vec<_> = weights
        .iter()
        .map(|w| Quantized4Weight::from_log_weight(*w, params))
        .collect();
    PackedWeights4::from_weights(&quantized)
}

/// Compute quantization statistics for a batch of weights.
pub fn compute_quantization_stats_8bit(
    weights: &[LogWeight],
    params: &QuantizationParams,
) -> QuantizationStats {
    let mut stats = QuantizationStats::new();
    for w in weights {
        let original = w.value();
        let quantized = Quantized8Weight::from_log_weight(*w, params);
        let dequantized = quantized.to_log_weight(params).value();
        stats.add(original, dequantized);
    }
    stats
}

/// Compute quantization statistics for 4-bit quantization.
pub fn compute_quantization_stats_4bit(
    weights: &[LogWeight],
    params: &QuantizationParams,
) -> QuantizationStats {
    let mut stats = QuantizationStats::new();
    for w in weights {
        let original = w.value();
        let quantized = Quantized4Weight::from_log_weight(*w, params);
        let dequantized = quantized.to_log_weight(params).value();
        stats.add(original, dequantized);
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quantization_params() {
        let params = QuantizationParams::new(-10.0, 10.0);

        // Test boundary values
        assert_eq!(params.quantize_8bit(-10.0), 0);
        assert_eq!(params.quantize_8bit(10.0), 255);

        // Test midpoint
        let mid = params.quantize_8bit(0.0);
        assert!((mid as i32 - 127).abs() <= 1); // Should be around 127-128
    }

    #[test]
    fn test_roundtrip_8bit() {
        let params = QuantizationParams::for_log_weights();

        let original = LogWeight::new(-5.0);
        let quantized = Quantized8Weight::from_log_weight(original, &params);
        let recovered = quantized.to_log_weight(&params);

        // Should be within quantization error (~0.08 for 256 levels over range of 20)
        assert!((recovered.value() - original.value()).abs() < 0.1);
    }

    #[test]
    fn test_roundtrip_4bit() {
        let params = QuantizationParams::for_log_weights();

        let original = LogWeight::new(-10.0);
        let quantized = Quantized4Weight::from_log_weight(original, &params);
        let recovered = quantized.to_log_weight(&params);

        // 4-bit has larger error (~1.33 for 16 levels over range of 20)
        assert!((recovered.value() - original.value()).abs() < 2.0);
    }

    #[test]
    fn test_packed_weights() {
        let params = QuantizationParams::for_log_weights();
        let weights: Vec<LogWeight> = (0..10).map(|i| LogWeight::new(-i as f64 * 2.0)).collect();

        let packed = quantize_and_pack_4bit(&weights, &params);

        assert_eq!(packed.len(), 10);
        assert_eq!(packed.memory_size(), 5); // 10 weights in 5 bytes

        // Verify roundtrip
        for (i, w) in weights.iter().enumerate() {
            let packed_w = packed.get(i).expect("valid index");
            let recovered = packed_w.to_log_weight(&params);
            assert!((recovered.value() - w.value()).abs() < 2.0);
        }
    }

    #[test]
    fn test_quantization_stats() {
        let params = QuantizationParams::for_log_weights();
        let weights: Vec<LogWeight> = (0..100).map(|i| LogWeight::new(-i as f64 * 0.2)).collect();

        let stats = compute_quantization_stats_8bit(&weights, &params);

        assert_eq!(stats.count, 100);
        assert!(stats.mean_error() < 0.1);
        assert!(stats.rmse() < 0.1);
    }

    #[test]
    fn test_infinity_handling() {
        let params = QuantizationParams::for_log_weights();

        let inf = LogWeight::new(f64::INFINITY);
        let neg_inf = LogWeight::new(f64::NEG_INFINITY);

        let q_inf = Quantized8Weight::from_log_weight(inf, &params);
        let q_neg_inf = Quantized8Weight::from_log_weight(neg_inf, &params);

        assert_eq!(q_inf.raw(), 255);
        assert_eq!(q_neg_inf.raw(), 0);
    }

    #[test]
    fn test_tropical_quantization() {
        let params = QuantizationParams::for_tropical_weights();

        let original = TropicalWeight::new(5.0);
        let quantized = Quantized8Weight::from_tropical_weight(original, &params);
        let recovered = quantized.to_tropical_weight(&params);

        assert!((recovered.value() - original.value()).abs() < 0.1);
    }

    #[test]
    fn test_params_from_data() {
        let values = vec![1.0, 5.0, 3.0, 7.0, 2.0];
        let params = QuantizationParams::from_data(values.into_iter()).expect("valid data");

        // Should include some margin
        assert!(params.min_val < 1.0);
        assert!(params.max_val > 7.0);
    }

    #[test]
    fn test_batch_quantize() {
        let params = QuantizationParams::for_log_weights();
        let weights: Vec<LogWeight> = vec![
            LogWeight::new(-1.0),
            LogWeight::new(-5.0),
            LogWeight::new(-10.0),
        ];

        let quantized = quantize_log_weights_8bit(&weights, &params);
        let dequantized = dequantize_8bit_to_log(&quantized, &params);

        assert_eq!(dequantized.len(), 3);
        for (orig, deq) in weights.iter().zip(dequantized.iter()) {
            assert!((orig.value() - deq.value()).abs() < 0.1);
        }
    }

    #[test]
    fn test_packed_weights_set() {
        let mut packed = PackedWeights4::with_capacity(4);
        packed.push(Quantized4Weight::from_raw(3));
        packed.push(Quantized4Weight::from_raw(7));
        packed.push(Quantized4Weight::from_raw(11));
        packed.push(Quantized4Weight::from_raw(15));

        assert_eq!(packed.get(0).expect("valid").raw(), 3);
        assert_eq!(packed.get(1).expect("valid").raw(), 7);

        packed.set(1, Quantized4Weight::from_raw(9));
        assert_eq!(packed.get(1).expect("valid").raw(), 9);

        // Verify other values unchanged
        assert_eq!(packed.get(0).expect("valid").raw(), 3);
        assert_eq!(packed.get(2).expect("valid").raw(), 11);
    }
}
