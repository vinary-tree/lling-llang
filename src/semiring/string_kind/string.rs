//! String semiring for label accumulation in WFSTs.
//!
//! The string semiring operates on strings with:
//! - **⊕ = lcp/lcs**: Longest common prefix (left) or suffix (right)
//! - **⊗ = ·**: String concatenation
//! - **0̄ = ∞**: Infinite string (identity for lcp/lcs)
//! - **1̄ = ε**: Empty string (identity for concatenation)
//!
//! # Variants
//!
//! - [`LeftStringWeight`]: Uses longest common prefix (left-distributive)
//! - [`RightStringWeight`]: Uses longest common suffix (right-distributive)
//!
//! # Not a True Semiring
//!
//! String semirings are only **weakly left/right distributive**, not fully
//! distributive. For example, with LeftStringWeight:
//!
//! - Left-distributive: `a ⊗ (b ⊕ c) = (a ⊗ b) ⊕ (a ⊗ c)` ✓
//! - Right-distributive: `(a ⊕ b) ⊗ c ≠ (a ⊗ c) ⊕ (b ⊗ c)` ✗
//!
//! This is acceptable for many WFST algorithms that only require one-sided
//! distributivity (e.g., determinization uses left-distributivity).
//!
//! # Why Not `Semiring` Trait?
//!
//! String weights contain `Vec<u8>` which cannot be `Copy`. The `Semiring` trait
//! requires `Copy` for efficiency in numeric weights. String weights provide the
//! same API through inherent methods instead of trait implementations.
//!
//! # Use Cases
//!
//! - Computing common label prefixes/suffixes among paths
//! - Label disambiguation in determinization
//! - Output label accumulation in composition
//!
//! # Example
//!
//! ```
//! use lling_llang::semiring::LeftStringWeight;
//!
//! let abc = LeftStringWeight::from_str("abc");
//! let abx = LeftStringWeight::from_str("abx");
//!
//! // Longest common prefix: "ab"
//! let lcp = abc.plus(&abx);
//! assert_eq!(lcp.as_str(), Some("ab"));
//!
//! // Concatenation: "abcabx"
//! let concat = abc.times(&abx);
//! assert_eq!(concat.as_str(), Some("abcabx"));
//! ```

/// Left string weight using longest common prefix.
///
/// Uses `Option<Vec<u8>>` where:
/// - `None` = ∞ (infinite string, additive identity)
/// - `Some(vec![])` = ε (empty string, multiplicative identity)
///
/// This is left-distributive: `a ⊗ (b ⊕ c) = (a ⊗ b) ⊕ (a ⊗ c)`
///
/// # Note
///
/// This type does not implement the [`Semiring`](super::Semiring) trait because
/// it contains `Vec<u8>` which cannot be `Copy`. It provides semiring-like
/// operations through inherent methods instead.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LeftStringWeight(Option<Vec<u8>>);

