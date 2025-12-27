//! Traits for lattice backend storage.

/// A vocabulary identifier.
///
/// This is a lightweight handle to an interned string stored in a backend.
/// The actual representation depends on the backend:
/// - For [`HashMapBackend`]: Sequential u32 index
/// - For [`PathMapBackend`]: PathMap path identifier
pub type VocabId = u32;

/// Backend trait for lattice edge storage and vocabulary management.
///
/// This trait abstracts over different storage strategies for vocabulary
/// interning. Implementations should:
/// - Efficiently intern strings (deduplicate identical strings)
/// - Provide O(1) or O(log n) lookup by ID
/// - Be thread-safe (implement Send + Sync)
///
/// # Type Parameters
///
/// None - the trait uses a fixed `VocabId` type (u32) for simplicity.
/// For PathMap integration, the backend converts PathId to VocabId internally.
///
/// # Example
///
/// ```rust
/// use lling_llang::backend::{LatticeBackend, HashMapBackend};
///
/// let mut backend = HashMapBackend::new();
///
/// // Intern returns the same ID for identical strings
/// let id1 = backend.intern("hello");
/// let id2 = backend.intern("hello");
/// assert_eq!(id1, id2);
///
/// // Lookup by ID
/// assert_eq!(backend.lookup(id1), Some("hello"));
/// ```
pub trait LatticeBackend: Clone + Send + Sync {
    /// Intern a word, returning its vocabulary ID.
    ///
    /// If the word was previously interned, returns the existing ID.
    /// Otherwise, allocates a new ID and stores the word.
    fn intern(&mut self, word: &str) -> VocabId;

    /// Look up a word by vocabulary ID.
    ///
    /// Returns `None` if the ID is invalid (not previously returned by `intern`).
    fn lookup(&self, id: VocabId) -> Option<&str>;

    /// Check if this backend supports structural sharing.
    ///
    /// Returns `true` for PathMap-based backends, `false` for simple hash maps.
    /// Structural sharing allows multiple lattices to share common vocabulary
    /// and edge storage efficiently.
    fn supports_sharing(&self) -> bool {
        false
    }

    /// Get the number of unique words in the vocabulary.
    fn vocab_size(&self) -> usize;

    /// Check if a word has been interned.
    fn contains(&self, word: &str) -> bool;

    /// Get the vocabulary ID for a word without interning.
    ///
    /// Returns `None` if the word has not been interned.
    fn get_id(&self, word: &str) -> Option<VocabId>;

    /// Iterate over all vocabulary entries.
    ///
    /// Returns an iterator of (VocabId, &str) pairs.
    fn iter(&self) -> impl Iterator<Item = (VocabId, &str)>;
}

/// Marker trait for backends that support structural sharing.
///
/// This trait is automatically implemented for backends that can share
/// underlying storage across multiple lattices. Currently only available
/// with the `f1r3fly` feature for PathMap integration.
pub trait SharingBackend: LatticeBackend {
    /// Check if two backends share underlying storage.
    fn shares_storage_with(&self, other: &Self) -> bool;

    /// Create a fork of this backend that shares structure.
    ///
    /// Modifications to the fork will use copy-on-write semantics,
    /// preserving the original backend's data.
    fn fork(&self) -> Self;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::HashMapBackend;

    #[test]
    fn test_intern_returns_same_id() {
        let mut backend = HashMapBackend::new();
        let id1 = backend.intern("hello");
        let id2 = backend.intern("hello");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_different_words_different_ids() {
        let mut backend = HashMapBackend::new();
        let id1 = backend.intern("hello");
        let id2 = backend.intern("world");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_lookup_valid_id() {
        let mut backend = HashMapBackend::new();
        let id = backend.intern("hello");
        assert_eq!(backend.lookup(id), Some("hello"));
    }

    #[test]
    fn test_lookup_invalid_id() {
        let backend = HashMapBackend::new();
        assert_eq!(backend.lookup(999), None);
    }

    #[test]
    fn test_vocab_size() {
        let mut backend = HashMapBackend::new();
        assert_eq!(backend.vocab_size(), 0);
        backend.intern("hello");
        assert_eq!(backend.vocab_size(), 1);
        backend.intern("world");
        assert_eq!(backend.vocab_size(), 2);
        backend.intern("hello"); // duplicate
        assert_eq!(backend.vocab_size(), 2);
    }

    #[test]
    fn test_contains() {
        let mut backend = HashMapBackend::new();
        assert!(!backend.contains("hello"));
        backend.intern("hello");
        assert!(backend.contains("hello"));
        assert!(!backend.contains("world"));
    }

    #[test]
    fn test_get_id() {
        let mut backend = HashMapBackend::new();
        assert_eq!(backend.get_id("hello"), None);
        let id = backend.intern("hello");
        assert_eq!(backend.get_id("hello"), Some(id));
    }

    #[test]
    fn test_iter() {
        let mut backend = HashMapBackend::new();
        let id1 = backend.intern("hello");
        let id2 = backend.intern("world");

        let mut entries: Vec<_> = backend.iter().collect();
        entries.sort_by_key(|(id, _)| *id);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], (id1, "hello"));
        assert_eq!(entries[1], (id2, "world"));
    }
}
