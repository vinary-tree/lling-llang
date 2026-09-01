//! KAT `BooleanTest` predicate and its `BooleanAlgebra` adapter (`KatBooleanAlgebra`).
//!
//! Hoisted from prattail (Task #21 / ADR-018). `BooleanTest` is the Boolean
//! subalgebra of Kleene Algebra with Tests (KAT); `KatBooleanAlgebra` adapts it to
//! the effective-`BooleanAlgebra` interface so KAT guards drive symbolic automata.
//! The full KAT expression language / Hoare logic remains in prattail (`crate::kat`).

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{Hash, Hasher};

use super::BooleanAlgebra;

/// A Boolean test (predicate) in KAT.
///
/// Tests form a Boolean subalgebra of the Kleene algebra. They are used
/// as guards (preconditions/postconditions) in Hoare triples.
pub enum BooleanTest {
    /// Boolean true (the test that always passes).
    True,
    /// Boolean false (the test that always fails).
    False,
    /// Atomic test (e.g., "at_eof", "token_is_open_paren").
    Atom(String),
    /// Negation of a test.
    Not(Box<BooleanTest>),
    /// Conjunction of two tests.
    And(Box<BooleanTest>, Box<BooleanTest>),
    /// Disjunction of two tests.
    Or(Box<BooleanTest>, Box<BooleanTest>),
}

impl Clone for BooleanTest {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Clone(&'a BooleanTest),
            Not,
            And,
            Or,
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(test) => match test {
                    BooleanTest::True => values.push(BooleanTest::True),
                    BooleanTest::False => values.push(BooleanTest::False),
                    BooleanTest::Atom(name) => values.push(BooleanTest::Atom(name.clone())),
                    BooleanTest::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Clone(inner));
                    }
                    BooleanTest::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    BooleanTest::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                },
                Task::Not => {
                    let inner = values.pop().expect("negated test clone is present");
                    values.push(BooleanTest::Not(Box::new(inner)));
                }
                Task::And | Task::Or => {
                    let right = values.pop().expect("right test clone is present");
                    let left = values.pop().expect("left test clone is present");
                    values.push(if matches!(task, Task::And) {
                        BooleanTest::And(Box::new(left), Box::new(right))
                    } else {
                        BooleanTest::Or(Box::new(left), Box::new(right))
                    });
                }
            }
        }
        values.pop().expect("the root test produces one clone")
    }
}

impl PartialEq for BooleanTest {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (BooleanTest::True, BooleanTest::True)
                | (BooleanTest::False, BooleanTest::False) => {}
                (BooleanTest::Atom(left), BooleanTest::Atom(right)) if left == right => {}
                (BooleanTest::Not(left), BooleanTest::Not(right)) => {
                    pending.push((left, right));
                }
                (BooleanTest::And(ll, lr), BooleanTest::And(rl, rr))
                | (BooleanTest::Or(ll, lr), BooleanTest::Or(rl, rr)) => {
                    pending.push((lr, rr));
                    pending.push((ll, rl));
                }
                _ => return false,
            }
        }
        true
    }
}

impl Eq for BooleanTest {}

impl Hash for BooleanTest {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut pending = vec![self];
        while let Some(test) = pending.pop() {
            std::mem::discriminant(test).hash(state);
            match test {
                BooleanTest::True | BooleanTest::False => {}
                BooleanTest::Atom(name) => name.hash(state),
                BooleanTest::Not(inner) => pending.push(inner),
                BooleanTest::And(left, right) | BooleanTest::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
            }
        }
    }
}

impl fmt::Debug for BooleanTest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Test(&'a BooleanTest),
            Text(&'static str),
        }
        let mut events = vec![Event::Test(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Test(test) => match test {
                    BooleanTest::True => write!(f, "True")?,
                    BooleanTest::False => write!(f, "False")?,
                    BooleanTest::Atom(name) => write!(f, "Atom({name:?})")?,
                    BooleanTest::Not(inner) => {
                        write!(f, "Not(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Test(inner));
                    }
                    BooleanTest::And(left, right) | BooleanTest::Or(left, right) => {
                        write!(
                            f,
                            "{}(",
                            if matches!(test, BooleanTest::And(_, _)) {
                                "And"
                            } else {
                                "Or"
                            }
                        )?;
                        events.push(Event::Text(")"));
                        events.push(Event::Test(right));
                        events.push(Event::Text(", "));
                        events.push(Event::Test(left));
                    }
                },
            }
        }
        Ok(())
    }
}