impl LeftStringWeight {
    /// Create a new string weight from bytes.
    #[inline]
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        LeftStringWeight(Some(bytes.into()))
    }

    /// Create from a string slice.
    #[inline]
    pub fn from_str(s: &str) -> Self {
        LeftStringWeight(Some(s.as_bytes().to_vec()))
    }

    /// Create the infinite string (additive identity, zero).
    #[inline]
    pub fn infinity() -> Self {
        LeftStringWeight(None)
    }

    /// Additive identity: infinite string.
    #[inline]
    pub fn zero() -> Self {
        Self::infinity()
    }

    /// Create the empty string (multiplicative identity, one).
    #[inline]
    pub fn epsilon() -> Self {
        LeftStringWeight(Some(Vec::new()))
    }

    /// Multiplicative identity: empty string.
    #[inline]
    pub fn one() -> Self {
        Self::epsilon()
    }

    /// Check if this is the infinite string (additive identity).
    #[inline]
    pub fn is_infinite(&self) -> bool {
        self.0.is_none()
    }

    /// Check if this is the zero element (infinite string).
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.is_infinite()
    }

    /// Check if this is the empty string (multiplicative identity).
    #[inline]
    pub fn is_empty(&self) -> bool {
        matches!(&self.0, Some(v) if v.is_empty())
    }

    /// Check if this is the one element (empty string).
    #[inline]
    pub fn is_one(&self) -> bool {
        self.is_empty()
    }

    /// Get the bytes if not infinite.
    #[inline]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        self.0.as_deref()
    }

    /// Consume the weight and return its bytes if not infinite.
    #[inline]
    pub fn into_bytes(self) -> Option<Vec<u8>> {
        self.0
    }

    /// Get as a UTF-8 string if valid.
    pub fn as_str(&self) -> Option<&str> {
        self.0
            .as_ref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
    }

    /// Get the length of the string (None for infinity).
    #[inline]
    pub fn len(&self) -> Option<usize> {
        self.0.as_ref().map(|v| v.len())
    }

    /// Compute the length of the longest common prefix of two byte slices.
    #[inline]
    pub fn longest_common_prefix_len(a: &[u8], b: &[u8]) -> usize {
        a.iter()
            .zip(b)
            .position(|(x, y)| x != y)
            .unwrap_or_else(|| a.len().min(b.len()))
    }

    /// Compute the longest common prefix of two byte slices.
    pub fn longest_common_prefix(a: &[u8], b: &[u8]) -> Vec<u8> {
        let end = Self::longest_common_prefix_len(a, b);
        a[..end].to_vec()
    }

    /// Addition: longest common prefix.
    pub fn plus(&self, other: &Self) -> Self {
        match (&self.0, &other.0) {
            (None, _) => other.clone(),
            (_, None) => self.clone(),
            (Some(a), Some(b)) if a == b || b.starts_with(a) => self.clone(),
            (Some(a), Some(b)) if a.starts_with(b) => other.clone(),
            (Some(a), Some(b)) => LeftStringWeight(Some(Self::longest_common_prefix(a, b))),
        }
    }

    /// Multiplication: concatenation.
    pub fn times(&self, other: &Self) -> Self {
        match (&self.0, &other.0) {
            (None, _) | (_, None) => LeftStringWeight::infinity(),
            (Some(a), Some(_)) if a.is_empty() => other.clone(),
            (Some(_), Some(b)) if b.is_empty() => self.clone(),
            (Some(a), Some(b)) => {
                let mut result = Vec::with_capacity(a.len() + b.len());
                result.extend_from_slice(a);
                result.extend_from_slice(b);
                LeftStringWeight(Some(result))
            }
        }
    }

    /// Kleene closure for string semiring.
    ///
    /// For string semirings, star always converges to epsilon because:
    /// - ε* = ε (trivially)
    /// - For non-empty s: lcp(ε, s, ss, sss, ...) = ε
    pub fn star(&self) -> Self {
        // star(s) = ε for all s
        Self::one()
    }

    /// Check approximate equality (exact for strings).
    pub fn approx_eq(&self, other: &Self, _epsilon: f64) -> bool {
        self == other
    }

    /// Natural ordering: prefixes are "better" because they are selected by
    /// longest-common-prefix addition.
    pub fn natural_less(&self, other: &Self) -> Option<bool> {
        match (&self.0, &other.0) {
            (None, None) => Some(false),
            (None, Some(_)) => Some(false), // infinity is "worse"
            (Some(_), None) => Some(true),  // finite is "better" than infinity
            (Some(a), Some(b)) if a == b => Some(false),
            (Some(a), Some(b)) if b.starts_with(a) => Some(true),
            (Some(a), Some(b)) if a.starts_with(b) => Some(false),
            (Some(_), Some(_)) => None,
        }
    }

    /// Convert to bytes for serialization.
    pub fn to_bytes(&self) -> Vec<u8> {
        match &self.0 {
            None => vec![0xFF], // Special marker for infinity
            Some(bytes) => {
                let mut result = Vec::with_capacity(1 + bytes.len());
                result.push(0x00); // Marker for finite string
                result.extend(bytes);
                result
            }
        }
    }
}

impl Default for LeftStringWeight {
    /// Default is epsilon (empty string, multiplicative identity).
    #[inline]
    fn default() -> Self {
        Self::one()
    }
}

