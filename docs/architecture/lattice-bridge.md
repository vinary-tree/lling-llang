# The semiring ↔ lattice bridge

> **Thesis.** A feature-gated blanket implementation makes *every idempotent
> semiring* usable as a `libdictenstein`/`llattice` dictionary value, by
> recognizing that an idempotent $`\oplus`$ already satisfies the join-semilattice
> laws — so a dictionary's union-merge `join` becomes exactly the semiring
> $`\oplus`$.

This document covers `src/lattice_bridge.rs`
([source](../../src/lattice_bridge.rs)) and the `lattice` / `lattice-persistent`
feature gates declared in [`Cargo.toml`](../../Cargo.toml).

---

## Terms & symbols

Symbols link to [`NOTATION.md`](../NOTATION.md); conventions in
[`STYLE.md`](../STYLE.md).

| Symbol / term | Meaning |
|---|---|
| **Semiring** | $`(K, \oplus, \otimes, \bar{0}, \bar{1})`$ — the algebra of weights (see [semirings](semirings.md)). |
| $`\oplus`$ | Semiring *plus*: combines **alternatives**. Associative, commutative, identity $`\bar{0}`$. |
| $`\otimes`$ | Semiring *times*: combines **sequential** steps. Associative, identity $`\bar{1}`$, distributes over $`\oplus`$. |
| $`\bar{0}`$ / $`\bar{1}`$ | The $`\oplus`$- and $`\otimes`$-identities ("no path" / "empty path"). |
| **Idempotent** | A semiring is idempotent when $`a \oplus a = a`$ for all $`a \in K`$ (the [`IdempotentSemiring`](../../src/semiring/traits.rs) marker). |
| **Lattice** | A set with `join` (least upper bound) and `meet` (greatest lower bound) — the [`llattice::Lattice`](../../src/lattice_bridge.rs) trait. |
| **Join semilattice** | A set with an associative, commutative, idempotent binary `join` (no `meet` required). |
| $`\lor`$ / $`\land`$ | Lattice `join` / `meet`. |
| **Dictionary value** | A type stored against a key in a `libdictenstein` trie (the `DictionaryValue` marker). |
| **Union-merge** | When two values are stored for the same key, they are combined by `join` ($`\lor`$) — the CRDT-style merge. |

This module is **structural glue**, not an algorithm: it states *why* an
idempotent semiring is already a lattice and supplies the wrapper type that
carries that fact across the crate boundary.

---

## Formal model

### Idempotent $`\oplus`$ is a join

A **join semilattice** is a commutative, associative, idempotent magma. The
semiring axioms already give $`\oplus`$ commutativity and associativity; adding
the [`IdempotentSemiring`](../../src/semiring/traits.rs) law
$`a \oplus a = a`$ supplies the third:

```text
commutativity:  a ⊕ b = b ⊕ a
associativity:  (a ⊕ b) ⊕ c = a ⊕ (b ⊕ c)
idempotency:    a ⊕ a = a               ⟸ IdempotentSemiring
───────────────────────────────────────────────────────────
⇒ (K, ⊕) is a join semilattice, with  join := ⊕  and  0̄ as bottom (⊥).
```

The induced partial order is the **natural order**
$`a \le b \iff a \oplus b = a`$ (for cost semirings, $`\le`$ ranks "at least as
good"); $`\bar{0}`$ is the bottom because $`\bar{0} \oplus a = a`$ for all $`a`$ (see
[Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009) on natural orders and the
[`Semiring::natural_less`](../../src/semiring/traits.rs) method).

### The bridge

The crate defines a blanket marker and an adapter wrapper
([`src/lattice_bridge.rs`](../../src/lattice_bridge.rs)):

```text
trait SemiringLattice : Semiring + IdempotentSemiring {}
impl<S> SemiringLattice for S where S : Semiring + IdempotentSemiring {}   // blanket

struct SemiringLatticeWrapper<S>(pub S);
impl<S> llattice::Lattice for SemiringLatticeWrapper<S>   where S : Semiring + IdempotentSemiring + Clone + Send + Sync {
    join = λ self other. SemiringLatticeWrapper( self.0 ⊕ other.0 )      // ∨ = ⊕
    meet = λ self other. SemiringLatticeWrapper( self.0 ⊗ other.0 )      // ∧ = ⊗ (caveat below)
}
```

