//! Cache-conscious refinable partitions used by certified bisimulation.
//!
//! Every partition owns seven dense index arrays. Members of one block occupy a
//! contiguous interval in `elements`; marking swaps an element into that
//! interval's marked prefix. A split rewrites only the smaller child and gives
//! that child the fresh block identifier. Consequently, an element can be
//! charged at most logarithmically many times.

use super::BisimulationError;

/// The structural result of one non-trivial block split.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PartitionSplit {
    pub(super) old_block: usize,
    pub(super) new_block: usize,
    pub(super) new_is_marked: bool,
    pub(super) parent_representative: usize,
}

/// Valmari's refinable partition over the dense carrier `0..element_count`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RefinablePartition {
    elements: Vec<usize>,
    location: Vec<usize>,
    block_of: Vec<usize>,
    first: Vec<usize>,
    past: Vec<usize>,
    marked: Vec<usize>,
    touched: Vec<usize>,
}

impl RefinablePartition {
    /// Construct a canonical initial partition from one arbitrary class value
    /// per element. Fixed-width radix passes keep this phase linear on the
    /// word-RAM model and make its result independent of hash seeds.
    pub(super) fn from_classes(classes: &[usize]) -> Result<Self, BisimulationError> {
        let count = classes.len();
        let mut elements = reserved_vec(count, "state partition elements")?;
        elements.extend(0..count);
        radix_sort_indices_by_usize_key(&mut elements, classes)?;

        let mut location = filled_vec(count, 0, "partition locations")?;
        let mut block_of = filled_vec(count, 0, "partition block map")?;
        let mut first = reserved_vec(count, "partition block starts")?;
        let mut past = reserved_vec(count, "partition block ends")?;
        let mut marked = reserved_vec(count, "partition mark counts")?;

        if count != 0 {
            let mut start = 0;
            while start < count {
                let class = classes[elements[start]];
                let mut end = start + 1;
                while end < count && classes[elements[end]] == class {
                    end += 1;
                }
                let block = first.len();
                first.push(start);
                past.push(end);
                marked.push(0);
                for index in start..end {
                    let element = elements[index];
                    location[element] = index;
                    block_of[element] = block;
                }
                start = end;
            }
        }

        Ok(Self {
            elements,
            location,
            block_of,
            first,
            past,
            marked,
            touched: reserved_vec(count, "partition touched blocks")?,
        })
    }

    #[inline]
    pub(super) fn block_count(&self) -> usize {
        self.first.len()
    }

    #[inline]
    pub(super) fn block_of(&self, element: usize) -> usize {
        self.block_of[element]
    }

    #[inline]
    pub(super) fn block_map(&self) -> &[usize] {
        &self.block_of
    }

    #[inline]
    pub(super) fn members(&self, block: usize) -> &[usize] {
        &self.elements[self.first[block]..self.past[block]]
    }

    /// Mark an element once. Repeated marks during the same phase are ignored;
    /// nondeterministic LTS states may own several transitions in one cluster.
    #[inline]
    pub(super) fn mark(&mut self, element: usize) {
        let block = self.block_of[element];
        let location = self.location[element];
        let boundary = self.first[block] + self.marked[block];
        if location < boundary {
            return;
        }

        let displaced = self.elements[boundary];
        self.elements.swap(location, boundary);
        self.location[element] = boundary;
        self.location[displaced] = location;
        if self.marked[block] == 0 {
            self.touched.push(block);
        }
        self.marked[block] += 1;
    }