impl From<&str> for LeftStringWeight {
    fn from(s: &str) -> Self {
        LeftStringWeight::from_str(s)
    }
}

impl From<String> for LeftStringWeight {
    fn from(s: String) -> Self {
        LeftStringWeight(Some(s.into_bytes()))
    }
}

impl From<Vec<u8>> for LeftStringWeight {
    fn from(bytes: Vec<u8>) -> Self {
        LeftStringWeight(Some(bytes))
    }
}

impl std::ops::Add for LeftStringWeight {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl std::ops::Add<&LeftStringWeight> for LeftStringWeight {
    type Output = Self;

    fn add(self, other: &Self) -> Self {
        self.plus(other)
    }
}

impl std::ops::Mul for LeftStringWeight {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::Mul<&LeftStringWeight> for LeftStringWeight {
    type Output = Self;

    fn mul(self, other: &Self) -> Self {
        self.times(other)
    }
}

/// Right string weight using longest common suffix.
///
/// Uses `Option<Vec<u8>>` where:
/// - `None` = ∞ (infinite string, additive identity)
/// - `Some(vec![])` = ε (empty string, multiplicative identity)
///
/// This is right-distributive: `(a ⊕ b) ⊗ c = (a ⊗ c) ⊕ (b ⊗ c)`
///
/// # Note
///
/// This type does not implement the [`Semiring`](super::Semiring) trait because
/// it contains `Vec<u8>` which cannot be `Copy`. It provides semiring-like
/// operations through inherent methods instead.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RightStringWeight(Option<Vec<u8>>);

impl RightStringWeight {
    /// Create a new string weight from bytes.
    #[inline]
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        RightStringWeight(Some(bytes.into()))
    }

    /// Create from a string slice.
    #[inline]
    pub fn from_str(s: &str) -> Self {
        RightStringWeight(Some(s.as_bytes().to_vec()))
    }

    /// Create the infinite string (additive identity, zero).
    #[inline]
    pub fn infinity() -> Self {
        RightStringWeight(None)
    }

    /// Additive identity: infinite string.
    #[inline]
    pub fn zero() -> Self {
        Self::infinity()
    }

    /// Create the empty string (multiplicative identity, one).
    #[inline]
    pub fn epsilon() -> Self {
        RightStringWeight(Some(Vec::new()))
    }

    /// Multiplicative identity: empty string.
    #[inline]
    pub fn one() -> Self {
        Self::epsilon()
    }

    /// Check if this is the infinite string (additive identity).
    #[inline]
    pub fn is_infinite(&self) -> bool {
        self.0.is_none()
    }

    /// Check if this is the zero element (infinite string).
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.is_infinite()
    }

    /// Check if this is the empty string (multiplicative identity).
    #[inline]
    pub fn is_empty(&self) -> bool {
        matches!(&self.0, Some(v) if v.is_empty())
    }

    /// Check if this is the one element (empty string).
    #[inline]
    pub fn is_one(&self) -> bool {
        self.is_empty()
    }