impl Drop for BooleanTest {
    fn drop(&mut self) {
        fn drain(test: &mut BooleanTest, pending: &mut Vec<BooleanTest>) {
            match test {
                BooleanTest::Not(inner) => {
                    pending.push(std::mem::replace(&mut **inner, BooleanTest::True));
                }
                BooleanTest::And(left, right) | BooleanTest::Or(left, right) => {
                    pending.push(std::mem::replace(&mut **right, BooleanTest::True));
                    pending.push(std::mem::replace(&mut **left, BooleanTest::True));
                }
                BooleanTest::True | BooleanTest::False | BooleanTest::Atom(_) => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut test) = pending.pop() {
            drain(&mut test, &mut pending);
        }
    }
}

impl BooleanTest {
    /// Create an atomic test.
    pub fn atom(name: impl Into<String>) -> Self {
        BooleanTest::Atom(name.into())
    }

    /// Negate a test.
    pub fn not(test: BooleanTest) -> Self {
        BooleanTest::Not(Box::new(test))
    }

    /// Conjunction of two tests.
    pub fn and(a: BooleanTest, b: BooleanTest) -> Self {
        BooleanTest::And(Box::new(a), Box::new(b))
    }

    /// Disjunction of two tests.
    pub fn or(a: BooleanTest, b: BooleanTest) -> Self {
        BooleanTest::Or(Box::new(a), Box::new(b))
    }

    /// Collect all atomic test names.
    pub fn atoms(&self) -> HashSet<String> {
        let mut result = HashSet::new();
        self.collect_atoms(&mut result);
        result
    }

