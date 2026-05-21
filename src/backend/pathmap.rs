//! PathMap-optimized backend with structural sharing.
//!
//! This module provides a [`LatticeBackend`] implementation that uses PathMap
//! for persistent trie-based storage with structural sharing.
//!
//! # Feature Gate
//!
//! This module is only available when the `f1r3fly` feature is enabled.
//!
//! # Note
//!
//! This is currently a stub implementation. Full PathMap integration
//! will be implemented in Phase 10.

use std::sync::Arc;

use pathmap::PathMap;

use super::traits::{LatticeBackend, VocabId};

/// PathMap path identifier type.
pub type PathId = u64;

/// Extended backend trait for PathMap's structural sharing capabilities.
///
/// This trait extends [`LatticeBackend`] with PathMap-specific operations
/// for structural sharing and copy-on-write semantics.
pub trait PathMapSharingBackend: LatticeBackend {
    /// Create a shared reference to a subtrie rooted at the given prefix.
    ///
    /// Uses PathMap's copy-on-write structural sharing.
    fn share_prefix(&self, prefix: &[u8]) -> Option<Self>
    where
        Self: Sized;

    /// Check if two backends share underlying PathMap structure.
    fn shares_structure_with(&self, other: &Self) -> bool;
}

/// PathMap-backed vocabulary and lattice storage.
///
/// This backend provides:
/// - Structural sharing via PathMap's persistent trie
/// - Copy-on-write semantics for efficient updates
/// - S-expression path compatibility for MORK integration
///
/// # Example
///
/// ```ignore
/// use lling_llang::backend::PathMapBackend;
///
/// let backend = PathMapBackend::new();
/// let id = backend.intern("hello");
/// ```
#[derive(Clone)]
pub struct PathMapBackend {
    /// Shared PathMap storage
    storage: Arc<PathMap<VocabMetadata>>,
    /// Vocabulary mapping: word → PathId
    vocab: indexmap::IndexMap<Arc<str>, VocabId, ahash::RandomState>,
    /// Reverse mapping: VocabId → word
    vocab_reverse: Vec<Arc<str>>,
}

/// Metadata stored with vocabulary entries in PathMap.
#[derive(Clone, Debug, Default)]
pub struct VocabMetadata {
    /// Frequency count (for statistical models)
    pub frequency: u64,
    /// POS tags associated with this word
    pub pos_tags: Vec<String>,
}

impl PathMapBackend {
    /// Create a new PathMap backend.
    pub fn new() -> Self {
        Self {
            storage: Arc::new(PathMap::new()),
            vocab: indexmap::IndexMap::default(),
            vocab_reverse: Vec::new(),
        }
    }

    /// Get the underlying PathMap storage.
    pub fn storage(&self) -> &Arc<PathMap<VocabMetadata>> {
        &self.storage
    }
}

impl Default for PathMapBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl LatticeBackend for PathMapBackend {
    fn intern(&mut self, word: &str) -> VocabId {
        if let Some(&id) = self.vocab.get(word) {
            return id;
        }

        let id = self.vocab_reverse.len() as VocabId;
        let word_arc: Arc<str> = word.into();
        self.vocab.insert(word_arc.clone(), id);
        self.vocab_reverse.push(word_arc);
        id
    }

    fn lookup(&self, id: VocabId) -> Option<&str> {
        self.vocab_reverse.get(id as usize).map(|s| s.as_ref())
    }

    fn vocab_size(&self) -> usize {
        self.vocab_reverse.len()
    }

    fn supports_sharing(&self) -> bool {
        true
    }

    fn contains(&self, word: &str) -> bool {
        self.vocab.contains_key(word)
    }

    fn get_id(&self, word: &str) -> Option<VocabId> {
        self.vocab.get(word).copied()
    }

    fn iter(&self) -> impl Iterator<Item = (VocabId, &str)> {
        self.vocab_reverse
            .iter()
            .enumerate()
            .map(|(i, s)| (i as VocabId, s.as_ref()))
    }
}