    /// Split every touched block, assigning the fresh identifier to the
    /// smaller half. `charges`, when supplied, is incremented exactly for the
    /// elements moved into fresh smaller blocks.
    pub(super) fn split_touched(
        &mut self,
        mut charges: Option<&mut [usize]>,
        output: &mut Vec<PartitionSplit>,
    ) -> Result<(), BisimulationError> {
        output.clear();
        output.try_reserve(self.touched.len()).map_err(|_| {
            BisimulationError::AllocationFailed {
                context: "partition split records",
            }
        })?;

        for touched_index in 0..self.touched.len() {
            let block = self.touched[touched_index];
            let marked_count = self.marked[block];
            let first = self.first[block];
            let past = self.past[block];
            let size = past - first;

            if marked_count == 0 || marked_count == size {
                self.marked[block] = 0;
                continue;
            }

            let split_point = first + marked_count;
            let new_block = self.first.len();
            let marked_is_smaller = marked_count <= size - marked_count;
            let (new_first, new_past, new_is_marked) = if marked_is_smaller {
                self.first[block] = split_point;
                (first, split_point, true)
            } else {
                self.past[block] = split_point;
                (split_point, past, false)
            };

            self.first.push(new_first);
            self.past.push(new_past);
            self.marked.push(0);
            self.marked[block] = 0;

            for index in new_first..new_past {
                let element = self.elements[index];
                self.block_of[element] = new_block;
                if let Some(counts) = charges.as_deref_mut() {
                    counts[element] = counts[element].checked_add(1).ok_or(
                        BisimulationError::ArithmeticOverflow {
                            context: "smaller-half charge counter",
                        },
                    )?;
                }
            }

            output.push(PartitionSplit {
                old_block: block,
                new_block,
                new_is_marked,
                parent_representative: self.elements[first],
            });
        }
        self.touched.clear();
        Ok(())
    }

    /// Number of allocated element slots in the partition's core arrays.
    pub(super) fn heap_cells(&self) -> usize {
        self.elements.capacity()
            + self.location.capacity()
            + self.block_of.capacity()
            + self.first.capacity()
            + self.past.capacity()
            + self.marked.capacity()
            + self.touched.capacity()
    }
}

fn reserved_vec<T>(capacity: usize, context: &'static str) -> Result<Vec<T>, BisimulationError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(capacity)
        .map_err(|_| BisimulationError::AllocationFailed { context })?;
    Ok(result)
}

fn filled_vec<T: Clone>(
    len: usize,
    value: T,
    context: &'static str,
) -> Result<Vec<T>, BisimulationError> {
    let mut result = reserved_vec(len, context)?;
    result.resize(len, value);
    Ok(result)
}

fn radix_sort_indices_by_usize_key(
    indices: &mut Vec<usize>,
    keys: &[usize],
) -> Result<(), BisimulationError> {
    if indices.len() < 2 {
        return Ok(());
    }
    let mut scratch = filled_vec(indices.len(), 0, "partition radix scratch")?;
    for byte in 0..core::mem::size_of::<usize>() {
        let shift = byte * 8;
        let mut counts = [0usize; 256];
        for &index in indices.iter() {
            counts[(keys[index] >> shift) & 0xff] += 1;
        }
        let mut total = 0usize;
        for count in &mut counts {
            let bucket = *count;
            *count = total;
            total = total
                .checked_add(bucket)
                .ok_or(BisimulationError::ArithmeticOverflow {
                    context: "partition radix prefix sum",
                })?;
        }
        for &index in indices.iter() {
            let bucket = (keys[index] >> shift) & 0xff;
            scratch[counts[bucket]] = index;
            counts[bucket] += 1;
        }
        core::mem::swap(indices, &mut scratch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_classes_and_smaller_half_are_exact() {
        let mut partition = RefinablePartition::from_classes(&[9, 2, 9, 2, 9]).unwrap();
        assert_eq!(partition.members(0), &[1, 3]);
        assert_eq!(partition.members(1), &[0, 2, 4]);

        partition.mark(0);
        let mut charges = vec![0; 5];
        let mut splits = Vec::new();
        partition
            .split_touched(Some(&mut charges), &mut splits)
            .unwrap();
        assert_eq!(splits.len(), 1);
        assert_eq!(partition.members(splits[0].new_block), &[0]);
        assert_eq!(charges, [1, 0, 0, 0, 0]);
    }

    #[test]
    fn repeated_marks_are_idempotent() {
        let mut partition = RefinablePartition::from_classes(&[0, 0, 0]).unwrap();
        partition.mark(1);
        partition.mark(1);
        let mut splits = Vec::new();
        partition.split_touched(None, &mut splits).unwrap();
        assert_eq!(splits.len(), 1);
        assert_eq!(partition.members(splits[0].new_block), &[1]);
    }
}
