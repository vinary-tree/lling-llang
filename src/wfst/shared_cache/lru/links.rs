//! Inline primitive links, promoted to copy-on-write blocks as residency grows.
//!
//! A published root shares every unchanged block. Mutating the private staged
//! directory first makes its entry private, then makes that entry's block
//! private. Repeated field writes in one block reuse that staged copy. No
//! independently published pointers or unsafe initialization are involved.

use super::Links;
use imbl::Vector;
use std::ops::{Index, IndexMut};
use std::sync::Arc;

const BLOCK_ROWS: usize = 16;
const INLINE_ROWS: usize = 2;
type Block = [Links; BLOCK_ROWS];
const EMPTY_ROW: Links = Links {
    id: 0,
    previous: None,
    next: None,
};

#[derive(Clone)]
enum Backing {
    Inline([Links; INLINE_ROWS]),
    Blocks(Vector<Arc<Block>>),
}

#[derive(Clone)]
pub(super) struct LinkSlots {
    backing: Backing,
    len: usize,
}

impl LinkSlots {
    pub(super) fn new() -> Self {
        Self {
            backing: Backing::Inline([EMPTY_ROW; INLINE_ROWS]),
            len: 0,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn push_back(&mut self, links: Links) {
        let new_len = self.len.checked_add(1).expect("resident count fits usize");
        match &mut self.backing {
            Backing::Inline(rows) if self.len < INLINE_ROWS => rows[self.len] = links,
            Backing::Inline(rows) => {
                // Promotion preserves indices and copies no payload references.
                // Published inline roots retain their independent primitive rows.
                let mut block = [EMPTY_ROW; BLOCK_ROWS];
                block[..INLINE_ROWS].copy_from_slice(rows);
                block[INLINE_ROWS] = links;
                let mut blocks = Vector::new();
                blocks.push_back(Arc::new(block));
                self.backing = Backing::Blocks(blocks);
            }
            Backing::Blocks(blocks) => {
                let offset = self.len % BLOCK_ROWS;
                if offset == 0 {
                    let mut block = [EMPTY_ROW; BLOCK_ROWS];
                    block[0] = links;
                    blocks.push_back(Arc::new(block));
                } else {
                    let block = &mut blocks[self.len / BLOCK_ROWS];
                    Arc::make_mut(block)[offset] = links;
                }
            }
        }
        self.len = new_len;
    }
}

impl Index<usize> for LinkSlots {
    type Output = Links;

    fn index(&self, index: usize) -> &Links {
        assert!(index < self.len, "link index must name an occupied slot");
        match &self.backing {
            Backing::Inline(rows) => &rows[index],
            Backing::Blocks(blocks) => &blocks[index / BLOCK_ROWS][index % BLOCK_ROWS],
        }
    }
}

impl IndexMut<usize> for LinkSlots {
    fn index_mut(&mut self, index: usize) -> &mut Links {
        assert!(index < self.len, "link index must name an occupied slot");
        match &mut self.backing {
            Backing::Inline(rows) => &mut rows[index],
            Backing::Blocks(blocks) => {
                let block = &mut blocks[index / BLOCK_ROWS];
                &mut Arc::make_mut(block)[index % BLOCK_ROWS]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(len: usize) -> LinkSlots {
        let mut slots = LinkSlots::new();
        for id in 0..len {
            slots.push_back(Links {
                id: id as u64,
                ..EMPTY_ROW
            });
        }
        slots
    }

    #[test]
    fn retained_roots_share_only_unchanged_blocks_and_reuse_staged_copies() {
        for len in [1, 2, 3, 15, 16, 17, 63, 64, 65, 1023, 1024, 1025] {
            let original = filled(len);
            let mut staged = original.clone();
            let indices = [0, 15.min(len - 1), 16.min(len - 1), len - 1];
            for index in indices {
                staged[index].id = u64::MAX;
                let private = match &staged.backing {
                    Backing::Inline(_) => None,
                    Backing::Blocks(blocks) => Some(Arc::as_ptr(&blocks[index / BLOCK_ROWS])),
                };
                staged[index].id = u64::MAX - 1;
                if let Backing::Blocks(blocks) = &staged.backing {
                    assert_eq!(
                        private,
                        Some(Arc::as_ptr(&blocks[index / BLOCK_ROWS])),
                        "successive writes must reuse the staged block"
                    );
                } else {
                    assert!(private.is_none());
                }
            }
            if let (Backing::Blocks(original_blocks), Backing::Blocks(staged_blocks)) =
                (&original.backing, &staged.backing)
            {
                for (block, original_block) in original_blocks.iter().enumerate() {
                    let changed = indices.iter().any(|index| index / BLOCK_ROWS == block);
                    assert_eq!(Arc::ptr_eq(original_block, &staged_blocks[block]), !changed);
                }
            } else {
                assert!(len <= INLINE_ROWS);
            }
            for index in 0..len {
                assert_eq!(original[index].id, index as u64, "old root is immutable");
                assert_eq!(
                    staged[index].id,
                    if indices.contains(&index) {
                        u64::MAX - 1
                    } else {
                        index as u64
                    }
                );
            }
        }
    }

    #[test]
    fn growth_preserves_partial_and_full_blocks_in_retained_roots() {
        for len in [0, 1, 2, 3, 15, 16, 17, 1023, 1024, 1025] {
            let original = filled(len);
            let mut growing = original.clone();
            growing.push_back(Links {
                id: u64::MAX,
                ..EMPTY_ROW
            });
            assert_eq!(original.len(), len);
            assert_eq!(growing.len(), len + 1);
            match &growing.backing {
                Backing::Inline(_) => assert!(len + 1 <= INLINE_ROWS),
                Backing::Blocks(blocks) => {
                    assert!(len + 1 > INLINE_ROWS);
                    assert_eq!(blocks.len(), (len + 1).div_ceil(BLOCK_ROWS));
                }
            }
            for index in 0..len {
                assert_eq!(original[index], growing[index]);
            }
            assert_eq!(growing[len].id, u64::MAX);
            if let Backing::Blocks(original_blocks) = &original.backing {
                let Backing::Blocks(growing_blocks) = &growing.backing else {
                    panic!("growth cannot demote blocks to inline storage");
                };
                for (index, block) in original_blocks.iter().enumerate() {
                    let partial_tail = len % BLOCK_ROWS != 0 && index + 1 == original_blocks.len();
                    assert_eq!(Arc::ptr_eq(block, &growing_blocks[index]), !partial_tail);
                }
            }
        }
    }

    #[test]
    fn unused_partial_block_rows_are_not_addressable() {
        for len in [0, 1, 2, 3, 17] {
            let slots = filled(len);
            assert!(std::panic::catch_unwind(|| slots[len]).is_err());
            assert!(std::panic::catch_unwind(|| slots[usize::MAX]).is_err());
            let mut staged = slots.clone();
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                staged[len].id = 99;
            }))
            .is_err());
            assert_eq!(staged.len(), len);
            for index in 0..len {
                assert_eq!(slots[index].id, index as u64);
            }
        }
    }

    #[test]
    fn inline_growth_preserves_all_retained_roots_and_reports_layout() {
        eprintln!(
            "cache_layout links={} old_block_slots={} inline_slots={} lru={} storage={} snapshot={}",
            size_of::<Links>(),
            size_of::<(Vector<Arc<Block>>, usize)>(),
            size_of::<LinkSlots>(),
            size_of::<super::super::LruStorage<()>>(),
            size_of::<super::super::super::Storage<()>>(),
            size_of::<super::super::super::Snapshot<()>>(),
        );
        let mut growing = LinkSlots::new();
        let mut retained = Vec::new();
        for id in 0..17 {
            retained.push(growing.clone());
            growing.push_back(Links { id, ..EMPTY_ROW });
            for (len, root) in retained.iter().enumerate() {
                assert_eq!(root.len(), len);
                assert_eq!(
                    matches!(root.backing, Backing::Inline(_)),
                    len <= INLINE_ROWS
                );
                for index in 0..len {
                    assert_eq!(root[index].id, index as u64);
                }
            }
        }
    }
}