    /// Get the bytes if not infinite.
    #[inline]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        self.0.as_deref()
    }

    /// Consume the weight and return its bytes if not infinite.
    #[inline]
    pub fn into_bytes(self) -> Option<Vec<u8>> {
        self.0
    }

    /// Get as a UTF-8 string if valid.
    pub fn as_str(&self) -> Option<&str> {
        self.0
            .as_ref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
    }

    /// Get the length of the string (None for infinity).
    #[inline]
    pub fn len(&self) -> Option<usize> {
        self.0.as_ref().map(|v| v.len())
    }

    /// Compute the length of the longest common suffix of two byte slices.
    #[inline]
    pub fn longest_common_suffix_len(a: &[u8], b: &[u8]) -> usize {
        a.iter()
            .rev()
            .zip(b.iter().rev())
            .position(|(x, y)| x != y)
            .unwrap_or_else(|| a.len().min(b.len()))
    }

    /// Compute the longest common suffix of two byte slices.
    pub fn longest_common_suffix(a: &[u8], b: &[u8]) -> Vec<u8> {
        let len = Self::longest_common_suffix_len(a, b);
        a[a.len() - len..].to_vec()
    }

    /// Addition: longest common suffix.
    pub fn plus(&self, other: &Self) -> Self {
        match (&self.0, &other.0) {
            (None, _) => other.clone(),
            (_, None) => self.clone(),
            (Some(a), Some(b)) if a == b || b.ends_with(a) => self.clone(),
            (Some(a), Some(b)) if a.ends_with(b) => other.clone(),
            (Some(a), Some(b)) => RightStringWeight(Some(Self::longest_common_suffix(a, b))),
        }
    }

    /// Multiplication: concatenation.
    pub fn times(&self, other: &Self) -> Self {
        match (&self.0, &other.0) {
            (None, _) | (_, None) => RightStringWeight::infinity(),
            (Some(a), Some(_)) if a.is_empty() => other.clone(),
            (Some(_), Some(b)) if b.is_empty() => self.clone(),
            (Some(a), Some(b)) => {
                let mut result = Vec::with_capacity(a.len() + b.len());
                result.extend_from_slice(a);
                result.extend_from_slice(b);
                RightStringWeight(Some(result))
            }
        }
    }

    /// Kleene closure converges to epsilon for all strings.
    pub fn star(&self) -> Self {
        Self::one()
    }

    /// Check approximate equality (exact for strings).
    pub fn approx_eq(&self, other: &Self, _epsilon: f64) -> bool {
        self == other
    }

    /// Natural ordering: suffixes are "better" because they are selected by
    /// longest-common-suffix addition.
    pub fn natural_less(&self, other: &Self) -> Option<bool> {
        match (&self.0, &other.0) {
            (None, None) => Some(false),
            (None, Some(_)) => Some(false),
            (Some(_), None) => Some(true),
            (Some(a), Some(b)) if a == b => Some(false),
            (Some(a), Some(b)) if b.ends_with(a) => Some(true),
            (Some(a), Some(b)) if a.ends_with(b) => Some(false),
            (Some(_), Some(_)) => None,
        }
    }

    /// Convert to bytes for serialization.
    pub fn to_bytes(&self) -> Vec<u8> {
        match &self.0 {
            None => vec![0xFF],
            Some(bytes) => {
                let mut result = Vec::with_capacity(1 + bytes.len());
                result.push(0x00);
                result.extend(bytes);
                result
            }
        }
    }
}

impl Default for RightStringWeight {
    /// Default is epsilon (empty string, multiplicative identity).
    #[inline]
    fn default() -> Self {
        Self::one()
    }
}

impl From<&str> for RightStringWeight {
    fn from(s: &str) -> Self {
        RightStringWeight::from_str(s)
    }
}

impl From<String> for RightStringWeight {
    fn from(s: String) -> Self {
        RightStringWeight(Some(s.into_bytes()))
    }
}

impl From<Vec<u8>> for RightStringWeight {
    fn from(bytes: Vec<u8>) -> Self {
        RightStringWeight(Some(bytes))
    }
}

impl std::ops::Add for RightStringWeight {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        self.plus(&other)
    }
}

impl std::ops::Add<&RightStringWeight> for RightStringWeight {
    type Output = Self;

    fn add(self, other: &Self) -> Self {
        self.plus(other)
    }
}

impl std::ops::Mul for RightStringWeight {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        self.times(&other)
    }
}

impl std::ops::Mul<&RightStringWeight> for RightStringWeight {
    type Output = Self;