`join` is therefore *definitionally* the semiring $`\oplus`$. When a
`libdictenstein` dictionary union-merges two values stored under the same key,
it calls `join` — so **the dictionary's merge equals $`\oplus`$**, which is
the central claim of this module.

> **Meet caveat.** `meet` is wired to $`\otimes`$ for convenience, but
> $`\otimes`$ is *path composition*, not a lattice meet in general. It coincides
> with the true $`\land`$ only for special algebras — e.g. the Boolean semiring,
> where $`\otimes = \text{AND}`$ is the greatest lower bound. For a correct `meet` on
> other semirings, implement [`llattice::Lattice`](../../src/lattice_bridge.rs)
> directly. The source documents exactly this in its type-level doc comments.

### Why the impl lives here (the orphan rule)

Rust's **orphan rule** forbids implementing a foreign trait
(`llattice::Lattice`, `libdictenstein::DictionaryValue`) for a foreign
type unless the implementing crate owns one of them. `lling-llang` owns the
semiring types, so wrapping them in the local `SemiringLatticeWrapper` makes
the impls legal *here* and breaks what would otherwise be a dependency cycle
(`libdictenstein → lling-llang → libdictenstein`). The wrapper was relocated out
of `libdictenstein` for exactly this reason.

---

## Intuition — two dictionary values merging via $`\oplus`$

Store two `TropicalWeight` values under the same dictionary key — say the
costs $`10.0`$ and $`5.0`$ arrive from two sources. The union-merge calls
$`\text{join} = \oplus = \min`$, keeping the cheaper:

```text
left  = Wrapper(Tropical 10.0)
right = Wrapper(Tropical  5.0)
                       join (⊕ = min)
left.join(&right) = Wrapper(Tropical 5.0)        ▷ 5.0 = min(10.0, 5.0)
```

For the Boolean semiring the same merge is logical OR: $`\text{true} \lor \text{false} = \text{true}`$. This is the object snapshot in [§ Diagrams](#diagrams) — the merge is
commutative, associative, and idempotent, which is precisely what a
conflict-free (CRDT-style) dictionary merge requires.

---

## Architecture & API

```text
lattice_bridge  (cfg(feature = "lattice"))
├── trait SemiringLattice : Semiring + IdempotentSemiring   (blanket impl for all such S)
└── struct SemiringLatticeWrapper<S>(pub S)
      ├── impl llattice::Lattice         join = ⊕ , meet = ⊗
      ├── impl DictionaryValue           (basic bounds)              when NOT lattice-persistent
      └── impl DictionaryValue           (serde-bounded)             when     lattice-persistent
```

| Item | Responsibility |
|---|---|
| [`SemiringLattice`](../../src/lattice_bridge.rs) | Marker trait + blanket impl: *any* `S: Semiring + IdempotentSemiring` is a `SemiringLattice`. |
| [`SemiringLatticeWrapper<S>`](../../src/lattice_bridge.rs) | Newtype adapter carrying an `S` value; supplies $`\text{join} = \oplus`$ and $`\text{meet} = \otimes`$ and serves as a dictionary value. |
| `impl Lattice for Wrapper` | `join(&self, other) = Wrapper(self.0.plus(&other.0))`; `meet(&self, other) = Wrapper(self.0.times(&other.0))`. |
| `impl DictionaryValue` | Two cfg-gated impls — basic bounds vs serde-bounded — so the wrapper is storable in in-memory and disk-backed dictionaries respectively. |

The wrapper derives `Clone, Copy, Debug, Default, PartialEq`, and under
`lattice-persistent` additionally derives transparent
`serde::Serialize`/`Deserialize` so the inner weight serializes directly.

---

## Feature gates

From [`Cargo.toml`](../../Cargo.toml):

```toml
[features]
# libdictenstein lattice bridge: lling-llang semirings as dictionary values (cycle-break)
lattice = ["dep:llattice", "dep:libdictenstein"]
# ...with serde-bounded DictionaryValue for disk-backed (persistent-artrie) dictionaries
lattice-persistent = ["lattice", "libdictenstein/persistent-artrie", "dep:serde"]
```

| Feature | Effect |
|---|---|
| **`lattice`** | Compiles `pub mod lattice_bridge` (gated in [`lib.rs`](../../src/lib.rs)), pulls in `llattice` and `libdictenstein`, and provides the basic-bounds `DictionaryValue` impl. |
| **`lattice-persistent`** | Implies `lattice`; enables `libdictenstein`'s `persistent-artrie` backend and `serde`, swapping in the **serde-bounded** `DictionaryValue` impl so wrapped weights can be persisted to disk-backed dictionaries. |

The two `DictionaryValue` impls are mutually exclusive via
`#[cfg(not(feature = "lattice-persistent"))]` and
`#[cfg(feature = "lattice-persistent")]`, so exactly one is active for any
feature selection.

---

## Algorithms — the union-merge join

There is no iterative algorithm here; the "algorithm" is the one-line merge the
dictionary invokes when two values collide on a key. Its intent: combine
contributions order-independently. The relevant invariant is that
`join` is idempotent/commutative/associative, so repeated or reordered
merges converge to the same value (the CRDT property).

```text
⟨ union-merge on key collision ⟩ ≡
  on insert(key, new) where dict already holds (key, old):
      dict[key] ← old.join(&new)            ▷ join = ⊕  (idempotent semiring plus)
  ── properties inherited from idempotent ⊕ ──
  old.join(&old)               = old                      (idempotent)
  old.join(&new)               = new.join(&old)           (commutative)
  (a.join(&b)).join(&c)        = a.join(&b.join(&c))      (associative)
```

Each merge is $`O(\operatorname{cost}(\oplus))`$ — $`O(1)`$ for scalar semirings such as
Tropical/Boolean. Because the operation is a lattice join, a dictionary built
this way is a conflict-free replicated value: merging two replicas in any order
yields the least upper bound of their contents.

**Trace** (Tropical): inserting $`5.0`$ where $`10.0`$ is stored runs
$`10.0.\text{join}(\&5.0) = 10.0 \oplus 5.0 = \min(10.0, 5.0) = 5.0`$; inserting
$`5.0`$ again is idempotent ($`5.0 \oplus 5.0 = 5.0`$), leaving the entry
unchanged. $`\blacksquare`$

---

## Examples

Snippets are from the module's `#[cfg(test)]` suite (compiler-checked under
`--features lattice`).

