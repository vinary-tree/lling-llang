//! Generic symbolic-regex engine: an effective Boolean algebra of **symbolic
//! regular languages over any element algebra** `A: BooleanAlgebra`.
//!
//! A [`RegexPred<P>`] (with `P = A::Predicate`) is a symbolic regex whose
//! character class is an element predicate of `A`. It compiles — via a Thompson
//! epsilon-NFA, epsilon-eliminated — to a [`SymbolicAutomaton<A>`], so the
//! decision procedures are exact regular-language operations:
//!
//! - `and`/`or`/`not` = `Inter`/`Alt`/`Compl`, realized by the SFA's
//!   `intersect`/`union`/`complement`;
//! - `is_satisfiable` = SFA non-emptiness;
//! - `witness` = shortest accepted word ([`SymbolicAutomaton::shortest_accepted`]);
//! - `evaluate(p, xs)` = SFA simulation on the sequence `xs`.
//!
//! [`RegexAlgebra<A>`] is therefore the **list algebra**: its domain is
//! `Vec<A::Domain>` (sequences of elements). It is what the string algebra
//! ([`crate::string_algebra`]) instantiates at `A = CharClassAlgebra`, and what
//! the collection layer uses for `List`. Bags/maps (order-insensitive) use a
//! separate multiset model.

use std::collections::HashSet;
use std::fmt::{self, Debug};
use std::hash::{Hash, Hasher};

use super::{BooleanAlgebra, SymbolicAutomaton};

// ══════════════════════════════════════════════════════════════════════════════
// RegexPred — symbolic regex over element predicates of type P
// ══════════════════════════════════════════════════════════════════════════════

/// A symbolic regular expression whose character class is an element predicate
/// `P` (`= A::Predicate`).
pub enum RegexPred<P> {
    /// `∅` — matches no sequence.
    Empty,
    /// `{ [] }` — matches only the empty sequence.
    Epsilon,
    /// One element drawn from the element predicate.
    Elem(P),
    /// A length constraint `lo ≤ len ≤ hi` (`hi = None` is unbounded above).
    Length(usize, Option<usize>),
    /// Concatenation.
    Concat(Box<RegexPred<P>>, Box<RegexPred<P>>),
    /// Alternation (union).
    Alt(Box<RegexPred<P>>, Box<RegexPred<P>>),
    /// Kleene star.
    Star(Box<RegexPred<P>>),
    /// Intersection.
    Inter(Box<RegexPred<P>>, Box<RegexPred<P>>),
    /// Complement (relative to `Σ*`).
    Compl(Box<RegexPred<P>>),
}