    fn mul(self, other: &Self) -> Self {
        self.times(other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== LeftStringWeight Tests ==========

    #[test]
    fn test_left_basic_operations() {
        let abc = LeftStringWeight::from_str("abc");
        let abx = LeftStringWeight::from_str("abx");
        let def = LeftStringWeight::from_str("def");

        // Longest common prefix
        let lcp = abc.plus(&abx);
        assert_eq!(lcp.as_str(), Some("ab"));

        // No common prefix
        let lcp2 = abc.plus(&def);
        assert_eq!(lcp2.as_str(), Some(""));

        // Concatenation
        let concat = abc.times(&def);
        assert_eq!(concat.as_str(), Some("abcdef"));
    }

    #[test]
    fn test_left_identities() {
        let abc = LeftStringWeight::from_str("abc");

        // Zero is additive identity
        let sum = abc.plus(&LeftStringWeight::zero());
        assert_eq!(sum, abc);

        let sum2 = LeftStringWeight::zero().plus(&abc);
        assert_eq!(sum2, abc);

        // One is multiplicative identity
        let prod = abc.times(&LeftStringWeight::one());
        assert_eq!(prod, abc);

        let prod2 = LeftStringWeight::one().times(&abc);
        assert_eq!(prod2, abc);
    }

    #[test]
    fn test_left_annihilation() {
        let abc = LeftStringWeight::from_str("abc");

        // Zero annihilates
        let prod = abc.times(&LeftStringWeight::zero());
        assert!(prod.is_zero());

        let prod2 = LeftStringWeight::zero().times(&abc);
        assert!(prod2.is_zero());
    }

    #[test]
    fn test_left_commutativity() {
        let abc = LeftStringWeight::from_str("abc");
        let abx = LeftStringWeight::from_str("abx");

        // Addition is commutative
        assert_eq!(abc.plus(&abx), abx.plus(&abc));
    }

    #[test]
    fn test_left_associativity() {
        let ab = LeftStringWeight::from_str("ab");
        let abc = LeftStringWeight::from_str("abc");
        let abcd = LeftStringWeight::from_str("abcd");

        // Additive associativity
        let left = ab.plus(&abc).plus(&abcd);
        let right = ab.plus(&abc.plus(&abcd));
        assert_eq!(left, right);

        // Multiplicative associativity
        let a = LeftStringWeight::from_str("a");
        let b = LeftStringWeight::from_str("b");
        let c = LeftStringWeight::from_str("c");
        let left = a.times(&b).times(&c);
        let right = a.times(&b.times(&c));
        assert_eq!(left, right);
        assert_eq!(left.as_str(), Some("abc"));
    }

    #[test]
    fn test_left_distributivity() {
        let a = LeftStringWeight::from_str("x");
        let b = LeftStringWeight::from_str("ab");
        let c = LeftStringWeight::from_str("ac");

        // Left distributivity: a * (b + c) = (a * b) + (a * c)
        let left = a.times(&b.plus(&c));
        let right = a.times(&b).plus(&a.times(&c));
        assert_eq!(left, right);
        // b + c = lcp("ab", "ac") = "a"
        // a * (b + c) = "x" * "a" = "xa"
        // a * b = "xab", a * c = "xac"
        // (a * b) + (a * c) = lcp("xab", "xac") = "xa"
    }

    #[test]
    fn test_left_star() {
        let abc = LeftStringWeight::from_str("abc");
        let eps = LeftStringWeight::epsilon();

        // star(ε) = ε
        assert_eq!(eps.star(), LeftStringWeight::one());

        // star(s) = ε for non-empty s
        assert_eq!(abc.star(), LeftStringWeight::one());
    }

    #[test]
    fn test_left_natural_order_uses_prefix_order() {
        let ab = LeftStringWeight::from_str("ab");
        let abc = LeftStringWeight::from_str("abc");
        let ac = LeftStringWeight::from_str("ac");
        let empty = LeftStringWeight::epsilon();
        let infinity = LeftStringWeight::zero();

        assert_eq!(ab.natural_less(&abc), Some(true));
        assert_eq!(abc.natural_less(&ab), Some(false));
        assert_eq!(ab.natural_less(&ab), Some(false));
        assert_eq!(ab.natural_less(&ac), None);
        assert_eq!(empty.natural_less(&abc), Some(true));
        assert_eq!(abc.natural_less(&infinity), Some(true));
        assert_eq!(infinity.natural_less(&abc), Some(false));
    }

    #[test]
    fn test_left_natural_order_matches_plus_selection() {
        let ab = LeftStringWeight::from_str("ab");
        let abc = LeftStringWeight::from_str("abc");
        let ac = LeftStringWeight::from_str("ac");

        assert_eq!(ab.plus(&abc), ab);
        assert_eq!(ab.natural_less(&abc), Some(true));
        assert_eq!(abc.natural_less(&ab), Some(false));

        let common = ab.plus(&ac);
        assert_ne!(common, ab);
        assert_ne!(common, ac);
        assert_eq!(ab.natural_less(&ac), None);
    }

    // ========== RightStringWeight Tests ==========

    #[test]
    fn test_right_basic_operations() {
        let abc = RightStringWeight::from_str("abc");
        let xbc = RightStringWeight::from_str("xbc");
        let def = RightStringWeight::from_str("def");

        // Longest common suffix
        let lcs = abc.plus(&xbc);
        assert_eq!(lcs.as_str(), Some("bc"));

        // No common suffix
        let lcs2 = abc.plus(&def);
        assert_eq!(lcs2.as_str(), Some(""));

        // Concatenation
        let concat = abc.times(&def);
        assert_eq!(concat.as_str(), Some("abcdef"));
    }

    #[test]
    fn test_right_identities() {
        let abc = RightStringWeight::from_str("abc");

        // Zero is additive identity
        let sum = abc.plus(&RightStringWeight::zero());
        assert_eq!(sum, abc);

        // One is multiplicative identity
        let prod = abc.times(&RightStringWeight::one());
        assert_eq!(prod, abc);
    }

    #[test]
    fn test_right_distributivity() {
        let a = RightStringWeight::from_str("ab");
        let b = RightStringWeight::from_str("cb");
        let c = RightStringWeight::from_str("x");

        // Right distributivity: (a + b) * c = (a * c) + (b * c)
        let left = a.plus(&b).times(&c);
        let right = a.times(&c).plus(&b.times(&c));
        assert_eq!(left, right);
        // a + b = lcs("ab", "cb") = "b"
        // (a + b) * c = "b" * "x" = "bx"
        // a * c = "abx", b * c = "cbx"
        // (a * c) + (b * c) = lcs("abx", "cbx") = "bx"
    }

    #[test]
    fn test_right_star() {
        let abc = RightStringWeight::from_str("abc");

        // star(s) = ε for any s
        assert_eq!(abc.star(), RightStringWeight::one());
    }

    #[test]
    fn test_right_natural_order_uses_suffix_order() {
        let bc = RightStringWeight::from_str("bc");
        let abc = RightStringWeight::from_str("abc");
        let xbc = RightStringWeight::from_str("xbc");
        let bd = RightStringWeight::from_str("bd");
        let empty = RightStringWeight::epsilon();
        let infinity = RightStringWeight::zero();

        assert_eq!(bc.natural_less(&abc), Some(true));
        assert_eq!(abc.natural_less(&bc), Some(false));
        assert_eq!(bc.natural_less(&bc), Some(false));
        assert_eq!(bc.natural_less(&bd), None);
        assert_eq!(empty.natural_less(&xbc), Some(true));
        assert_eq!(abc.natural_less(&infinity), Some(true));
        assert_eq!(infinity.natural_less(&abc), Some(false));
    }

    #[test]
    fn test_right_natural_order_matches_plus_selection() {
        let bc = RightStringWeight::from_str("bc");
        let abc = RightStringWeight::from_str("abc");
        let xbc = RightStringWeight::from_str("xbc");
        let bd = RightStringWeight::from_str("bd");

        assert_eq!(bc.plus(&abc), bc);
        assert_eq!(bc.natural_less(&abc), Some(true));
        assert_eq!(abc.natural_less(&bc), Some(false));

        assert_eq!(abc.plus(&xbc), bc);
        assert_eq!(bc.natural_less(&abc), Some(true));
        assert_eq!(bc.natural_less(&xbc), Some(true));
        assert_eq!(bc.natural_less(&bd), None);
    }

    // ========== Shared Property Tests ==========

    #[test]
    fn test_empty_string_lcp() {
        let empty = LeftStringWeight::epsilon();
        let abc = LeftStringWeight::from_str("abc");

        // lcp(ε, s) = ε
        assert_eq!(empty.plus(&abc), LeftStringWeight::epsilon());
    }

    #[test]
    fn test_empty_string_lcs() {
        let empty = RightStringWeight::epsilon();
        let abc = RightStringWeight::from_str("abc");

        // lcs(ε, s) = ε
        assert_eq!(empty.plus(&abc), RightStringWeight::epsilon());
    }

    #[test]
    fn test_bytes_conversion() {
        let bytes = vec![0x48, 0x65, 0x6c, 0x6c, 0x6f]; // "Hello"
        let left = LeftStringWeight::new(bytes.clone());
        let right = RightStringWeight::new(bytes.clone());

        assert_eq!(left.as_bytes(), Some(bytes.as_slice()));
        assert_eq!(left.as_str(), Some("Hello"));

        assert_eq!(right.as_bytes(), Some(bytes.as_slice()));
        assert_eq!(right.as_str(), Some("Hello"));
    }

    #[test]
    fn test_non_utf8_bytes() {
        let bytes = vec![0xFF, 0xFE]; // Invalid UTF-8
        let left = LeftStringWeight::new(bytes.clone());

        assert_eq!(left.as_bytes(), Some(bytes.as_slice()));
        assert_eq!(left.as_str(), None); // Not valid UTF-8
    }

    #[test]
    fn test_owned_byte_extraction() {
        let bytes = vec![0xF0, 0x9F, 0xA6, 0x80];
        let left = LeftStringWeight::new(bytes.clone());
        let right = RightStringWeight::new(bytes.clone());

        assert_eq!(left.into_bytes(), Some(bytes.clone()));
        assert_eq!(right.into_bytes(), Some(bytes));
        assert_eq!(LeftStringWeight::zero().into_bytes(), None);
        assert_eq!(RightStringWeight::zero().into_bytes(), None);
    }

    #[test]
    fn test_common_prefix_suffix_lengths() {
        assert_eq!(
            LeftStringWeight::longest_common_prefix_len(b"alphabet", b"alpine"),
            3
        );
        assert_eq!(
            LeftStringWeight::longest_common_prefix_len(b"same", b"same"),
            4
        );
        assert_eq!(
            RightStringWeight::longest_common_suffix_len(b"testing", b"nesting"),
            6
        );
        assert_eq!(
            RightStringWeight::longest_common_suffix_len(b"same", b"same"),
            4
        );
    }

    #[test]
    fn test_longest_common_prefix_function() {
        assert_eq!(
            LeftStringWeight::longest_common_prefix(b"abc", b"abx"),
            b"ab".to_vec()
        );
        assert_eq!(
            LeftStringWeight::longest_common_prefix(b"abc", b"abc"),
            b"abc".to_vec()
        );
        assert_eq!(
            LeftStringWeight::longest_common_prefix(b"abc", b"xyz"),
            b"".to_vec()
        );
        assert_eq!(
            LeftStringWeight::longest_common_prefix(b"", b"abc"),
            b"".to_vec()
        );
    }

    #[test]
    fn test_longest_common_suffix_function() {
        assert_eq!(
            RightStringWeight::longest_common_suffix(b"abc", b"xbc"),
            b"bc".to_vec()
        );
        assert_eq!(
            RightStringWeight::longest_common_suffix(b"abc", b"abc"),
            b"abc".to_vec()
        );
        assert_eq!(
            RightStringWeight::longest_common_suffix(b"abc", b"xyz"),
            b"".to_vec()
        );
        assert_eq!(
            RightStringWeight::longest_common_suffix(b"", b"abc"),
            b"".to_vec()
        );
    }

    #[test]
    fn test_operator_overloading() {
        let abc = LeftStringWeight::from_str("abc");
        let abx = LeftStringWeight::from_str("abx");
        let def = LeftStringWeight::from_str("def");

        // Addition via +
        let lcp = abc.clone() + abx.clone();
        assert_eq!(lcp.as_str(), Some("ab"));

        // Multiplication via *
        let concat = abc.clone() * def.clone();
        assert_eq!(concat.as_str(), Some("abcdef"));

        // With references
        let lcp_ref = abc.clone() + &abx;
        assert_eq!(lcp_ref.as_str(), Some("ab"));

        let concat_ref = abc.clone() * &def;
        assert_eq!(concat_ref.as_str(), Some("abcdef"));
    }
}