### Tropical: join = $`\oplus = \min`$, meet = $`\otimes = +`$

```rust,ignore
use lling_llang::lattice_bridge::SemiringLatticeWrapper;
use lling_llang::semiring::TropicalWeight;
use llattice::Lattice;
use ordered_float::OrderedFloat;

let a = SemiringLatticeWrapper(TropicalWeight(OrderedFloat(10.0)));
let b = SemiringLatticeWrapper(TropicalWeight(OrderedFloat(5.0)));

// join = plus = min  (the union-merge a dictionary performs)
assert_eq!(a.join(&b).0 .0 .0, 5.0);
// meet = times = +   (path composition, NOT a true lattice meet)
assert_eq!(a.meet(&b).0 .0 .0, 15.0);
```

### Boolean: join = $`\oplus = \text{OR}`$

```rust,ignore
use lling_llang::lattice_bridge::SemiringLatticeWrapper;
use lling_llang::semiring::BoolWeight;
use llattice::Lattice;

let t = SemiringLatticeWrapper(BoolWeight(true));
let f = SemiringLatticeWrapper(BoolWeight(false));

assert!(t.join(&f).0 .0);   // true  OR false = true
assert!(!f.join(&f).0 .0);  // false OR false = false   (idempotent on false)
```

### Any idempotent semiring is a `SemiringLattice`

```rust,ignore
use lling_llang::lattice_bridge::SemiringLattice;
use lling_llang::semiring::{BoolWeight, TropicalWeight};

// The blanket impl means these bounds are satisfied with no extra code:
fn assert_is_semiring_lattice<S: SemiringLattice>() {}
assert_is_semiring_lattice::<TropicalWeight>();
assert_is_semiring_lattice::<BoolWeight>();
```

---

## Diagrams

### Blanket impl + two values merging via $`\oplus`$

![Class diagram of the semiring↔lattice bridge: Semiring is refined by IdempotentSemiring and the blanket SemiringLattice; SemiringLatticeWrapper<S> implements llattice::Lattice (join = ⊕, meet = ⊗) and libdictenstein::DictionaryValue. Below, an object snapshot shows two Tropical wrapper values joining (⊕ = min) into a merged value.](../diagrams/architecture/lattice-bridge.svg)