impl PathMapSharingBackend for PathMapBackend {
    /// Create a shared reference to a subtrie rooted at the given prefix.
    ///
    /// This creates a new backend that:
    /// - Shares the underlying PathMap storage via Arc (structural sharing)
    /// - Filters the vocabulary to only include words starting with the prefix
    /// - Creates a new ID mapping for the filtered vocabulary
    ///
    /// # Arguments
    ///
    /// * `prefix` - The byte prefix to filter vocabulary by
    ///
    /// # Returns
    ///
    /// A new `PathMapBackend` with filtered vocabulary, or `None` if no words
    /// match the prefix.
    fn share_prefix(&self, prefix: &[u8]) -> Option<Self> {
        // Convert prefix to string for vocabulary filtering
        let prefix_str = std::str::from_utf8(prefix).ok()?;

        // Filter vocabulary to only include words starting with the prefix
        let mut new_vocab = indexmap::IndexMap::default();
        let mut new_vocab_reverse = Vec::new();

        for (word, _old_id) in &self.vocab {
            if word.starts_with(prefix_str) {
                let new_id = new_vocab_reverse.len() as VocabId;
                new_vocab.insert(word.clone(), new_id);
                new_vocab_reverse.push(word.clone());
            }
        }

        // Return None if no words match the prefix
        if new_vocab_reverse.is_empty() {
            return None;
        }

        // Create new backend sharing the same PathMap storage
        Some(Self {
            storage: Arc::clone(&self.storage),
            vocab: new_vocab,
            vocab_reverse: new_vocab_reverse,
        })
    }

    fn shares_structure_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.storage, &other.storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pathmap_backend_new() {
        let backend = PathMapBackend::new();
        assert_eq!(backend.vocab_size(), 0);
        assert!(backend.supports_sharing());
    }

    #[test]
    fn test_pathmap_backend_intern() {
        let mut backend = PathMapBackend::new();

        let id1 = backend.intern("hello");
        let id2 = backend.intern("world");
        let id3 = backend.intern("hello");

        assert_eq!(id1, id3);
        assert_ne!(id1, id2);
        assert_eq!(backend.vocab_size(), 2);
    }

    #[test]
    fn test_pathmap_backend_lookup() {
        let mut backend = PathMapBackend::new();

        let id = backend.intern("test");
        assert_eq!(backend.lookup(id), Some("test"));
        assert_eq!(backend.lookup(999), None);
    }

    #[test]
    fn test_pathmap_backend_sharing() {
        let backend1 = PathMapBackend::new();
        let backend2 = backend1.clone();

        assert!(backend1.shares_structure_with(&backend2));

        let backend3 = PathMapBackend::new();
        assert!(!backend1.shares_structure_with(&backend3));
    }

    #[test]
    fn test_pathmap_backend_share_prefix() {
        let mut backend = PathMapBackend::new();

        // Add some words
        backend.intern("hello");
        backend.intern("help");
        backend.intern("helicopter");
        backend.intern("world");
        backend.intern("wonder");

        // Share prefix "hel"
        let shared = backend.share_prefix(b"hel");
        assert!(shared.is_some());

        let shared = shared.unwrap();
        assert_eq!(shared.vocab_size(), 3); // hello, help, helicopter
        assert!(shared.contains("hello"));
        assert!(shared.contains("help"));
        assert!(shared.contains("helicopter"));
        assert!(!shared.contains("world"));
        assert!(!shared.contains("wonder"));

        // Should share underlying storage
        assert!(backend.shares_structure_with(&shared));
    }

    #[test]
    fn test_pathmap_backend_share_prefix_no_match() {
        let mut backend = PathMapBackend::new();

        backend.intern("hello");
        backend.intern("world");

        // No words start with "xyz"
        let shared = backend.share_prefix(b"xyz");
        assert!(shared.is_none());
    }

    #[test]
    fn test_pathmap_backend_share_prefix_empty() {
        let backend = PathMapBackend::new();

        // Empty backend has no words
        let shared = backend.share_prefix(b"any");
        assert!(shared.is_none());
    }
}
