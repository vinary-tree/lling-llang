//! KAT `BooleanTest` predicate and its `BooleanAlgebra` adapter (`KatBooleanAlgebra`).
//!
//! Hoisted from prattail (Task #21 / ADR-018). `BooleanTest` is the Boolean
//! subalgebra of Kleene Algebra with Tests (KAT); `KatBooleanAlgebra` adapts it to
//! the effective-`BooleanAlgebra` interface so KAT guards drive symbolic automata.
//! The full KAT expression language / Hoare logic remains in prattail (`crate::kat`).

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::symbolic::BooleanAlgebra;

/// A Boolean test (predicate) in KAT.
///
/// Tests form a Boolean subalgebra of the Kleene algebra. They are used
/// as guards (preconditions/postconditions) in Hoare triples.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
        match self {
            BooleanTest::True | BooleanTest::False => {},
            BooleanTest::Atom(name) => {
                acc.insert(name.clone());
            },
            BooleanTest::Not(inner) => inner.collect_atoms(acc),
            BooleanTest::And(a, b) | BooleanTest::Or(a, b) => {
                a.collect_atoms(acc);
                b.collect_atoms(acc);
            },
        }
    }
}

impl fmt::Display for BooleanTest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BooleanTest::True => write!(f, "1"),
            BooleanTest::False => write!(f, "0"),
            BooleanTest::Atom(name) => write!(f, "{}", name),
            BooleanTest::Not(inner) => write!(f, "~{}", inner),
            BooleanTest::And(a, b) => write!(f, "({} & {})", a, b),
            BooleanTest::Or(a, b) => write!(f, "({} | {})", a, b),
        }
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
    match test {
        BooleanTest::True => true,
        BooleanTest::False => false,
        BooleanTest::Atom(name) => *valuation.get(name).unwrap_or(&false),
        BooleanTest::Not(inner) => !eval_test_public(inner, valuation),
        BooleanTest::And(a, b) => eval_test_public(a, valuation) && eval_test_public(b, valuation),
        BooleanTest::Or(a, b) => eval_test_public(a, valuation) || eval_test_public(b, valuation),
    }
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