impl<P: Clone> Clone for RegexPred<P> {
    fn clone(&self) -> Self {
        enum Task<'a, P> {
            Clone(&'a RegexPred<P>),
            Concat,
            Alt,
            Star,
            Inter,
            Compl,
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(predicate) => match predicate {
                    RegexPred::Empty => values.push(RegexPred::Empty),
                    RegexPred::Epsilon => values.push(RegexPred::Epsilon),
                    RegexPred::Elem(inner) => values.push(RegexPred::Elem(inner.clone())),
                    RegexPred::Length(lo, hi) => values.push(RegexPred::Length(*lo, *hi)),
                    RegexPred::Concat(left, right) => {
                        tasks.push(Task::Concat);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    RegexPred::Alt(left, right) => {
                        tasks.push(Task::Alt);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    RegexPred::Star(inner) => {
                        tasks.push(Task::Star);
                        tasks.push(Task::Clone(inner));
                    }
                    RegexPred::Inter(left, right) => {
                        tasks.push(Task::Inter);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    RegexPred::Compl(inner) => {
                        tasks.push(Task::Compl);
                        tasks.push(Task::Clone(inner));
                    }
                },
                Task::Concat | Task::Alt | Task::Inter => {
                    let right = values.pop().expect("right regex clone is present");
                    let left = values.pop().expect("left regex clone is present");
                    values.push(match task {
                        Task::Concat => RegexPred::Concat(Box::new(left), Box::new(right)),
                        Task::Alt => RegexPred::Alt(Box::new(left), Box::new(right)),
                        Task::Inter => RegexPred::Inter(Box::new(left), Box::new(right)),
                        _ => unreachable!("binary regex clone task is known"),
                    });
                }
                Task::Star | Task::Compl => {
                    let inner = values.pop().expect("unary regex clone is present");
                    values.push(if matches!(task, Task::Star) {
                        RegexPred::Star(Box::new(inner))
                    } else {
                        RegexPred::Compl(Box::new(inner))
                    });
                }
            }
        }
        values.pop().expect("the root regex produces one clone")
    }
}

impl<P: PartialEq> PartialEq for RegexPred<P> {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (RegexPred::Empty, RegexPred::Empty) | (RegexPred::Epsilon, RegexPred::Epsilon) => {
                }
                (RegexPred::Elem(left), RegexPred::Elem(right)) if left == right => {}
                (RegexPred::Length(ll, lh), RegexPred::Length(rl, rh)) if ll == rl && lh == rh => {}
                (RegexPred::Concat(ll, lr), RegexPred::Concat(rl, rr))
                | (RegexPred::Alt(ll, lr), RegexPred::Alt(rl, rr))
                | (RegexPred::Inter(ll, lr), RegexPred::Inter(rl, rr)) => {
                    pending.push((lr, rr));
                    pending.push((ll, rl));
                }
                (RegexPred::Star(left), RegexPred::Star(right))
                | (RegexPred::Compl(left), RegexPred::Compl(right)) => {
                    pending.push((left, right));
                }
                _ => return false,
            }
        }
        true
    }
}

impl<P: Eq> Eq for RegexPred<P> {}

impl<P: Hash> Hash for RegexPred<P> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut pending = vec![self];
        while let Some(predicate) = pending.pop() {
            std::mem::discriminant(predicate).hash(state);
            match predicate {
                RegexPred::Empty | RegexPred::Epsilon => {}
                RegexPred::Elem(inner) => inner.hash(state),
                RegexPred::Length(lo, hi) => {
                    lo.hash(state);
                    hi.hash(state);
                }
                RegexPred::Concat(left, right)
                | RegexPred::Alt(left, right)
                | RegexPred::Inter(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                RegexPred::Star(inner) | RegexPred::Compl(inner) => pending.push(inner),
            }
        }
    }
}