    /// Accumulate the atomic proposition names of this test into `acc`.
    /// `pub` so prattail's KAT-expression analysis (the residual after the
    /// Task #21 hoist) can collect atoms across an expression tree.
    pub fn collect_atoms(&self, acc: &mut HashSet<String>) {
        let mut pending = vec![self];
        while let Some(test) = pending.pop() {
            match test {
                BooleanTest::True | BooleanTest::False => {}
                BooleanTest::Atom(name) => {
                    acc.insert(name.clone());
                }
                BooleanTest::Not(inner) => pending.push(inner),
                BooleanTest::And(left, right) | BooleanTest::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
            }
        }
    }
}

impl fmt::Display for BooleanTest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Test(&'a BooleanTest),
            Text(&'static str),
        }
        let mut events = vec![Event::Test(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Test(test) => match test {
                    BooleanTest::True => write!(f, "1")?,
                    BooleanTest::False => write!(f, "0")?,
                    BooleanTest::Atom(name) => write!(f, "{name}")?,
                    BooleanTest::Not(inner) => {
                        write!(f, "~")?;
                        events.push(Event::Test(inner));
                    }
                    BooleanTest::And(left, right) | BooleanTest::Or(left, right) => {
                        write!(f, "(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Test(right));
                        events.push(Event::Text(if matches!(test, BooleanTest::And(_, _)) {
                            " & "
                        } else {
                            " | "
                        }));
                        events.push(Event::Test(left));
                    }
                },
            }
        }
        Ok(())
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// KatBooleanAlgebra — adapter for KAT BooleanTest
// ══════════════════════════════════════════════════════════════════════════════

/// Boolean algebra adapter for the KAT module's `BooleanTest` type.
///
/// This algebra bridges the KAT module's propositional tests with the
/// symbolic automata framework. The domain is truth assignments:
/// `HashMap<String, bool>` mapping proposition names to truth values.
///
/// # Satisfiability
///
/// Since the domain is finite (2^n valuations for n atoms), satisfiability
/// is decided by exhaustive enumeration. This is tractable for the small
/// number of atoms typical in PraTTaIL grammars (usually fewer than 10).
#[derive(Clone, Debug)]
pub struct KatBooleanAlgebra {
    /// All proposition (atom) names known to this algebra.
    pub atoms: Vec<String>,
}

impl KatBooleanAlgebra {
    /// Create a new KAT Boolean algebra with the given atom names.
    pub fn new(atoms: Vec<String>) -> Self {
        KatBooleanAlgebra { atoms }
    }

    /// Create a KAT Boolean algebra by extracting atoms from a BooleanTest.
    pub fn from_test(test: &BooleanTest) -> Self {
        let atom_set = test.atoms();
        let mut atoms: Vec<String> = atom_set.into_iter().collect();
        atoms.sort();
        KatBooleanAlgebra { atoms }
    }

    /// Generate all 2^n truth assignments for the atoms.
    fn all_valuations(&self) -> Vec<HashMap<String, bool>> {
        let n = self.atoms.len();
        let num_valuations = 1usize << n;
        let mut valuations = Vec::with_capacity(num_valuations);
        for bits in 0..num_valuations {
            let mut valuation = HashMap::with_capacity(n);
            for (i, name) in self.atoms.iter().enumerate() {
                valuation.insert(name.clone(), (bits >> i) & 1 == 1);
            }
            valuations.push(valuation);
        }
        valuations
    }
}

/// Evaluate a `BooleanTest` under a truth assignment.
///
/// Public helper for use by the symbolic automata module and tests.
/// Atoms not present in the valuation are treated as `false`.
pub fn eval_test_public(test: &BooleanTest, valuation: &HashMap<String, bool>) -> bool {
    enum Frame<'a> {
        Eval(&'a BooleanTest),
        Not,
        AndRight(&'a BooleanTest),
        OrRight(&'a BooleanTest),
    }
    let mut frames = vec![Frame::Eval(test)];
    let mut result = None;
    while let Some(frame) = frames.pop() {
        match frame {
            Frame::Eval(predicate) => match predicate {
                BooleanTest::True => result = Some(true),
                BooleanTest::False => result = Some(false),
                BooleanTest::Atom(name) => {
                    result = Some(*valuation.get(name).unwrap_or(&false));
                }
                BooleanTest::Not(inner) => {
                    frames.push(Frame::Not);
                    frames.push(Frame::Eval(inner));
                }
                BooleanTest::And(left, right) => {
                    frames.push(Frame::AndRight(right));
                    frames.push(Frame::Eval(left));
                }
                BooleanTest::Or(left, right) => {
                    frames.push(Frame::OrRight(right));
                    frames.push(Frame::Eval(left));
                }
            },
            Frame::Not => result = Some(!result.take().expect("negated test is evaluated")),
            Frame::AndRight(right) => {
                if result.take().expect("left test conjunction is evaluated") {
                    frames.push(Frame::Eval(right));
                } else {
                    result = Some(false);
                }
            }
            Frame::OrRight(right) => {
                if result.take().expect("left test disjunction is evaluated") {
                    result = Some(true);
                } else {
                    frames.push(Frame::Eval(right));
                }
            }
        }
    }
    result.expect("the root Boolean test produces one result")
}

impl BooleanAlgebra for KatBooleanAlgebra {
    type Predicate = BooleanTest;
    type Domain = HashMap<String, bool>;

    fn true_pred(&self) -> BooleanTest {
        BooleanTest::True
    }

    fn false_pred(&self) -> BooleanTest {
        BooleanTest::False
    }

    fn and(&self, a: &BooleanTest, b: &BooleanTest) -> BooleanTest {
        BooleanTest::And(Box::new(a.clone()), Box::new(b.clone()))
    }

    fn or(&self, a: &BooleanTest, b: &BooleanTest) -> BooleanTest {
        BooleanTest::Or(Box::new(a.clone()), Box::new(b.clone()))
    }

    fn not(&self, a: &BooleanTest) -> BooleanTest {
        BooleanTest::Not(Box::new(a.clone()))
    }

    fn is_satisfiable(&self, a: &BooleanTest) -> bool {
        // Exhaustive search over 2^n truth assignments.
        self.all_valuations().iter().any(|v| eval_test_public(a, v))
    }

    fn witness(&self, a: &BooleanTest) -> Option<HashMap<String, bool>> {
        self.all_valuations()
            .into_iter()
            .find(|v| eval_test_public(a, v))
    }

    fn evaluate(&self, pred: &BooleanTest, elem: &HashMap<String, bool>) -> bool {
        eval_test_public(pred, elem)
    }
}
