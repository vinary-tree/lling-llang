//! HashMap-based lattice backend.
//!
//! This module provides a simple, dependency-free backend using Rust's
//! standard hash map for vocabulary interning.

use std::sync::Arc;
use rustc_hash::FxHashMap;

use super::traits::{LatticeBackend, VocabId};

/// A simple HashMap-based backend for vocabulary storage.
///
/// This backend uses an FxHashMap for fast string-to-id lookups and
/// a Vec for id-to-string lookups. It provides O(1) operations for
/// both directions.
///
/// # Thread Safety
///
/// This type implements `Send + Sync` via `Arc<str>` for string storage.
/// For mutable access from multiple threads, wrap in a `Mutex` or use
/// a concurrent data structure.
///
/// # Example
///
/// ```rust
/// use lling_llang::backend::{LatticeBackend, HashMapBackend};
///
/// let mut backend = HashMapBackend::new();
///
/// let id = backend.intern("hello");
/// assert_eq!(backend.lookup(id), Some("hello"));
/// assert_eq!(backend.vocab_size(), 1);
/// ```
#[derive(Clone, Debug)]
pub struct HashMapBackend {
    /// Map from word to vocabulary ID.
    word_to_id: FxHashMap<Arc<str>, VocabId>,
    /// Map from vocabulary ID to word.
    id_to_word: Vec<Arc<str>>,
}

impl HashMapBackend {
    /// Create a new empty backend.
    #[inline]
    pub fn new() -> Self {
        Self {
            word_to_id: FxHashMap::default(),
            id_to_word: Vec::new(),
        }
    }

    /// Create a new backend with pre-allocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            word_to_id: FxHashMap::with_capacity_and_hasher(capacity, Default::default()),
            id_to_word: Vec::with_capacity(capacity),
        }
    }

    /// Reserve capacity for additional vocabulary entries.
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.word_to_id.reserve(additional);
        self.id_to_word.reserve(additional);
    }

    /// Shrink internal storage to fit current size.
    #[inline]
    pub fn shrink_to_fit(&mut self) {
        self.word_to_id.shrink_to_fit();
        self.id_to_word.shrink_to_fit();
    }

    /// Clear all vocabulary entries.
    #[inline]
    pub fn clear(&mut self) {
        self.word_to_id.clear();
        self.id_to_word.clear();
    }
}

impl Default for HashMapBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl LatticeBackend for HashMapBackend {
    #[inline]
    fn intern(&mut self, word: &str) -> VocabId {
        if let Some(&id) = self.word_to_id.get(word) {
            return id;
        }

        let id = self.id_to_word.len() as VocabId;
        let word_arc: Arc<str> = word.into();
        self.word_to_id.insert(word_arc.clone(), id);
        self.id_to_word.push(word_arc);
        id
    }

    #[inline]
    fn lookup(&self, id: VocabId) -> Option<&str> {
        self.id_to_word.get(id as usize).map(|s| s.as_ref())
    }

    #[inline]
    fn supports_sharing(&self) -> bool {
        false
    }

    #[inline]
    fn vocab_size(&self) -> usize {
        self.id_to_word.len()
    }

    #[inline]
    fn contains(&self, word: &str) -> bool {
        self.word_to_id.contains_key(word)
    }

    #[inline]
    fn get_id(&self, word: &str) -> Option<VocabId> {
        self.word_to_id.get(word).copied()
    }

    fn iter(&self) -> impl Iterator<Item = (VocabId, &str)> {
        self.id_to_word
            .iter()
            .enumerate()
            .map(|(id, word)| (id as VocabId, word.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_empty() {
        let backend = HashMapBackend::new();
        assert_eq!(backend.vocab_size(), 0);
    }

    #[test]
    fn test_with_capacity() {
        let backend = HashMapBackend::with_capacity(100);
        assert_eq!(backend.vocab_size(), 0);
    }

    #[test]
    fn test_intern_multiple() {
        let mut backend = HashMapBackend::new();

        let words = ["the", "quick", "brown", "fox", "jumps", "over", "the", "lazy", "dog"];
        let mut ids = Vec::new();

        for word in &words {
            ids.push(backend.intern(word));
        }

        // "the" appears twice, should have same ID
        assert_eq!(ids[0], ids[6]);

        // All unique words should have different IDs
        let unique_ids: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique_ids.len(), 8); // 9 words - 1 duplicate = 8 unique
    }

    #[test]
    fn test_reserve() {
        let mut backend = HashMapBackend::new();
        backend.reserve(100);
        // Should not panic or affect vocab size
        assert_eq!(backend.vocab_size(), 0);
    }

    #[test]
    fn test_clear() {
        let mut backend = HashMapBackend::new();
        backend.intern("hello");
        backend.intern("world");
        assert_eq!(backend.vocab_size(), 2);

        backend.clear();
        assert_eq!(backend.vocab_size(), 0);
        assert!(!backend.contains("hello"));
    }

    #[test]
    fn test_default() {
        let backend = HashMapBackend::default();
        assert_eq!(backend.vocab_size(), 0);
    }

    #[test]
    fn test_supports_sharing() {
        let backend = HashMapBackend::new();
        assert!(!backend.supports_sharing());
    }

    #[test]
    fn test_sequential_ids() {
        let mut backend = HashMapBackend::new();

        let id0 = backend.intern("zero");
        let id1 = backend.intern("one");
        let id2 = backend.intern("two");

        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn test_unicode() {
        let mut backend = HashMapBackend::new();

        let id1 = backend.intern("hello");
        let id2 = backend.intern("héllo"); // accent
        let id3 = backend.intern("你好");   // Chinese
        let id4 = backend.intern("🦀");     // Emoji

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id3, id4);

        assert_eq!(backend.lookup(id1), Some("hello"));
        assert_eq!(backend.lookup(id2), Some("héllo"));
        assert_eq!(backend.lookup(id3), Some("你好"));
        assert_eq!(backend.lookup(id4), Some("🦀"));
    }

    #[test]
    fn test_empty_string() {
        let mut backend = HashMapBackend::new();

        let id = backend.intern("");
        assert_eq!(backend.lookup(id), Some(""));
        assert!(backend.contains(""));
    }

    #[test]
    fn test_clone() {
        let mut backend = HashMapBackend::new();
        backend.intern("hello");
        backend.intern("world");

        let cloned = backend.clone();
        assert_eq!(cloned.vocab_size(), 2);
        assert_eq!(cloned.lookup(0), Some("hello"));
        assert_eq!(cloned.lookup(1), Some("world"));
    }
}
