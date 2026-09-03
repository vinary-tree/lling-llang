//! Order-insensitive collection algebras: [`BagAlgebra`] (multisets) and
//! [`MapAlgebra`] (key→value maps).
//!
//! Ordered sequences use the regex/SFA list algebra
//! ([`crate::symbolic::regex_sfa::ListAlgebra`]); bags and maps are unordered and are
//! decided by **counting** rather than by automaton structure.
//!
//! ## Bag decision procedure (exact)
//!
//! A bag predicate is a boolean combination of cardinality atoms
//! `Count{class, [lo, hi]}` = "the number of elements satisfying `class` lies in
//! `[lo, hi]`". To decide satisfiability exactly:
//!
//! 1. Take the **minterms** of all classes appearing in the predicate (maximal
//!    satisfiable conjunctions of the classes and their negations, computed over
//!    the element algebra). Every element falls in exactly one minterm, so a bag
//!    is fully characterized by its per-minterm count vector
//!    $`(c_0,\ldots,c_{k-1})`$.
//! 2. Each `Count{class, [lo,hi]}` becomes a linear constraint
//!    $`\sum_{m\subseteq\mathrm{class}}c_m\in[\mathrm{lo},\mathrm{hi}]`$
//!    on that vector.
//! 3. Feasibility is integer-linear over $`\mathbb{N}^k`$. Every threshold
//!    is below
//!    $`B=1+\max(\text{all lo},\text{all finite hi})`$, so counts
//!    $`\ge B`$ are indistinguishable and a bounded search over
//!    $`[0,B]^k`$ is **exact**.
//!
//! `evaluate` uses the bag's actual counts (uncapped); `witness` materializes a
//! bag from a feasible count vector. The search is exponential in the number of
//! minterms ($`\le 2^{\#\mathrm{classes}}`$), which is small for real
//! guards—correctness holds regardless of cost.

use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};

use super::kat_algebra::BooleanTest;
use super::ordered_field::{OrderedFieldAlgebra, OrderedFieldPred, OrderedPoint};
use super::string_algebra::{StrPred, StringAlgebra};
use super::{BooleanAlgebra, CharClassAlgebra, CharClassPred, IntervalAlgebra, IntervalPred};

// ══════════════════════════════════════════════════════════════════════════════
// Minterms (shared helper)
// ══════════════════════════════════════════════════════════════════════════════

/// The satisfiable minterms of a set of element predicates: maximal satisfiable
/// conjunctions of each predicate or its negation. Every domain element
/// satisfies exactly one minterm.
pub(crate) fn minterms<A: BooleanAlgebra>(elem: &A, classes: &[A::Predicate]) -> Vec<A::Predicate> {
    let mut ms = vec![elem.true_pred()];
    for c in classes {
        let nc = elem.not(c);
        let mut next = Vec::with_capacity(ms.len() * 2);
        for m in &ms {
            let pos = elem.and(m, c);
            if elem.is_satisfiable(&pos) {
                next.push(pos);
            }
            let neg = elem.and(m, &nc);
            if elem.is_satisfiable(&neg) {
                next.push(neg);
            }
        }
        ms = next;
    }
    ms
}

// ══════════════════════════════════════════════════════════════════════════════
// BagAlgebra
// ══════════════════════════════════════════════════════════════════════════════

/// A bag predicate: a boolean combination of cardinality atoms over an element
/// predicate type `P`.
pub enum BagPred<P> {
    /// Satisfied by every bag.
    True,
    /// Satisfied by no bag.
    False,
    /// ```math
    /// \mathrm{lo}\le
    /// \#\{e\in\mathrm{bag}:e\models\mathrm{class}\}
    /// \le\mathrm{hi}
    /// ```
    ///
    /// `hi = None` is unbounded above.
    Count { class: P, lo: u64, hi: Option<u64> },
    /// Conjunction.
    And(Box<BagPred<P>>, Box<BagPred<P>>),
    /// Disjunction.
    Or(Box<BagPred<P>>, Box<BagPred<P>>),
    /// Negation.
    Not(Box<BagPred<P>>),
}

impl<P: Clone> Clone for BagPred<P> {
    fn clone(&self) -> Self {
        enum Task<'a, P> {
            Clone(&'a BagPred<P>),
            And,
            Or,
            Not,
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(predicate) => match predicate {
                    BagPred::True => values.push(BagPred::True),
                    BagPred::False => values.push(BagPred::False),
                    BagPred::Count { class, lo, hi } => values.push(BagPred::Count {
                        class: class.clone(),
                        lo: *lo,
                        hi: *hi,
                    }),
                    BagPred::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    BagPred::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    BagPred::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Clone(inner));
                    }
                },
                Task::And | Task::Or => {
                    let right = values.pop().expect("right bag clone is present");
                    let left = values.pop().expect("left bag clone is present");
                    values.push(if matches!(task, Task::And) {
                        BagPred::And(Box::new(left), Box::new(right))
                    } else {
                        BagPred::Or(Box::new(left), Box::new(right))
                    });
                }
                Task::Not => {
                    let inner = values.pop().expect("negated bag clone is present");
                    values.push(BagPred::Not(Box::new(inner)));
                }
            }
        }
        values
            .pop()
            .expect("the root bag predicate produces one clone")
    }
}

