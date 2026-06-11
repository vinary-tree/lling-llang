//! `libdictenstein` lattice bridge — `IdempotentSemiring` ⇒ [`llattice::Lattice`].
//!
//! Feature-gated on `lattice`. Lets any lling-llang idempotent semiring be used
//! as a `libdictenstein` dictionary value whose union-merge join is the semiring
//! `plus` (⊕).
//!
//! Relocated from `libdictenstein` to break a dependency cycle: lling-llang owns
//! the semiring types, so the orphan rule permits these impls to live here.

use crate::semiring::{IdempotentSemiring, Semiring};
use llattice::Lattice;

/// Marker trait for types that implement [`Lattice`] via [`IdempotentSemiring`].
///
/// For an idempotent semiring, `plus` (⊕) satisfies the join-semilattice laws,
/// so the semiring forms a join semilattice. `times` (⊗) is path composition,
/// not lattice meet, so a true meet must be supplied separately.
pub trait SemiringLattice: Semiring + IdempotentSemiring {}

impl<S> SemiringLattice for S where S: Semiring + IdempotentSemiring {}

/// Adapter wrapping a semiring value so it can serve as a join-lattice.
///
/// - `join` = semiring `plus` (⊕)
/// - `meet` = semiring `times` (⊗) — note that `times` is path composition, which
///   coincides with lattice meet only for some semirings (e.g. Boolean, where
///   `times` = AND). For correct meet semantics on other semirings, implement
///   [`Lattice`] directly.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(
    feature = "lattice-persistent",
    derive(serde::Serialize, serde::Deserialize)
)]
#[cfg_attr(feature = "lattice-persistent", serde(transparent))]
pub struct SemiringLatticeWrapper<S>(pub S);

impl<S: Semiring + IdempotentSemiring + Clone + Send + Sync> Lattice for SemiringLatticeWrapper<S> {
    #[inline]
    fn join(&self, other: &Self) -> Self {
        SemiringLatticeWrapper(self.0.plus(&other.0))
    }

    #[inline]
    fn meet(&self, other: &Self) -> Self {
        SemiringLatticeWrapper(self.0.times(&other.0))
    }
}

// Implement `DictionaryValue` so the wrapper can be stored as a dictionary value.
// Without `lattice-persistent`, only the basic bounds are required.
#[cfg(not(feature = "lattice-persistent"))]
impl<S: Clone + Default + Send + Sync + Unpin + 'static> libdictenstein::value::DictionaryValue
    for SemiringLatticeWrapper<S>
{
}

// With `lattice-persistent` (disk-backed dictionaries), `DictionaryValue` requires
// serde, so the wrapper's element must be serializable too.
#[cfg(feature = "lattice-persistent")]
impl<
        S: Clone
            + Default
            + Send
            + Sync
            + Unpin
            + 'static
            + serde::Serialize
            + serde::de::DeserializeOwned,
    > libdictenstein::value::DictionaryValue for SemiringLatticeWrapper<S>
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::{BoolWeight, TropicalWeight};
    use ordered_float::OrderedFloat;

    #[test]
    fn tropical_join_is_plus_meet_is_times() {
        let a = SemiringLatticeWrapper(TropicalWeight(OrderedFloat(10.0)));
        let b = SemiringLatticeWrapper(TropicalWeight(OrderedFloat(5.0)));
        // join = plus = min (tropical)
        assert_eq!(a.join(&b).0 .0 .0, 5.0);
        // meet = times = + (path composition, not a true lattice meet)
        assert_eq!(a.meet(&b).0 .0 .0, 15.0);
    }

    #[test]
    fn bool_join_is_or() {
        let t = SemiringLatticeWrapper(BoolWeight(true));
        let f = SemiringLatticeWrapper(BoolWeight(false));
        assert!(t.join(&f).0 .0); // true OR false = true
        assert!(!f.join(&f).0 .0); // false OR false = false
    }
}