*Blue = the core `Semiring` / `Lattice` interfaces; amber =
idempotent marker and the wrapper/value objects; green = the
`SemiringLattice` blanket marker and the merged result; bold green arrows =
the union-merge $`\text{join} = \oplus`$ of $`\text{Tropical}(10)`$ and $`\text{Tropical}(5)`$
into $`\text{Tropical}(5) = \min(10,5)`$; neutral grey = the `DictionaryValue`
marker bound.*

<details><summary>Text view</summary>

```text
        Semiring (K, ⊕, ⊗, 0̄, 1̄)
              ▲
       IdempotentSemiring  (a ⊕ a = a)
              ▲  blanket impl for all S : Semiring + IdempotentSemiring
        SemiringLattice ─────────────► SemiringLatticeWrapper<S>
                                          │  impl llattice::Lattice
                                          │     join = self.0 ⊕ other.0
                                          │     meet = self.0 ⊗ other.0   (⊗ = path compose, not true meet)
                                          └  impl DictionaryValue (serde-bounded iff lattice-persistent)

   union-merge of two values for the SAME key:
       left  = Wrapper(Tropical 10.0) ─┐ join (⊕)
                                        ├──► merged = Wrapper(Tropical 5.0) = min(10, 5)
       right = Wrapper(Tropical  5.0) ─┘ join (⊕)
```

</details>

---

## Relation to the library

- **Semirings.** The bridge consumes the
  [`IdempotentSemiring`](semirings.md) marker; Tropical ($`\min(a,a)=a`$) and
  Boolean ($`a \lor a = a`$) qualify, while non-idempotent algebras
  (Probability, Count) intentionally do not — they have no lawful join, so the
  blanket impl excludes them.
- **Dictionaries & lattices.** With `lattice` enabled, any qualifying weight
  becomes a `libdictenstein` dictionary value via
  [`SemiringLatticeWrapper`](../../src/lattice_bridge.rs); the
  [`llattice::Lattice`](../../src/lattice_bridge.rs) trait supplies the shared
  `join`/`meet` vocabulary so downstream crates need not re-derive it.
- **Lattices in this crate vs `llattice`.** The WFST
  [`Lattice`](lattices.md) type (a weighted DAG of hypotheses) is unrelated to
  the order-theoretic `llattice::Lattice` used here; this bridge concerns
  the latter (join/meet algebra), which is why it lives in `architecture/`
  beside the semiring docs.
- **Cycle-breaking placement.** Hosting the impls in `lling-llang` (which owns
  the semiring types) satisfies the orphan rule and keeps
  `libdictenstein` free of a back-dependency on this crate.

---

## References

- <a id="ref-mohri2009"></a>**[Mohri 2009]** Mohri, M. (2009). *Weighted
  Automata Algorithms.* In *Handbook of Weighted Automata*, pp. 213–254.
  Springer.
  [doi:10.1007/978-3-642-01492-5_6](https://doi.org/10.1007/978-3-642-01492-5_6)
  — natural orders of semirings; see
  [`BIBLIOGRAPHY.md`](../BIBLIOGRAPHY.md#ref-mohri2009).
- <a id="ref-davey2002"></a>**[Davey & Priestley 2002]** Davey, B. A., &
  Priestley, H. A. (2002). *Introduction to Lattices and Order* (2nd ed.).
  Cambridge University Press.
  [doi:10.1017/CBO9780511809088](https://doi.org/10.1017/CBO9780511809088)
  — join/meet semilattices and the natural order.
- <a id="ref-shapiro2011"></a>**[Shapiro 2011]** Shapiro, M., Preguiça, N.,
  Baquero, C., & Zawirski, M. (2011). *Conflict-Free Replicated Data Types.* SSS
  2011, LNCS 6976:386–400.
  [doi:10.1007/978-3-642-24550-3_29](https://doi.org/10.1007/978-3-642-24550-3_29)
  — join-semilattice merges as the basis of CvRDTs.
- <a id="ref-droste2009"></a>**[Droste & Kuich 2009]** Droste, M., & Kuich, W.
  (2009). *Semirings and Formal Power Series.* In *Handbook of Weighted
  Automata*, pp. 3–28. Springer.
  [doi:10.1007/978-3-642-01492-5_1](https://doi.org/10.1007/978-3-642-01492-5_1)
  — idempotent semirings and their order structure.