impl<P: PartialEq> PartialEq for BagPred<P> {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (BagPred::True, BagPred::True) | (BagPred::False, BagPred::False) => {}
                (
                    BagPred::Count {
                        class: lc,
                        lo: ll,
                        hi: lh,
                    },
                    BagPred::Count {
                        class: rc,
                        lo: rl,
                        hi: rh,
                    },
                ) if lc == rc && ll == rl && lh == rh => {}
                (BagPred::And(ll, lr), BagPred::And(rl, rr))
                | (BagPred::Or(ll, lr), BagPred::Or(rl, rr)) => {
                    pending.push((lr, rr));
                    pending.push((ll, rl));
                }
                (BagPred::Not(left), BagPred::Not(right)) => pending.push((left, right)),
                _ => return false,
            }
        }
        true
    }
}

impl<P: Eq> Eq for BagPred<P> {}

impl<P: Hash> Hash for BagPred<P> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut pending = vec![self];
        while let Some(predicate) = pending.pop() {
            std::mem::discriminant(predicate).hash(state);
            match predicate {
                BagPred::True | BagPred::False => {}
                BagPred::Count { class, lo, hi } => {
                    class.hash(state);
                    lo.hash(state);
                    hi.hash(state);
                }
                BagPred::And(left, right) | BagPred::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                BagPred::Not(inner) => pending.push(inner),
            }
        }
    }
}

impl<P: fmt::Debug> fmt::Debug for BagPred<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a, P> {
            Pred(&'a BagPred<P>),
            Text(&'static str),
        }
        let mut events = vec![Event::Pred(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Pred(predicate) => match predicate {
                    BagPred::True => write!(f, "True")?,
                    BagPred::False => write!(f, "False")?,
                    BagPred::Count { class, lo, hi } => {
                        write!(f, "Count {{ class: {class:?}, lo: {lo:?}, hi: {hi:?} }}")?;
                    }
                    BagPred::And(left, right) | BagPred::Or(left, right) => {
                        write!(
                            f,
                            "{}(",
                            if matches!(predicate, BagPred::And(_, _)) {
                                "And"
                            } else {
                                "Or"
                            }
                        )?;
                        events.push(Event::Text(")"));
                        events.push(Event::Pred(right));
                        events.push(Event::Text(", "));
                        events.push(Event::Pred(left));
                    }
                    BagPred::Not(inner) => {
                        write!(f, "Not(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Pred(inner));
                    }
                },
            }
        }
        Ok(())
    }
}

impl<P> Drop for BagPred<P> {
    fn drop(&mut self) {
        fn drain<P>(predicate: &mut BagPred<P>, pending: &mut Vec<BagPred<P>>) {
            match predicate {
                BagPred::And(left, right) | BagPred::Or(left, right) => {
                    pending.push(std::mem::replace(&mut **right, BagPred::True));
                    pending.push(std::mem::replace(&mut **left, BagPred::True));
                }
                BagPred::Not(inner) => {
                    pending.push(std::mem::replace(&mut **inner, BagPred::True));
                }
                BagPred::True | BagPred::False | BagPred::Count { .. } => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut predicate) = pending.pop() {
            drain(&mut predicate, &mut pending);
        }
    }
}

/// The effective Boolean algebra of multisets (bags) over an element algebra.
#[derive(Clone, Debug)]
pub struct BagAlgebra<A: BooleanAlgebra> {
    /// The element algebra.
    pub elem: A,
}

impl<A: BooleanAlgebra> BagAlgebra<A> {
    /// Construct the bag algebra over the given element algebra.
    pub fn new(elem: A) -> Self {
        BagAlgebra { elem }
    }

    /// $`\forall e\in\mathrm{bag}.\,e\models p`$—equivalently, zero
    /// elements satisfy $`\lnot p`$.
    pub fn all(&self, p: A::Predicate) -> BagPred<A::Predicate> {
        BagPred::Count {
            class: self.elem.not(&p),
            lo: 0,
            hi: Some(0),
        }
    }

    /// $`\exists e\in\mathrm{bag}.\,e\models p`$—at least one element
    /// satisfies $`p`$.
    pub fn any_elem(&self, p: A::Predicate) -> BagPred<A::Predicate> {
        BagPred::Count {
            class: p,
            lo: 1,
            hi: None,
        }
    }

    /// $`\mathrm{lo}\le\lvert\mathrm{bag}\rvert\le\mathrm{hi}`$
    /// (total cardinality).
    pub fn size(&self, lo: u64, hi: Option<u64>) -> BagPred<A::Predicate> {
        BagPred::Count {
            class: self.elem.true_pred(),
            lo,
            hi,
        }
    }

    /// Collect every distinct `class` appearing in `Count` atoms.
    fn collect_classes(&self, pred: &BagPred<A::Predicate>, out: &mut Vec<A::Predicate>) {
        let mut pending = vec![pred];
        while let Some(predicate) = pending.pop() {
            match predicate {
                BagPred::Count { class, .. } => {
                    if !out.iter().any(|candidate| candidate == class) {
                        out.push(class.clone());
                    }
                }
                BagPred::And(left, right) | BagPred::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                BagPred::Not(inner) => pending.push(inner),
                BagPred::True | BagPred::False => {}
            }
        }
    }

    /// For each `Count` class, the indices of the minterms it covers.
    fn covering(
        &self,
        classes: &[A::Predicate],
        ms: &[A::Predicate],
    ) -> HashMap<A::Predicate, Vec<usize>> {
        let mut map = HashMap::new();
        for class in classes {
            let cover: Vec<usize> = ms
                .iter()
                .enumerate()
                // A minterm is atomic w.r.t. `class`: it is wholly inside or
                // wholly outside. Inside iff (minterm ∧ class) is satisfiable.
                .filter(|(_, m)| self.elem.is_satisfiable(&self.elem.and(m, class)))
                .map(|(i, _)| i)
                .collect();
            map.insert(class.clone(), cover);
        }
        map
    }

