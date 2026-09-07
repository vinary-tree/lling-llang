//! Exact recency over compact internal residency slots, not external state IDs.
//!
//! All mutations apply to a private staged snapshot. The caller publishes the
//! complete result with one generation-checked CAS; no link is independently
//! atomic. A slot is resolved again whenever that publication fails.

use super::{Cached, StateIndex};
use std::num::NonZeroUsize;
use std::sync::Arc;

mod links;
use links::LinkSlots;

/// One-based internal index: optional links occupy one machine word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Slot(NonZeroUsize);

impl Slot {
    fn new(index: usize) -> Self {
        Self(
            NonZeroUsize::new(index.checked_add(1).expect("resident index fits usize"))
                .expect("one-based slot"),
        )
    }

    fn index(self) -> usize {
        self.0.get() - 1
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Links {
    id: u64,
    previous: Option<Slot>,
    next: Option<Slot>,
}

pub(super) struct Entry<T> {
    slot: Slot,
    pub(super) value: Arc<T>,
}

impl<T> Clone for Entry<T> {
    fn clone(&self) -> Self {
        Self {
            slot: self.slot,
            value: Arc::clone(&self.value),
        }
    }
}

pub(super) struct LruStorage<T> {
    pub(super) capacity: NonZeroUsize,
    // Slots, unlike recency stamps, do not change on hits. Combining the slot
    // and payload here eliminates a second ownership structure on misses
    // while leaving this entire HAMT unchanged on a metadata-only touch.
    pub(super) entries: StateIndex<Entry<T>>,
    links: LinkSlots,
    head: Option<Slot>,
    tail: Option<Slot>,
}

impl<T> Clone for LruStorage<T> {
    fn clone(&self) -> Self {
        Self {
            capacity: self.capacity,
            entries: self.entries.clone(),
            links: self.links.clone(),
            head: self.head,
            tail: self.tail,
        }
    }
}

impl<T> LruStorage<T> {
    pub(super) fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            entries: StateIndex::with_hasher(ahash::RandomState::new()),
            links: LinkSlots::new(),
            head: None,
            tail: None,
        }
    }

    pub(super) fn lookup(&self, id: u64) -> Option<Cached<'_, T>> {
        let entry = self.entries.get(&id)?;
        Some(Cached {
            value: &entry.value,
            touch: (self.tail != Some(entry.slot)).then_some(entry.slot),
        })
    }

    fn append(&mut self, slot: Slot) {
        self.links[slot.index()].previous = self.tail;
        self.links[slot.index()].next = None;
        if let Some(tail) = self.tail {
            self.links[tail.index()].next = Some(slot);
        } else {
            self.head = Some(slot);
        }
        self.tail = Some(slot);
    }

    pub(super) fn touch(&mut self, slot: Slot) {
        if self.tail == Some(slot) {
            return;
        }
        let previous = self.links[slot.index()].previous;
        let next = self.links[slot.index()].next;
        if let Some(previous) = previous {
            self.links[previous.index()].next = next;
        } else {
            self.head = next;
        }
        if let Some(next) = next {
            self.links[next.index()].previous = previous;
        }
        // Mutate fields sequentially: at capacity two, next is also the tail.
        // Replacing a pre-copied neighbor record could restore a removed link.
        self.append(slot);
    }

    /// Admit a currently absent ID. Returns whether an old resident was evicted.
    pub(super) fn insert(&mut self, id: u64, value: Arc<T>) -> bool {
        debug_assert!(!self.entries.contains_key(&id));
        if self.entries.len() == self.capacity.get() {
            let slot = self.head.expect("full positive-capacity LRU has a head");
            let old_id = self.links[slot.index()].id;
            self.entries.remove(&old_id);
            self.entries.insert(id, Entry { slot, value });
            self.links[slot.index()].id = id;
            self.touch(slot);
            true
        } else {
            let slot = Slot::new(self.links.len());
            self.entries.insert(id, Entry { slot, value });
            self.links.push_back(Links {
                id,
                previous: None,
                next: None,
            });
            self.append(slot);
            false
        }
    }

    #[cfg(test)]
    pub(super) fn checked_order(&self) -> Vec<u64> {
        let count = self.entries.len();
        assert_eq!(count, self.links.len());
        assert!(count <= self.capacity.get());
        assert_eq!(self.head.is_none(), count == 0);
        assert_eq!(self.tail.is_none(), count == 0);
        let mut seen = vec![false; count];
        let mut order = Vec::with_capacity(count);
        let mut previous = None;
        let mut cursor = self.head;
        while let Some(slot) = cursor {
            assert!(slot.index() < count);
            assert!(!seen[slot.index()], "recency must not cycle");
            seen[slot.index()] = true;
            let links = &self.links[slot.index()];
            assert_eq!(links.previous, previous);
            let id = links.id;
            assert_eq!(self.entries.get(&id).map(|entry| entry.slot), Some(slot));
            order.push(id);
            previous = Some(slot);
            cursor = links.next;
        }
        assert_eq!(
            order.len(),
            count,
            "every slot occurs once in recency order"
        );
        assert_eq!(previous, self.tail);
        order
    }
}