impl<P: fmt::Debug> fmt::Debug for RegexPred<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a, P> {
            Pred(&'a RegexPred<P>),
            Text(&'static str),
        }
        let mut events = vec![Event::Pred(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Pred(predicate) => match predicate {
                    RegexPred::Empty => write!(f, "Empty")?,
                    RegexPred::Epsilon => write!(f, "Epsilon")?,
                    RegexPred::Elem(inner) => write!(f, "Elem({inner:?})")?,
                    RegexPred::Length(lo, hi) => write!(f, "Length({lo:?}, {hi:?})")?,
                    RegexPred::Concat(left, right)
                    | RegexPred::Alt(left, right)
                    | RegexPred::Inter(left, right) => {
                        let name = match predicate {
                            RegexPred::Concat(_, _) => "Concat",
                            RegexPred::Alt(_, _) => "Alt",
                            RegexPred::Inter(_, _) => "Inter",
                            _ => unreachable!("binary regex variant is known"),
                        };
                        write!(f, "{name}(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Pred(right));
                        events.push(Event::Text(", "));
                        events.push(Event::Pred(left));
                    }
                    RegexPred::Star(inner) | RegexPred::Compl(inner) => {
                        write!(
                            f,
                            "{}(",
                            if matches!(predicate, RegexPred::Star(_)) {
                                "Star"
                            } else {
                                "Compl"
                            }
                        )?;
                        events.push(Event::Text(")"));
                        events.push(Event::Pred(inner));
                    }
                },
            }
        }
        Ok(())
    }
}

impl<P> Drop for RegexPred<P> {
    fn drop(&mut self) {
        fn drain<P>(predicate: &mut RegexPred<P>, pending: &mut Vec<RegexPred<P>>) {
            match predicate {
                RegexPred::Concat(left, right)
                | RegexPred::Alt(left, right)
                | RegexPred::Inter(left, right) => {
                    pending.push(std::mem::replace(&mut **right, RegexPred::Empty));
                    pending.push(std::mem::replace(&mut **left, RegexPred::Empty));
                }
                RegexPred::Star(inner) | RegexPred::Compl(inner) => {
                    pending.push(std::mem::replace(&mut **inner, RegexPred::Empty));
                }
                RegexPred::Empty
                | RegexPred::Epsilon
                | RegexPred::Elem(_)
                | RegexPred::Length(_, _) => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut predicate) = pending.pop() {
            drain(&mut predicate, &mut pending);
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Epsilon-NFA over element predicates (compilation target)
// ══════════════════════════════════════════════════════════════════════════════

struct EpsNfa<P> {
    n: usize,
    eps: Vec<(usize, usize)>,
    chr: Vec<(usize, P, usize)>,
    initials: Vec<usize>,
    accepts: Vec<usize>,
}

impl<P: Clone> EpsNfa<P> {
    fn empty() -> Self {
        EpsNfa {
            n: 1,
            eps: Vec::new(),
            chr: Vec::new(),
            initials: vec![0],
            accepts: Vec::new(),
        }
    }

    fn epsilon() -> Self {
        EpsNfa {
            n: 1,
            eps: Vec::new(),
            chr: Vec::new(),
            initials: vec![0],
            accepts: vec![0],
        }
    }

    fn elem(class: P) -> Self {
        EpsNfa {
            n: 2,
            eps: Vec::new(),
            chr: vec![(0, class, 1)],
            initials: vec![0],
            accepts: vec![1],
        }
    }

    fn concat(a: EpsNfa<P>, b: EpsNfa<P>) -> Self {
        let EpsNfa {
            n: a_n,
            mut eps,
            mut chr,
            initials,
            accepts: a_accepts,
        } = a;
        let EpsNfa {
            n: b_n,
            eps: b_eps,
            chr: b_chr,
            initials: b_initials,
            accepts: b_accepts,
        } = b;
        let off = a_n;
        eps.extend(b_eps.into_iter().map(|(x, y)| (x + off, y + off)));
        chr.extend(
            b_chr
                .into_iter()
                .map(|(x, guard, y)| (x + off, guard, y + off)),
        );
        for ai in a_accepts {
            for &bi in &b_initials {
                eps.push((ai, bi + off));
            }
        }
        let accepts = b_accepts.into_iter().map(|state| state + off).collect();
        EpsNfa {
            n: a_n + b_n,
            eps,
            chr,
            initials,
            accepts,
        }
    }

    fn alt(a: EpsNfa<P>, b: EpsNfa<P>) -> Self {
        let EpsNfa {
            n: a_n,
            mut eps,
            mut chr,
            mut initials,
            mut accepts,
        } = a;
        let EpsNfa {
            n: b_n,
            eps: b_eps,
            chr: b_chr,
            initials: b_initials,
            accepts: b_accepts,
        } = b;
        let off = a_n;
        eps.extend(b_eps.into_iter().map(|(x, y)| (x + off, y + off)));
        chr.extend(
            b_chr
                .into_iter()
                .map(|(x, guard, y)| (x + off, guard, y + off)),
        );
        initials.extend(b_initials.into_iter().map(|state| state + off));
        accepts.extend(b_accepts.into_iter().map(|state| state + off));
        EpsNfa {
            n: a_n + b_n,
            eps,
            chr,
            initials,
            accepts,
        }
    }

    fn star(a: EpsNfa<P>) -> Self {
        let EpsNfa {
            n,
            mut eps,
            chr,
            initials,
            accepts,
        } = a;
        let q = n;
        for &ai in &initials {
            eps.push((q, ai));
        }
        for acc in accepts {
            eps.push((acc, q));
        }
        EpsNfa {
            n: n + 1,
            eps,
            chr,
            initials: vec![q],
            accepts: vec![q],
        }
    }

    fn from_sfa<A>(sfa: &SymbolicAutomaton<A>) -> Self
    where
        A: BooleanAlgebra<Predicate = P>,
    {
        let chr = sfa
            .transitions
            .iter()
            .map(|t| (t.from, t.guard.clone(), t.to))
            .collect();
        let mut initials: Vec<usize> = sfa.initial_states.iter().copied().collect();
        initials.sort_unstable();
        let mut accepts: Vec<usize> = sfa.accepting_states.iter().copied().collect();
        accepts.sort_unstable();
        EpsNfa {
            n: sfa.states.len().max(1),
            eps: Vec::new(),
            chr,
            initials,
            accepts,
        }
    }

    fn eclosure(&self, seeds: &[usize]) -> HashSet<usize> {
        let mut seen: HashSet<usize> = seeds.iter().copied().collect();
        let mut stack: Vec<usize> = seeds.to_vec();
        while let Some(s) = stack.pop() {
            for &(a, b) in &self.eps {
                if a == s && seen.insert(b) {
                    stack.push(b);
                }
            }
        }
        seen
    }

    fn to_sfa<A>(&self, algebra: A) -> SymbolicAutomaton<A>
    where
        A: BooleanAlgebra<Predicate = P>,
    {
        let accept_set: HashSet<usize> = self.accepts.iter().copied().collect();
        let ecl: Vec<HashSet<usize>> = (0..self.n).map(|s| self.eclosure(&[s])).collect();
        let mut sfa = SymbolicAutomaton::new(algebra);
        for i in 0..self.n {
            let is_acc = ecl[i].iter().any(|s| accept_set.contains(s));
            sfa.add_state(is_acc, None);
        }
        for &init in &self.initials {
            sfa.set_initial(init);
        }
        for (u, g, v) in &self.chr {
            for (s, closure) in ecl.iter().enumerate() {
                if closure.contains(u) {
                    sfa.add_transition(s, *v, g.clone());
                }
            }
        }
        sfa
    }
}

/// Compile a [`RegexPred`] to an epsilon-NFA over `A`'s element predicates.
fn compile_eps<A>(algebra: &A, p: &RegexPred<A::Predicate>) -> EpsNfa<A::Predicate>
where
    A: BooleanAlgebra,
{
    enum Task<'a, P> {
        Compile(&'a RegexPred<P>),
        Concat,
        Alt,
        Star,
        Inter,
        Compl,
    }
    let mut tasks = vec![Task::Compile(p)];
    let mut values = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Compile(predicate) => match predicate {
                RegexPred::Empty => values.push(EpsNfa::empty()),
                RegexPred::Epsilon => values.push(EpsNfa::epsilon()),
                RegexPred::Elem(class) => values.push(EpsNfa::elem(class.clone())),
                RegexPred::Length(lo, hi) => {
                    let sigma = || EpsNfa::elem(algebra.true_pred());
                    let mut acc = EpsNfa::epsilon();
                    for _ in 0..*lo {
                        acc = EpsNfa::concat(acc, sigma());
                    }
                    values.push(match hi {
                        None => EpsNfa::concat(acc, EpsNfa::star(sigma())),
                        Some(upper) => {
                            for _ in 0..upper.saturating_sub(*lo) {
                                acc = EpsNfa::concat(acc, EpsNfa::alt(EpsNfa::epsilon(), sigma()));
                            }
                            acc
                        }
                    });
                }
                RegexPred::Concat(left, right) => {
                    tasks.push(Task::Concat);
                    tasks.push(Task::Compile(right));
                    tasks.push(Task::Compile(left));
                }
                RegexPred::Alt(left, right) => {
                    tasks.push(Task::Alt);
                    tasks.push(Task::Compile(right));
                    tasks.push(Task::Compile(left));
                }
                RegexPred::Star(inner) => {
                    tasks.push(Task::Star);
                    tasks.push(Task::Compile(inner));
                }
                RegexPred::Inter(left, right) => {
                    tasks.push(Task::Inter);
                    tasks.push(Task::Compile(right));
                    tasks.push(Task::Compile(left));
                }
                RegexPred::Compl(inner) => {
                    tasks.push(Task::Compl);
                    tasks.push(Task::Compile(inner));
                }
            },
            Task::Concat | Task::Alt | Task::Inter => {
                let right = values.pop().expect("right compiled regex is present");
                let left = values.pop().expect("left compiled regex is present");
                values.push(match task {
                    Task::Concat => EpsNfa::concat(left, right),
                    Task::Alt => EpsNfa::alt(left, right),
                    Task::Inter => {
                        let left = left.to_sfa(algebra.clone());
                        let right = right.to_sfa(algebra.clone());
                        EpsNfa::from_sfa(&left.intersect(&right))
                    }
                    _ => unreachable!("binary regex compilation task is known"),
                });
            }
            Task::Star => {
                let inner = values.pop().expect("star operand is compiled");
                values.push(EpsNfa::star(inner));
            }
            Task::Compl => {
                let inner = values.pop().expect("complement operand is compiled");
                let automaton = inner.to_sfa(algebra.clone());
                values.push(EpsNfa::from_sfa(&automaton.complement()));
            }
        }
    }
    values
        .pop()
        .expect("the root regex produces one epsilon-NFA")
}

/// Compile a [`RegexPred`] to an SFA over `A`.
pub fn compile<A>(algebra: &A, p: &RegexPred<A::Predicate>) -> SymbolicAutomaton<A>
where
    A: BooleanAlgebra,
{
    compile_eps(algebra, p).to_sfa(algebra.clone())
}

// ══════════════════════════════════════════════════════════════════════════════
// RegexAlgebra (= the list algebra over A)
// ══════════════════════════════════════════════════════════════════════════════

/// The effective Boolean algebra of symbolic regular languages over `A` — i.e.
/// the **list algebra** over sequences of `A`'s domain.
#[derive(Clone, Debug)]
pub struct RegexAlgebra<A: BooleanAlgebra> {
    /// The element algebra.
    pub elem: A,
}

/// Alias: the list algebra is the symbolic-regular-language algebra over the
/// element algebra.
pub type ListAlgebra<A> = RegexAlgebra<A>;

impl<A: BooleanAlgebra> RegexAlgebra<A> {
    /// Construct the list/regex algebra over the given element algebra.
    pub fn new(elem: A) -> Self {
        RegexAlgebra { elem }
    }

    /// `Σ*` — every sequence.
    pub fn any(&self) -> RegexPred<A::Predicate> {
        RegexPred::Star(Box::new(RegexPred::Elem(self.elem.true_pred())))
    }

    /// `∀ e ∈ xs. e ⊨ p` — every element satisfies `p` (includes the empty list).
    pub fn all(&self, p: A::Predicate) -> RegexPred<A::Predicate> {
        RegexPred::Star(Box::new(RegexPred::Elem(p)))
    }

    /// `∃ e ∈ xs. e ⊨ p` — some element satisfies `p`.
    pub fn any_elem(&self, p: A::Predicate) -> RegexPred<A::Predicate> {
        let sigma_star = self.any();
        RegexPred::Concat(
            Box::new(sigma_star.clone()),
            Box::new(RegexPred::Concat(
                Box::new(RegexPred::Elem(p)),
                Box::new(sigma_star),
            )),
        )
    }
}

impl<A: BooleanAlgebra> BooleanAlgebra for RegexAlgebra<A> {
    type Predicate = RegexPred<A::Predicate>;
    type Domain = Vec<A::Domain>;

    fn true_pred(&self) -> Self::Predicate {
        self.any()
    }

    fn false_pred(&self) -> Self::Predicate {
        RegexPred::Empty
    }

    fn and(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        RegexPred::Inter(Box::new(a.clone()), Box::new(b.clone()))
    }

    fn or(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        RegexPred::Alt(Box::new(a.clone()), Box::new(b.clone()))
    }

    fn not(&self, a: &Self::Predicate) -> Self::Predicate {
        RegexPred::Compl(Box::new(a.clone()))
    }

    fn is_satisfiable(&self, a: &Self::Predicate) -> bool {
        !compile(&self.elem, a).is_empty()
    }

    fn witness(&self, a: &Self::Predicate) -> Option<Self::Domain> {
        compile(&self.elem, a).shortest_accepted()
    }

    fn evaluate(&self, pred: &Self::Predicate, elem: &Self::Domain) -> bool {
        compile(&self.elem, pred).accepts(elem)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{IntervalAlgebra, IntervalPred};
    use super::*;

    fn list_alg() -> RegexAlgebra<IntervalAlgebra> {
        RegexAlgebra::new(IntervalAlgebra::new(0, 100))
    }

    #[test]
    fn all_elements_in_range() {
        let alg = list_alg();
        let all_small = alg.all(IntervalPred::Range(0, 10));
        assert!(alg.evaluate(&all_small, &vec![])); // empty list, vacuous
        assert!(alg.evaluate(&all_small, &vec![1, 5, 9]));
        assert!(!alg.evaluate(&all_small, &vec![1, 50]));
        assert!(alg.is_satisfiable(&all_small));
    }

    #[test]
    fn some_element_satisfies() {
        let alg = list_alg();
        let some_big = alg.any_elem(IntervalPred::Range(50, 100));
        assert!(!alg.evaluate(&some_big, &vec![])); // empty has no element
        assert!(!alg.evaluate(&some_big, &vec![1, 2, 3]));
        assert!(alg.evaluate(&some_big, &vec![1, 60, 3]));
    }

    #[test]
    fn length_and_content_intersection_exact() {
        let alg = list_alg();
        // exactly 2 elements AND all in [0,10) AND some in [5,10)
        let p = alg.and(
            &alg.and(
                &RegexPred::Length(2, Some(2)),
                &alg.all(IntervalPred::Range(0, 10)),
            ),
            &alg.any_elem(IntervalPred::Range(5, 10)),
        );
        assert!(alg.is_satisfiable(&p));
        assert!(alg.evaluate(&p, &vec![3, 7]));
        assert!(!alg.evaluate(&p, &vec![3, 4])); // none in [5,10)
        assert!(!alg.evaluate(&p, &vec![7])); // length 1
        assert!(!alg.evaluate(&p, &vec![3, 7, 8])); // length 3
        let w = alg.witness(&p).expect("nonempty");
        assert!(alg.evaluate(&p, &w));
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn complement_and_laws() {
        let alg = list_alg();
        let all_small = alg.all(IntervalPred::Range(0, 10));
        let not_all_small = alg.not(&all_small);
        assert!(!alg.evaluate(&not_all_small, &vec![1, 2])); // all small → not in complement
        assert!(alg.evaluate(&not_all_small, &vec![1, 50])); // has a big one
        assert!(!alg.is_satisfiable(&alg.and(&all_small, &not_all_small)));
        // unsatisfiable conjunction of disjoint length constraints
        let p = alg.and(
            &RegexPred::Length(1, Some(1)),
            &RegexPred::Length(2, Some(2)),
        );
        assert!(!alg.is_satisfiable(&p));
    }

    #[test]
    fn empty_and_top() {
        let alg = list_alg();
        assert!(!alg.is_satisfiable(&alg.false_pred()));
        assert!(alg.is_satisfiable(&alg.true_pred()));
        assert!(alg.evaluate(&alg.true_pred(), &vec![1, 2, 3]));
    }
}