    /// Evaluate the predicate against a per-minterm count vector.
    fn eval_counts(
        &self,
        pred: &BagPred<A::Predicate>,
        counts: &[u64],
        cover: &HashMap<A::Predicate, Vec<usize>>,
    ) -> bool {
        enum Frame<'a, P> {
            Eval(&'a BagPred<P>),
            Not,
            AndRight(&'a BagPred<P>),
            OrRight(&'a BagPred<P>),
        }
        let mut frames = vec![Frame::Eval(pred)];
        let mut result = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Eval(predicate) => match predicate {
                    BagPred::True => result = Some(true),
                    BagPred::False => result = Some(false),
                    BagPred::Count { class, lo, hi } => {
                        let sum: u64 = cover
                            .get(class)
                            .map(|indices| indices.iter().map(|&index| counts[index]).sum())
                            .unwrap_or(0);
                        result = Some(sum >= *lo && hi.map(|bound| sum <= bound).unwrap_or(true));
                    }
                    BagPred::And(left, right) => {
                        frames.push(Frame::AndRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    BagPred::Or(left, right) => {
                        frames.push(Frame::OrRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    BagPred::Not(inner) => {
                        frames.push(Frame::Not);
                        frames.push(Frame::Eval(inner));
                    }
                },
                Frame::Not => result = Some(!result.take().expect("negated count is evaluated")),
                Frame::AndRight(right) => {
                    if result.take().expect("left count conjunction is evaluated") {
                        frames.push(Frame::Eval(right));
                    } else {
                        result = Some(false);
                    }
                }
                Frame::OrRight(right) => {
                    if result.take().expect("left count disjunction is evaluated") {
                        result = Some(true);
                    } else {
                        frames.push(Frame::Eval(right));
                    }
                }
            }
        }
        result.expect("the root bag predicate produces one result")
    }

    /// The smallest count cap $`B`$ such that counts $`\ge B`$ are
    /// indistinguishable from $`B`$ for every atom
    /// ($`1+\text{maximum threshold}`$).
    fn count_cap(&self, pred: &BagPred<A::Predicate>) -> u64 {
        let mut acc = 0;
        let mut pending = vec![pred];
        while let Some(predicate) = pending.pop() {
            match predicate {
                BagPred::Count { lo, hi, .. } => {
                    acc = acc.max(*lo);
                    if let Some(h) = hi {
                        acc = acc.max(*h);
                    }
                }
                BagPred::And(left, right) | BagPred::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                BagPred::Not(inner) => pending.push(inner),
                BagPred::True | BagPred::False => {}
            }
        }
        acc + 1
    }

    /// Search for a feasible count vector; returns it if one exists.
    fn feasible_counts(
        &self,
        pred: &BagPred<A::Predicate>,
    ) -> Option<(Vec<A::Predicate>, Vec<u64>)> {
        let mut classes = Vec::new();
        self.collect_classes(pred, &mut classes);
        let ms = minterms(&self.elem, &classes);
        let cover = self.covering(&classes, &ms);
        let k = ms.len();
        let cap = self.count_cap(pred);

        // Bounded search over [0, cap]^k.
        let mut counts = vec![0u64; k];
        loop {
            if self.eval_counts(pred, &counts, &cover) {
                return Some((ms, counts));
            }
            // Increment the mixed-radix counter (base cap+1).
            let mut i = 0;
            loop {
                if i == k {
                    return None; // exhausted
                }
                if counts[i] < cap {
                    counts[i] += 1;
                    break;
                }
                counts[i] = 0;
                i += 1;
            }
        }
    }
}

impl<A: BooleanAlgebra> BooleanAlgebra for BagAlgebra<A> {
    type Predicate = BagPred<A::Predicate>;
    type Domain = Vec<A::Domain>;

    fn true_pred(&self) -> Self::Predicate {
        BagPred::True
    }

    fn false_pred(&self) -> Self::Predicate {
        BagPred::False
    }

    fn and(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        match (a, b) {
            (BagPred::False, _) | (_, BagPred::False) => BagPred::False,
            (BagPred::True, x) | (x, BagPred::True) => x.clone(),
            _ => BagPred::And(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn or(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        match (a, b) {
            (BagPred::True, _) | (_, BagPred::True) => BagPred::True,
            (BagPred::False, x) | (x, BagPred::False) => x.clone(),
            _ => BagPred::Or(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn not(&self, a: &Self::Predicate) -> Self::Predicate {
        BagPred::Not(Box::new(a.clone()))
    }

    fn is_satisfiable(&self, a: &Self::Predicate) -> bool {
        self.feasible_counts(a).is_some()
    }

    fn witness(&self, a: &Self::Predicate) -> Option<Self::Domain> {
        let (ms, counts) = self.feasible_counts(a)?;
        let mut bag = Vec::new();
        for (i, &count) in counts.iter().enumerate() {
            if count == 0 {
                continue;
            }
            let elem = self.elem.witness(&ms[i])?;
            for _ in 0..count {
                bag.push(elem.clone());
            }
        }
        Some(bag)
    }

    fn evaluate(&self, pred: &Self::Predicate, elem: &Self::Domain) -> bool {
        enum Frame<'a, P> {
            Eval(&'a BagPred<P>),
            Not,
            AndRight(&'a BagPred<P>),
            OrRight(&'a BagPred<P>),
        }
        let mut frames = vec![Frame::Eval(pred)];
        let mut result = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Eval(predicate) => match predicate {
                    BagPred::True => result = Some(true),
                    BagPred::False => result = Some(false),
                    BagPred::Count { class, lo, hi } => {
                        let count = elem
                            .iter()
                            .filter(|value| self.elem.evaluate(class, value))
                            .count() as u64;
                        result =
                            Some(count >= *lo && hi.map(|bound| count <= bound).unwrap_or(true));
                    }
                    BagPred::And(left, right) => {
                        frames.push(Frame::AndRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    BagPred::Or(left, right) => {
                        frames.push(Frame::OrRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    BagPred::Not(inner) => {
                        frames.push(Frame::Not);
                        frames.push(Frame::Eval(inner));
                    }
                },
                Frame::Not => result = Some(!result.take().expect("negated bag is evaluated")),
                Frame::AndRight(right) => {
                    if result.take().expect("left bag conjunction is evaluated") {
                        frames.push(Frame::Eval(right));
                    } else {
                        result = Some(false);
                    }
                }
                Frame::OrRight(right) => {
                    if result.take().expect("left bag disjunction is evaluated") {
                        result = Some(true);
                    } else {
                        frames.push(Frame::Eval(right));
                    }
                }
            }
        }
        result.expect("the root bag predicate produces one result")
    }
}

#[cfg(test)]
mod tests {
    use super::super::{IntervalAlgebra, IntervalPred};
    use super::*;

    fn bag_alg() -> BagAlgebra<IntervalAlgebra> {
        BagAlgebra::new(IntervalAlgebra::new(0, 100))
    }

    #[test]
    fn count_and_size() {
        let alg = bag_alg();
        // at least 2 elements in [50,100)
        let p = BagPred::Count {
            class: IntervalPred::Range(50, 100),
            lo: 2,
            hi: None,
        };
        assert!(alg.evaluate(&p, &vec![60, 70, 1]));
        assert!(!alg.evaluate(&p, &vec![60, 1]));
        assert!(alg.is_satisfiable(&p));
        let w = alg.witness(&p).expect("nonempty");
        assert!(alg.evaluate(&p, &w));
    }

    #[test]
    fn all_and_any() {
        let alg = bag_alg();
        let all_small = alg.all(IntervalPred::Range(0, 10));
        assert!(alg.evaluate(&all_small, &vec![])); // vacuous
        assert!(alg.evaluate(&all_small, &vec![1, 5, 9]));
        assert!(!alg.evaluate(&all_small, &vec![1, 50]));

        let any_big = alg.any_elem(IntervalPred::Range(50, 100));
        assert!(!alg.evaluate(&any_big, &vec![]));
        assert!(!alg.evaluate(&any_big, &vec![1, 2]));
        assert!(alg.evaluate(&any_big, &vec![1, 60]));
        assert!(alg.is_satisfiable(&any_big));
    }

    #[test]
    fn overlapping_classes_minterm_feasibility() {
        let alg = bag_alg();
        // at least 1 in [0,60) AND at least 1 in [40,100); the overlap [40,60)
        // could satisfy both with a single element, or two disjoint elements.
        let p = alg.and(
            &BagPred::Count {
                class: IntervalPred::Range(0, 60),
                lo: 1,
                hi: None,
            },
            &BagPred::Count {
                class: IntervalPred::Range(40, 100),
                lo: 1,
                hi: None,
            },
        );
        assert!(alg.is_satisfiable(&p));
        assert!(alg.evaluate(&p, &vec![50])); // single element covers both
        assert!(alg.evaluate(&p, &vec![10, 80])); // two disjoint
        assert!(!alg.evaluate(&p, &vec![10])); // misses [40,100)
        let w = alg.witness(&p).expect("nonempty");
        assert!(alg.evaluate(&p, &w));
    }

    #[test]
    fn unsatisfiable_count() {
        let alg = bag_alg();
        // exactly 0 AND at least 1 in the same class → unsat
        let p = alg.and(
            &BagPred::Count {
                class: IntervalPred::Range(0, 100),
                lo: 0,
                hi: Some(0),
            },
            &BagPred::Count {
                class: IntervalPred::Range(0, 100),
                lo: 1,
                hi: None,
            },
        );
        assert!(!alg.is_satisfiable(&p));
    }

    #[test]
    fn negation() {
        let alg = bag_alg();
        let any_big = alg.any_elem(IntervalPred::Range(50, 100));
        let none_big = alg.not(&any_big);
        assert!(alg.evaluate(&none_big, &vec![1, 2, 3]));
        assert!(!alg.evaluate(&none_big, &vec![1, 60]));
        assert!(!alg.is_satisfiable(&alg.and(&any_big, &none_big)));
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Singleton — point predicates (needed to materialize distinct map keys)
// ══════════════════════════════════════════════════════════════════════════════

/// An algebra that can express a **point predicate**: one satisfied only by a
/// given element. Required of map *key* algebras so distinct keys can be
/// generated by witness-with-exclusion.
pub trait Singleton: BooleanAlgebra {
    /// A predicate satisfied exactly by `value`.
    fn point(&self, value: &Self::Domain) -> Self::Predicate;
}

impl Singleton for IntervalAlgebra {
    fn point(&self, value: &i64) -> IntervalPred {
        // {v} = [v, v+1); v < max_val <= i64::MAX for in-universe keys.
        IntervalPred::Range(*value, value.saturating_add(1))
    }
}

impl Singleton for CharClassAlgebra {
    fn point(&self, value: &char) -> CharClassPred {
        CharClassPred::Range(*value, *value)
    }
}

impl<P: OrderedPoint> Singleton for OrderedFieldAlgebra<P> {
    fn point(&self, value: &P) -> OrderedFieldPred<P> {
        OrderedFieldPred::point(value.clone())
    }
}

impl Singleton for StringAlgebra {
    fn point(&self, value: &String) -> StrPred {
        StrPred::Literal(value.clone())
    }
}

impl Singleton for super::KatBooleanAlgebra {
    fn point(&self, value: &HashMap<String, bool>) -> BooleanTest {
        let mut acc = BooleanTest::True;
        for atom in &self.atoms {
            let lit = if *value.get(atom).unwrap_or(&false) {
                BooleanTest::Atom(atom.clone())
            } else {
                BooleanTest::Not(Box::new(BooleanTest::Atom(atom.clone())))
            };
            acc = match acc {
                BooleanTest::True => lit,
                other => BooleanTest::And(Box::new(other), Box::new(lit)),
            };
        }
        acc
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// MapAlgebra
// ══════════════════════════════════════════════════════════════════════════════

/// A map predicate: a boolean combination of entry-cardinality atoms over a key
/// predicate type `KP` and value predicate type `VP`.
pub enum MapPred<KP, VP> {
    /// Satisfied by every map.
    True,
    /// Satisfied by no map.
    False,
    /// ```math
    /// \mathrm{lo}\le
    /// \#\{(k,v):k\models\mathrm{key\_class}\land
    /// v\models\mathrm{val\_class}\}\le\mathrm{hi}
    /// ```
    CountEntries {
        key_class: KP,
        val_class: VP,
        lo: u64,
        hi: Option<u64>,
    },
    /// Conjunction.
    And(Box<MapPred<KP, VP>>, Box<MapPred<KP, VP>>),
    /// Disjunction.
    Or(Box<MapPred<KP, VP>>, Box<MapPred<KP, VP>>),
    /// Negation.
    Not(Box<MapPred<KP, VP>>),
}

impl<KP: Clone, VP: Clone> Clone for MapPred<KP, VP> {
    fn clone(&self) -> Self {
        enum Task<'a, KP, VP> {
            Clone(&'a MapPred<KP, VP>),
            And,
            Or,
            Not,
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(predicate) => match predicate {
                    MapPred::True => values.push(MapPred::True),
                    MapPred::False => values.push(MapPred::False),
                    MapPred::CountEntries {
                        key_class,
                        val_class,
                        lo,
                        hi,
                    } => values.push(MapPred::CountEntries {
                        key_class: key_class.clone(),
                        val_class: val_class.clone(),
                        lo: *lo,
                        hi: *hi,
                    }),
                    MapPred::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    MapPred::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    MapPred::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Clone(inner));
                    }
                },
                Task::And | Task::Or => {
                    let right = values.pop().expect("right map clone is present");
                    let left = values.pop().expect("left map clone is present");
                    values.push(if matches!(task, Task::And) {
                        MapPred::And(Box::new(left), Box::new(right))
                    } else {
                        MapPred::Or(Box::new(left), Box::new(right))
                    });
                }
                Task::Not => {
                    let inner = values.pop().expect("negated map clone is present");
                    values.push(MapPred::Not(Box::new(inner)));
                }
            }
        }
        values
            .pop()
            .expect("the root map predicate produces one clone")
    }
}

impl<KP: PartialEq, VP: PartialEq> PartialEq for MapPred<KP, VP> {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (MapPred::True, MapPred::True) | (MapPred::False, MapPred::False) => {}
                (
                    MapPred::CountEntries {
                        key_class: lk,
                        val_class: lv,
                        lo: ll,
                        hi: lh,
                    },
                    MapPred::CountEntries {
                        key_class: rk,
                        val_class: rv,
                        lo: rl,
                        hi: rh,
                    },
                ) if lk == rk && lv == rv && ll == rl && lh == rh => {}
                (MapPred::And(ll, lr), MapPred::And(rl, rr))
                | (MapPred::Or(ll, lr), MapPred::Or(rl, rr)) => {
                    pending.push((lr, rr));
                    pending.push((ll, rl));
                }
                (MapPred::Not(left), MapPred::Not(right)) => pending.push((left, right)),
                _ => return false,
            }
        }
        true
    }
}

impl<KP: Eq, VP: Eq> Eq for MapPred<KP, VP> {}

impl<KP: Hash, VP: Hash> Hash for MapPred<KP, VP> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut pending = vec![self];
        while let Some(predicate) = pending.pop() {
            std::mem::discriminant(predicate).hash(state);
            match predicate {
                MapPred::True | MapPred::False => {}
                MapPred::CountEntries {
                    key_class,
                    val_class,
                    lo,
                    hi,
                } => {
                    key_class.hash(state);
                    val_class.hash(state);
                    lo.hash(state);
                    hi.hash(state);
                }
                MapPred::And(left, right) | MapPred::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                MapPred::Not(inner) => pending.push(inner),
            }
        }
    }
}

impl<KP: fmt::Debug, VP: fmt::Debug> fmt::Debug for MapPred<KP, VP> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a, KP, VP> {
            Pred(&'a MapPred<KP, VP>),
            Text(&'static str),
        }
        let mut events = vec![Event::Pred(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Pred(predicate) => match predicate {
                    MapPred::True => write!(f, "True")?,
                    MapPred::False => write!(f, "False")?,
                    MapPred::CountEntries {
                        key_class,
                        val_class,
                        lo,
                        hi,
                    } => write!(
                        f,
                        "CountEntries {{ key_class: {key_class:?}, val_class: {val_class:?}, lo: {lo:?}, hi: {hi:?} }}"
                    )?,
                    MapPred::And(left, right) | MapPred::Or(left, right) => {
                        write!(
                            f,
                            "{}(",
                            if matches!(predicate, MapPred::And(_, _)) {
                                "And"
                            } else {
                                "Or"
                            }
                        )?;
                        events.push(Event::Text(")"));
                        events.push(Event::Pred(right));
                        events.push(Event::Text(", "));
                        events.push(Event::Pred(left));
                    }
                    MapPred::Not(inner) => {
                        write!(f, "Not(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Pred(inner));
                    }
                },
            }
        }
        Ok(())
    }
}

impl<KP, VP> Drop for MapPred<KP, VP> {
    fn drop(&mut self) {
        fn drain<KP, VP>(predicate: &mut MapPred<KP, VP>, pending: &mut Vec<MapPred<KP, VP>>) {
            match predicate {
                MapPred::And(left, right) | MapPred::Or(left, right) => {
                    pending.push(std::mem::replace(&mut **right, MapPred::True));
                    pending.push(std::mem::replace(&mut **left, MapPred::True));
                }
                MapPred::Not(inner) => {
                    pending.push(std::mem::replace(&mut **inner, MapPred::True));
                }
                MapPred::True | MapPred::False | MapPred::CountEntries { .. } => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut predicate) = pending.pop() {
            drain(&mut predicate, &mut pending);
        }
    }
}

type FeasibleMapCounts<KP, VP> = (Vec<KP>, Vec<VP>, Vec<Vec<u64>>);

/// The effective Boolean algebra of finite key→value maps (unique keys).
#[derive(Clone, Debug)]
pub struct MapAlgebra<K: Singleton, V: BooleanAlgebra> {
    /// The key algebra (must support point predicates).
    pub key: K,
    /// The value algebra.
    pub val: V,
}

impl<K: Singleton, V: BooleanAlgebra> MapAlgebra<K, V> {
    /// Construct the map algebra.
    pub fn new(key: K, val: V) -> Self {
        MapAlgebra { key, val }
    }

    /// $`\mathrm{lo}\le\lvert\mathrm{map}\rvert\le\mathrm{hi}`$.
    pub fn size(&self, lo: u64, hi: Option<u64>) -> MapPred<K::Predicate, V::Predicate> {
        MapPred::CountEntries {
            key_class: self.key.true_pred(),
            val_class: self.val.true_pred(),
            lo,
            hi,
        }
    }

    /// $`\exists(k,\_)\in\mathrm{map}.\,k\models kp`$.
    pub fn has_key(&self, kp: K::Predicate) -> MapPred<K::Predicate, V::Predicate> {
        MapPred::CountEntries {
            key_class: kp,
            val_class: self.val.true_pred(),
            lo: 1,
            hi: None,
        }
    }

    /// $`\exists(k,v)\in\mathrm{map}.\,k\models kp\land v\models vp`$.
    pub fn entry(&self, kp: K::Predicate, vp: V::Predicate) -> MapPred<K::Predicate, V::Predicate> {
        MapPred::CountEntries {
            key_class: kp,
            val_class: vp,
            lo: 1,
            hi: None,
        }
    }

    /// $`\forall(\_,v)\in\mathrm{map}.\,v\models vp`$; no entry has a
    /// value satisfying $`\lnot vp`$.
    pub fn all_values(&self, vp: V::Predicate) -> MapPred<K::Predicate, V::Predicate> {
        MapPred::CountEntries {
            key_class: self.key.true_pred(),
            val_class: self.val.not(&vp),
            lo: 0,
            hi: Some(0),
        }
    }

    fn collect_classes(
        &self,
        pred: &MapPred<K::Predicate, V::Predicate>,
        keys: &mut Vec<K::Predicate>,
        vals: &mut Vec<V::Predicate>,
    ) {
        let mut pending = vec![pred];
        while let Some(predicate) = pending.pop() {
            match predicate {
                MapPred::CountEntries {
                    key_class,
                    val_class,
                    ..
                } => {
                    if !keys.iter().any(|candidate| candidate == key_class) {
                        keys.push(key_class.clone());
                    }
                    if !vals.iter().any(|candidate| candidate == val_class) {
                        vals.push(val_class.clone());
                    }
                }
                MapPred::And(left, right) | MapPred::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                MapPred::Not(inner) => pending.push(inner),
                MapPred::True | MapPred::False => {}
            }
        }
    }

    fn count_cap(&self, pred: &MapPred<K::Predicate, V::Predicate>) -> u64 {
        let mut acc = 0;
        let mut pending = vec![pred];
        while let Some(predicate) = pending.pop() {
            match predicate {
                MapPred::CountEntries { lo, hi, .. } => {
                    acc = acc.max(*lo);
                    if let Some(h) = hi {
                        acc = acc.max(*h);
                    }
                }
                MapPred::And(left, right) | MapPred::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                MapPred::Not(inner) => pending.push(inner),
                MapPred::True | MapPred::False => {}
            }
        }
        acc + 1
    }

    /// Up to `cap` distinct keys drawn from `key_minterm` (witness-with-exclusion).
    fn distinct_keys(&self, key_minterm: &K::Predicate, cap: u64) -> Vec<K::Domain> {
        let mut keys = Vec::new();
        let mut remaining = key_minterm.clone();
        for _ in 0..cap {
            match self.key.witness(&remaining) {
                Some(k) => {
                    remaining = self.key.and(&remaining, &self.key.not(&self.key.point(&k)));
                    keys.push(k);
                }
                None => break,
            }
        }
        keys
    }

    /// Evaluate the predicate against per-(key-minterm, value-minterm) counts.
    fn eval_counts(
        &self,
        pred: &MapPred<K::Predicate, V::Predicate>,
        counts: &[Vec<u64>],
        key_ms: &[K::Predicate],
        val_ms: &[V::Predicate],
    ) -> bool {
        enum Frame<'a, KP, VP> {
            Eval(&'a MapPred<KP, VP>),
            Not,
            AndRight(&'a MapPred<KP, VP>),
            OrRight(&'a MapPred<KP, VP>),
        }
        let mut frames = vec![Frame::Eval(pred)];
        let mut result = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Eval(predicate) => match predicate {
                    MapPred::True => result = Some(true),
                    MapPred::False => result = Some(false),
                    MapPred::CountEntries {
                        key_class,
                        val_class,
                        lo,
                        hi,
                    } => {
                        let mut sum = 0u64;
                        for (i, key_minterm) in key_ms.iter().enumerate() {
                            if !self
                                .key
                                .is_satisfiable(&self.key.and(key_minterm, key_class))
                            {
                                continue;
                            }
                            for (j, value_minterm) in val_ms.iter().enumerate() {
                                if self
                                    .val
                                    .is_satisfiable(&self.val.and(value_minterm, val_class))
                                {
                                    sum += counts[i][j];
                                }
                            }
                        }
                        result = Some(sum >= *lo && hi.map(|bound| sum <= bound).unwrap_or(true));
                    }
                    MapPred::And(left, right) => {
                        frames.push(Frame::AndRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    MapPred::Or(left, right) => {
                        frames.push(Frame::OrRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    MapPred::Not(inner) => {
                        frames.push(Frame::Not);
                        frames.push(Frame::Eval(inner));
                    }
                },
                Frame::Not => {
                    result = Some(!result.take().expect("negated map count is evaluated"))
                }
                Frame::AndRight(right) => {
                    if result
                        .take()
                        .expect("left map count conjunction is evaluated")
                    {
                        frames.push(Frame::Eval(right));
                    } else {
                        result = Some(false);
                    }
                }
                Frame::OrRight(right) => {
                    if result
                        .take()
                        .expect("left map count disjunction is evaluated")
                    {
                        result = Some(true);
                    } else {
                        frames.push(Frame::Eval(right));
                    }
                }
            }
        }
        result.expect("the root map predicate produces one result")
    }

    /// Search for a feasible (key-minterm × value-minterm) count matrix honoring
    /// the per-key-region distinct-key cap. Returns the matrix and the minterms.
    fn feasible(
        &self,
        pred: &MapPred<K::Predicate, V::Predicate>,
    ) -> Option<FeasibleMapCounts<K::Predicate, V::Predicate>> {
        let mut key_classes = Vec::new();
        let mut val_classes = Vec::new();
        self.collect_classes(pred, &mut key_classes, &mut val_classes);
        let key_ms = minterms(&self.key, &key_classes);
        let val_ms = minterms(&self.val, &val_classes);
        let cap = self.count_cap(pred);
        let ki = key_ms.len();
        let vj = val_ms.len();

        // Distinct-key availability per key-minterm (capped at `cap`).
        let avail: Vec<u64> = key_ms
            .iter()
            .map(|km| self.distinct_keys(km, cap).len() as u64)
            .collect();

        // Bounded search over the ki×vj count matrix (each cell in [0, cap]),
        // pruned by the per-key-region distinct-key cap.
        let total_cells = ki * vj;
        let mut flat = vec![0u64; total_cells];
        loop {
            // Reshape and check the per-key-region cap.
            let matrix: Vec<Vec<u64>> = (0..ki)
                .map(|i| flat[i * vj..(i + 1) * vj].to_vec())
                .collect();
            let cap_ok = (0..ki).all(|i| {
                let used: u64 = matrix[i].iter().sum();
                used <= avail[i]
            });
            if cap_ok && self.eval_counts(pred, &matrix, &key_ms, &val_ms) {
                return Some((key_ms, val_ms, matrix));
            }
            // Increment the mixed-radix counter (base cap+1).
            let mut idx = 0;
            loop {
                if idx == total_cells {
                    return None;
                }
                if flat[idx] < cap {
                    flat[idx] += 1;
                    break;
                }
                flat[idx] = 0;
                idx += 1;
            }
        }
    }
}

impl<K: Singleton, V: BooleanAlgebra> BooleanAlgebra for MapAlgebra<K, V> {
    type Predicate = MapPred<K::Predicate, V::Predicate>;
    type Domain = Vec<(K::Domain, V::Domain)>;

    fn true_pred(&self) -> Self::Predicate {
        MapPred::True
    }

    fn false_pred(&self) -> Self::Predicate {
        MapPred::False
    }

    fn and(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        match (a, b) {
            (MapPred::False, _) | (_, MapPred::False) => MapPred::False,
            (MapPred::True, x) | (x, MapPred::True) => x.clone(),
            _ => MapPred::And(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn or(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        match (a, b) {
            (MapPred::True, _) | (_, MapPred::True) => MapPred::True,
            (MapPred::False, x) | (x, MapPred::False) => x.clone(),
            _ => MapPred::Or(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn not(&self, a: &Self::Predicate) -> Self::Predicate {
        MapPred::Not(Box::new(a.clone()))
    }

    fn is_satisfiable(&self, a: &Self::Predicate) -> bool {
        self.feasible(a).is_some()
    }

    fn witness(&self, a: &Self::Predicate) -> Option<Self::Domain> {
        let (key_ms, val_ms, matrix) = self.feasible(a)?;
        let mut map = Vec::new();
        for (i, km) in key_ms.iter().enumerate() {
            let needed: u64 = matrix[i].iter().sum();
            if needed == 0 {
                continue;
            }
            let keys = self.distinct_keys(km, needed);
            if (keys.len() as u64) < needed {
                return None;
            }
            let mut key_iter = keys.into_iter();
            for (j, vm) in val_ms.iter().enumerate() {
                for _ in 0..matrix[i][j] {
                    let k = key_iter.next()?;
                    let v = self.val.witness(vm)?;
                    map.push((k, v));
                }
            }
        }
        Some(map)
    }

    fn evaluate(&self, pred: &Self::Predicate, elem: &Self::Domain) -> bool {
        enum Frame<'a, KP, VP> {
            Eval(&'a MapPred<KP, VP>),
            Not,
            AndRight(&'a MapPred<KP, VP>),
            OrRight(&'a MapPred<KP, VP>),
        }
        let mut frames = vec![Frame::Eval(pred)];
        let mut result = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Eval(predicate) => match predicate {
                    MapPred::True => result = Some(true),
                    MapPred::False => result = Some(false),
                    MapPred::CountEntries {
                        key_class,
                        val_class,
                        lo,
                        hi,
                    } => {
                        let count = elem
                            .iter()
                            .filter(|(key, value)| {
                                self.key.evaluate(key_class, key)
                                    && self.val.evaluate(val_class, value)
                            })
                            .count() as u64;
                        result =
                            Some(count >= *lo && hi.map(|bound| count <= bound).unwrap_or(true));
                    }
                    MapPred::And(left, right) => {
                        frames.push(Frame::AndRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    MapPred::Or(left, right) => {
                        frames.push(Frame::OrRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    MapPred::Not(inner) => {
                        frames.push(Frame::Not);
                        frames.push(Frame::Eval(inner));
                    }
                },
                Frame::Not => result = Some(!result.take().expect("negated map is evaluated")),
                Frame::AndRight(right) => {
                    if result.take().expect("left map conjunction is evaluated") {
                        frames.push(Frame::Eval(right));
                    } else {
                        result = Some(false);
                    }
                }
                Frame::OrRight(right) => {
                    if result.take().expect("left map disjunction is evaluated") {
                        result = Some(true);
                    } else {
                        frames.push(Frame::Eval(right));
                    }
                }
            }
        }
        result.expect("the root map predicate produces one result")
    }
}

#[cfg(test)]
mod map_tests {
    use super::super::{IntervalAlgebra, IntervalPred};
    use super::*;

    fn map_alg() -> MapAlgebra<IntervalAlgebra, IntervalAlgebra> {
        MapAlgebra::new(IntervalAlgebra::new(0, 1000), IntervalAlgebra::new(0, 1000))
    }

    #[test]
    fn entry_and_has_key() {
        let alg = map_alg();
        let p = alg.entry(IntervalPred::Range(0, 10), IntervalPred::Range(100, 200));
        assert!(alg.evaluate(&p, &vec![(5, 150)]));
        assert!(!alg.evaluate(&p, &vec![(5, 50)])); // value out of range
        assert!(!alg.evaluate(&p, &vec![(50, 150)])); // key out of range
        assert!(alg.is_satisfiable(&p));
        let w = alg.witness(&p).expect("nonempty");
        assert!(alg.evaluate(&p, &w));
    }

    #[test]
    fn size_and_all_values() {
        let alg = map_alg();
        let p = alg.and(
            &alg.size(2, Some(2)),
            &alg.all_values(IntervalPred::Range(0, 10)),
        );
        assert!(alg.evaluate(&p, &vec![(1, 5), (2, 9)]));
        assert!(!alg.evaluate(&p, &vec![(1, 5)])); // size 1
        assert!(!alg.evaluate(&p, &vec![(1, 5), (2, 50)])); // value out of range
        assert!(alg.is_satisfiable(&p));
        let w = alg.witness(&p).expect("nonempty");
        assert!(alg.evaluate(&p, &w));
        assert_eq!(w.len(), 2);
        // witness keys are distinct
        assert_ne!(w[0].0, w[1].0);
    }

    #[test]
    fn distinct_key_cap_on_finite_key_region() {
        // Only 2 distinct keys exist in [0,2); demanding 3 entries there is unsat.
        let alg = MapAlgebra::new(IntervalAlgebra::new(0, 1000), IntervalAlgebra::new(0, 1000));
        let p = MapPred::CountEntries {
            key_class: IntervalPred::Range(0, 2),
            val_class: IntervalPred::True,
            lo: 3,
            hi: None,
        };
        assert!(!alg.is_satisfiable(&p));
        // demanding 2 is satisfiable
        let p2 = MapPred::CountEntries {
            key_class: IntervalPred::Range(0, 2),
            val_class: IntervalPred::True,
            lo: 2,
            hi: None,
        };
        assert!(alg.is_satisfiable(&p2));
        let w = alg.witness(&p2).expect("nonempty");
        assert_eq!(w.len(), 2);
        assert_ne!(w[0].0, w[1].0);
    }

    #[test]
    fn map_negation() {
        let alg = map_alg();
        let has = alg.has_key(IntervalPred::Range(0, 10));
        let not_has = alg.not(&has);
        assert!(alg.evaluate(&not_has, &vec![(50, 1)]));
        assert!(!alg.evaluate(&not_has, &vec![(5, 1)]));
        assert!(!alg.is_satisfiable(&alg.and(&has, &not_has)));
    }
}
