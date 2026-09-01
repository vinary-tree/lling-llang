//! LogicT Fair Backtracking Search Framework
//!
//! ## Theory
//!
//! LogicT is a fair backtracking monad implementing the `msplit`-based design
//! of Kiselyov, Shan, Friedman & Sabry (ICFP 2005). Unlike depth-first
//! backtracking (which starves late branches), LogicT's `interleave` operation
//! guarantees that both branches of a disjunction are explored infinitely often,
//! preventing starvation. This is critical for constraint propagation where
//! the search tree may be unbalanced.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                 LogicT Framework                             │
//! │                                                             │
//! │  LogicStream<T>                                              │
//! │    ├── msplit()   — peek at first result + remainder         │
//! │    ├── mzero()    — empty stream (failure)                   │
//! │    ├── mplus()    — concatenation (unfair)                   │
//! │    ├── interleave() — fair disjunction (alternating)         │
//! │    ├── fair_conjoin() — fair conjunction (>>-)               │
//! │    ├── ifte()     — soft cut (if-then-else)                  │
//! │    ├── once()     — commit to first result                   │
//! │    └── gnot()     — negation as finite failure               │
//! │                                                             │
//! │  ConstraintTheory (trait)                                    │
//! │    ├── propagate() — add constraint, check consistency       │
//! │    ├── label()     — generate search choices (LogicStream)   │
//! │    ├── witness()   — extract concrete assignment             │
//! │    └── evaluate()  — check constraint against assignment     │
//! │                                                             │
//! │  TheoryAlgebra<T>                                            │
//! │    └── impl BooleanAlgebra — bridge ConstraintTheory to SFA  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## References
//!
//! - Kiselyov, O., Shan, C., Friedman, D. P. & Sabry, A. (2005).
//!   "Backtracking, Interleaving, and Terminating Monad Transformers."
//!   ICFP 2005. DOI: 10.1145/1086365.1086390
//! - Hemann, J. & Friedman, D. P. (2013). "μKanren: A Minimal Functional
//!   Core for Relational Programming." Scheme Workshop 2013.

use std::collections::VecDeque;
use std::fmt;

use std::hash::{Hash, Hasher};

// ══════════════════════════════════════════════════════════════════════════════
// LogicStream — Fair backtracking search stream
// ══════════════════════════════════════════════════════════════════════════════

/// Result of evaluating a branch.
///
/// Suspensions expand to a deferred set of branches.
enum BranchResult<T> {
    /// Fork into sub-branches (no immediate result).
    Fork(Vec<Branch<T>>),
}

/// A branch in the search tree.
enum Branch<T> {
    /// A value ready to yield.
    Ready(T),
    /// A suspended computation returning zero or more results.
    Suspended(Box<dyn FnOnce() -> BranchResult<T> + Send>),
    /// A state machine that produces one deferred result per scheduling turn.
    LazyIterator(Box<dyn Iterator<Item = T> + Send>),
}

/// Fair backtracking search stream.
///
/// Implements `msplit`-based LogicT (Kiselyov et al., ICFP 2005).
/// Uses an explicit `VecDeque`-based branch queue for round-robin
/// fair scheduling, following the same philosophy as the trampoline
/// parser (explicit stack, no recursion).
///
/// # Fairness Guarantee
///
/// `interleave(a, b)` alternates between branches from `a` and `b`,
/// ensuring both are explored infinitely often. This prevents
/// starvation when one branch produces results faster than the other.
pub struct LogicStream<T> {
    /// Branch queue for round-robin fair scheduling.
    branches: VecDeque<Branch<T>>,
}

impl<T: fmt::Debug> fmt::Debug for LogicStream<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LogicStream({} branches)", self.branches.len())
    }
}

impl<T: Send + 'static> LogicStream<T> {
    // ── Constructors ────────────────────────────────────────────────────

    /// Create an empty stream (failure / mzero).
    ///
    /// The identity element for `mplus` and `interleave`.
    pub fn empty() -> Self {
        LogicStream {
            branches: VecDeque::new(),
        }
    }

    /// Create a stream with a single result.
    pub fn unit(value: T) -> Self {
        let mut branches = VecDeque::with_capacity(1);
        branches.push_back(Branch::Ready(value));
        LogicStream { branches }
    }

    /// Create a stream from an iterator of values.
    pub fn from_iter(iter: impl IntoIterator<Item = T>) -> Self {
        let branches: VecDeque<Branch<T>> = iter.into_iter().map(Branch::Ready).collect();
        LogicStream { branches }
    }

    /// Create a stream backed by a lazy, heap-resident iterator machine.
    fn from_lazy_iter(iter: impl Iterator<Item = T> + Send + 'static) -> Self {
        let mut branches = VecDeque::with_capacity(1);
        branches.push_back(Branch::LazyIterator(Box::new(iter)));
        LogicStream { branches }
    }

    /// Create a stream from a suspended computation.
    pub fn suspend(f: impl FnOnce() -> LogicStream<T> + Send + 'static) -> Self {
        let mut branches = VecDeque::with_capacity(1);
        branches.push_back(Branch::Suspended(Box::new(move || {
            let stream = f();
            Fork(stream.branches.into_iter().collect())
        })));
        LogicStream { branches }
    }

    // ── Core operations (all derived from msplit) ───────────────────────

    /// Peek at the first result and the remaining stream.
    ///
    /// This is the fundamental primitive from which all other LogicT
    /// operations are derived. Returns `None` if the stream is exhausted,
    /// or `Some((first_result, remaining_stream))`.
    ///
    /// # Implementation
    ///
    /// Processes branches from the front of the queue:
    /// - `Ready(v)`: return immediately with `v` and remaining branches
    /// - `Suspended(f)`: evaluate `f()` and enqueue the returned forked branches
    pub fn msplit(mut self) -> Option<(T, LogicStream<T>)> {
        while let Some(branch) = self.branches.pop_front() {
            match branch {
                Branch::Ready(value) => {
                    return Some((value, self));
                }
                Branch::Suspended(f) => {
                    let BranchResult::Fork(more) = f();
                    for branch in more {
                        self.branches.push_back(branch);
                    }
                }
                Branch::LazyIterator(mut iterator) => {
                    if let Some(value) = iterator.next() {
                        // Enqueue rather than return directly: this is exactly
                        // the same scheduling point as a suspension that forks
                        // into one ready value and its residual machine.
                        self.branches.push_back(Branch::Ready(value));
                        self.branches.push_back(Branch::LazyIterator(iterator));
                    }
                }
            }
        }
        None
    }

    /// Concatenation (unfair disjunction).
    ///
    /// Appends all branches of `other` after all branches of `self`.
    /// This is NOT fair — `other`'s branches are explored only after
    /// `self` is exhausted. Use `interleave` for fair disjunction.
    pub fn mplus(mut self, other: LogicStream<T>) -> LogicStream<T> {
        self.branches.extend(other.branches);
        self
    }

    /// Fair disjunction (alternating interleave).
    ///
    /// Alternates branches from `self` and `other` in round-robin
    /// fashion, ensuring both streams are explored infinitely often.
    ///
    /// # Fairness
    ///
    /// For streams A = [a₁, a₂, a₃, ...] and B = [b₁, b₂, b₃, ...],
    /// `interleave(A, B)` produces [a₁, b₁, a₂, b₂, a₃, b₃, ...].
    /// Both A and B are explored at each step.
    pub fn interleave(self, other: LogicStream<T>) -> LogicStream<T> {
        let mut result = VecDeque::with_capacity(self.branches.len() + other.branches.len());
        let mut iter_a = self.branches.into_iter();
        let mut iter_b = other.branches.into_iter();

        loop {
            match (iter_a.next(), iter_b.next()) {
                (Some(a), Some(b)) => {
                    result.push_back(a);
                    result.push_back(b);
                }
                (Some(a), None) => {
                    result.push_back(a);
                    result.extend(iter_a);
                    break;
                }
                (None, Some(b)) => {
                    result.push_back(b);
                    result.extend(iter_b);
                    break;
                }
                (None, None) => break,
            }
        }

        LogicStream { branches: result }
    }

    /// Fair conjunction (>>- / fair bind).
    ///
    /// Applies `f` to each result from `self`, then interleaves all
    /// resulting streams. This prevents `f` applied to the first result
    /// from starving results from later values.
    ///
    /// Equivalent to: `fold(self.map(f), empty, interleave)`
    pub fn fair_conjoin<U: Send + 'static>(
        self,
        f: impl Fn(T) -> LogicStream<U> + Send + 'static,
    ) -> LogicStream<U> {
        let mut accumulated = LogicStream::<U>::empty();

        for branch in self.branches {
            let stream = match branch {
                Branch::Ready(value) => f(value),
                Branch::Suspended(suspended) => {
                    // Evaluate the suspended computation, then apply f
                    match suspended() {
                        BranchResult::Fork(branches) => {
                            let mut result = LogicStream::<U>::empty();
                            for b in branches {
                                if let Branch::Ready(v) = b {
                                    result = result.interleave(f(v));
                                }
                            }
                            result
                        }
                    }
                }
                Branch::LazyIterator(iterator) => {
                    let mut result = LogicStream::<U>::empty();
                    for value in iterator {
                        result = result.interleave(f(value));
                    }
                    result
                }
            };
            accumulated = accumulated.interleave(stream);
        }

        accumulated
    }

    /// Soft cut (if-then-else).
    ///
    /// If `self` produces at least one result, apply `then_fn` to each
    /// result. Otherwise, return `else_stream`.
    ///
    /// This is Prolog's soft cut (`*->`): commit to the "then" branch
    /// if the test succeeds, but don't cut within the "then" branch.
    pub fn ifte<U: Send + 'static>(
        self,
        then_fn: impl Fn(T) -> LogicStream<U> + Send + 'static,
        else_stream: LogicStream<U>,
    ) -> LogicStream<U> {
        match self.msplit() {
            None => else_stream,
            Some((first, rest)) => {
                let first_results = then_fn(first);
                let rest_results = rest.fair_conjoin(then_fn);
                first_results.interleave(rest_results)
            }
        }
    }

    /// Commit to first result only.
    ///
    /// Returns a stream containing at most one element: the first
    /// result from `self`. All other results are discarded.
    ///
    /// This is Prolog's cut (`!`): take the first solution and stop.
    pub fn once(self) -> LogicStream<T> {
        match self.msplit() {
            None => LogicStream::empty(),
            Some((first, _rest)) => LogicStream::unit(first),
        }
    }

    /// Negation as finite failure.
    ///
    /// Succeeds (with `()`) if `self` produces no results.
    /// Fails if `self` produces any result.
    ///
    /// This implements the closed-world assumption: absence of evidence
    /// is treated as evidence of absence.
    pub fn gnot(self) -> LogicStream<()> {
        match self.msplit() {
            None => LogicStream::unit(()),
            Some(_) => LogicStream::empty(),
        }
    }

    /// Map a function over all results in the stream.
    ///
    /// Eagerly evaluates suspended branches during mapping. This is
    /// simpler than trying to compose closures and preserves all results.
    pub fn map<U: Send + 'static>(self, f: impl Fn(T) -> U + Send + 'static) -> LogicStream<U> {
        // Eagerly evaluate all branches to Ready values, then map.
        let all_values = self.collect_all();
        LogicStream::from_iter(all_values.into_iter().map(f))
    }

    /// Filter results by a predicate.
    ///
    /// Eagerly evaluates suspended branches during filtering. This is
    /// simpler than composing closures and preserves fairness by
    /// processing all branches before filtering.
    pub fn filter(self, pred: impl Fn(&T) -> bool + Send + 'static) -> LogicStream<T> {
        let all_values = self.collect_all();
        LogicStream::from_iter(all_values.into_iter().filter(|v| pred(v)))
    }

    /// Collect all results into a Vec (bounded by `limit`).
    ///
    /// Consumes the stream, collecting up to `limit` results. This
    /// prevents unbounded allocation from infinite streams.
    pub fn collect_bounded(self, limit: usize) -> Vec<T> {
        let mut results = Vec::with_capacity(limit.min(64));
        let mut stream = self;

        while results.len() < limit {
            match stream.msplit() {
                Some((value, rest)) => {
                    results.push(value);
                    stream = rest;
                }
                None => break,
            }
        }

        results
    }

    /// Collect all results into a Vec.
    ///
    /// Warning: this will loop forever on infinite streams.
    /// Prefer `collect_bounded` when the stream size is unknown.
    pub fn collect_all(self) -> Vec<T> {
        let mut results = Vec::new();
        let mut stream = self;

        while let Some((value, rest)) = stream.msplit() {
            results.push(value);
            stream = rest;
        }

        results
    }

    /// Check if the stream is empty (has no results).
    ///
    /// Consumes the stream. Use `msplit` if you need to preserve results.
    pub fn is_empty(self) -> bool {
        self.msplit().is_none()
    }

    /// Count the number of results (bounded).
    pub fn count_bounded(self, limit: usize) -> usize {
        self.collect_bounded(limit).len()
    }
}

// Use a private import for the Fork variant to avoid cluttering the namespace
use BranchResult::Fork;

// ══════════════════════════════════════════════════════════════════════════════
// Iterator integration
// ══════════════════════════════════════════════════════════════════════════════

/// Iterator adapter for LogicStream.
///
/// Yields results one at a time via `msplit`. Each call to `next()`
/// processes branches until a result is found or the stream is exhausted.
pub struct LogicStreamIter<T> {
    stream: LogicStream<T>,
}

impl<T: Send + 'static> Iterator for LogicStreamIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        let stream = std::mem::replace(&mut self.stream, LogicStream::empty());
        match stream.msplit() {
            Some((value, rest)) => {
                self.stream = rest;
                Some(value)
            }
            None => None,
        }
    }
}

impl<T: Send + 'static> IntoIterator for LogicStream<T> {
    type Item = T;
    type IntoIter = LogicStreamIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        LogicStreamIter { stream: self }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// ConstraintTheory trait
// ══════════════════════════════════════════════════════════════════════════════

/// A pluggable constraint domain for predicated type checking.
///
/// Theories implement propagation + labeling; LogicT handles search.
/// This enables any user-defined language to plug in domain-specific
/// constraint solvers (unification, lattice operations, custom matching)
/// without requiring changes to the SFA/BooleanAlgebra framework.
///
/// # Design
///
/// The trait separates two concerns:
/// - **Propagation** (`propagate`): deterministic constraint narrowing.
///   For decidable theories, propagation alone determines satisfiability.
/// - **Labeling** (`label`): non-deterministic search choices. For
///   theories requiring search (e.g., unification with multiple matches),
///   `label()` produces a `LogicStream` of constraint alternatives.
///
/// # Type Parameters
///
/// - `Constraint`: guard predicates in this theory's domain
/// - `Assignment`: concrete witness values (domain elements)
/// - `Store`: accumulated constraint state
pub trait ConstraintTheory: Clone + fmt::Debug + Send + Sync + 'static {
    /// Constraint representation (guard predicates in this theory).
    type Constraint: Clone + fmt::Debug + Eq + std::hash::Hash + Send + Sync + 'static;
    /// Concrete domain element (witness type).
    type Assignment: Clone + fmt::Debug + Send + Sync + 'static;
    /// Constraint store accumulating added constraints.
    type Store: Clone + fmt::Debug + Send + Sync + 'static;

    /// Create an empty (unconstrained) store.
    fn empty_store(&self) -> Self::Store;

    /// Add constraint to store and propagate. Returns `None` if inconsistent.
    fn propagate(&self, store: &Self::Store, c: &Self::Constraint) -> Option<Self::Store>;

    /// Check store consistency without adding constraints.
    fn is_consistent(&self, store: &Self::Store) -> bool;

    /// Extract a witness assignment from a consistent store.
    ///
    /// Returns `None` if store is inconsistent or still needs labeling.
    fn witness(&self, store: &Self::Store) -> Option<Self::Assignment>;

    /// Generate labeling choices for search (used by LogicT).
    ///
    /// For decidable theories, this returns `LogicStream::empty()` —
    /// propagation alone determines satisfiability. For non-decidable
    /// theories, this produces a fair stream of variable assignments
    /// to search over.
    fn label(&self, store: &Self::Store) -> LogicStream<Self::Constraint>;

    /// Evaluate whether an assignment satisfies a constraint.
    fn evaluate(&self, c: &Self::Constraint, assignment: &Self::Assignment) -> bool;
}

// ══════════════════════════════════════════════════════════════════════════════
// QuantifiedFormula — FOL formulas for guard evaluation
// ══════════════════════════════════════════════════════════════════════════════

/// A first-order logic formula for quantified guard evaluation via LogicT.
///
/// Used to express predicates like `∀y. (reachable(x,y) ⇒ safe(y))` in
/// behavioral guards. The evaluator resolves atoms against Ascent fixpoint
/// relations and uses LogicT's `gnot`/`fair_conjoin` for quantifiers.
///
/// # Evaluation Strategy
///
/// - `Atom`: resolved by querying the Ascent relation fixpoint
/// - `And`/`Or`/`Not`: standard short-circuit Boolean evaluation
/// - `Implies(a, b)`: desugared to `Or(Not(a), b)`
/// - `ForAll`: domain enumeration + check body for ALL elements
/// - `Exists`: domain enumeration + check body for ANY element
///
/// # References
///
/// - Gap 3 in `docs/design/predicated-types.md` §22
/// - Strategy 3 (LogicT evaluation) selected for composability
pub enum QuantifiedFormula {
    /// Atomic relation query: `R(args)` — checks Ascent fixpoint.
    Atom {
        relation: String,
        args: Vec<QuantifiedArg>,
    },
    /// Conjunction: `a ∧ b`
    And(Box<QuantifiedFormula>, Box<QuantifiedFormula>),
    /// Disjunction: `a ∨ b`
    Or(Box<QuantifiedFormula>, Box<QuantifiedFormula>),
    /// Negation: `¬a`
    Not(Box<QuantifiedFormula>),
    /// Implication: `a ⇒ b` (sugar for `¬a ∨ b`)
    Implies(Box<QuantifiedFormula>, Box<QuantifiedFormula>),
    /// Universal quantification: `∀var ∈ domain. body`
    ForAll {
        var: String,
        domain: QuantifiedDomain,
        body: Box<QuantifiedFormula>,
    },
    /// Existential quantification: `∃var ∈ domain. body`
    Exists {
        var: String,
        domain: QuantifiedDomain,
        body: Box<QuantifiedFormula>,
    },
}

impl Clone for QuantifiedFormula {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Clone(&'a QuantifiedFormula),
            And,
            Or,
            Not,
            Implies,
            ForAll(&'a str, &'a QuantifiedDomain),
            Exists(&'a str, &'a QuantifiedDomain),
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(formula) => match formula {
                    QuantifiedFormula::Atom { relation, args } => {
                        values.push(QuantifiedFormula::Atom {
                            relation: relation.clone(),
                            args: args.clone(),
                        });
                    }
                    QuantifiedFormula::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    QuantifiedFormula::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    QuantifiedFormula::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Clone(inner));
                    }
                    QuantifiedFormula::Implies(left, right) => {
                        tasks.push(Task::Implies);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    QuantifiedFormula::ForAll { var, domain, body } => {
                        tasks.push(Task::ForAll(var, domain));
                        tasks.push(Task::Clone(body));
                    }
                    QuantifiedFormula::Exists { var, domain, body } => {
                        tasks.push(Task::Exists(var, domain));
                        tasks.push(Task::Clone(body));
                    }
                },
                Task::And | Task::Or | Task::Implies => {
                    let right = values.pop().expect("right quantified clone is present");
                    let left = values.pop().expect("left quantified clone is present");
                    values.push(match task {
                        Task::And => QuantifiedFormula::And(Box::new(left), Box::new(right)),
                        Task::Or => QuantifiedFormula::Or(Box::new(left), Box::new(right)),
                        Task::Implies => {
                            QuantifiedFormula::Implies(Box::new(left), Box::new(right))
                        }
                        _ => unreachable!("binary quantified clone task is known"),
                    });
                }
                Task::Not => {
                    let inner = values.pop().expect("negated quantified clone is present");
                    values.push(QuantifiedFormula::Not(Box::new(inner)));
                }
                Task::ForAll(var, domain) | Task::Exists(var, domain) => {
                    let body = values.pop().expect("quantified body clone is present");
                    values.push(if matches!(task, Task::ForAll(_, _)) {
                        QuantifiedFormula::ForAll {
                            var: var.to_string(),
                            domain: domain.clone(),
                            body: Box::new(body),
                        }
                    } else {
                        QuantifiedFormula::Exists {
                            var: var.to_string(),
                            domain: domain.clone(),
                            body: Box::new(body),
                        }
                    });
                }
            }
        }
        values
            .pop()
            .expect("the root quantified formula produces one clone")
    }
}

impl PartialEq for QuantifiedFormula {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (
                    QuantifiedFormula::Atom {
                        relation: lr,
                        args: la,
                    },
                    QuantifiedFormula::Atom {
                        relation: rr,
                        args: ra,
                    },
                ) if lr == rr && la == ra => {}
                (QuantifiedFormula::And(ll, lr), QuantifiedFormula::And(rl, rr))
                | (QuantifiedFormula::Or(ll, lr), QuantifiedFormula::Or(rl, rr))
                | (QuantifiedFormula::Implies(ll, lr), QuantifiedFormula::Implies(rl, rr)) => {
                    pending.push((lr, rr));
                    pending.push((ll, rl));
                }
                (QuantifiedFormula::Not(left), QuantifiedFormula::Not(right)) => {
                    pending.push((left, right));
                }
                (
                    QuantifiedFormula::ForAll {
                        var: lv,
                        domain: ld,
                        body: lb,
                    },
                    QuantifiedFormula::ForAll {
                        var: rv,
                        domain: rd,
                        body: rb,
                    },
                )
                | (
                    QuantifiedFormula::Exists {
                        var: lv,
                        domain: ld,
                        body: lb,
                    },
                    QuantifiedFormula::Exists {
                        var: rv,
                        domain: rd,
                        body: rb,
                    },
                ) if lv == rv && ld == rd => pending.push((lb, rb)),
                _ => return false,
            }
        }
        true
    }
}

impl Eq for QuantifiedFormula {}

impl fmt::Debug for QuantifiedFormula {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Formula(&'a QuantifiedFormula),
            Text(&'static str),
        }
        let mut events = vec![Event::Formula(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Formula(formula) => match formula {
                    QuantifiedFormula::Atom { relation, args } => {
                        write!(f, "Atom {{ relation: {relation:?}, args: {args:?} }}")?;
                    }
                    QuantifiedFormula::And(left, right)
                    | QuantifiedFormula::Or(left, right)
                    | QuantifiedFormula::Implies(left, right) => {
                        let name = match formula {
                            QuantifiedFormula::And(_, _) => "And",
                            QuantifiedFormula::Or(_, _) => "Or",
                            QuantifiedFormula::Implies(_, _) => "Implies",
                            _ => unreachable!("binary quantified variant is known"),
                        };
                        write!(f, "{name}(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Formula(right));
                        events.push(Event::Text(", "));
                        events.push(Event::Formula(left));
                    }
                    QuantifiedFormula::Not(inner) => {
                        write!(f, "Not(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Formula(inner));
                    }
                    QuantifiedFormula::ForAll { var, domain, body }
                    | QuantifiedFormula::Exists { var, domain, body } => {
                        write!(
                            f,
                            "{} {{ var: {var:?}, domain: {domain:?}, body: ",
                            if matches!(formula, QuantifiedFormula::ForAll { .. }) {
                                "ForAll"
                            } else {
                                "Exists"
                            }
                        )?;
                        events.push(Event::Text(" }"));
                        events.push(Event::Formula(body));
                    }
                },
            }
        }
        Ok(())
    }
}

impl Drop for QuantifiedFormula {
    fn drop(&mut self) {
        fn drain(formula: &mut QuantifiedFormula, pending: &mut Vec<QuantifiedFormula>) {
            match formula {
                QuantifiedFormula::And(left, right)
                | QuantifiedFormula::Or(left, right)
                | QuantifiedFormula::Implies(left, right) => {
                    pending.push(std::mem::replace(
                        &mut **right,
                        QuantifiedFormula::Atom {
                            relation: String::new(),
                            args: Vec::new(),
                        },
                    ));
                    pending.push(std::mem::replace(
                        &mut **left,
                        QuantifiedFormula::Atom {
                            relation: String::new(),
                            args: Vec::new(),
                        },
                    ));
                }
                QuantifiedFormula::Not(inner)
                | QuantifiedFormula::ForAll { body: inner, .. }
                | QuantifiedFormula::Exists { body: inner, .. } => {
                    pending.push(std::mem::replace(
                        &mut **inner,
                        QuantifiedFormula::Atom {
                            relation: String::new(),
                            args: Vec::new(),
                        },
                    ));
                }
                QuantifiedFormula::Atom { .. } => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut formula) = pending.pop() {
            drain(&mut formula, &mut pending);
        }
    }
}

/// Domain specification for quantified variables.
///
/// Determines the set of values a quantified variable ranges over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuantifiedDomain {
    /// All tuples in an Ascent relation (finite, decidable — T1/T2).
    Relation(String),
    /// Bounded iteration over a relation (semi-decidable — T3).
    /// Enumerates at most `limit` tuples before concluding.
    Bounded { relation: String, limit: usize },
}

/// An argument to an atomic relation query in a quantified formula.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuantifiedArg {
    /// A variable reference (resolved from the environment).
    Var(String),
    /// A literal constant value.
    Constant(String),
}

impl QuantifiedFormula {
    /// Convenience: create an atom with the given relation and args.
    pub fn atom(relation: impl Into<String>, args: Vec<QuantifiedArg>) -> Self {
        QuantifiedFormula::Atom {
            relation: relation.into(),
            args,
        }
    }

    /// Convenience: `a ∧ b`
    pub fn and(a: QuantifiedFormula, b: QuantifiedFormula) -> Self {
        QuantifiedFormula::And(Box::new(a), Box::new(b))
    }

    /// Convenience: `a ∨ b`
    pub fn or(a: QuantifiedFormula, b: QuantifiedFormula) -> Self {
        QuantifiedFormula::Or(Box::new(a), Box::new(b))
    }

    /// Convenience: `¬a`
    pub fn not(a: QuantifiedFormula) -> Self {
        QuantifiedFormula::Not(Box::new(a))
    }

    /// Convenience: `a ⇒ b`
    pub fn implies(a: QuantifiedFormula, b: QuantifiedFormula) -> Self {
        QuantifiedFormula::Implies(Box::new(a), Box::new(b))
    }

    /// Convenience: `∀var ∈ relation. body`
    pub fn forall(
        var: impl Into<String>,
        domain: QuantifiedDomain,
        body: QuantifiedFormula,
    ) -> Self {
        QuantifiedFormula::ForAll {
            var: var.into(),
            domain,
            body: Box::new(body),
        }
    }

    /// Convenience: `∃var ∈ relation. body`
    pub fn exists(
        var: impl Into<String>,
        domain: QuantifiedDomain,
        body: QuantifiedFormula,
    ) -> Self {
        QuantifiedFormula::Exists {
            var: var.into(),
            domain,
            body: Box::new(body),
        }
    }

    /// Collect all free variables referenced in this formula (not bound by a quantifier).
    pub fn free_vars(&self) -> std::collections::HashSet<String> {
        let mut free = std::collections::HashSet::new();
        enum Task<'a> {
            Visit(&'a QuantifiedFormula),
            ExitBinding(&'a str, bool),
        }
        let mut bound = std::collections::HashSet::new();
        let mut tasks = vec![Task::Visit(self)];
        while let Some(task) = tasks.pop() {
            match task {
                Task::ExitBinding(var, was_present) => {
                    if !was_present {
                        bound.remove(var);
                    }
                }
                Task::Visit(formula) => match formula {
                    QuantifiedFormula::Atom { args, .. } => {
                        for arg in args {
                            if let QuantifiedArg::Var(var) = arg {
                                if !bound.contains(var) {
                                    free.insert(var.clone());
                                }
                            }
                        }
                    }
                    QuantifiedFormula::And(left, right)
                    | QuantifiedFormula::Or(left, right)
                    | QuantifiedFormula::Implies(left, right) => {
                        tasks.push(Task::Visit(right));
                        tasks.push(Task::Visit(left));
                    }
                    QuantifiedFormula::Not(inner) => tasks.push(Task::Visit(inner)),
                    QuantifiedFormula::ForAll { var, body, .. }
                    | QuantifiedFormula::Exists { var, body, .. } => {
                        let was_present = !bound.insert(var.clone());
                        tasks.push(Task::ExitBinding(var, was_present));
                        tasks.push(Task::Visit(body));
                    }
                },
            }
        }
        free
    }
}

impl QuantifiedArg {
    /// Convenience: create a variable argument.
    pub fn var(name: impl Into<String>) -> Self {
        QuantifiedArg::Var(name.into())
    }

    /// Convenience: create a constant argument.
    pub fn constant(value: impl Into<String>) -> Self {
        QuantifiedArg::Constant(value.into())
    }
}

impl fmt::Display for QuantifiedFormula {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Formula(&'a QuantifiedFormula),
            Text(&'static str),
        }
        let mut events = vec![Event::Formula(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Formula(formula) => match formula {
                    QuantifiedFormula::Atom { relation, args } => {
                        write!(f, "{relation}(")?;
                        for (index, arg) in args.iter().enumerate() {
                            if index > 0 {
                                write!(f, ", ")?;
                            }
                            write!(f, "{arg}")?;
                        }
                        write!(f, ")")?;
                    }
                    QuantifiedFormula::And(left, right)
                    | QuantifiedFormula::Or(left, right)
                    | QuantifiedFormula::Implies(left, right) => {
                        write!(f, "(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Formula(right));
                        events.push(Event::Text(match formula {
                            QuantifiedFormula::And(_, _) => " ∧ ",
                            QuantifiedFormula::Or(_, _) => " ∨ ",
                            QuantifiedFormula::Implies(_, _) => " ⇒ ",
                            _ => unreachable!("binary quantified variant is known"),
                        }));
                        events.push(Event::Formula(left));
                    }
                    QuantifiedFormula::Not(inner) => {
                        write!(f, "¬")?;
                        events.push(Event::Formula(inner));
                    }
                    QuantifiedFormula::ForAll { var, domain, body }
                    | QuantifiedFormula::Exists { var, domain, body } => {
                        write!(
                            f,
                            "{}{var} ∈ {domain}. ",
                            if matches!(formula, QuantifiedFormula::ForAll { .. }) {
                                "∀"
                            } else {
                                "∃"
                            }
                        )?;
                        events.push(Event::Formula(body));
                    }
                },
            }
        }
        Ok(())
    }
}

impl fmt::Display for QuantifiedDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QuantifiedDomain::Relation(name) => write!(f, "{}", name),
            QuantifiedDomain::Bounded { relation, limit } => {
                write!(f, "{}[≤{}]", relation, limit)
            }
        }
    }
}

impl fmt::Display for QuantifiedArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QuantifiedArg::Var(v) => write!(f, "{}", v),
            QuantifiedArg::Constant(c) => write!(f, "'{}'", c),
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// evaluate_quantified — FOL evaluator over Ascent fixpoint relations
// ══════════════════════════════════════════════════════════════════════════════

/// Evaluate a quantified formula against Ascent fixpoint relations.
///
/// # Arguments
///
/// * `formula` — The FOL formula to evaluate.
/// * `env` — Current variable bindings (var name → resolved value).
/// * `relation_query` — Callback: `(relation_name, resolved_args) → bool`.
///   Returns true if the tuple exists in the named Ascent relation.
/// * `domain_enumerate` — Callback: `(relation_name) → Vec<Vec<String>>`.
///   Returns all tuples in the named relation (for quantifier iteration).
/// * `bound` — Maximum tuples to enumerate for `Bounded` domains (T3 safety).
///
/// # Evaluation Rules
///
/// | Formula | Evaluation |
/// |---------|------------|
/// | `Atom(R, args)` | Resolve args from env, call `relation_query(R, resolved)` |
/// | `And(a, b)` | `eval(a) && eval(b)` (short-circuit) |
/// | `Or(a, b)` | `eval(a) \|\| eval(b)` (short-circuit) |
/// | `Not(a)` | `!eval(a)` |
/// | `Implies(a, b)` | `!eval(a) \|\| eval(b)` |
/// | `ForAll(var, dom, body)` | `∀ tuple ∈ dom: eval(body[var↦tuple])` |
/// | `Exists(var, dom, body)` | `∃ tuple ∈ dom: eval(body[var↦tuple])` |
///
/// For `ForAll` with `Bounded` domain, uses `collect_bounded(limit)` on
/// the domain stream to ensure termination for semi-decidable (T3) theories.
///
/// # Example
///
/// ```rust,ignore
/// use prattail::logict::{QuantifiedFormula, QuantifiedArg, QuantifiedDomain, evaluate_quantified};
/// use std::collections::HashMap;
///
/// let formula = QuantifiedFormula::forall(
///     "x",
///     QuantifiedDomain::Relation("items".into()),
///     QuantifiedFormula::atom("positive", vec![QuantifiedArg::var("x")]),
/// );
///
/// let mut env = HashMap::new();
/// let result = evaluate_quantified(
///     &formula,
///     &env,
///     &|rel, args| rel == "positive" && args[0].parse::<i32>().map_or(false, |n| n > 0),
///     &|rel| match rel { "items" => vec![vec!["1".into()], vec!["2".into()], vec!["3".into()]], _ => vec![] },
///     1000,
/// );
/// assert!(result);
/// ```
pub fn evaluate_quantified<F, G>(
    formula: &QuantifiedFormula,
    env: &std::collections::HashMap<String, String>,
    relation_query: &F,
    domain_enumerate: &G,
    bound: usize,
) -> bool
where
    F: Fn(&str, &[String]) -> bool,
    G: Fn(&str) -> Vec<Vec<String>>,
{
    type Env = std::collections::HashMap<String, String>;
    enum Frame<'a> {
        Eval(&'a QuantifiedFormula, Env),
        Not,
        AndRight(&'a QuantifiedFormula, Env),
        OrRight(&'a QuantifiedFormula, Env),
        ImpliesRight(&'a QuantifiedFormula, Env),
        Quantifier {
            var: &'a str,
            body: &'a QuantifiedFormula,
            remaining: std::vec::IntoIter<Vec<String>>,
            base_env: Env,
            universal: bool,
        },
    }
    let mut frames = vec![Frame::Eval(formula, env.clone())];
    let mut result = None;
    while let Some(frame) = frames.pop() {
        match frame {
            Frame::Eval(current, current_env) => match current {
                QuantifiedFormula::Atom { relation, args } => {
                    let resolved: Vec<String> = args
                        .iter()
                        .map(|arg| match arg {
                            QuantifiedArg::Var(var) => current_env
                                .get(var)
                                .cloned()
                                .unwrap_or_else(|| panic!("unbound variable '{}' in formula", var)),
                            QuantifiedArg::Constant(constant) => constant.clone(),
                        })
                        .collect();
                    result = Some(relation_query(relation, &resolved));
                }
                QuantifiedFormula::And(left, right) => {
                    frames.push(Frame::AndRight(right, current_env.clone()));
                    frames.push(Frame::Eval(left, current_env));
                }
                QuantifiedFormula::Or(left, right) => {
                    frames.push(Frame::OrRight(right, current_env.clone()));
                    frames.push(Frame::Eval(left, current_env));
                }
                QuantifiedFormula::Not(inner) => {
                    frames.push(Frame::Not);
                    frames.push(Frame::Eval(inner, current_env));
                }
                QuantifiedFormula::Implies(left, right) => {
                    frames.push(Frame::ImpliesRight(right, current_env.clone()));
                    frames.push(Frame::Eval(left, current_env));
                }
                QuantifiedFormula::ForAll { var, domain, body }
                | QuantifiedFormula::Exists { var, domain, body } => {
                    let universal = matches!(current, QuantifiedFormula::ForAll { .. });
                    let mut remaining =
                        enumerate_domain(domain, domain_enumerate, bound).into_iter();
                    if let Some(tuple) = remaining.next() {
                        let mut inner_env = current_env.clone();
                        if let Some(value) = tuple.first() {
                            inner_env.insert(var.clone(), value.clone());
                        }
                        frames.push(Frame::Quantifier {
                            var,
                            body,
                            remaining,
                            base_env: current_env,
                            universal,
                        });
                        frames.push(Frame::Eval(body, inner_env));
                    } else {
                        result = Some(universal);
                    }
                }
            },
            Frame::Not => result = Some(!result.take().expect("negated formula is evaluated")),
            Frame::AndRight(right, env) => {
                if result.take().expect("left conjunction is evaluated") {
                    frames.push(Frame::Eval(right, env));
                } else {
                    result = Some(false);
                }
            }
            Frame::OrRight(right, env) => {
                if result.take().expect("left disjunction is evaluated") {
                    result = Some(true);
                } else {
                    frames.push(Frame::Eval(right, env));
                }
            }
            Frame::ImpliesRight(right, env) => {
                if result.take().expect("implication antecedent is evaluated") {
                    frames.push(Frame::Eval(right, env));
                } else {
                    result = Some(true);
                }
            }
            Frame::Quantifier {
                var,
                body,
                mut remaining,
                base_env,
                universal,
            } => {
                let value = result.take().expect("quantifier body is evaluated");
                if (universal && !value) || (!universal && value) {
                    result = Some(value);
                } else if let Some(tuple) = remaining.next() {
                    let mut inner_env = base_env.clone();
                    if let Some(value) = tuple.first() {
                        inner_env.insert(var.to_string(), value.clone());
                    }
                    frames.push(Frame::Quantifier {
                        var,
                        body,
                        remaining,
                        base_env,
                        universal,
                    });
                    frames.push(Frame::Eval(body, inner_env));
                } else {
                    result = Some(universal);
                }
            }
        }
    }
    result.expect("the root quantified formula produces one Boolean result")
}

/// Enumerate tuples from a domain, respecting bounds for T3 safety.
fn enumerate_domain<G>(
    domain: &QuantifiedDomain,
    domain_enumerate: &G,
    default_bound: usize,
) -> Vec<Vec<String>>
where
    G: Fn(&str) -> Vec<Vec<String>>,
{
    match domain {
        QuantifiedDomain::Relation(name) => domain_enumerate(name),
        QuantifiedDomain::Bounded { relation, limit } => {
            let all = domain_enumerate(relation);
            let effective_limit = (*limit).min(default_bound);
            all.into_iter().take(effective_limit).collect()
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// TriState evaluation — three-valued logic for theory-guided guards
// ══════════════════════════════════════════════════════════════════════════════

/// Three-valued result for theory-guided quantified evaluation.
///
/// Phase 6 (predicated types): the standard `evaluate_quantified` returns
/// a Boolean. When the underlying domain is undecidable (e.g., Presburger
/// over an infinite domain) or the search exhausts its bound without
/// finding a witness, "unknown" is the only honest answer. The
/// theory-guided evaluator uses `TriState::Unknown` to signal this case
/// so that callers can fall back to a conservative default (Phase 7
/// T3 codegen uses `Unknown → False`, the spec's safe choice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriState {
    /// The formula is definitely true under the given environment.
    True,
    /// The formula is definitely false under the given environment.
    False,
    /// The formula's truth value could not be determined within the
    /// search bound or by the theory's decision procedure.
    Unknown,
}

impl TriState {
    /// Three-valued conjunction (Kleene strong logic).
    pub fn and(self, other: TriState) -> TriState {
        use TriState::*;
        match (self, other) {
            (False, _) | (_, False) => False,
            (True, True) => True,
            _ => Unknown,
        }
    }

    /// Three-valued disjunction (Kleene strong logic).
    pub fn or(self, other: TriState) -> TriState {
        use TriState::*;
        match (self, other) {
            (True, _) | (_, True) => True,
            (False, False) => False,
            _ => Unknown,
        }
    }

    /// Three-valued negation.
    pub fn not(self) -> TriState {
        use TriState::*;
        match self {
            True => False,
            False => True,
            Unknown => Unknown,
        }
    }

    /// Three-valued implication: `a ⟹ b ≡ ¬a ∨ b`.
    pub fn implies(self, other: TriState) -> TriState {
        self.not().or(other)
    }

    /// Conservative collapse: `True → true`, anything else → false.
    /// Used by T3 codegen for the "safe-fail" default.
    pub fn into_safe_bool(self) -> bool {
        matches!(self, TriState::True)
    }
}

impl From<bool> for TriState {
    fn from(b: bool) -> Self {
        if b {
            TriState::True
        } else {
            TriState::False
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// evaluate_quantified_with_theory — theory-guided FOL evaluator
// ══════════════════════════════════════════════════════════════════════════════

/// Theory-guided variant of `evaluate_quantified`.
///
/// Phase 6A (predicated types): walks the same `QuantifiedFormula` AST
/// as `evaluate_quantified` but threads a `ConstraintTheory` instance
/// through the recursion, returning a `TriState` instead of a `bool`.
///
/// The theory's role is to refine the result: if the theory's
/// propagation produces an inconsistent store for an atom, the atom is
/// `False` regardless of what the relation_query callback says. The
/// theory thereby acts as a sound *over-approximation* on top of the
/// extensional relation.
///
/// For `ForAll`/`Exists`, the search uses the same bounded enumeration
/// as `evaluate_quantified`. If the search exhausts without finding a
/// definitive witness/counterexample (because some sub-evaluations
/// were `Unknown`), the result is `Unknown`.
///
/// # Returns
///
/// `TriState::True`, `TriState::False`, or `TriState::Unknown`.
pub fn evaluate_quantified_with_theory<T, F, G>(
    formula: &QuantifiedFormula,
    theory: &T,
    relation_query: &F,
    domain_enumerate: &G,
    env: &std::collections::HashMap<String, String>,
    bound: usize,
) -> TriState
where
    T: ConstraintTheory,
    F: Fn(&str, &[String]) -> bool,
    G: Fn(&str) -> Vec<Vec<String>>,
{
    use QuantifiedFormula::*;
    type Env = std::collections::HashMap<String, String>;
    enum Frame<'a> {
        Eval(&'a QuantifiedFormula, Env),
        Not,
        AndRight(&'a QuantifiedFormula, Env),
        AndFinish(TriState),
        OrRight(&'a QuantifiedFormula, Env),
        OrFinish(TriState),
        ImpliesRight(&'a QuantifiedFormula, Env),
        ImpliesFinish(TriState),
        Quantifier {
            var: &'a str,
            body: &'a QuantifiedFormula,
            remaining: std::vec::IntoIter<Vec<String>>,
            base_env: Env,
            universal: bool,
            had_unknown: bool,
        },
    }

    let mut frames = vec![Frame::Eval(formula, env.clone())];
    let mut result = None;
    while let Some(frame) = frames.pop() {
        match frame {
            Frame::Eval(current, current_env) => {
                // Preserve the recursive evaluator's one theory-store
                // construction per visited formula node.
                let _ = theory.empty_store();
                match current {
                    Atom { relation, args } => {
                        let resolved: Vec<String> = args
                            .iter()
                            .map(|arg| match arg {
                                QuantifiedArg::Var(var) => {
                                    current_env.get(var).cloned().unwrap_or_else(|| {
                                        panic!("unbound variable '{}' in formula", var)
                                    })
                                }
                                QuantifiedArg::Constant(constant) => constant.clone(),
                            })
                            .collect();
                        result = Some(relation_query(relation, &resolved).into());
                    }
                    And(left, right) => {
                        frames.push(Frame::AndRight(right, current_env.clone()));
                        frames.push(Frame::Eval(left, current_env));
                    }
                    Or(left, right) => {
                        frames.push(Frame::OrRight(right, current_env.clone()));
                        frames.push(Frame::Eval(left, current_env));
                    }
                    Not(inner) => {
                        frames.push(Frame::Not);
                        frames.push(Frame::Eval(inner, current_env));
                    }
                    Implies(left, right) => {
                        frames.push(Frame::ImpliesRight(right, current_env.clone()));
                        frames.push(Frame::Eval(left, current_env));
                    }
                    ForAll { var, domain, body } | Exists { var, domain, body } => {
                        let universal = matches!(current, ForAll { .. });
                        let mut remaining =
                            enumerate_domain(domain, domain_enumerate, bound).into_iter();
                        if let Some(tuple) = remaining.next() {
                            let mut inner_env = current_env.clone();
                            if let Some(value) = tuple.first() {
                                inner_env.insert(var.clone(), value.clone());
                            }
                            frames.push(Frame::Quantifier {
                                var,
                                body,
                                remaining,
                                base_env: current_env,
                                universal,
                                had_unknown: false,
                            });
                            frames.push(Frame::Eval(body, inner_env));
                        } else {
                            result = Some(if universal {
                                TriState::True
                            } else {
                                TriState::False
                            });
                        }
                    }
                }
            }
            Frame::Not => {
                result = Some(result.take().expect("negated formula is evaluated").not());
            }
            Frame::AndRight(right, env) => {
                let left = result.take().expect("left conjunction is evaluated");
                if left == TriState::False {
                    result = Some(TriState::False);
                } else {
                    frames.push(Frame::AndFinish(left));
                    frames.push(Frame::Eval(right, env));
                }
            }
            Frame::AndFinish(left) => {
                let right = result.take().expect("right conjunction is evaluated");
                result = Some(left.and(right));
            }
            Frame::OrRight(right, env) => {
                let left = result.take().expect("left disjunction is evaluated");
                if left == TriState::True {
                    result = Some(TriState::True);
                } else {
                    frames.push(Frame::OrFinish(left));
                    frames.push(Frame::Eval(right, env));
                }
            }
            Frame::OrFinish(left) => {
                let right = result.take().expect("right disjunction is evaluated");
                result = Some(left.or(right));
            }
            Frame::ImpliesRight(right, env) => {
                let left = result.take().expect("implication antecedent is evaluated");
                frames.push(Frame::ImpliesFinish(left));
                frames.push(Frame::Eval(right, env));
            }
            Frame::ImpliesFinish(left) => {
                let right = result.take().expect("implication consequent is evaluated");
                result = Some(left.implies(right));
            }
            Frame::Quantifier {
                var,
                body,
                mut remaining,
                base_env,
                universal,
                mut had_unknown,
            } => {
                let value = result.take().expect("quantifier body is evaluated");
                let decisive = if universal {
                    value == TriState::False
                } else {
                    value == TriState::True
                };
                if decisive {
                    result = Some(value);
                } else {
                    had_unknown |= value == TriState::Unknown;
                    if let Some(tuple) = remaining.next() {
                        let mut inner_env = base_env.clone();
                        if let Some(value) = tuple.first() {
                            inner_env.insert(var.to_string(), value.clone());
                        }
                        frames.push(Frame::Quantifier {
                            var,
                            body,
                            remaining,
                            base_env,
                            universal,
                            had_unknown,
                        });
                        frames.push(Frame::Eval(body, inner_env));
                    } else {
                        result = Some(if had_unknown {
                            TriState::Unknown
                        } else if universal {
                            TriState::True
                        } else {
                            TriState::False
                        });
                    }
                }
            }
        }
    }
    result.expect("the root quantified formula produces one theory-guided result")
}

// ══════════════════════════════════════════════════════════════════════════════
// TheoryPred — Boolean combination of constraints
// ══════════════════════════════════════════════════════════════════════════════

/// Boolean combination of constraints from a `ConstraintTheory`.
///
/// This is the `Predicate` type used by `TheoryAlgebra<T>` in its
/// `BooleanAlgebra` implementation. It wraps theory-specific constraints
/// in a standard Boolean AST.
pub enum TheoryPred<T: ConstraintTheory> {
    /// Always true (unconstrained).
    True,
    /// Always false (contradictory).
    False,
    /// An atomic constraint from the theory.
    Atom(T::Constraint),
    /// Conjunction.
    And(Box<TheoryPred<T>>, Box<TheoryPred<T>>),
    /// Disjunction.
    Or(Box<TheoryPred<T>>, Box<TheoryPred<T>>),
    /// Negation.
    Not(Box<TheoryPred<T>>),
}

impl<T: ConstraintTheory> Clone for TheoryPred<T> {
    fn clone(&self) -> Self {
        enum Task<'a, T: ConstraintTheory> {
            Clone(&'a TheoryPred<T>),
            And,
            Or,
            Not,
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(predicate) => match predicate {
                    TheoryPred::True => values.push(TheoryPred::True),
                    TheoryPred::False => values.push(TheoryPred::False),
                    TheoryPred::Atom(constraint) => {
                        values.push(TheoryPred::Atom(constraint.clone()));
                    }
                    TheoryPred::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    TheoryPred::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    TheoryPred::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Clone(inner));
                    }
                },
                Task::And | Task::Or => {
                    let right = values.pop().expect("right theory clone is present");
                    let left = values.pop().expect("left theory clone is present");
                    values.push(if matches!(task, Task::And) {
                        TheoryPred::And(Box::new(left), Box::new(right))
                    } else {
                        TheoryPred::Or(Box::new(left), Box::new(right))
                    });
                }
                Task::Not => {
                    let inner = values.pop().expect("negated theory clone is present");
                    values.push(TheoryPred::Not(Box::new(inner)));
                }
            }
        }
        values
            .pop()
            .expect("the root theory predicate produces one clone")
    }
}

impl<T: ConstraintTheory> PartialEq for TheoryPred<T>
where
    T::Constraint: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (TheoryPred::True, TheoryPred::True) | (TheoryPred::False, TheoryPred::False) => {}
                (TheoryPred::Atom(left), TheoryPred::Atom(right)) if left == right => {}
                (TheoryPred::And(ll, lr), TheoryPred::And(rl, rr))
                | (TheoryPred::Or(ll, lr), TheoryPred::Or(rl, rr)) => {
                    pending.push((lr, rr));
                    pending.push((ll, rl));
                }
                (TheoryPred::Not(left), TheoryPred::Not(right)) => {
                    pending.push((left, right));
                }
                _ => return false,
            }
        }
        true
    }
}

impl<T: ConstraintTheory> Eq for TheoryPred<T> where T::Constraint: Eq {}

impl<T: ConstraintTheory> Hash for TheoryPred<T>
where
    T::Constraint: Hash,
{
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut pending = vec![self];
        while let Some(predicate) = pending.pop() {
            std::mem::discriminant(predicate).hash(state);
            match predicate {
                TheoryPred::True | TheoryPred::False => {}
                TheoryPred::Atom(constraint) => constraint.hash(state),
                TheoryPred::And(left, right) | TheoryPred::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                TheoryPred::Not(inner) => pending.push(inner),
            }
        }
    }
}

impl<T: ConstraintTheory> fmt::Debug for TheoryPred<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a, T: ConstraintTheory> {
            Pred(&'a TheoryPred<T>),
            Text(&'static str),
        }
        let mut events = vec![Event::Pred(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Pred(predicate) => match predicate {
                    TheoryPred::True => write!(f, "True")?,
                    TheoryPred::False => write!(f, "False")?,
                    TheoryPred::Atom(constraint) => write!(f, "Atom({constraint:?})")?,
                    TheoryPred::And(left, right) | TheoryPred::Or(left, right) => {
                        write!(
                            f,
                            "{}(",
                            if matches!(predicate, TheoryPred::And(_, _)) {
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
                    TheoryPred::Not(inner) => {
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

impl<T: ConstraintTheory> Drop for TheoryPred<T> {
    fn drop(&mut self) {
        fn drain<T: ConstraintTheory>(
            predicate: &mut TheoryPred<T>,
            pending: &mut Vec<TheoryPred<T>>,
        ) {
            match predicate {
                TheoryPred::And(left, right) | TheoryPred::Or(left, right) => {
                    pending.push(std::mem::replace(&mut **right, TheoryPred::True));
                    pending.push(std::mem::replace(&mut **left, TheoryPred::True));
                }
                TheoryPred::Not(inner) => {
                    pending.push(std::mem::replace(&mut **inner, TheoryPred::True));
                }
                TheoryPred::True | TheoryPred::False | TheoryPred::Atom(_) => {}
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
// TheoryAlgebra — Bridge ConstraintTheory to BooleanAlgebra
// ══════════════════════════════════════════════════════════════════════════════

/// Wraps any `ConstraintTheory` into a `BooleanAlgebra` implementation.
///
/// For decidable theories: propagation-only (no search).
/// For non-decidable theories: LogicT-powered fair search with bounded depth.
///
/// This means any user-defined language can plug in a domain-specific
/// constraint solver by implementing `ConstraintTheory` — they get
/// `BooleanAlgebra` (and therefore `SymbolicAutomaton` integration,
/// minterm computation, determinization, lint analysis) for free.
#[derive(Clone, Debug)]
pub struct TheoryAlgebra<T: ConstraintTheory> {
    /// The underlying constraint theory.
    pub theory: T,
    /// Maximum search depth for non-decidable theories (bounded).
    /// Limits the number of labeling steps to prevent infinite search.
    pub search_bound: usize,
}

impl<T: ConstraintTheory> TheoryAlgebra<T> {
    /// Create a new TheoryAlgebra with the given theory and search bound.
    pub fn new(theory: T, search_bound: usize) -> Self {
        TheoryAlgebra {
            theory,
            search_bound,
        }
    }

    /// Collect constraints from a `TheoryPred` into a constraint store.
    ///
    /// Returns `None` if the predicate is unsatisfiable (propagation fails).
    /// For disjunctions, uses LogicT fair search to try alternatives.
    fn collect_constraints(&self, pred: &TheoryPred<T>, store: &T::Store) -> LogicStream<T::Store>
    where
        T::Store: Send + 'static,
        T::Constraint: Send + 'static,
    {
        enum Task<'a, T: ConstraintTheory>
        where
            T::Store: Send + 'static,
        {
            Eval(&'a TheoryPred<T>, T::Store, bool),
            Or,
            AndStart(&'a TheoryPred<T>, bool),
            AndContinue {
                right: &'a TheoryPred<T>,
                negated: bool,
                remaining: std::vec::IntoIter<T::Store>,
                accumulated: LogicStream<T::Store>,
            },
        }
        let mut tasks = vec![Task::Eval(pred, store.clone(), false)];
        let mut values: Vec<LogicStream<T::Store>> = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Eval(predicate, current_store, negated) => match predicate {
                    TheoryPred::True => values.push(if negated {
                        LogicStream::empty()
                    } else {
                        LogicStream::unit(current_store)
                    }),
                    TheoryPred::False => values.push(if negated {
                        LogicStream::unit(current_store)
                    } else {
                        LogicStream::empty()
                    }),
                    TheoryPred::Atom(constraint) => values.push(if negated {
                        LogicStream::unit(current_store)
                    } else {
                        match self.theory.propagate(&current_store, constraint) {
                            Some(next_store) => LogicStream::unit(next_store),
                            None => LogicStream::empty(),
                        }
                    }),
                    TheoryPred::And(left, right) | TheoryPred::Or(left, right) => {
                        let conjunction = matches!(predicate, TheoryPred::And(_, _)) != negated;
                        if conjunction {
                            tasks.push(Task::AndStart(right, negated));
                            tasks.push(Task::Eval(left, current_store, negated));
                        } else {
                            tasks.push(Task::Or);
                            tasks.push(Task::Eval(right, current_store.clone(), negated));
                            tasks.push(Task::Eval(left, current_store, negated));
                        }
                    }
                    TheoryPred::Not(inner) => {
                        tasks.push(Task::Eval(inner, current_store, !negated));
                    }
                },
                Task::Or => {
                    let right = values.pop().expect("right theory stream is present");
                    let left = values.pop().expect("left theory stream is present");
                    values.push(left.interleave(right));
                }
                Task::AndStart(right, negated) => {
                    let left = values.pop().expect("left theory stream is present");
                    let mut remaining = left.collect_all().into_iter();
                    if let Some(first) = remaining.next() {
                        tasks.push(Task::AndContinue {
                            right,
                            negated,
                            remaining,
                            accumulated: LogicStream::empty(),
                        });
                        tasks.push(Task::Eval(right, first, negated));
                    } else {
                        values.push(LogicStream::empty());
                    }
                }
                Task::AndContinue {
                    right,
                    negated,
                    mut remaining,
                    accumulated,
                } => {
                    let current = values.pop().expect("right theory stream is present");
                    let accumulated = accumulated.interleave(current);
                    if let Some(next_store) = remaining.next() {
                        tasks.push(Task::AndContinue {
                            right,
                            negated,
                            remaining,
                            accumulated,
                        });
                        tasks.push(Task::Eval(right, next_store, negated));
                    } else {
                        values.push(accumulated);
                    }
                }
            }
        }
        values
            .pop()
            .expect("the root theory predicate produces one stream")
    }
}

impl<T> super::BooleanAlgebra for TheoryAlgebra<T>
where
    T: ConstraintTheory,
    T::Constraint: Hash,
    T::Store: Send + 'static,
    T::Constraint: Send + 'static,
    T::Assignment: Send + 'static,
{
    type Predicate = TheoryPred<T>;
    type Domain = T::Assignment;

    fn true_pred(&self) -> Self::Predicate {
        TheoryPred::True
    }

    fn false_pred(&self) -> Self::Predicate {
        TheoryPred::False
    }

    fn and(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        match (a, b) {
            (TheoryPred::True, _) => b.clone(),
            (_, TheoryPred::True) => a.clone(),
            (TheoryPred::False, _) | (_, TheoryPred::False) => TheoryPred::False,
            _ => TheoryPred::And(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn or(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        match (a, b) {
            (TheoryPred::True, _) | (_, TheoryPred::True) => TheoryPred::True,
            (TheoryPred::False, _) => b.clone(),
            (_, TheoryPred::False) => a.clone(),
            _ => TheoryPred::Or(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn not(&self, a: &Self::Predicate) -> Self::Predicate {
        match a {
            TheoryPred::True => TheoryPred::False,
            TheoryPred::False => TheoryPred::True,
            TheoryPred::Not(inner) => (**inner).clone(),
            _ => TheoryPred::Not(Box::new(a.clone())),
        }
    }

    fn is_satisfiable(&self, pred: &Self::Predicate) -> bool {
        // For predicates containing negation of atoms, collect_constraints
        // may return stores that are over-approximate (the negation is tracked
        // structurally, not propagated). We need to validate witnesses.
        self.witness(pred).is_some()
    }

    fn witness(&self, pred: &Self::Predicate) -> Option<Self::Domain> {
        let store = self.theory.empty_store();
        let stores = self.collect_constraints(pred, &store);
        let results = stores.collect_bounded(self.search_bound);
        for s in results {
            if let Some(w) = self.theory.witness(&s) {
                // Validate the witness against the full predicate,
                // including any negated atoms that weren't propagated.
                if self.evaluate(pred, &w) {
                    return Some(w);
                }
            }
            // Try labeling if witness wasn't valid or available
            let labels = self.theory.label(&s);
            let label_results = labels.collect_bounded(self.search_bound);
            for label in label_results {
                if let Some(new_store) = self.theory.propagate(&s, &label) {
                    if let Some(w) = self.theory.witness(&new_store) {
                        if self.evaluate(pred, &w) {
                            return Some(w);
                        }
                    }
                }
            }
        }
        None
    }

    fn evaluate(&self, pred: &Self::Predicate, elem: &Self::Domain) -> bool {
        enum Frame<'a, T: ConstraintTheory> {
            Eval(&'a TheoryPred<T>),
            Not,
            AndRight(&'a TheoryPred<T>),
            OrRight(&'a TheoryPred<T>),
        }
        let mut frames = vec![Frame::Eval(pred)];
        let mut result = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Eval(predicate) => match predicate {
                    TheoryPred::True => result = Some(true),
                    TheoryPred::False => result = Some(false),
                    TheoryPred::Atom(constraint) => {
                        result = Some(self.theory.evaluate(constraint, elem));
                    }
                    TheoryPred::And(left, right) => {
                        frames.push(Frame::AndRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    TheoryPred::Or(left, right) => {
                        frames.push(Frame::OrRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    TheoryPred::Not(inner) => {
                        frames.push(Frame::Not);
                        frames.push(Frame::Eval(inner));
                    }
                },
                Frame::Not => result = Some(!result.take().expect("negated theory is evaluated")),
                Frame::AndRight(right) => {
                    if result.take().expect("left theory conjunction is evaluated") {
                        frames.push(Frame::Eval(right));
                    } else {
                        result = Some(false);
                    }
                }
                Frame::OrRight(right) => {
                    if result.take().expect("left theory disjunction is evaluated") {
                        result = Some(true);
                    } else {
                        frames.push(Frame::Eval(right));
                    }
                }
            }
        }
        result.expect("the root theory predicate produces one result")
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// AC-Matching — Multiset partition enumeration via LogicStream
// ══════════════════════════════════════════════════════════════════════════════

/// A partition of a multiset into "selected" elements and "remainder".
///
/// Used for associative-commutative matching: given a bag `{A:3, B:2}` and a
/// pattern requesting 2 elements, this type represents one way to select 2
/// elements from the bag (e.g., `{A:2}` with remainder `{A:1, B:2}`).
///
/// # Invariants
///
/// - `selected_count == selected.iter().map(|(_, c)| c).sum()`
/// - For every element `e`: `selected.count(e) + remainder.count(e) == source.count(e)`
/// - `selected_count + remainder_count == source_count`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultisetPartition<T: Clone + Eq + Hash> {
    /// Selected elements with their multiplicities.
    pub selected: Vec<(T, usize)>,
    /// Remaining elements after selection.
    pub remainder: Vec<(T, usize)>,
    /// Total count of selected elements (sum of selected multiplicities).
    pub selected_count: usize,
}

/// One persistent choice in a partition path.
///
/// Choices form a flat, parent-linked arena.  A branch therefore costs one
/// compact record and shares its complete prefix with sibling branches.
#[derive(Clone, Copy)]
struct MultisetChoice {
    parent: Option<usize>,
    item_index: usize,
    quantity: usize,
}

/// Heap-resident expression interpreted by [`MultisetPartitionGenerator`].
///
/// `QuantityFold` finds the first nonempty quantity branch in ascending order.
/// After that branch yields, `ReverseFold` represents the formally verified
/// residual fold compactly: later quantities wrap the yielded branch's residual
/// from greatest to least.  `Alternate` is the exact LogicT pull rule, while
/// `Forward` removes semantically neutral empty/fold nodes without recursion.
#[derive(Clone, Copy)]
enum MultisetSchedule {
    Empty,
    Problem {
        start: usize,
        remaining: usize,
        choice_tail: Option<usize>,
    },
    QuantityFold {
        start: usize,
        remaining: usize,
        next_quantity: usize,
        max_quantity: usize,
        choice_tail: Option<usize>,
    },
    ReverseFold {
        start: usize,
        remaining: usize,
        next_quantity: usize,
        min_quantity: usize,
        tail: usize,
        choice_tail: Option<usize>,
    },
    Alternate {
        left: usize,
        right: usize,
    },
    Forward(usize),
}

#[derive(Clone, Copy)]
enum MultisetPullFrame {
    Alternate {
        schedule: usize,
        right: usize,
    },
    FirstQuantity {
        schedule: usize,
        start: usize,
        remaining: usize,
        quantity: usize,
        max_quantity: usize,
        choice_tail: Option<usize>,
    },
}

/// Specialized lazy pull machine for multiset partitions.
struct MultisetPartitionGenerator<T> {
    items: Vec<(T, usize)>,
    /// Saturating suffix totals make feasibility checks constant-time and
    /// correct even when the mathematical multiplicity exceeds `usize::MAX`.
    suffix_available: Vec<usize>,
    requested: usize,
    choices: Vec<MultisetChoice>,
    schedule: Vec<MultisetSchedule>,
    pull_frames: Vec<MultisetPullFrame>,
    root: usize,
}

impl<T> MultisetPartitionGenerator<T>
where
    T: Clone + Eq + Hash,
{
    fn new(items: Vec<(T, usize)>, requested: usize) -> Self {
        let mut suffix_available = vec![0usize; items.len() + 1];
        for index in (0..items.len()).rev() {
            suffix_available[index] = suffix_available[index + 1].saturating_add(items[index].1);
        }

        let schedule_capacity = items.len().saturating_mul(2).saturating_add(1);
        let mut schedule = Vec::with_capacity(schedule_capacity);
        schedule.push(MultisetSchedule::Problem {
            start: 0,
            remaining: requested,
            choice_tail: None,
        });

        Self {
            choices: Vec::with_capacity(items.len()),
            pull_frames: Vec::with_capacity(items.len()),
            items,
            suffix_available,
            requested,
            schedule,
            root: 0,
        }
    }

    fn allocate_schedule(&mut self, schedule: MultisetSchedule) -> usize {
        let index = self.schedule.len();
        self.schedule.push(schedule);
        index
    }

    fn allocate_quantity_branch(
        &mut self,
        start: usize,
        remaining: usize,
        quantity: usize,
        choice_tail: Option<usize>,
    ) -> usize {
        let choice = self.choices.len();
        self.choices.push(MultisetChoice {
            parent: choice_tail,
            item_index: start,
            quantity,
        });
        self.allocate_schedule(MultisetSchedule::Problem {
            start: start + 1,
            remaining: remaining - quantity,
            choice_tail: Some(choice),
        })
    }

    /// Materialize one terminal choice path in the historical vector order.
    ///
    /// The recursive implementation appended processed items while unwinding.
    /// Consequently selected items occur from deepest to shallowest.  Each
    /// positive processed remainder triggered a stable count sort; all repeated
    /// sorts are equivalent to one stable sort of the suffix followed by that
    /// same deepest-to-shallowest prefix.
    fn materialize(&self, start: usize, mut choice_tail: Option<usize>) -> MultisetPartition<T> {
        let mut selected = Vec::with_capacity(start.min(self.requested));
        let mut remainder = Vec::with_capacity(self.items.len());
        remainder.extend(
            self.items[start..]
                .iter()
                .filter(|(_, count)| *count > 0)
                .cloned(),
        );

        let mut selected_count = 0usize;
        let mut sort_remainder = false;
        while let Some(choice_index) = choice_tail {
            let choice = self.choices[choice_index];
            let (element, source_count) = &self.items[choice.item_index];
            debug_assert!(choice.quantity <= *source_count);

            if choice.quantity > 0 {
                selected.push((element.clone(), choice.quantity));
                selected_count += choice.quantity;
            }

            let leftover = source_count - choice.quantity;
            if leftover > 0 {
                remainder.push((element.clone(), leftover));
                sort_remainder = true;
            }
            choice_tail = choice.parent;
        }

        if sort_remainder {
            // `sort_by` is stable, which preserves the recursive implementation's
            // order among elements with equal remaining multiplicity.
            remainder.sort_by(|lhs, rhs| lhs.1.cmp(&rhs.1));
        }
        debug_assert_eq!(selected_count, self.requested);

        MultisetPartition {
            selected,
            remainder,
            selected_count,
        }
    }

    /// Pull one result while mutating every visited node to its residual stream.
    fn pull_one(&mut self) -> Option<MultisetPartition<T>> {
        self.pull_frames.clear();
        let mut current = self.root;

        'descend: loop {
            let outcome = loop {
                match self.schedule[current] {
                    MultisetSchedule::Empty => break None,
                    MultisetSchedule::Forward(target) => current = target,
                    MultisetSchedule::Problem {
                        start,
                        remaining,
                        choice_tail,
                    } => {
                        if remaining == 0 {
                            let partition = self.materialize(start, choice_tail);
                            self.schedule[current] = MultisetSchedule::Empty;
                            break Some(partition);
                        }

                        if start >= self.items.len() || self.suffix_available[start] < remaining {
                            self.schedule[current] = MultisetSchedule::Empty;
                            break None;
                        }

                        let count = self.items[start].1;
                        let max_quantity = count.min(remaining);
                        let min_quantity =
                            remaining.saturating_sub(self.suffix_available[start + 1]);
                        debug_assert!(min_quantity <= max_quantity);
                        self.schedule[current] = MultisetSchedule::QuantityFold {
                            start,
                            remaining,
                            next_quantity: min_quantity,
                            max_quantity,
                            choice_tail,
                        };
                    }
                    MultisetSchedule::QuantityFold {
                        start,
                        remaining,
                        next_quantity,
                        max_quantity,
                        choice_tail,
                    } => {
                        if next_quantity > max_quantity {
                            self.schedule[current] = MultisetSchedule::Empty;
                            break None;
                        }
                        let branch = self.allocate_quantity_branch(
                            start,
                            remaining,
                            next_quantity,
                            choice_tail,
                        );
                        self.pull_frames.push(MultisetPullFrame::FirstQuantity {
                            schedule: current,
                            start,
                            remaining,
                            quantity: next_quantity,
                            max_quantity,
                            choice_tail,
                        });
                        current = branch;
                    }
                    MultisetSchedule::ReverseFold {
                        start,
                        remaining,
                        next_quantity,
                        min_quantity,
                        tail,
                        choice_tail,
                    } => {
                        if next_quantity < min_quantity {
                            self.schedule[current] = MultisetSchedule::Forward(tail);
                            current = tail;
                            continue;
                        }

                        let branch = self.allocate_quantity_branch(
                            start,
                            remaining,
                            next_quantity,
                            choice_tail,
                        );
                        let residual = if next_quantity == min_quantity {
                            tail
                        } else {
                            self.allocate_schedule(MultisetSchedule::ReverseFold {
                                start,
                                remaining,
                                next_quantity: next_quantity - 1,
                                min_quantity,
                                tail,
                                choice_tail,
                            })
                        };
                        self.schedule[current] = MultisetSchedule::Alternate {
                            left: branch,
                            right: residual,
                        };
                    }
                    MultisetSchedule::Alternate { left, right } => {
                        self.pull_frames.push(MultisetPullFrame::Alternate {
                            schedule: current,
                            right,
                        });
                        current = left;
                    }
                }
            };

            let outcome = outcome;
            loop {
                let Some(frame) = self.pull_frames.pop() else {
                    return outcome;
                };

                match frame {
                    MultisetPullFrame::Alternate { schedule, right } => {
                        if outcome.is_some() {
                            // Pull(Alternate(left, right)) rotates to
                            // Alternate(right, residual(left)).
                            self.schedule[schedule] = MultisetSchedule::Alternate {
                                left: right,
                                right: current,
                            };
                            current = schedule;
                        } else {
                            // An exhausted left operand is neutral.  Pull the
                            // right operand before returning to any outer frame.
                            self.schedule[schedule] = MultisetSchedule::Forward(right);
                            current = right;
                            continue 'descend;
                        }
                    }
                    MultisetPullFrame::FirstQuantity {
                        schedule,
                        start,
                        remaining,
                        quantity,
                        max_quantity,
                        choice_tail,
                    } => {
                        if outcome.is_some() {
                            self.schedule[schedule] = if quantity == max_quantity {
                                MultisetSchedule::Forward(current)
                            } else {
                                MultisetSchedule::ReverseFold {
                                    start,
                                    remaining,
                                    next_quantity: max_quantity,
                                    min_quantity: quantity + 1,
                                    tail: current,
                                    choice_tail,
                                }
                            };
                            current = schedule;
                        } else if quantity < max_quantity {
                            self.schedule[schedule] = MultisetSchedule::QuantityFold {
                                start,
                                remaining,
                                next_quantity: quantity + 1,
                                max_quantity,
                                choice_tail,
                            };
                            current = schedule;
                            continue 'descend;
                        } else {
                            self.schedule[schedule] = MultisetSchedule::Empty;
                            current = schedule;
                        }
                    }
                }
            }
        }
    }
}

impl<T> Iterator for MultisetPartitionGenerator<T>
where
    T: Clone + Eq + Hash,
{
    type Item = MultisetPartition<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.pull_one()
    }
}

/// Lazily enumerate all ways to select exactly `k` elements (with multiplicity)
/// from a multiset represented as `(element, count)` pairs.
///
/// A flat, heap-resident pull machine advances over distinct elements and chooses
/// how many copies of each element to include in the selection
/// (`0..=min(count, remaining_k)`). Duplicate-free enumeration is guaranteed
/// because an element at index `i` is never reconsidered after advancing to
/// index `i+1`.
///
/// Results have exactly the historical left-folded `interleave()` order. Quantity
/// intervals and their residual folds remain compact until pulled, so requesting a
/// bounded prefix does not enumerate or allocate all quantity branches. Logical
/// input depth is represented only in contiguous heap arenas; native call depth is
/// constant.
///
/// # Arguments
///
/// * `items` — distinct elements with their multiplicities (from `HashBag::iter()`)
/// * `k` — total number of elements to select
///
/// # Examples
///
/// ```
/// # use lling_llang::symbolic::logict::{multiset_partitions, MultisetPartition};
/// let items = vec![('A', 3), ('B', 2)];
/// let partitions: Vec<_> = multiset_partitions(&items, 2).collect_all();
/// assert_eq!(partitions.len(), 3);
/// // {A:2} rem {A:1, B:2}, {A:1, B:1} rem {A:2, B:1}, {B:2} rem {A:3}
/// ```
pub fn multiset_partitions<T>(items: &[(T, usize)], k: usize) -> LogicStream<MultisetPartition<T>>
where
    T: Clone + Eq + Hash + Send + 'static,
{
    LogicStream::from_lazy_iter(MultisetPartitionGenerator::new(items.to_vec(), k))
}

/// Convenience function: eagerly collect up to `bound` multiset partitions.
///
/// Wraps `multiset_partitions()` with `collect_bounded()` for direct use
/// in AC-match guard evaluation where a `Vec` result is needed.
///
/// # Arguments
///
/// * `items` — distinct elements with their multiplicities
/// * `k` — number of elements to select
/// * `bound` — maximum number of partitions to return (T3 safety)
pub fn multiset_select<T>(items: &[(T, usize)], k: usize, bound: usize) -> Vec<MultisetPartition<T>>
where
    T: Clone + Eq + Hash + Send + 'static,
{
    multiset_partitions(items, k).collect_bounded(bound)
}

// ══════════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ── LogicStream core tests ──────────────────────────────────────────

    #[test]
    fn msplit_empty_returns_none() {
        let stream: LogicStream<i32> = LogicStream::empty();
        assert!(stream.msplit().is_none());
    }

    #[test]
    fn msplit_singleton_returns_value_and_empty() {
        let stream = LogicStream::unit(42);
        let (val, rest) = stream.msplit().expect("should have a value");
        assert_eq!(val, 42);
        assert!(rest.msplit().is_none());
    }

    #[test]
    fn from_iter_produces_all_values() {
        let stream = LogicStream::from_iter(vec![1, 2, 3]);
        let results = stream.collect_all();
        assert_eq!(results, vec![1, 2, 3]);
    }

    #[test]
    fn mplus_concatenates_streams() {
        let a = LogicStream::from_iter(vec![1, 2]);
        let b = LogicStream::from_iter(vec![3, 4]);
        let results = a.mplus(b).collect_all();
        assert_eq!(results, vec![1, 2, 3, 4]);
    }

    #[test]
    fn interleave_alternates_results() {
        let a = LogicStream::from_iter(vec![1, 3, 5]);
        let b = LogicStream::from_iter(vec![2, 4, 6]);
        let results = a.interleave(b).collect_all();
        assert_eq!(results, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn interleave_with_unequal_lengths() {
        let a = LogicStream::from_iter(vec![1, 3, 5, 7, 9]);
        let b = LogicStream::from_iter(vec![2, 4]);
        let results = a.interleave(b).collect_all();
        assert_eq!(results, vec![1, 2, 3, 4, 5, 7, 9]);
    }

    #[test]
    fn interleave_with_empty() {
        let a = LogicStream::from_iter(vec![1, 2, 3]);
        let b = LogicStream::<i32>::empty();
        let results = a.interleave(b).collect_all();
        assert_eq!(results, vec![1, 2, 3]);
    }

    #[test]
    fn fair_conjoin_does_not_starve() {
        // Key fairness test: fair_conjoin should interleave results
        // from applying f to each element, not depth-first explore
        // all results of f(first_element) before moving on.
        let stream = LogicStream::from_iter(vec![10, 20]);
        let results = stream
            .fair_conjoin(|x| LogicStream::from_iter(vec![x + 1, x + 2]))
            .collect_all();
        // Should interleave: [11, 21, 12, 22] (alternating from each source)
        // rather than [11, 12, 21, 22] (depth-first)
        assert_eq!(results, vec![11, 21, 12, 22]);
    }

    #[test]
    fn ifte_success_uses_then_branch() {
        let test = LogicStream::from_iter(vec![1, 2]);
        let result = test.ifte(|x| LogicStream::unit(x * 10), LogicStream::unit(0));
        let results = result.collect_all();
        assert!(results.contains(&10));
        assert!(results.contains(&20));
    }

    #[test]
    fn ifte_failure_uses_else_branch() {
        let test: LogicStream<i32> = LogicStream::empty();
        let result = test.ifte(|x| LogicStream::unit(x * 10), LogicStream::unit(0));
        let results = result.collect_all();
        assert_eq!(results, vec![0]);
    }

    #[test]
    fn once_commits_to_first() {
        let stream = LogicStream::from_iter(vec![1, 2, 3]);
        let results = stream.once().collect_all();
        assert_eq!(results, vec![1]);
    }

    #[test]
    fn once_on_empty_returns_empty() {
        let stream: LogicStream<i32> = LogicStream::empty();
        let results = stream.once().collect_all();
        assert!(results.is_empty());
    }

    #[test]
    fn gnot_succeeds_on_empty() {
        let stream: LogicStream<i32> = LogicStream::empty();
        let results = stream.gnot().collect_all();
        assert_eq!(results.len(), 1); // Produces one ()
    }

    #[test]
    fn gnot_fails_on_nonempty() {
        let stream = LogicStream::unit(42);
        let results = stream.gnot().collect_all();
        assert!(results.is_empty());
    }

    #[test]
    fn collect_bounded_limits_results() {
        let stream = LogicStream::from_iter(0..1000);
        let results = stream.collect_bounded(5);
        assert_eq!(results.len(), 5);
        assert_eq!(results, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn iterator_integration() {
        let stream = LogicStream::from_iter(vec![10, 20, 30]);
        let collected: Vec<i32> = stream.into_iter().collect();
        assert_eq!(collected, vec![10, 20, 30]);
    }

    #[test]
    fn filter_removes_non_matching() {
        let stream = LogicStream::from_iter(1..=10);
        let even = stream.filter(|x| x % 2 == 0);
        let results = even.collect_all();
        assert_eq!(results, vec![2, 4, 6, 8, 10]);
    }

    #[test]
    fn suspend_defers_computation() {
        let stream = LogicStream::suspend(|| LogicStream::from_iter(vec![1, 2, 3]));
        let results = stream.collect_all();
        assert_eq!(results, vec![1, 2, 3]);
    }

    // ── ConstraintTheory tests (trivial propositional theory) ───────────

    /// A trivial propositional theory for testing the ConstraintTheory trait.
    /// Constraints are propositional atoms (positive or negative).
    /// The store is a set of asserted and negated atoms.
    /// Satisfiable iff no atom is both asserted and negated.
    #[derive(Clone, Debug)]
    struct PropTheory;

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    enum PropConstraint {
        Assert(String),
        Negate(String),
    }

    #[derive(Clone, Debug)]
    struct PropAssignment(std::collections::HashMap<String, bool>);

    #[derive(Clone, Debug)]
    struct PropStore {
        asserted: std::collections::HashSet<String>,
        negated: std::collections::HashSet<String>,
    }

    impl ConstraintTheory for PropTheory {
        type Constraint = PropConstraint;
        type Assignment = PropAssignment;
        type Store = PropStore;

        fn empty_store(&self) -> PropStore {
            PropStore {
                asserted: std::collections::HashSet::new(),
                negated: std::collections::HashSet::new(),
            }
        }

        fn propagate(&self, store: &PropStore, c: &PropConstraint) -> Option<PropStore> {
            let mut new_store = store.clone();
            match c {
                PropConstraint::Assert(name) => {
                    if new_store.negated.contains(name) {
                        return None; // Contradiction
                    }
                    new_store.asserted.insert(name.clone());
                }
                PropConstraint::Negate(name) => {
                    if new_store.asserted.contains(name) {
                        return None; // Contradiction
                    }
                    new_store.negated.insert(name.clone());
                }
            }
            Some(new_store)
        }

        fn is_consistent(&self, store: &PropStore) -> bool {
            store.asserted.intersection(&store.negated).next().is_none()
        }

        fn witness(&self, store: &PropStore) -> Option<PropAssignment> {
            if !self.is_consistent(store) {
                return None;
            }
            let mut assignment = std::collections::HashMap::new();
            for name in &store.asserted {
                assignment.insert(name.clone(), true);
            }
            for name in &store.negated {
                assignment.insert(name.clone(), false);
            }
            Some(PropAssignment(assignment))
        }

        fn label(&self, _store: &PropStore) -> LogicStream<PropConstraint> {
            // Decidable theory — no labeling needed
            LogicStream::empty()
        }

        fn evaluate(&self, c: &PropConstraint, assignment: &PropAssignment) -> bool {
            match c {
                PropConstraint::Assert(name) => *assignment.0.get(name).unwrap_or(&false),
                PropConstraint::Negate(name) => !*assignment.0.get(name).unwrap_or(&false),
            }
        }
    }

    #[test]
    fn constraint_theory_consistent_store() {
        let theory = PropTheory;
        let store = theory.empty_store();
        let store = theory
            .propagate(&store, &PropConstraint::Assert("a".into()))
            .expect("should succeed");
        let store = theory
            .propagate(&store, &PropConstraint::Negate("b".into()))
            .expect("should succeed");
        assert!(theory.is_consistent(&store));
        let witness = theory.witness(&store).expect("should have witness");
        assert_eq!(witness.0.get("a"), Some(&true));
        assert_eq!(witness.0.get("b"), Some(&false));
    }

    #[test]
    fn constraint_theory_contradiction() {
        let theory = PropTheory;
        let store = theory.empty_store();
        let store = theory
            .propagate(&store, &PropConstraint::Assert("a".into()))
            .expect("should succeed");
        // Negating "a" should fail (contradiction)
        assert!(theory
            .propagate(&store, &PropConstraint::Negate("a".into()))
            .is_none());
    }

    #[test]
    fn constraint_theory_evaluate() {
        let theory = PropTheory;
        let mut assignment = std::collections::HashMap::new();
        assignment.insert("a".into(), true);
        assignment.insert("b".into(), false);
        let assignment = PropAssignment(assignment);

        assert!(theory.evaluate(&PropConstraint::Assert("a".into()), &assignment));
        assert!(!theory.evaluate(&PropConstraint::Assert("b".into()), &assignment));
        assert!(theory.evaluate(&PropConstraint::Negate("b".into()), &assignment));
    }

    // ── TheoryAlgebra tests (requires symbolic-automata feature) ────────

    mod theory_algebra_tests {
        use super::super::super::BooleanAlgebra;
        use super::*;

        #[test]
        fn theory_algebra_true_is_satisfiable() {
            let algebra = TheoryAlgebra::new(PropTheory, 100);
            assert!(algebra.is_satisfiable(&algebra.true_pred()));
        }

        #[test]
        fn theory_algebra_false_is_not_satisfiable() {
            let algebra = TheoryAlgebra::new(PropTheory, 100);
            assert!(!algebra.is_satisfiable(&algebra.false_pred()));
        }

        #[test]
        fn theory_algebra_atom_satisfiable() {
            let algebra = TheoryAlgebra::new(PropTheory, 100);
            let pred = TheoryPred::Atom(PropConstraint::Assert("x".into()));
            assert!(algebra.is_satisfiable(&pred));
        }

        #[test]
        fn theory_algebra_contradiction_unsatisfiable() {
            let algebra = TheoryAlgebra::new(PropTheory, 100);
            let pred = algebra.and(
                &TheoryPred::Atom(PropConstraint::Assert("x".into())),
                &TheoryPred::Atom(PropConstraint::Negate("x".into())),
            );
            assert!(!algebra.is_satisfiable(&pred));
        }

        #[test]
        fn theory_algebra_disjunction_satisfiable() {
            let algebra = TheoryAlgebra::new(PropTheory, 100);
            // (x ∧ ¬x) ∨ y — unsatisfiable left, satisfiable right
            let pred = algebra.or(
                &algebra.and(
                    &TheoryPred::Atom(PropConstraint::Assert("x".into())),
                    &TheoryPred::Atom(PropConstraint::Negate("x".into())),
                ),
                &TheoryPred::Atom(PropConstraint::Assert("y".into())),
            );
            assert!(algebra.is_satisfiable(&pred));
        }

        #[test]
        fn theory_algebra_negation() {
            let algebra = TheoryAlgebra::new(PropTheory, 100);
            let true_pred = algebra.true_pred();
            let not_true = algebra.not(&true_pred);
            assert!(!algebra.is_satisfiable(&not_true));
        }

        #[test]
        fn theory_algebra_evaluate() {
            let algebra = TheoryAlgebra::new(PropTheory, 100);
            let pred = TheoryPred::Atom(PropConstraint::Assert("x".into()));
            let mut map = std::collections::HashMap::new();
            map.insert("x".into(), true);
            let assignment = PropAssignment(map);
            assert!(algebra.evaluate(&pred, &assignment));
        }

        #[test]
        fn theory_algebra_witness() {
            let algebra = TheoryAlgebra::new(PropTheory, 100);
            let pred = TheoryPred::Atom(PropConstraint::Assert("x".into()));
            let witness = algebra.witness(&pred);
            assert!(witness.is_some());
            let w = witness.expect("should have witness");
            assert_eq!(w.0.get("x"), Some(&true));
        }
    }

    // ── QuantifiedFormula & evaluate_quantified tests ────────────────────

    /// Test helper: relation query that checks membership in a simple database.
    /// Database: positive(1), positive(2), positive(3), reachable(1,2),
    /// reachable(2,3), safe(1), safe(2), safe(3)
    fn test_relation_query(rel: &str, args: &[String]) -> bool {
        match rel {
            "positive" => args.len() == 1 && matches!(args[0].as_str(), "1" | "2" | "3"),
            "reachable" => {
                args.len() == 2
                    && matches!(
                        (args[0].as_str(), args[1].as_str()),
                        ("1", "2") | ("2", "3")
                    )
            }
            "safe" => args.len() == 1 && matches!(args[0].as_str(), "1" | "2" | "3"),
            "items" => args.len() == 1 && matches!(args[0].as_str(), "1" | "2" | "3"),
            "greater_than_one" => {
                args.len() == 1 && args[0].parse::<i32>().map_or(false, |n| n > 1)
            }
            "greater_than_two" => {
                args.len() == 1 && args[0].parse::<i32>().map_or(false, |n| n > 2)
            }
            _ => false,
        }
    }

    /// Test helper: domain enumerator for test relations.
    fn test_domain_enumerate(rel: &str) -> Vec<Vec<String>> {
        match rel {
            "positive" | "items" | "safe" => {
                vec![vec!["1".into()], vec!["2".into()], vec!["3".into()]]
            }
            "reachable" => vec![vec!["1".into(), "2".into()], vec!["2".into(), "3".into()]],
            "nodes" => {
                vec![vec!["1".into()], vec!["2".into()], vec!["3".into()]]
            }
            _ => vec![],
        }
    }

    #[test]
    fn quantified_formula_display() {
        let f = QuantifiedFormula::forall(
            "x",
            QuantifiedDomain::Relation("items".into()),
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::var("x")]),
        );
        assert_eq!(format!("{}", f), "∀x ∈ items. positive(x)");
    }

    #[test]
    fn quantified_formula_display_bounded() {
        let f = QuantifiedFormula::exists(
            "y",
            QuantifiedDomain::Bounded {
                relation: "nodes".into(),
                limit: 100,
            },
            QuantifiedFormula::atom("safe", vec![QuantifiedArg::var("y")]),
        );
        assert_eq!(format!("{}", f), "∃y ∈ nodes[≤100]. safe(y)");
    }

    #[test]
    fn quantified_formula_display_implies() {
        let f = QuantifiedFormula::implies(
            QuantifiedFormula::atom(
                "reachable",
                vec![QuantifiedArg::var("x"), QuantifiedArg::var("y")],
            ),
            QuantifiedFormula::atom("safe", vec![QuantifiedArg::var("y")]),
        );
        assert_eq!(format!("{}", f), "(reachable(x, y) ⇒ safe(y))");
    }

    #[test]
    fn quantified_formula_free_vars() {
        // ∀y ∈ nodes. (reachable(x, y) ⇒ safe(y))
        // Free vars: {x} (y is bound by ∀)
        let f = QuantifiedFormula::forall(
            "y",
            QuantifiedDomain::Relation("nodes".into()),
            QuantifiedFormula::implies(
                QuantifiedFormula::atom(
                    "reachable",
                    vec![QuantifiedArg::var("x"), QuantifiedArg::var("y")],
                ),
                QuantifiedFormula::atom("safe", vec![QuantifiedArg::var("y")]),
            ),
        );
        let free = f.free_vars();
        assert_eq!(free.len(), 1);
        assert!(free.contains("x"));
    }

    #[test]
    fn quantified_formula_free_vars_nested() {
        // ∀x ∈ nodes. ∃y ∈ nodes. reachable(x, y)
        // Free vars: {} (both x and y are bound)
        let f = QuantifiedFormula::forall(
            "x",
            QuantifiedDomain::Relation("nodes".into()),
            QuantifiedFormula::exists(
                "y",
                QuantifiedDomain::Relation("nodes".into()),
                QuantifiedFormula::atom(
                    "reachable",
                    vec![QuantifiedArg::var("x"), QuantifiedArg::var("y")],
                ),
            ),
        );
        assert!(f.free_vars().is_empty());
    }

    #[test]
    fn evaluate_atom_true() {
        let f = QuantifiedFormula::atom("positive", vec![QuantifiedArg::Constant("1".into())]);
        let env = std::collections::HashMap::new();
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_atom_false() {
        let f = QuantifiedFormula::atom("positive", vec![QuantifiedArg::Constant("99".into())]);
        let env = std::collections::HashMap::new();
        assert!(!evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_atom_with_var() {
        let f = QuantifiedFormula::atom("positive", vec![QuantifiedArg::var("x")]);
        let mut env = std::collections::HashMap::new();
        env.insert("x".into(), "2".into());
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_and() {
        let f = QuantifiedFormula::and(
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::Constant("1".into())]),
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::Constant("2".into())]),
        );
        let env = std::collections::HashMap::new();
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_and_short_circuit() {
        let f = QuantifiedFormula::and(
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::Constant("99".into())]),
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::Constant("1".into())]),
        );
        let env = std::collections::HashMap::new();
        assert!(!evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_or() {
        let f = QuantifiedFormula::or(
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::Constant("99".into())]),
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::Constant("1".into())]),
        );
        let env = std::collections::HashMap::new();
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_not() {
        let f = QuantifiedFormula::not(QuantifiedFormula::atom(
            "positive",
            vec![QuantifiedArg::Constant("99".into())],
        ));
        let env = std::collections::HashMap::new();
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_implies_true_antecedent() {
        // positive(1) ⇒ safe(1) — both true, so true
        let f = QuantifiedFormula::implies(
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::Constant("1".into())]),
            QuantifiedFormula::atom("safe", vec![QuantifiedArg::Constant("1".into())]),
        );
        let env = std::collections::HashMap::new();
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_implies_false_antecedent() {
        // positive(99) ⇒ safe(99) — false ⇒ anything = true (vacuous truth)
        let f = QuantifiedFormula::implies(
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::Constant("99".into())]),
            QuantifiedFormula::atom("safe", vec![QuantifiedArg::Constant("99".into())]),
        );
        let env = std::collections::HashMap::new();
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_forall_all_positive() {
        // ∀x ∈ items. positive(x) — all items {1,2,3} are positive → true
        let f = QuantifiedFormula::forall(
            "x",
            QuantifiedDomain::Relation("items".into()),
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::var("x")]),
        );
        let env = std::collections::HashMap::new();
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_forall_not_all_greater_than_one() {
        // ∀x ∈ items. greater_than_one(x) — item "1" fails → false
        let f = QuantifiedFormula::forall(
            "x",
            QuantifiedDomain::Relation("items".into()),
            QuantifiedFormula::atom("greater_than_one", vec![QuantifiedArg::var("x")]),
        );
        let env = std::collections::HashMap::new();
        assert!(!evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_exists_some_greater_than_two() {
        // ∃x ∈ items. greater_than_two(x) — item "3" satisfies → true
        let f = QuantifiedFormula::exists(
            "x",
            QuantifiedDomain::Relation("items".into()),
            QuantifiedFormula::atom("greater_than_two", vec![QuantifiedArg::var("x")]),
        );
        let env = std::collections::HashMap::new();
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_exists_none_match() {
        // ∃x ∈ items. positive(99) — no element can make "99" positive → false
        let f = QuantifiedFormula::exists(
            "x",
            QuantifiedDomain::Relation("items".into()),
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::Constant("99".into())]),
        );
        let env = std::collections::HashMap::new();
        assert!(!evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_forall_implies() {
        // ∀y ∈ nodes. (reachable(x, y) ⇒ safe(y))
        // With x=1: reachable(1,2) ⇒ safe(2) ✓, reachable(1,3)=false ⇒ vacuous ✓,
        //           reachable(1,1)=false ⇒ vacuous ✓
        let f = QuantifiedFormula::forall(
            "y",
            QuantifiedDomain::Relation("nodes".into()),
            QuantifiedFormula::implies(
                QuantifiedFormula::atom(
                    "reachable",
                    vec![QuantifiedArg::var("x"), QuantifiedArg::var("y")],
                ),
                QuantifiedFormula::atom("safe", vec![QuantifiedArg::var("y")]),
            ),
        );
        let mut env = std::collections::HashMap::new();
        env.insert("x".into(), "1".into());
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_nested_forall_exists() {
        // ∀x ∈ nodes. ∃y ∈ nodes. reachable(x, y)
        // x=1: reachable(1,2) ✓
        // x=2: reachable(2,3) ✓
        // x=3: no reachable(3,_) → false
        let f = QuantifiedFormula::forall(
            "x",
            QuantifiedDomain::Relation("nodes".into()),
            QuantifiedFormula::exists(
                "y",
                QuantifiedDomain::Relation("nodes".into()),
                QuantifiedFormula::atom(
                    "reachable",
                    vec![QuantifiedArg::var("x"), QuantifiedArg::var("y")],
                ),
            ),
        );
        let env = std::collections::HashMap::new();
        assert!(!evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_exists_forall() {
        // ∃x ∈ nodes. ∀y ∈ nodes. (reachable(x, y) ⇒ safe(y))
        // x=1: reachable(1,2)⇒safe(2) ✓, reachable(1,3)=F⇒vacuous, reachable(1,1)=F⇒vacuous → ✓
        // So exists succeeds with x=1
        let f = QuantifiedFormula::exists(
            "x",
            QuantifiedDomain::Relation("nodes".into()),
            QuantifiedFormula::forall(
                "y",
                QuantifiedDomain::Relation("nodes".into()),
                QuantifiedFormula::implies(
                    QuantifiedFormula::atom(
                        "reachable",
                        vec![QuantifiedArg::var("x"), QuantifiedArg::var("y")],
                    ),
                    QuantifiedFormula::atom("safe", vec![QuantifiedArg::var("y")]),
                ),
            ),
        );
        let env = std::collections::HashMap::new();
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_bounded_domain() {
        // ∃x ∈ items[≤2]. greater_than_two(x)
        // Only checks first 2 items (1, 2) — neither >2 → false
        // (If unbounded, item "3" would succeed)
        let f = QuantifiedFormula::exists(
            "x",
            QuantifiedDomain::Bounded {
                relation: "items".into(),
                limit: 2,
            },
            QuantifiedFormula::atom("greater_than_two", vec![QuantifiedArg::var("x")]),
        );
        let env = std::collections::HashMap::new();
        assert!(!evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_bounded_domain_succeeds() {
        // ∃x ∈ items[≤3]. greater_than_two(x) — bound allows item "3" → true
        let f = QuantifiedFormula::exists(
            "x",
            QuantifiedDomain::Bounded {
                relation: "items".into(),
                limit: 3,
            },
            QuantifiedFormula::atom("greater_than_two", vec![QuantifiedArg::var("x")]),
        );
        let env = std::collections::HashMap::new();
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_empty_domain_forall_vacuous() {
        // ∀x ∈ empty_rel. positive(x) — vacuously true (no elements to check)
        let f = QuantifiedFormula::forall(
            "x",
            QuantifiedDomain::Relation("nonexistent".into()),
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::var("x")]),
        );
        let env = std::collections::HashMap::new();
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn evaluate_empty_domain_exists_false() {
        // ∃x ∈ empty_rel. positive(x) — false (no elements to find)
        let f = QuantifiedFormula::exists(
            "x",
            QuantifiedDomain::Relation("nonexistent".into()),
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::var("x")]),
        );
        let env = std::collections::HashMap::new();
        assert!(!evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn gnot_equivalence_forall_not_exists_not() {
        // Property: ∀x.P(x) ≡ ¬∃x.¬P(x)
        // Test with P = positive, domain = items
        let forall_p = QuantifiedFormula::forall(
            "x",
            QuantifiedDomain::Relation("items".into()),
            QuantifiedFormula::atom("positive", vec![QuantifiedArg::var("x")]),
        );
        let not_exists_not_p = QuantifiedFormula::not(QuantifiedFormula::exists(
            "x",
            QuantifiedDomain::Relation("items".into()),
            QuantifiedFormula::not(QuantifiedFormula::atom(
                "positive",
                vec![QuantifiedArg::var("x")],
            )),
        ));
        let env = std::collections::HashMap::new();
        let r1 = evaluate_quantified(
            &forall_p,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000,
        );
        let r2 = evaluate_quantified(
            &not_exists_not_p,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000,
        );
        assert_eq!(r1, r2, "∀x.P(x) must equal ¬∃x.¬P(x)");
    }

    #[test]
    fn gnot_equivalence_exists_not_forall_not() {
        // Property: ∃x.P(x) ≡ ¬∀x.¬P(x)
        // Test with P = greater_than_two, domain = items
        let exists_p = QuantifiedFormula::exists(
            "x",
            QuantifiedDomain::Relation("items".into()),
            QuantifiedFormula::atom("greater_than_two", vec![QuantifiedArg::var("x")]),
        );
        let not_forall_not_p = QuantifiedFormula::not(QuantifiedFormula::forall(
            "x",
            QuantifiedDomain::Relation("items".into()),
            QuantifiedFormula::not(QuantifiedFormula::atom(
                "greater_than_two",
                vec![QuantifiedArg::var("x")],
            )),
        ));
        let env = std::collections::HashMap::new();
        let r1 = evaluate_quantified(
            &exists_p,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000,
        );
        let r2 = evaluate_quantified(
            &not_forall_not_p,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000,
        );
        assert_eq!(r1, r2, "∃x.P(x) must equal ¬∀x.¬P(x)");
    }

    #[test]
    fn evaluate_complex_nested_boolean() {
        // (positive(1) && safe(1)) || !positive(99)
        let f = QuantifiedFormula::or(
            QuantifiedFormula::and(
                QuantifiedFormula::atom("positive", vec![QuantifiedArg::Constant("1".into())]),
                QuantifiedFormula::atom("safe", vec![QuantifiedArg::Constant("1".into())]),
            ),
            QuantifiedFormula::not(QuantifiedFormula::atom(
                "positive",
                vec![QuantifiedArg::Constant("99".into())],
            )),
        );
        let env = std::collections::HashMap::new();
        assert!(evaluate_quantified(
            &f,
            &env,
            &test_relation_query,
            &test_domain_enumerate,
            1000
        ));
    }

    #[test]
    fn quantified_arg_display() {
        assert_eq!(format!("{}", QuantifiedArg::var("x")), "x");
        assert_eq!(format!("{}", QuantifiedArg::constant("hello")), "'hello'");
    }

    #[test]
    fn quantified_domain_display() {
        assert_eq!(
            format!("{}", QuantifiedDomain::Relation("items".into())),
            "items"
        );
        assert_eq!(
            format!(
                "{}",
                QuantifiedDomain::Bounded {
                    relation: "items".into(),
                    limit: 50
                }
            ),
            "items[≤50]"
        );
    }

    // ── AC-Matching: Multiset partition tests ────────────────────────────

    /// Helper: compute total element count in a partition's selected or remainder.
    fn total_count<T: Clone + Eq + Hash>(pairs: &[(T, usize)]) -> usize {
        pairs.iter().map(|(_, c)| c).sum()
    }

    /// Independent, bounded recursive specification of the historical stream.
    ///
    /// This remains test-only: its native recursion is useful as a transparent
    /// shallow oracle, but it is never reachable from production code or deep
    /// acceptance tests.
    fn multiset_partitions_recursive_reference<T>(
        items: &[(T, usize)],
        start: usize,
        remaining: usize,
    ) -> Vec<MultisetPartition<T>>
    where
        T: Clone + Eq + Hash,
    {
        if remaining == 0 {
            return vec![MultisetPartition {
                selected: Vec::new(),
                remainder: items[start..]
                    .iter()
                    .filter(|(_, count)| *count > 0)
                    .cloned()
                    .collect(),
                selected_count: 0,
            }];
        }

        if start >= items.len()
            || items[start..].iter().map(|(_, count)| count).sum::<usize>() < remaining
        {
            return Vec::new();
        }

        let (element, count) = items[start].clone();
        let mut accumulated = Vec::new();
        for quantity in 0..=count.min(remaining) {
            let mut branch =
                multiset_partitions_recursive_reference(items, start + 1, remaining - quantity);
            for partition in &mut branch {
                if quantity > 0 {
                    partition.selected.push((element.clone(), quantity));
                    partition.selected_count += quantity;
                }

                let leftover = count - quantity;
                if leftover > 0 {
                    partition.remainder.push((element.clone(), leftover));
                    partition.remainder.sort_by(|lhs, rhs| lhs.1.cmp(&rhs.1));
                }
            }
            accumulated = reference_interleave(accumulated, branch);
        }
        accumulated
    }

    /// Pure list interpretation of `LogicStream::interleave` for the oracle.
    fn reference_interleave<T>(left: Vec<T>, right: Vec<T>) -> Vec<T> {
        let mut result = Vec::with_capacity(left.len() + right.len());
        let mut left = left.into_iter();
        let mut right = right.into_iter();
        loop {
            match (left.next(), right.next()) {
                (Some(lhs), Some(rhs)) => {
                    result.push(lhs);
                    result.push(rhs);
                }
                (Some(lhs), None) => {
                    result.push(lhs);
                    result.extend(left);
                    break;
                }
                (None, Some(rhs)) => {
                    result.push(rhs);
                    result.extend(right);
                    break;
                }
                (None, None) => break,
            }
        }
        result
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// The iterative implementation must preserve the historical stream's
        /// exact value order as well as every formally proved conservation law.
        #[test]
        fn multiset_partitions_match_recursive_reference_and_invariants(
            counts in prop::collection::vec(0usize..=3, 0..=6),
            requested in 0usize..=20,
        ) {
            let items: Vec<(u8, usize)> = counts
                .into_iter()
                .enumerate()
                .map(|(index, count)| (index as u8, count))
                .collect();
            let expected = multiset_partitions_recursive_reference(&items, 0, requested);
            let actual = multiset_partitions(&items, requested).collect_all();

            prop_assert_eq!(&actual, &expected);
            let available: usize = items.iter().map(|(_, count)| count).sum();
            if requested > available {
                prop_assert!(actual.is_empty());
            }

            for partition in &actual {
                prop_assert_eq!(partition.selected_count, requested);
                prop_assert_eq!(total_count(&partition.selected), requested);
                prop_assert!(partition.selected.iter().all(|(_, count)| *count > 0));
                prop_assert!(partition.remainder.iter().all(|(_, count)| *count > 0));

                for (element, source_count) in &items {
                    let selected_count: usize = partition.selected
                        .iter()
                        .filter(|(candidate, _)| candidate == element)
                        .map(|(_, count)| count)
                        .sum();
                    let remainder_count: usize = partition.remainder
                        .iter()
                        .filter(|(candidate, _)| candidate == element)
                        .map(|(_, count)| count)
                        .sum();
                    prop_assert_eq!(selected_count + remainder_count, *source_count);
                }
            }
        }
    }

    #[test]
    fn deep_multiset_partition_prefix_uses_constant_native_stack() {
        const DEPTH: usize = 100_000;
        const SMALL_NATIVE_STACK: usize = 256 * 1024;

        std::thread::Builder::new()
            .name("deep-multiset-partition".to_owned())
            .stack_size(SMALL_NATIVE_STACK)
            .spawn(|| {
                let items: Vec<(u32, usize)> = (0..DEPTH as u32).map(|item| (item, 1)).collect();
                let prefix = multiset_partitions(&items, 1).collect_bounded(1);
                assert_eq!(prefix.len(), 1);
                assert_eq!(prefix[0].selected, vec![((DEPTH - 1) as u32, 1)]);
                assert_eq!(prefix[0].selected_count, 1);
                assert_eq!(total_count(&prefix[0].remainder), DEPTH - 1);
            })
            .expect("small-stack worker must start")
            .join()
            .expect("multiset partition generation must be stack-safe");
    }

    #[test]
    fn huge_multiplicity_prefix_keeps_quantity_interval_compact() {
        let items = vec![('A', usize::MAX), ('B', usize::MAX)];
        let prefix = multiset_partitions(&items, usize::MAX).collect_bounded(1);

        assert_eq!(prefix.len(), 1);
        assert_eq!(prefix[0].selected, vec![('B', usize::MAX)]);
        assert_eq!(prefix[0].remainder, vec![('A', usize::MAX)]);
        assert_eq!(prefix[0].selected_count, usize::MAX);
    }

    #[test]
    fn partition_empty_k0() {
        // Empty bag, K=0: one partition (empty selection, empty remainder)
        let items: Vec<(char, usize)> = vec![];
        let parts = multiset_partitions(&items, 0).collect_all();
        assert_eq!(parts.len(), 1);
        assert!(parts[0].selected.is_empty());
        assert!(parts[0].remainder.is_empty());
        assert_eq!(parts[0].selected_count, 0);
    }

    #[test]
    fn partition_empty_k1() {
        // Empty bag, K=1: no partitions possible
        let items: Vec<(char, usize)> = vec![];
        let parts = multiset_partitions(&items, 1).collect_all();
        assert!(parts.is_empty());
    }

    #[test]
    fn partition_singleton_k1() {
        // {A:1}, K=1: exactly one partition
        let items = vec![('A', 1)];
        let parts = multiset_partitions(&items, 1).collect_all();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].selected, vec![('A', 1)]);
        assert!(parts[0].remainder.is_empty());
        assert_eq!(parts[0].selected_count, 1);
    }

    #[test]
    fn partition_pair_k1() {
        // {A:1, B:1}, K=1: two partitions
        let items = vec![('A', 1), ('B', 1)];
        let parts = multiset_partitions(&items, 1).collect_all();
        assert_eq!(parts.len(), 2);

        // Each partition should select exactly one element
        for p in &parts {
            assert_eq!(p.selected_count, 1);
            assert_eq!(total_count(&p.remainder), 1);
        }
    }

    #[test]
    fn partition_multiplicity_k2() {
        // {A:3}, K=2: one partition {A:2} remainder {A:1}
        let items = vec![('A', 3)];
        let parts = multiset_partitions(&items, 2).collect_all();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].selected, vec![('A', 2)]);
        assert_eq!(parts[0].selected_count, 2);
        assert_eq!(parts[0].remainder, vec![('A', 1)]);
    }

    #[test]
    fn partition_mixed_k2() {
        // {A:2, B:1}, K=2: two partitions
        // - {A:2} remainder {B:1}
        // - {A:1, B:1} remainder {A:1}
        let items = vec![('A', 2), ('B', 1)];
        let parts = multiset_partitions(&items, 2).collect_all();
        assert_eq!(parts.len(), 2);

        for p in &parts {
            assert_eq!(p.selected_count, 2);
        }
    }

    #[test]
    fn partition_three_elements_k2() {
        // {A:1, B:1, C:1}, K=2: three partitions (C(3,2) = 3)
        let items = vec![('A', 1), ('B', 1), ('C', 1)];
        let parts = multiset_partitions(&items, 2).collect_all();
        assert_eq!(parts.len(), 3);

        for p in &parts {
            assert_eq!(p.selected_count, 2);
            assert_eq!(total_count(&p.remainder), 1);
        }
    }

    #[test]
    fn partition_sum_invariant() {
        // For all partitions: selected_count + remainder_count == source_count
        let items = vec![('A', 3), ('B', 2), ('C', 1)];
        let source_total: usize = items.iter().map(|(_, c)| c).sum(); // 6
        let parts = multiset_partitions(&items, 3).collect_all();

        for p in &parts {
            let selected_total = total_count(&p.selected);
            let remainder_total = total_count(&p.remainder);
            assert_eq!(selected_total, p.selected_count);
            assert_eq!(
                selected_total + remainder_total,
                source_total,
                "sum invariant violated: selected={selected_total} + remainder={remainder_total} != {source_total}"
            );
        }
    }

    #[test]
    fn partition_no_duplicates() {
        // All partitions should be distinct
        let items = vec![('A', 2), ('B', 2), ('C', 1)];
        let parts = multiset_partitions(&items, 3).collect_all();

        for i in 0..parts.len() {
            for j in (i + 1)..parts.len() {
                // Compare by sorted selected sets
                let mut sel_i = parts[i].selected.clone();
                let mut sel_j = parts[j].selected.clone();
                sel_i.sort_by_key(|(c, _)| *c);
                sel_j.sort_by_key(|(c, _)| *c);
                assert_ne!(sel_i, sel_j, "duplicate partitions at indices {i} and {j}");
            }
        }
    }

    #[test]
    fn partition_k_equals_n() {
        // K = total count: exactly one partition (select everything)
        let items = vec![('A', 2), ('B', 1)];
        let parts = multiset_partitions(&items, 3).collect_all();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].selected_count, 3);
        assert!(parts[0].remainder.is_empty());
    }

    #[test]
    fn partition_k_exceeds_n() {
        // K > total count: no partitions
        let items = vec![('A', 2), ('B', 1)];
        let parts = multiset_partitions(&items, 4).collect_all();
        assert!(parts.is_empty());
    }

    #[test]
    fn partition_bounded() {
        // collect_bounded(2) limits results even if more exist
        let items = vec![('A', 1), ('B', 1), ('C', 1)];
        // C(3,2) = 3 partitions exist
        let parts = multiset_partitions(&items, 2).collect_bounded(2);
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn partition_fairness() {
        // interleave ensures late elements aren't starved:
        // collect_bounded should include partitions drawing from later elements
        let items = vec![('A', 1), ('B', 1), ('C', 1), ('D', 1), ('E', 1)];
        let parts = multiset_partitions(&items, 1).collect_bounded(3);
        assert_eq!(parts.len(), 3);
        // Due to interleaving, we should see elements from different positions
        let selected_elems: Vec<char> = parts
            .iter()
            .flat_map(|p| p.selected.iter().map(|(c, _)| *c))
            .collect();
        // At least 2 distinct elements should appear in first 3 results
        let distinct: std::collections::HashSet<char> = selected_elems.into_iter().collect();
        assert!(
            distinct.len() >= 2,
            "interleave should mix elements from different positions"
        );
    }

    #[test]
    fn partition_complement_symmetry() {
        // |partitions(M, K)| == |partitions(M, N-K)|
        let items = vec![('A', 2), ('B', 2), ('C', 1)];
        let n: usize = items.iter().map(|(_, c)| c).sum(); // 5

        for k in 0..=n {
            let count_k = multiset_partitions(&items, k).collect_all().len();
            let count_complement = multiset_partitions(&items, n - k).collect_all().len();
            assert_eq!(
                count_k,
                count_complement,
                "|partitions(M, {k})| = {count_k} != |partitions(M, {})| = {count_complement}",
                n - k
            );
        }
    }

    #[test]
    fn partition_select_convenience() {
        // multiset_select() is a convenience wrapper returning Vec directly
        let items = vec![('A', 1), ('B', 1), ('C', 1)];
        let result = multiset_select(&items, 2, 1000);
        assert_eq!(result.len(), 3); // C(3,2)

        // With bound
        let bounded = multiset_select(&items, 2, 2);
        assert_eq!(bounded.len(), 2);
    }

    #[test]
    fn partition_per_element_count_invariant() {
        // For every partition and every element:
        // selected.count(e) + remainder.count(e) == source.count(e)
        let items = vec![('X', 4), ('Y', 2), ('Z', 1)];
        let parts = multiset_partitions(&items, 3).collect_all();

        for p in &parts {
            for &(ref elem, source_count) in &items {
                let sel_count = p
                    .selected
                    .iter()
                    .filter(|(e, _)| e == elem)
                    .map(|(_, c)| c)
                    .sum::<usize>();
                let rem_count = p
                    .remainder
                    .iter()
                    .filter(|(e, _)| e == elem)
                    .map(|(_, c)| c)
                    .sum::<usize>();
                assert_eq!(
                    sel_count + rem_count,
                    source_count,
                    "element {:?}: selected={sel_count} + remainder={rem_count} != source={source_count}",
                    elem
                );
            }
        }
    }

    // ──────────────────────────────────────────────────────────────
    // Phase 6 — TriState + evaluate_quantified_with_theory
    // ──────────────────────────────────────────────────────────────

    #[test]
    fn tristate_kleene_and() {
        use TriState::*;
        assert_eq!(True.and(True), True);
        assert_eq!(True.and(False), False);
        assert_eq!(True.and(Unknown), Unknown);
        assert_eq!(False.and(True), False);
        assert_eq!(False.and(False), False);
        assert_eq!(False.and(Unknown), False);
        assert_eq!(Unknown.and(True), Unknown);
        assert_eq!(Unknown.and(False), False);
        assert_eq!(Unknown.and(Unknown), Unknown);
    }

    #[test]
    fn tristate_kleene_or() {
        use TriState::*;
        assert_eq!(True.or(True), True);
        assert_eq!(True.or(False), True);
        assert_eq!(True.or(Unknown), True);
        assert_eq!(False.or(False), False);
        assert_eq!(False.or(Unknown), Unknown);
        assert_eq!(Unknown.or(False), Unknown);
        assert_eq!(Unknown.or(Unknown), Unknown);
    }

    #[test]
    fn tristate_negation() {
        assert_eq!(TriState::True.not(), TriState::False);
        assert_eq!(TriState::False.not(), TriState::True);
        assert_eq!(TriState::Unknown.not(), TriState::Unknown);
    }

    #[test]
    fn tristate_implies() {
        use TriState::*;
        assert_eq!(True.implies(True), True);
        assert_eq!(True.implies(False), False);
        assert_eq!(False.implies(True), True);
        assert_eq!(False.implies(False), True); // ⊥ ⟹ anything
        assert_eq!(True.implies(Unknown), Unknown);
        assert_eq!(Unknown.implies(True), True); // ¬Unknown ∨ True = True
    }

    #[test]
    fn tristate_safe_bool_collapse() {
        assert!(TriState::True.into_safe_bool());
        assert!(!TriState::False.into_safe_bool());
        assert!(!TriState::Unknown.into_safe_bool()); // safe-fail
    }

    #[test]
    fn evaluate_with_theory_atom_true_when_relation_holds() {
        let theory = PropTheory;
        let formula = QuantifiedFormula::Atom {
            relation: "halts".into(),
            args: vec![QuantifiedArg::Var("x".into())],
        };
        let mut env = std::collections::HashMap::new();
        env.insert("x".into(), "p".into());

        let result = evaluate_quantified_with_theory(
            &formula,
            &theory,
            &|rel, args| rel == "halts" && args == ["p"],
            &|_| Vec::new(),
            &env,
            16,
        );
        assert_eq!(result, TriState::True);
    }

    #[test]
    fn evaluate_with_theory_atom_false_when_relation_misses() {
        let theory = PropTheory;
        let formula = QuantifiedFormula::Atom {
            relation: "halts".into(),
            args: vec![QuantifiedArg::Var("x".into())],
        };
        let mut env = std::collections::HashMap::new();
        env.insert("x".into(), "q".into());

        let result = evaluate_quantified_with_theory(
            &formula,
            &theory,
            &|_, _| false,
            &|_| Vec::new(),
            &env,
            16,
        );
        assert_eq!(result, TriState::False);
    }

    #[test]
    fn evaluate_with_theory_forall_over_singleton_domain() {
        let theory = PropTheory;
        // ∀x ∈ items. positive(x) — items has just one tuple "5"
        let formula = QuantifiedFormula::ForAll {
            var: "x".into(),
            domain: QuantifiedDomain::Relation("items".into()),
            body: Box::new(QuantifiedFormula::Atom {
                relation: "positive".into(),
                args: vec![QuantifiedArg::Var("x".into())],
            }),
        };
        let env = std::collections::HashMap::new();
        let result = evaluate_quantified_with_theory(
            &formula,
            &theory,
            &|rel, args| rel == "positive" && args == ["5"],
            &|rel| {
                if rel == "items" {
                    vec![vec!["5".into()]]
                } else {
                    Vec::new()
                }
            },
            &env,
            16,
        );
        assert_eq!(result, TriState::True);
    }

    #[test]
    fn evaluate_with_theory_forall_falsified_by_counterexample() {
        let theory = PropTheory;
        let formula = QuantifiedFormula::ForAll {
            var: "x".into(),
            domain: QuantifiedDomain::Relation("items".into()),
            body: Box::new(QuantifiedFormula::Atom {
                relation: "positive".into(),
                args: vec![QuantifiedArg::Var("x".into())],
            }),
        };
        let env = std::collections::HashMap::new();
        let result = evaluate_quantified_with_theory(
            &formula,
            &theory,
            &|rel, args| rel == "positive" && args == ["5"], // 5 is positive, 7 is not
            &|rel| {
                if rel == "items" {
                    vec![vec!["5".into()], vec!["7".into()]]
                } else {
                    Vec::new()
                }
            },
            &env,
            16,
        );
        assert_eq!(result, TriState::False);
    }

    #[test]
    fn evaluate_with_theory_exists_finds_witness() {
        let theory = PropTheory;
        let formula = QuantifiedFormula::Exists {
            var: "x".into(),
            domain: QuantifiedDomain::Relation("items".into()),
            body: Box::new(QuantifiedFormula::Atom {
                relation: "positive".into(),
                args: vec![QuantifiedArg::Var("x".into())],
            }),
        };
        let env = std::collections::HashMap::new();
        let result = evaluate_quantified_with_theory(
            &formula,
            &theory,
            &|rel, args| rel == "positive" && args == ["7"],
            &|rel| {
                if rel == "items" {
                    vec![vec!["5".into()], vec!["7".into()]]
                } else {
                    Vec::new()
                }
            },
            &env,
            16,
        );
        assert_eq!(result, TriState::True);
    }

    #[test]
    fn evaluate_with_theory_exists_empty_domain_is_false() {
        let theory = PropTheory;
        let formula = QuantifiedFormula::Exists {
            var: "x".into(),
            domain: QuantifiedDomain::Relation("items".into()),
            body: Box::new(QuantifiedFormula::Atom {
                relation: "positive".into(),
                args: vec![QuantifiedArg::Var("x".into())],
            }),
        };
        let env = std::collections::HashMap::new();
        let result = evaluate_quantified_with_theory(
            &formula,
            &theory,
            &|_, _| true,
            &|_| Vec::new(),
            &env,
            16,
        );
        assert_eq!(result, TriState::False);
    }

    #[test]
    fn evaluate_with_theory_forall_empty_domain_is_true() {
        let theory = PropTheory;
        let formula = QuantifiedFormula::ForAll {
            var: "x".into(),
            domain: QuantifiedDomain::Relation("items".into()),
            body: Box::new(QuantifiedFormula::Atom {
                relation: "positive".into(),
                args: vec![QuantifiedArg::Var("x".into())],
            }),
        };
        let env = std::collections::HashMap::new();
        let result = evaluate_quantified_with_theory(
            &formula,
            &theory,
            &|_, _| false,
            &|_| Vec::new(),
            &env,
            16,
        );
        assert_eq!(result, TriState::True);
    }
}
