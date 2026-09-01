//! `BehavioralAlgebra` — an effective algebra of **behavioral** predicates over
//! the dynamics of terms (relational/Datalog facts now; modal and temporal
//! fragments added in later steps).
//!
//! Behavioral predicates are only *snapshot-relative*: a relation's absence from
//! the current fact base is not a proof of absence (more facts may be derived).
//! So `BehavioralAlgebra` implements [`HeytingAlgebra`] (intuitionistic — no
//! involutive complement, no excluded middle) and **NOT**
//! [`BooleanAlgebra`](crate::symbolic::BooleanAlgebra): the symbolic-automaton
//! classical operations are statically unavailable on it (the safety property of
//! the [algebra tower](crate::algebra_tower)). Computation against a *fixed*
//! finite snapshot is nonetheless decidable (closed-world over the snapshot),
//! returning [`Sat3::Sat`]/[`Sat3::Unsat`]; only an exceeded search budget
//! yields [`Sat3::DontKnow`].
//!
//! This module (M2.2a) provides the relational fragment: `Relation` atoms,
//! `forall`/`exists` quantifiers, and boolean combination, decided against a
//! [`FactBase`] over the active domain. The modal (`Diamond`/`Box`/`Mu`/`Nu`)
//! and temporal fragments — which use the [`HostTerm`] LTS — extend the
//! [`BehavioralFormula`] enum and the `evaluate`/`is_satisfiable_3v` dispatch in
//! subsequent steps.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::{self, Debug};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::algebra_tower::{HeytingAlgebra, RejectSafeAlgebra, Sat3};

/// Default cap on the number of free-variable assignments searched before
/// `is_satisfiable_3v` returns `DontKnow`.
const DEFAULT_SEARCH_BUDGET: usize = 100_000;

// ══════════════════════════════════════════════════════════════════════════════
// HostTerm — the LTS interface (used by the modal/temporal fragments)
// ══════════════════════════════════════════════════════════════════════════════

/// A host-language term that induces a labeled transition system: the seam the
/// modal/temporal behavioral fragments use. (The relational fragment ignores the
/// term.)
pub trait HostTerm: Clone + Debug + Eq + Hash + Send + Sync + 'static {
    /// One-step successors with action labels (the LTS edges). Backed by the
    /// host's reduction relation.
    fn successors(&self) -> Vec<(String, Self)>;
    /// A label for atomic-proposition matching at this state.
    fn label(&self) -> String;
}

/// A degenerate host term with no transitions — for relational-only use (the
/// relational fragment never inspects the term). A real, total LTS (the
/// single-state, no-edge system).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NoTerm;

impl HostTerm for NoTerm {
    fn successors(&self) -> Vec<(String, Self)> {
        Vec::new()
    }
    fn label(&self) -> String {
        String::new()
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Fact base
// ══════════════════════════════════════════════════════════════════════════════

/// A finite snapshot of Datalog-style relations (each a set of string tuples).
#[derive(Clone, Debug, Default)]
pub struct FactBase {
    relations: HashMap<String, HashSet<Vec<String>>>,
}

impl FactBase {
    /// An empty fact base.
    pub fn new() -> Self {
        FactBase {
            relations: HashMap::new(),
        }
    }

    /// Add a fact `relation(tuple)`.
    pub fn add_fact(&mut self, relation: impl Into<String>, tuple: Vec<String>) {
        self.relations
            .entry(relation.into())
            .or_default()
            .insert(tuple);
    }

    /// Whether `relation(tuple)` holds in this snapshot.
    pub fn holds(&self, relation: &str, tuple: &[String]) -> bool {
        self.relations
            .get(relation)
            .is_some_and(|s| s.contains(tuple))
    }

    /// The active domain: every constant appearing in any fact tuple.
    fn active_domain(&self) -> BTreeSet<String> {
        let mut dom = BTreeSet::new();
        for tuples in self.relations.values() {
            for t in tuples {
                for v in t {
                    dom.insert(v.clone());
                }
            }
        }
        dom
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Behavioral formula (relational fragment)
// ══════════════════════════════════════════════════════════════════════════════

/// An argument to a relation: a bound/free variable or a literal constant.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Arg {
    /// A variable (looked up in the binding environment).
    Var(String),
    /// A literal constant.
    Lit(String),
}

/// What a modal operator matches on an LTS edge label.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ActionPattern {
    /// Any action (`⟨-⟩` / `[-]`).
    Any,
    /// An internal/unlabeled step (`τ`): empty or `"tau"` label.
    Tau,
    /// A specific named action.
    Named(String),
}

impl ActionPattern {
    fn matches(&self, action: &str) -> bool {
        match self {
            ActionPattern::Any => true,
            ActionPattern::Tau => action.is_empty() || action == "tau",
            ActionPattern::Named(n) => action == n,
        }
    }
}

/// The domain a quantifier ranges over.
pub enum QDomain {
    /// An explicit set of values.
    Values(Vec<String>),
    /// Column `usize` of a relation.
    RelationColumn(String, usize),
    /// The active domain of the fact base.
    Active,
    /// Bounded iteration over an inner domain (semi-decidable — at most `usize`).
    Bounded(Box<QDomain>, usize),
}

/// A behavioral predicate. (Relational fragment; modal/temporal arms added
/// later.)
pub enum BehavioralFormula {
    /// Always true.
    Top,
    /// Always false.
    Bot,
    /// A relation atom `name(args)`.
    Relation { name: String, args: Vec<Arg> },
    /// `∀ var ∈ domain. body`.
    Forall {
        var: String,
        domain: QDomain,
        body: Box<BehavioralFormula>,
    },
    /// `∃ var ∈ domain. body`.
    Exists {
        var: String,
        domain: QDomain,
        body: Box<BehavioralFormula>,
    },
    /// A state proposition: the LTS state's `label()` equals this string.
    Atom(String),
    /// `⟨a⟩φ` — some `a`-labeled successor satisfies `φ`.
    Diamond(ActionPattern, Box<BehavioralFormula>),
    /// `[a]φ` — all `a`-labeled successors satisfy `φ`.
    BoxAll(ActionPattern, Box<BehavioralFormula>),
    /// Least fixpoint `μX.φ` (liveness/eventuality).
    Mu(String, Box<BehavioralFormula>),
    /// Greatest fixpoint `νX.φ` (safety/invariance).
    Nu(String, Box<BehavioralFormula>),
    /// A fixpoint variable.
    FixVar(String),
    /// Conjunction.
    And(Box<BehavioralFormula>, Box<BehavioralFormula>),
    /// Disjunction.
    Or(Box<BehavioralFormula>, Box<BehavioralFormula>),
    /// Negation (snapshot-relative — see module docs).
    Not(Box<BehavioralFormula>),
}

impl Clone for QDomain {
    fn clone(&self) -> Self {
        let mut current = self;
        let mut limits = Vec::new();
        while let QDomain::Bounded(inner, limit) = current {
            limits.push(*limit);
            current = inner;
        }
        let mut cloned = match current {
            QDomain::Values(values) => QDomain::Values(values.clone()),
            QDomain::RelationColumn(relation, column) => {
                QDomain::RelationColumn(relation.clone(), *column)
            }
            QDomain::Active => QDomain::Active,
            QDomain::Bounded(_, _) => unreachable!("bounded-domain spine reaches a base"),
        };
        while let Some(limit) = limits.pop() {
            cloned = QDomain::Bounded(Box::new(cloned), limit);
        }
        cloned
    }
}

impl PartialEq for QDomain {
    fn eq(&self, other: &Self) -> bool {
        let mut left = self;
        let mut right = other;
        loop {
            match (left, right) {
                (QDomain::Values(left), QDomain::Values(right)) => return left == right,
                (
                    QDomain::RelationColumn(left_relation, left_column),
                    QDomain::RelationColumn(right_relation, right_column),
                ) => return left_relation == right_relation && left_column == right_column,
                (QDomain::Active, QDomain::Active) => return true,
                (
                    QDomain::Bounded(left_inner, left_limit),
                    QDomain::Bounded(right_inner, right_limit),
                ) if left_limit == right_limit => {
                    left = left_inner;
                    right = right_inner;
                }
                _ => return false,
            }
        }
    }
}

impl Eq for QDomain {}

impl Hash for QDomain {
    fn hash<S: Hasher>(&self, state: &mut S) {
        enum Task<'a> {
            Domain(&'a QDomain),
            Limit(usize),
        }
        let mut tasks = vec![Task::Domain(self)];
        while let Some(task) = tasks.pop() {
            match task {
                Task::Limit(limit) => limit.hash(state),
                Task::Domain(domain) => {
                    std::mem::discriminant(domain).hash(state);
                    match domain {
                        QDomain::Values(values) => values.hash(state),
                        QDomain::RelationColumn(relation, column) => {
                            relation.hash(state);
                            column.hash(state);
                        }
                        QDomain::Active => {}
                        QDomain::Bounded(inner, limit) => {
                            tasks.push(Task::Limit(*limit));
                            tasks.push(Task::Domain(inner));
                        }
                    }
                }
            }
        }
    }
}

impl fmt::Debug for QDomain {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Domain(&'a QDomain),
            Text(&'static str),
            Limit(usize),
        }
        let mut events = vec![Event::Domain(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => formatter.write_str(text)?,
                Event::Limit(limit) => write!(formatter, "{limit:?}")?,
                Event::Domain(domain) => match domain {
                    QDomain::Values(values) => write!(formatter, "Values({values:?})")?,
                    QDomain::RelationColumn(relation, column) => {
                        write!(formatter, "RelationColumn({relation:?}, {column:?})")?;
                    }
                    QDomain::Active => formatter.write_str("Active")?,
                    QDomain::Bounded(inner, limit) => {
                        formatter.write_str("Bounded(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Limit(*limit));
                        events.push(Event::Text(", "));
                        events.push(Event::Domain(inner));
                    }
                },
            }
        }
        Ok(())
    }
}

impl Drop for QDomain {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        if let QDomain::Bounded(inner, _) = self {
            pending.push(std::mem::replace(&mut **inner, QDomain::Active));
        }
        while let Some(mut domain) = pending.pop() {
            if let QDomain::Bounded(inner, _) = &mut domain {
                pending.push(std::mem::replace(&mut **inner, QDomain::Active));
            }
        }
    }
}

impl Clone for BehavioralFormula {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Formula(&'a BehavioralFormula),
            Forall(&'a str, &'a QDomain),
            Exists(&'a str, &'a QDomain),
            Diamond(&'a ActionPattern),
            BoxAll(&'a ActionPattern),
            Mu(&'a str),
            Nu(&'a str),
            And,
            Or,
            Not,
        }
        let mut tasks = vec![Task::Formula(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Formula(formula) => match formula {
                    BehavioralFormula::Top => values.push(BehavioralFormula::Top),
                    BehavioralFormula::Bot => values.push(BehavioralFormula::Bot),
                    BehavioralFormula::Relation { name, args } => {
                        values.push(BehavioralFormula::Relation {
                            name: name.clone(),
                            args: args.clone(),
                        });
                    }
                    BehavioralFormula::Forall { var, domain, body } => {
                        tasks.push(Task::Forall(var, domain));
                        tasks.push(Task::Formula(body));
                    }
                    BehavioralFormula::Exists { var, domain, body } => {
                        tasks.push(Task::Exists(var, domain));
                        tasks.push(Task::Formula(body));
                    }
                    BehavioralFormula::Atom(label) => {
                        values.push(BehavioralFormula::Atom(label.clone()));
                    }
                    BehavioralFormula::Diamond(action, body) => {
                        tasks.push(Task::Diamond(action));
                        tasks.push(Task::Formula(body));
                    }
                    BehavioralFormula::BoxAll(action, body) => {
                        tasks.push(Task::BoxAll(action));
                        tasks.push(Task::Formula(body));
                    }
                    BehavioralFormula::Mu(var, body) => {
                        tasks.push(Task::Mu(var));
                        tasks.push(Task::Formula(body));
                    }
                    BehavioralFormula::Nu(var, body) => {
                        tasks.push(Task::Nu(var));
                        tasks.push(Task::Formula(body));
                    }
                    BehavioralFormula::FixVar(var) => {
                        values.push(BehavioralFormula::FixVar(var.clone()));
                    }
                    BehavioralFormula::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Formula(right));
                        tasks.push(Task::Formula(left));
                    }
                    BehavioralFormula::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Formula(right));
                        tasks.push(Task::Formula(left));
                    }
                    BehavioralFormula::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Formula(inner));
                    }
                },
                Task::Forall(var, domain) | Task::Exists(var, domain) => {
                    let body = values.pop().expect("behavioral quantifier body is cloned");
                    values.push(if matches!(task, Task::Forall(_, _)) {
                        BehavioralFormula::Forall {
                            var: var.to_string(),
                            domain: domain.clone(),
                            body: Box::new(body),
                        }
                    } else {
                        BehavioralFormula::Exists {
                            var: var.to_string(),
                            domain: domain.clone(),
                            body: Box::new(body),
                        }
                    });
                }
                Task::Diamond(action) | Task::BoxAll(action) => {
                    let body = values.pop().expect("behavioral modal body is cloned");
                    values.push(if matches!(task, Task::Diamond(_)) {
                        BehavioralFormula::Diamond(action.clone(), Box::new(body))
                    } else {
                        BehavioralFormula::BoxAll(action.clone(), Box::new(body))
                    });
                }
                Task::Mu(var) | Task::Nu(var) => {
                    let body = values.pop().expect("behavioral fixpoint body is cloned");
                    values.push(if matches!(task, Task::Mu(_)) {
                        BehavioralFormula::Mu(var.to_string(), Box::new(body))
                    } else {
                        BehavioralFormula::Nu(var.to_string(), Box::new(body))
                    });
                }
                Task::And | Task::Or => {
                    let right = values.pop().expect("right behavioral clone is present");
                    let left = values.pop().expect("left behavioral clone is present");
                    values.push(if matches!(task, Task::And) {
                        BehavioralFormula::And(Box::new(left), Box::new(right))
                    } else {
                        BehavioralFormula::Or(Box::new(left), Box::new(right))
                    });
                }
                Task::Not => {
                    let inner = values.pop().expect("negated behavioral clone is present");
                    values.push(BehavioralFormula::Not(Box::new(inner)));
                }
            }
        }
        values
            .pop()
            .expect("the root behavioral formula produces one clone")
    }
}

impl PartialEq for BehavioralFormula {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (BehavioralFormula::Top, BehavioralFormula::Top)
                | (BehavioralFormula::Bot, BehavioralFormula::Bot) => {}
                (
                    BehavioralFormula::Relation {
                        name: left_name,
                        args: left_args,
                    },
                    BehavioralFormula::Relation {
                        name: right_name,
                        args: right_args,
                    },
                ) if left_name == right_name && left_args == right_args => {}
                (
                    BehavioralFormula::Forall {
                        var: left_var,
                        domain: left_domain,
                        body: left_body,
                    },
                    BehavioralFormula::Forall {
                        var: right_var,
                        domain: right_domain,
                        body: right_body,
                    },
                )
                | (
                    BehavioralFormula::Exists {
                        var: left_var,
                        domain: left_domain,
                        body: left_body,
                    },
                    BehavioralFormula::Exists {
                        var: right_var,
                        domain: right_domain,
                        body: right_body,
                    },
                ) if left_var == right_var && left_domain == right_domain => {
                    pending.push((left_body, right_body));
                }
                (BehavioralFormula::Atom(left), BehavioralFormula::Atom(right))
                | (BehavioralFormula::FixVar(left), BehavioralFormula::FixVar(right))
                    if left == right => {}
                (
                    BehavioralFormula::Diamond(left_action, left_body),
                    BehavioralFormula::Diamond(right_action, right_body),
                )
                | (
                    BehavioralFormula::BoxAll(left_action, left_body),
                    BehavioralFormula::BoxAll(right_action, right_body),
                ) if left_action == right_action => pending.push((left_body, right_body)),
                (
                    BehavioralFormula::Mu(left_var, left_body),
                    BehavioralFormula::Mu(right_var, right_body),
                )
                | (
                    BehavioralFormula::Nu(left_var, left_body),
                    BehavioralFormula::Nu(right_var, right_body),
                ) if left_var == right_var => pending.push((left_body, right_body)),
                (
                    BehavioralFormula::And(left_left, left_right),
                    BehavioralFormula::And(right_left, right_right),
                )
                | (
                    BehavioralFormula::Or(left_left, left_right),
                    BehavioralFormula::Or(right_left, right_right),
                ) => {
                    pending.push((left_right, right_right));
                    pending.push((left_left, right_left));
                }
                (BehavioralFormula::Not(left), BehavioralFormula::Not(right)) => {
                    pending.push((left, right));
                }
                _ => return false,
            }
        }
        true
    }
}

impl Eq for BehavioralFormula {}

impl Hash for BehavioralFormula {
    fn hash<S: Hasher>(&self, state: &mut S) {
        let mut pending = vec![self];
        while let Some(formula) = pending.pop() {
            std::mem::discriminant(formula).hash(state);
            match formula {
                BehavioralFormula::Top | BehavioralFormula::Bot => {}
                BehavioralFormula::Relation { name, args } => {
                    name.hash(state);
                    args.hash(state);
                }
                BehavioralFormula::Forall { var, domain, body }
                | BehavioralFormula::Exists { var, domain, body } => {
                    var.hash(state);
                    domain.hash(state);
                    pending.push(body);
                }
                BehavioralFormula::Atom(label) | BehavioralFormula::FixVar(label) => {
                    label.hash(state);
                }
                BehavioralFormula::Diamond(action, body)
                | BehavioralFormula::BoxAll(action, body) => {
                    action.hash(state);
                    pending.push(body);
                }
                BehavioralFormula::Mu(var, body) | BehavioralFormula::Nu(var, body) => {
                    var.hash(state);
                    pending.push(body);
                }
                BehavioralFormula::And(left, right) | BehavioralFormula::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                BehavioralFormula::Not(inner) => pending.push(inner),
            }
        }
    }
}

impl fmt::Debug for BehavioralFormula {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a> {
            Formula(&'a BehavioralFormula),
            Text(&'static str),
        }
        let mut events = vec![Event::Formula(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => formatter.write_str(text)?,
                Event::Formula(formula) => match formula {
                    BehavioralFormula::Top => formatter.write_str("Top")?,
                    BehavioralFormula::Bot => formatter.write_str("Bot")?,
                    BehavioralFormula::Relation { name, args } => {
                        write!(formatter, "Relation {{ name: {name:?}, args: {args:?} }}")?;
                    }
                    BehavioralFormula::Forall { var, domain, body }
                    | BehavioralFormula::Exists { var, domain, body } => {
                        write!(
                            formatter,
                            "{} {{ var: {var:?}, domain: {domain:?}, body: ",
                            if matches!(formula, BehavioralFormula::Forall { .. }) {
                                "Forall"
                            } else {
                                "Exists"
                            }
                        )?;
                        events.push(Event::Text(" }"));
                        events.push(Event::Formula(body));
                    }
                    BehavioralFormula::Atom(label) => write!(formatter, "Atom({label:?})")?,
                    BehavioralFormula::FixVar(var) => write!(formatter, "FixVar({var:?})")?,
                    BehavioralFormula::Diamond(action, body)
                    | BehavioralFormula::BoxAll(action, body) => {
                        write!(
                            formatter,
                            "{}({action:?}, ",
                            if matches!(formula, BehavioralFormula::Diamond(..)) {
                                "Diamond"
                            } else {
                                "BoxAll"
                            }
                        )?;
                        events.push(Event::Text(")"));
                        events.push(Event::Formula(body));
                    }
                    BehavioralFormula::Mu(var, body) | BehavioralFormula::Nu(var, body) => {
                        write!(
                            formatter,
                            "{}({var:?}, ",
                            if matches!(formula, BehavioralFormula::Mu(..)) {
                                "Mu"
                            } else {
                                "Nu"
                            }
                        )?;
                        events.push(Event::Text(")"));
                        events.push(Event::Formula(body));
                    }
                    BehavioralFormula::And(left, right) | BehavioralFormula::Or(left, right) => {
                        formatter.write_str(if matches!(formula, BehavioralFormula::And(..)) {
                            "And("
                        } else {
                            "Or("
                        })?;
                        events.push(Event::Text(")"));
                        events.push(Event::Formula(right));
                        events.push(Event::Text(", "));
                        events.push(Event::Formula(left));
                    }
                    BehavioralFormula::Not(inner) => {
                        formatter.write_str("Not(")?;
                        events.push(Event::Text(")"));
                        events.push(Event::Formula(inner));
                    }
                },
            }
        }
        Ok(())
    }
}

impl Drop for BehavioralFormula {
    fn drop(&mut self) {
        fn drain(formula: &mut BehavioralFormula, pending: &mut Vec<BehavioralFormula>) {
            match formula {
                BehavioralFormula::Forall { body, .. }
                | BehavioralFormula::Exists { body, .. }
                | BehavioralFormula::Diamond(_, body)
                | BehavioralFormula::BoxAll(_, body)
                | BehavioralFormula::Mu(_, body)
                | BehavioralFormula::Nu(_, body)
                | BehavioralFormula::Not(body) => {
                    pending.push(std::mem::replace(&mut **body, BehavioralFormula::Top));
                }
                BehavioralFormula::And(left, right) | BehavioralFormula::Or(left, right) => {
                    pending.push(std::mem::replace(&mut **right, BehavioralFormula::Top));
                    pending.push(std::mem::replace(&mut **left, BehavioralFormula::Top));
                }
                BehavioralFormula::Top
                | BehavioralFormula::Bot
                | BehavioralFormula::Relation { .. }
                | BehavioralFormula::Atom(_)
                | BehavioralFormula::FixVar(_) => {}
            }
        }

        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut formula) = pending.pop() {
            drain(&mut formula, &mut pending);
        }
    }
}

impl BehavioralFormula {
    /// Collect the free variables (not bound by an enclosing quantifier).
    fn free_vars(&self, bound: &mut BTreeSet<String>, acc: &mut BTreeSet<String>) {
        enum Task<'a> {
            Visit(&'a BehavioralFormula),
            ExitBinding(&'a str, bool),
        }
        let mut tasks = vec![Task::Visit(self)];
        while let Some(task) = tasks.pop() {
            match task {
                Task::ExitBinding(var, was_present) => {
                    if !was_present {
                        bound.remove(var);
                    }
                }
                Task::Visit(formula) => match formula {
                    BehavioralFormula::Top | BehavioralFormula::Bot => {}
                    BehavioralFormula::Relation { args, .. } => {
                        for arg in args {
                            if let Arg::Var(var) = arg {
                                if !bound.contains(var) {
                                    acc.insert(var.clone());
                                }
                            }
                        }
                    }
                    BehavioralFormula::Forall { var, body, .. }
                    | BehavioralFormula::Exists { var, body, .. } => {
                        let was_present = !bound.insert(var.clone());
                        tasks.push(Task::ExitBinding(var, was_present));
                        tasks.push(Task::Visit(body));
                    }
                    BehavioralFormula::And(left, right) | BehavioralFormula::Or(left, right) => {
                        tasks.push(Task::Visit(right));
                        tasks.push(Task::Visit(left));
                    }
                    BehavioralFormula::Not(inner)
                    | BehavioralFormula::Diamond(_, inner)
                    | BehavioralFormula::BoxAll(_, inner)
                    | BehavioralFormula::Mu(_, inner)
                    | BehavioralFormula::Nu(_, inner) => tasks.push(Task::Visit(inner)),
                    // Fixpoint variables are in a separate state-set namespace.
                    BehavioralFormula::Atom(_) | BehavioralFormula::FixVar(_) => {}
                },
            }
        }
    }

    /// Whether the formula uses any modal/temporal operator (and therefore needs
    /// the LTS, not just the fact base).
    fn has_modal(&self) -> bool {
        let mut pending = vec![self];
        while let Some(formula) = pending.pop() {
            match formula {
                BehavioralFormula::Atom(_)
                | BehavioralFormula::Diamond(..)
                | BehavioralFormula::BoxAll(..)
                | BehavioralFormula::Mu(..)
                | BehavioralFormula::Nu(..)
                | BehavioralFormula::FixVar(_) => return true,
                BehavioralFormula::And(left, right) | BehavioralFormula::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                BehavioralFormula::Not(inner)
                | BehavioralFormula::Forall { body: inner, .. }
                | BehavioralFormula::Exists { body: inner, .. } => pending.push(inner),
                BehavioralFormula::Top
                | BehavioralFormula::Bot
                | BehavioralFormula::Relation { .. } => {}
            }
        }
        false
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// BehavioralWorld (domain element)
// ══════════════════════════════════════════════════════════════════════════════

/// A concrete element the behavioral predicate is evaluated against: a host term
/// (for the modal/temporal fragments) plus a binding environment (for the
/// relational fragment).
#[derive(Clone, Debug)]
pub struct BehavioralWorld<H: HostTerm> {
    /// The term (its LTS is used by modal/temporal fragments).
    pub term: H,
    /// Variable bindings.
    pub env: BTreeMap<String, String>,
}

impl<H: HostTerm> BehavioralWorld<H> {
    /// A world with the given term and no bindings.
    pub fn new(term: H) -> Self {
        BehavioralWorld {
            term,
            env: BTreeMap::new(),
        }
    }

    /// A world with the given term and bindings.
    pub fn with_env(term: H, env: BTreeMap<String, String>) -> Self {
        BehavioralWorld { term, env }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// BehavioralAlgebra
// ══════════════════════════════════════════════════════════════════════════════

/// The behavioral algebra over a fixed fact-base snapshot and a host-term type.
#[derive(Clone, Debug)]
pub struct BehavioralAlgebra<H: HostTerm> {
    facts: Arc<FactBase>,
    search_budget: usize,
    _marker: std::marker::PhantomData<fn() -> H>,
}

impl<H: HostTerm> BehavioralAlgebra<H> {
    /// Construct over the given fact base (default search budget).
    pub fn new(facts: FactBase) -> Self {
        BehavioralAlgebra {
            facts: Arc::new(facts),
            search_budget: DEFAULT_SEARCH_BUDGET,
            _marker: std::marker::PhantomData,
        }
    }

    /// Override the satisfiability search budget.
    pub fn with_budget(mut self, budget: usize) -> Self {
        self.search_budget = budget;
        self
    }

    fn resolve(&self, arg: &Arg, env: &BTreeMap<String, String>) -> Option<String> {
        match arg {
            Arg::Lit(s) => Some(s.clone()),
            Arg::Var(v) => env.get(v).cloned(),
        }
    }

    fn domain_values(&self, domain: &QDomain) -> (Vec<String>, bool) {
        // Returns (values, exact). `exact = false` means the domain was bounded
        // and may have been truncated.
        let mut current = domain;
        let mut limits = Vec::new();
        while let QDomain::Bounded(inner, limit) = current {
            limits.push(*limit);
            current = inner;
        }
        let mut values = match current {
            QDomain::Values(vs) => (vs.clone(), true),
            QDomain::Active => (self.facts.active_domain().into_iter().collect(), true),
            QDomain::RelationColumn(rel, col) => {
                let mut vals = BTreeSet::new();
                if let Some(tuples) = self.facts.relations.get(rel) {
                    for t in tuples {
                        if let Some(v) = t.get(*col) {
                            vals.insert(v.clone());
                        }
                    }
                }
                (vals.into_iter().collect(), true)
            }
            QDomain::Bounded(_, _) => unreachable!("bounded-domain spine reaches a base"),
        };
        while let Some(limit) = limits.pop() {
            let truncated = values.0.len() > limit;
            values.0.truncate(limit);
            values.1 &= !truncated;
        }
        values
    }

    /// Evaluate `formula` against the snapshot with the given bindings. Returns
    /// `(result, exact)`; `exact = false` when a bounded quantifier may have
    /// been truncated (so a `false`/`true` could be budget-limited).
    fn eval(&self, formula: &BehavioralFormula, env: &BTreeMap<String, String>) -> (bool, bool) {
        type Env = BTreeMap<String, String>;
        enum Frame<'a> {
            Eval(&'a BehavioralFormula, Env),
            Not,
            AndRight(&'a BehavioralFormula, Env),
            AndFinish(bool),
            OrRight(&'a BehavioralFormula, Env),
            OrFinish(bool),
            Quantifier {
                var: &'a str,
                body: &'a BehavioralFormula,
                remaining: std::vec::IntoIter<String>,
                base_env: Env,
                universal: bool,
                exact: bool,
            },
        }

        let mut frames = vec![Frame::Eval(formula, env.clone())];
        let mut result = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Eval(current, current_env) => match current {
                    BehavioralFormula::Top => result = Some((true, true)),
                    BehavioralFormula::Bot => result = Some((false, true)),
                    BehavioralFormula::Relation { name, args } => {
                        let tuple: Option<Vec<String>> = args
                            .iter()
                            .map(|argument| self.resolve(argument, &current_env))
                            .collect();
                        result = Some(match tuple {
                            Some(tuple) => (self.facts.holds(name, &tuple), true),
                            None => (false, true),
                        });
                    }
                    BehavioralFormula::Forall { var, domain, body }
                    | BehavioralFormula::Exists { var, domain, body } => {
                        let universal = matches!(current, BehavioralFormula::Forall { .. });
                        let (values, domain_exact) = self.domain_values(domain);
                        let mut remaining = values.into_iter();
                        if let Some(value) = remaining.next() {
                            let mut inner_env = current_env.clone();
                            inner_env.insert(var.clone(), value);
                            frames.push(Frame::Quantifier {
                                var,
                                body,
                                remaining,
                                base_env: current_env,
                                universal,
                                exact: domain_exact,
                            });
                            frames.push(Frame::Eval(body, inner_env));
                        } else {
                            result = Some((universal, domain_exact));
                        }
                    }
                    BehavioralFormula::And(left, right) => {
                        frames.push(Frame::AndRight(right, current_env.clone()));
                        frames.push(Frame::Eval(left, current_env));
                    }
                    BehavioralFormula::Or(left, right) => {
                        frames.push(Frame::OrRight(right, current_env.clone()));
                        frames.push(Frame::Eval(left, current_env));
                    }
                    BehavioralFormula::Not(inner) => {
                        frames.push(Frame::Not);
                        frames.push(Frame::Eval(inner, current_env));
                    }
                    BehavioralFormula::Atom(_)
                    | BehavioralFormula::Diamond(..)
                    | BehavioralFormula::BoxAll(..)
                    | BehavioralFormula::Mu(..)
                    | BehavioralFormula::Nu(..)
                    | BehavioralFormula::FixVar(_) => {
                        unreachable!("modal formula reached the relational evaluator")
                    }
                },
                Frame::Not => {
                    let (value, exact) = result.take().expect("negated formula is evaluated");
                    result = Some((!value, exact));
                }
                Frame::AndRight(right, env) => {
                    let (left, exact) = result.take().expect("left conjunction is evaluated");
                    if left {
                        frames.push(Frame::AndFinish(exact));
                        frames.push(Frame::Eval(right, env));
                    } else {
                        result = Some((false, exact));
                    }
                }
                Frame::AndFinish(left_exact) => {
                    let (right, right_exact) =
                        result.take().expect("right conjunction is evaluated");
                    result = Some((right, left_exact && right_exact));
                }
                Frame::OrRight(right, env) => {
                    let (left, exact) = result.take().expect("left disjunction is evaluated");
                    if left {
                        result = Some((true, exact));
                    } else {
                        frames.push(Frame::OrFinish(exact));
                        frames.push(Frame::Eval(right, env));
                    }
                }
                Frame::OrFinish(left_exact) => {
                    let (right, right_exact) =
                        result.take().expect("right disjunction is evaluated");
                    result = Some((right, left_exact && right_exact));
                }
                Frame::Quantifier {
                    var,
                    body,
                    mut remaining,
                    base_env,
                    universal,
                    mut exact,
                } => {
                    let (value, body_exact) = result
                        .take()
                        .expect("behavioral quantifier body is evaluated");
                    exact &= body_exact;
                    if (universal && !value) || (!universal && value) {
                        result = Some((value, exact));
                    } else if let Some(value) = remaining.next() {
                        let mut inner_env = base_env.clone();
                        inner_env.insert(var.to_string(), value);
                        frames.push(Frame::Quantifier {
                            var,
                            body,
                            remaining,
                            base_env,
                            universal,
                            exact,
                        });
                        frames.push(Frame::Eval(body, inner_env));
                    } else {
                        result = Some((universal, exact));
                    }
                }
            }
        }
        result.expect("the root behavioral formula produces one relational result")
    }

    /// Build the reachable LTS from `root` (BFS), capped at `MAX_REACH_STATES`.
    /// Returns the states (index 0 = root) and adjacency `(action, target)`.
    fn build_lts(&self, root: &H) -> (Vec<H>, Vec<Vec<(String, usize)>>) {
        let mut states = vec![root.clone()];
        let mut index: HashMap<H, usize> = HashMap::new();
        index.insert(root.clone(), 0);
        let mut adj: Vec<Vec<(String, usize)>> = vec![Vec::new()];
        let mut queue = VecDeque::from([0usize]);
        while let Some(i) = queue.pop_front() {
            for (action, next) in states[i].successors() {
                let j = match index.get(&next) {
                    Some(&j) => j,
                    None => {
                        if states.len() >= MAX_REACH_STATES {
                            continue; // truncated (reject-safe: missing edges only shrink modal sets)
                        }
                        let j = states.len();
                        states.push(next.clone());
                        index.insert(next, j);
                        adj.push(Vec::new());
                        queue.push_back(j);
                        j
                    }
                };
                adj[i].push((action, j));
            }
        }
        (states, adj)
    }

    /// The set of state indices satisfying `formula` (finite mu-calculus model
    /// checking over the reachable LTS). `fix` maps fixpoint variables to their
    /// current state sets.
    fn denote(
        &self,
        formula: &BehavioralFormula,
        states: &[H],
        adj: &[Vec<(String, usize)>],
        env: &BTreeMap<String, String>,
        fix: &HashMap<String, HashSet<usize>>,
    ) -> HashSet<usize> {
        type StateSet = HashSet<usize>;
        type Env = std::rc::Rc<BTreeMap<String, String>>;
        type Fix = std::rc::Rc<HashMap<String, StateSet>>;

        enum Frame<'a> {
            Eval(&'a BehavioralFormula, Env, Fix),
            Not,
            AndRight(&'a BehavioralFormula, Env, Fix),
            AndFinish(StateSet),
            OrRight(&'a BehavioralFormula, Env, Fix),
            OrFinish(StateSet),
            Quantifier {
                var: &'a str,
                body: &'a BehavioralFormula,
                remaining: std::vec::IntoIter<String>,
                base_env: Env,
                fix: Fix,
                accumulator: StateSet,
                universal: bool,
            },
            Modal {
                action: &'a ActionPattern,
                existential: bool,
            },
            Fixpoint {
                var: &'a str,
                body: &'a BehavioralFormula,
                env: Env,
                base_fix: Fix,
                current: StateSet,
                remaining: usize,
            },
        }

        let all_states = || (0..states.len()).collect::<StateSet>();
        let mut frames = vec![Frame::Eval(
            formula,
            std::rc::Rc::new(env.clone()),
            std::rc::Rc::new(fix.clone()),
        )];
        let mut result = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Eval(current, current_env, current_fix) => match current {
                    BehavioralFormula::Top => result = Some(all_states()),
                    BehavioralFormula::Bot => result = Some(StateSet::new()),
                    BehavioralFormula::Atom(label) => {
                        result = Some(
                            (0..states.len())
                                .filter(|&index| states[index].label() == *label)
                                .collect(),
                        );
                    }
                    BehavioralFormula::Relation { .. } => {
                        result = Some(if self.eval(current, current_env.as_ref()).0 {
                            all_states()
                        } else {
                            StateSet::new()
                        });
                    }
                    BehavioralFormula::Forall { var, domain, body }
                    | BehavioralFormula::Exists { var, domain, body } => {
                        let universal = matches!(current, BehavioralFormula::Forall { .. });
                        let (values, _) = self.domain_values(domain);
                        let mut remaining = values.into_iter();
                        let accumulator = if universal {
                            all_states()
                        } else {
                            StateSet::new()
                        };
                        if let Some(value) = remaining.next() {
                            let mut inner_env = current_env.as_ref().clone();
                            inner_env.insert(var.clone(), value);
                            frames.push(Frame::Quantifier {
                                var,
                                body,
                                remaining,
                                base_env: current_env,
                                fix: current_fix.clone(),
                                accumulator,
                                universal,
                            });
                            frames.push(Frame::Eval(
                                body,
                                std::rc::Rc::new(inner_env),
                                current_fix,
                            ));
                        } else {
                            result = Some(accumulator);
                        }
                    }
                    BehavioralFormula::And(left, right) => {
                        frames.push(Frame::AndRight(
                            right,
                            current_env.clone(),
                            current_fix.clone(),
                        ));
                        frames.push(Frame::Eval(left, current_env, current_fix));
                    }
                    BehavioralFormula::Or(left, right) => {
                        frames.push(Frame::OrRight(
                            right,
                            current_env.clone(),
                            current_fix.clone(),
                        ));
                        frames.push(Frame::Eval(left, current_env, current_fix));
                    }
                    BehavioralFormula::Not(inner) => {
                        frames.push(Frame::Not);
                        frames.push(Frame::Eval(inner, current_env, current_fix));
                    }
                    BehavioralFormula::Diamond(action, body)
                    | BehavioralFormula::BoxAll(action, body) => {
                        frames.push(Frame::Modal {
                            action,
                            existential: matches!(current, BehavioralFormula::Diamond(..)),
                        });
                        frames.push(Frame::Eval(body, current_env, current_fix));
                    }
                    BehavioralFormula::Mu(var, body) | BehavioralFormula::Nu(var, body) => {
                        let current_set = if matches!(current, BehavioralFormula::Mu(..)) {
                            StateSet::new()
                        } else {
                            all_states()
                        };
                        let mut body_fix = current_fix.as_ref().clone();
                        body_fix.insert(var.clone(), current_set.clone());
                        frames.push(Frame::Fixpoint {
                            var,
                            body,
                            env: current_env.clone(),
                            base_fix: current_fix,
                            current: current_set,
                            remaining: states.len(),
                        });
                        frames.push(Frame::Eval(body, current_env, std::rc::Rc::new(body_fix)));
                    }
                    BehavioralFormula::FixVar(var) => {
                        result = Some(current_fix.get(var).cloned().unwrap_or_default());
                    }
                },
                Frame::Not => {
                    let inner = result.take().expect("negated denotation is evaluated");
                    result = Some(
                        (0..states.len())
                            .filter(|index| !inner.contains(index))
                            .collect(),
                    );
                }
                Frame::AndRight(right, env, fix) => {
                    let left = result.take().expect("left denotation is evaluated");
                    frames.push(Frame::AndFinish(left));
                    frames.push(Frame::Eval(right, env, fix));
                }
                Frame::AndFinish(left) => {
                    let right = result.take().expect("right denotation is evaluated");
                    result = Some(left.intersection(&right).copied().collect());
                }
                Frame::OrRight(right, env, fix) => {
                    let left = result.take().expect("left denotation is evaluated");
                    frames.push(Frame::OrFinish(left));
                    frames.push(Frame::Eval(right, env, fix));
                }
                Frame::OrFinish(left) => {
                    let right = result.take().expect("right denotation is evaluated");
                    result = Some(left.union(&right).copied().collect());
                }
                Frame::Quantifier {
                    var,
                    body,
                    mut remaining,
                    base_env,
                    fix,
                    accumulator,
                    universal,
                } => {
                    let body_set = result
                        .take()
                        .expect("behavioral quantified denotation is evaluated");
                    let accumulator = if universal {
                        accumulator.intersection(&body_set).copied().collect()
                    } else {
                        accumulator.union(&body_set).copied().collect()
                    };
                    if let Some(value) = remaining.next() {
                        let mut inner_env = base_env.as_ref().clone();
                        inner_env.insert(var.to_string(), value);
                        frames.push(Frame::Quantifier {
                            var,
                            body,
                            remaining,
                            base_env,
                            fix: fix.clone(),
                            accumulator,
                            universal,
                        });
                        frames.push(Frame::Eval(body, std::rc::Rc::new(inner_env), fix));
                    } else {
                        result = Some(accumulator);
                    }
                }
                Frame::Modal {
                    action,
                    existential,
                } => {
                    let body = result.take().expect("modal body denotation is evaluated");
                    result = Some(
                        (0..states.len())
                            .filter(|&index| {
                                if existential {
                                    adj[index].iter().any(|(label, target)| {
                                        action.matches(label) && body.contains(target)
                                    })
                                } else {
                                    adj[index].iter().all(|(label, target)| {
                                        !action.matches(label) || body.contains(target)
                                    })
                                }
                            })
                            .collect(),
                    );
                }
                Frame::Fixpoint {
                    var,
                    body,
                    env,
                    base_fix,
                    current,
                    remaining,
                } => {
                    let next = result
                        .take()
                        .expect("fixpoint body denotation is evaluated");
                    if next == current || remaining == 0 {
                        result = Some(next);
                    } else {
                        let mut body_fix = base_fix.as_ref().clone();
                        body_fix.insert(var.to_string(), next.clone());
                        frames.push(Frame::Fixpoint {
                            var,
                            body,
                            env: env.clone(),
                            base_fix,
                            current: next,
                            remaining: remaining - 1,
                        });
                        frames.push(Frame::Eval(body, env, std::rc::Rc::new(body_fix)));
                    }
                }
            }
        }
        result.expect("the root behavioral formula produces one denotation")
    }
}

/// Cap on reachable-LTS size for modal model checking (beyond it the LTS is
/// truncated; missing edges only shrink modal satisfaction sets — reject-safe).
const MAX_REACH_STATES: usize = 10_000;

impl<H: HostTerm> RejectSafeAlgebra for BehavioralAlgebra<H> {
    type Predicate = BehavioralFormula;
    type Domain = BehavioralWorld<H>;

    fn true_pred(&self) -> BehavioralFormula {
        BehavioralFormula::Top
    }

    fn false_pred(&self) -> BehavioralFormula {
        BehavioralFormula::Bot
    }

    fn and(&self, a: &BehavioralFormula, b: &BehavioralFormula) -> BehavioralFormula {
        match (a, b) {
            (BehavioralFormula::Bot, _) | (_, BehavioralFormula::Bot) => BehavioralFormula::Bot,
            (BehavioralFormula::Top, x) | (x, BehavioralFormula::Top) => x.clone(),
            _ => BehavioralFormula::And(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn or(&self, a: &BehavioralFormula, b: &BehavioralFormula) -> BehavioralFormula {
        match (a, b) {
            (BehavioralFormula::Top, _) | (_, BehavioralFormula::Top) => BehavioralFormula::Top,
            (BehavioralFormula::Bot, x) | (x, BehavioralFormula::Bot) => x.clone(),
            _ => BehavioralFormula::Or(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn pseudo_complement(&self, a: &BehavioralFormula) -> BehavioralFormula {
        match a {
            BehavioralFormula::Top => BehavioralFormula::Bot,
            BehavioralFormula::Bot => BehavioralFormula::Top,
            BehavioralFormula::Not(inner) => (**inner).clone(),
            _ => BehavioralFormula::Not(Box::new(a.clone())),
        }
    }

    fn is_satisfiable_3v(&self, a: &BehavioralFormula) -> Sat3 {
        if a.has_modal() {
            // Modal/temporal satisfiability (∃ a model) is semi-decidable without
            // a full mu-calculus SAT engine; report DontKnow honestly (reject-safe
            // — never a wrong Sat/Unsat). The model-checking direction (evaluate
            // against a given term) is exact.
            return Sat3::DontKnow;
        }
        // Relational: existentially close the free variables over the active
        // domain and search; exact (Sat/Unsat) unless the search budget is
        // exceeded or a bounded quantifier truncated.
        let mut free = BTreeSet::new();
        a.free_vars(&mut BTreeSet::new(), &mut free);
        let free: Vec<String> = free.into_iter().collect();
        let domain: Vec<String> = self.facts.active_domain().into_iter().collect();

        // Budget: |domain|^|free| assignments.
        let assignments = (domain.len().max(1)).checked_pow(free.len() as u32);
        match assignments {
            Some(n) if n <= self.search_budget => {}
            _ => return Sat3::DontKnow, // search space too large
        }

        let mut env = BTreeMap::new();
        let mut all_exact = true;
        let mut idx = vec![0usize; free.len()];
        loop {
            for (i, var) in free.iter().enumerate() {
                // domain may be empty: then there are no free assignments, but a
                // closed formula still gets evaluated once below.
                if let Some(v) = domain.get(idx[i]) {
                    env.insert(var.clone(), v.clone());
                }
            }
            // If there are free vars but the domain is empty, no assignment can
            // satisfy a positive atom; evaluate once with empty env.
            let (sat, exact) = self.eval(a, &env);
            all_exact = all_exact && exact;
            if sat {
                return Sat3::Sat;
            }
            // advance mixed-radix counter over the domain
            if free.is_empty() || domain.is_empty() {
                break;
            }
            let mut i = 0;
            loop {
                if i == free.len() {
                    // exhausted all assignments
                    return if all_exact {
                        Sat3::Unsat
                    } else {
                        Sat3::DontKnow
                    };
                }
                idx[i] += 1;
                if idx[i] < domain.len() {
                    break;
                }
                idx[i] = 0;
                i += 1;
            }
        }
        if all_exact {
            Sat3::Unsat
        } else {
            Sat3::DontKnow
        }
    }

    fn evaluate(&self, pred: &BehavioralFormula, elem: &BehavioralWorld<H>) -> bool {
        if !pred.has_modal() {
            // Relational fast path: evaluate against the fact base + bindings.
            return self.eval(pred, &elem.env).0;
        }
        // Modal/temporal: model-check over the term's reachable LTS.
        let (states, adj) = self.build_lts(&elem.term);
        self.denote(pred, &states, &adj, &elem.env, &HashMap::new())
            .contains(&0)
    }
}

impl<H: HostTerm> HeytingAlgebra for BehavioralAlgebra<H> {
    fn implies(&self, a: &BehavioralFormula, b: &BehavioralFormula) -> BehavioralFormula {
        // reject-safe material implication ¬a ∨ b
        self.or(&self.pseudo_complement(a), b)
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// CTL temporal operators (sugar over the mu-calculus modal fragment)
// ══════════════════════════════════════════════════════════════════════════════
//
// The modal mu-calculus (Diamond/BoxAll/Mu/Nu) is strictly more expressive than
// CTL and LTL on finite transition systems, so the standard branching-time
// temporal operators are *derived* — each desugars to a fixpoint formula that
// the model checker (`denote`) already decides exactly. A single fixpoint
// variable name is reused throughout: nesting is handled by `denote`'s lexical
// shadowing (an inner fixpoint rebinds the variable for its own body), and CTL
// sugar is always closed, so no free occurrence ever escapes a constructor.
//
// Deadlock convention: maximal-run semantics. A state with no successors is the
// end of its run; the encodings include `⟨-⟩⊤` / `[-]⊥` guards so that, e.g.,
// `AF φ` is false at a φ-free deadlock and `AG φ`/`EG φ` are correct there.
//
// (Linear-time LTL with fairness — e.g. `GF p` — is the one fragment the
// branching mu-calculus cannot express; those properties route through the
// existing Büchi engine, `crate::buchi` / `crate::ltl`.)

const CTL_VAR: &str = "__ctl";

fn diamond_any(f: BehavioralFormula) -> BehavioralFormula {
    BehavioralFormula::Diamond(ActionPattern::Any, Box::new(f))
}
fn box_any(f: BehavioralFormula) -> BehavioralFormula {
    BehavioralFormula::BoxAll(ActionPattern::Any, Box::new(f))
}
fn fixvar() -> BehavioralFormula {
    BehavioralFormula::FixVar(CTL_VAR.to_string())
}
fn mu(body: BehavioralFormula) -> BehavioralFormula {
    BehavioralFormula::Mu(CTL_VAR.to_string(), Box::new(body))
}
fn nu(body: BehavioralFormula) -> BehavioralFormula {
    BehavioralFormula::Nu(CTL_VAR.to_string(), Box::new(body))
}
fn and(a: BehavioralFormula, b: BehavioralFormula) -> BehavioralFormula {
    BehavioralFormula::And(Box::new(a), Box::new(b))
}
fn or(a: BehavioralFormula, b: BehavioralFormula) -> BehavioralFormula {
    BehavioralFormula::Or(Box::new(a), Box::new(b))
}
/// `⟨-⟩⊤` — the state has at least one successor (is not a deadlock).
fn can_progress() -> BehavioralFormula {
    diamond_any(BehavioralFormula::Top)
}

/// `AX φ` — all successors satisfy `φ` (vacuously true at a deadlock).
pub fn ax(phi: BehavioralFormula) -> BehavioralFormula {
    box_any(phi)
}
/// `EX φ` — some successor satisfies `φ`.
pub fn ex(phi: BehavioralFormula) -> BehavioralFormula {
    diamond_any(phi)
}
/// `EF φ` — `φ` is reachable on some run.
pub fn ef(phi: BehavioralFormula) -> BehavioralFormula {
    mu(or(phi, diamond_any(fixvar())))
}
/// `AG φ` — `φ` holds on all states of all runs (safety/invariance).
pub fn ag(phi: BehavioralFormula) -> BehavioralFormula {
    nu(and(phi, box_any(fixvar())))
}
/// `AF φ` — `φ` holds eventually on every maximal run (false at a φ-free deadlock).
pub fn af(phi: BehavioralFormula) -> BehavioralFormula {
    mu(or(phi, and(box_any(fixvar()), can_progress())))
}
/// `EG φ` — some maximal run keeps `φ` true throughout.
pub fn eg(phi: BehavioralFormula) -> BehavioralFormula {
    // φ ∧ (⟨-⟩X ∨ deadlock); deadlock = [-]⊥.
    nu(and(
        phi,
        or(diamond_any(fixvar()), box_any(BehavioralFormula::Bot)),
    ))
}
/// `A(φ U ψ)` — on every maximal run, `φ` holds until `ψ`.
pub fn au(phi: BehavioralFormula, psi: BehavioralFormula) -> BehavioralFormula {
    mu(or(psi, and(phi, and(box_any(fixvar()), can_progress()))))
}
/// `E(φ U ψ)` — some run has `φ` until `ψ`.
pub fn eu(phi: BehavioralFormula, psi: BehavioralFormula) -> BehavioralFormula {
    mu(or(psi, and(phi, diamond_any(fixvar()))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(s: &str) -> Arg {
        Arg::Lit(s.to_string())
    }
    fn var(s: &str) -> Arg {
        Arg::Var(s.to_string())
    }

    fn sample_facts() -> FactBase {
        let mut f = FactBase::new();
        f.add_fact("edge", vec!["a".into(), "b".into()]);
        f.add_fact("edge", vec!["b".into(), "c".into()]);
        f.add_fact("safe", vec!["c".into()]);
        f
    }

    #[test]
    fn relation_evaluate() {
        let alg = BehavioralAlgebra::<NoTerm>::new(sample_facts());
        let p = BehavioralFormula::Relation {
            name: "edge".into(),
            args: vec![lit("a"), lit("b")],
        };
        let mut env = BTreeMap::new();
        let w = BehavioralWorld::with_env(NoTerm, env.clone());
        assert!(alg.evaluate(&p, &w));
        let q = BehavioralFormula::Relation {
            name: "edge".into(),
            args: vec![lit("a"), lit("c")],
        };
        assert!(!alg.evaluate(&q, &BehavioralWorld::new(NoTerm)));
        // with a binding
        env.insert("x".into(), "b".into());
        let r = BehavioralFormula::Relation {
            name: "edge".into(),
            args: vec![lit("a"), var("x")],
        };
        assert!(alg.evaluate(&r, &BehavioralWorld::with_env(NoTerm, env)));
    }

    #[test]
    fn satisfiable_existential() {
        let alg = BehavioralAlgebra::<NoTerm>::new(sample_facts());
        // ∃x. edge(a, x)  → Sat (x=b)
        let p = BehavioralFormula::Relation {
            name: "edge".into(),
            args: vec![lit("a"), var("x")],
        };
        assert_eq!(alg.is_satisfiable_3v(&p), Sat3::Sat);
        // edge(a, z) with z forced to a value not present → Unsat over active domain
        let q = BehavioralFormula::Relation {
            name: "edge".into(),
            args: vec![lit("z"), lit("z")],
        };
        assert_eq!(alg.is_satisfiable_3v(&q), Sat3::Unsat);
    }

    #[test]
    fn quantifiers() {
        let alg = BehavioralAlgebra::<NoTerm>::new(sample_facts());
        // ∃y. edge(a,y) ∧ ∃z. edge(y,z)   — a→b→c chain
        let inner = BehavioralFormula::Exists {
            var: "z".into(),
            domain: QDomain::Active,
            body: Box::new(BehavioralFormula::Relation {
                name: "edge".into(),
                args: vec![var("y"), var("z")],
            }),
        };
        let chain = BehavioralFormula::Exists {
            var: "y".into(),
            domain: QDomain::Active,
            body: Box::new(BehavioralFormula::And(
                Box::new(BehavioralFormula::Relation {
                    name: "edge".into(),
                    args: vec![lit("a"), var("y")],
                }),
                Box::new(inner),
            )),
        };
        assert_eq!(alg.is_satisfiable_3v(&chain), Sat3::Sat);
        assert!(alg.evaluate(&chain, &BehavioralWorld::new(NoTerm)));

        // ∀y. edge(a,y) → safe(y)  is FALSE (b is not safe)
        let univ = BehavioralFormula::Forall {
            var: "y".into(),
            domain: QDomain::Active,
            body: Box::new(BehavioralFormula::Or(
                Box::new(BehavioralFormula::Not(Box::new(
                    BehavioralFormula::Relation {
                        name: "edge".into(),
                        args: vec![lit("a"), var("y")],
                    },
                ))),
                Box::new(BehavioralFormula::Relation {
                    name: "safe".into(),
                    args: vec![var("y")],
                }),
            )),
        };
        assert!(!alg.evaluate(&univ, &BehavioralWorld::new(NoTerm)));
    }

    #[test]
    fn heyting_structure_and_safety() {
        let alg = BehavioralAlgebra::<NoTerm>::new(sample_facts());
        let p = BehavioralFormula::Relation {
            name: "safe".into(),
            args: vec![lit("c")],
        };
        let np = alg.pseudo_complement(&p);
        let w = BehavioralWorld::new(NoTerm);
        assert!(alg.evaluate(&p, &w));
        assert!(!alg.evaluate(&np, &w));
        // double negation collapses structurally here (Not(Not p) -> p via smart ctor)
        assert_eq!(alg.pseudo_complement(&np), p);
        // a ∧ ¬a is unsatisfiable over the snapshot
        assert_eq!(alg.is_satisfiable_3v(&alg.and(&p, &np)), Sat3::Unsat);

        // The safety property: a function bounded on BooleanAlgebra cannot accept
        // BehavioralAlgebra (it only implements HeytingAlgebra). We confirm it is
        // usable through the Heyting tier.
        fn via_heyting<A: HeytingAlgebra>(
            alg: &A,
            a: &A::Predicate,
            b: &A::Predicate,
        ) -> A::Predicate {
            alg.implies(a, b)
        }
        let _ = via_heyting(&alg, &p, &BehavioralFormula::Top);
    }

    #[test]
    fn budget_exceeded_is_dontknow() {
        // Force a tiny budget so a 2-free-var formula over a multi-value domain
        // exceeds it → DontKnow (honest reject-safe).
        let alg = BehavioralAlgebra::<NoTerm>::new(sample_facts()).with_budget(2);
        let p = BehavioralFormula::And(
            Box::new(BehavioralFormula::Relation {
                name: "edge".into(),
                args: vec![var("x"), var("y")],
            }),
            Box::new(BehavioralFormula::Relation {
                name: "safe".into(),
                args: vec![var("y")],
            }),
        );
        assert_eq!(alg.is_satisfiable_3v(&p), Sat3::DontKnow);
    }

    // A tiny LTS: 0 --step--> 1 --step--> 2(done), 2 terminal.
    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    struct TestProc(u32);
    impl HostTerm for TestProc {
        fn successors(&self) -> Vec<(String, Self)> {
            match self.0 {
                0 => vec![("step".into(), TestProc(1))],
                1 => vec![("step".into(), TestProc(2))],
                _ => vec![],
            }
        }
        fn label(&self) -> String {
            if self.0 == 2 {
                "done".into()
            } else {
                String::new()
            }
        }
    }

    #[test]
    fn modal_diamond_box() {
        let alg = BehavioralAlgebra::<TestProc>::new(FactBase::new());
        let can_step = BehavioralFormula::Diamond(
            ActionPattern::Named("step".into()),
            Box::new(BehavioralFormula::Top),
        );
        assert!(alg.evaluate(&can_step, &BehavioralWorld::new(TestProc(0))));
        assert!(!alg.evaluate(&can_step, &BehavioralWorld::new(TestProc(2)))); // terminal
                                                                               // [step]⊥ at the terminal state: no step successors → vacuously true.
        let no_step = BehavioralFormula::BoxAll(
            ActionPattern::Named("step".into()),
            Box::new(BehavioralFormula::Bot),
        );
        assert!(alg.evaluate(&no_step, &BehavioralWorld::new(TestProc(2))));
        assert!(!alg.evaluate(&no_step, &BehavioralWorld::new(TestProc(0)))); // has a step
    }

    #[test]
    fn modal_eventually_done() {
        let alg = BehavioralAlgebra::<TestProc>::new(FactBase::new());
        // μX. (done ∨ ⟨-⟩X) — eventually reaches a 'done' state.
        let eventually = BehavioralFormula::Mu(
            "X".into(),
            Box::new(BehavioralFormula::Or(
                Box::new(BehavioralFormula::Atom("done".into())),
                Box::new(BehavioralFormula::Diamond(
                    ActionPattern::Any,
                    Box::new(BehavioralFormula::FixVar("X".into())),
                )),
            )),
        );
        assert!(alg.evaluate(&eventually, &BehavioralWorld::new(TestProc(0))));
        assert!(alg.evaluate(&eventually, &BehavioralWorld::new(TestProc(2)))); // already done
                                                                                // Modal satisfiability is honestly DontKnow.
        assert_eq!(alg.is_satisfiable_3v(&eventually), Sat3::DontKnow);
    }

    #[test]
    fn modal_no_infinite_path() {
        let alg = BehavioralAlgebra::<TestProc>::new(FactBase::new());
        // νX. ⟨-⟩X — an infinite path exists; the chain terminates ⇒ false.
        let inf = BehavioralFormula::Nu(
            "X".into(),
            Box::new(BehavioralFormula::Diamond(
                ActionPattern::Any,
                Box::new(BehavioralFormula::FixVar("X".into())),
            )),
        );
        assert!(!alg.evaluate(&inf, &BehavioralWorld::new(TestProc(0))));
        assert!(!alg.evaluate(&inf, &BehavioralWorld::new(TestProc(2))));
    }

    #[test]
    fn modal_invariant_box_chain() {
        let alg = BehavioralAlgebra::<TestProc>::new(FactBase::new());
        // νX. ([−]X) — trivially true (safety with no atomic constraint): every
        // state, and all its successors transitively, are in the set.
        let always = BehavioralFormula::Nu(
            "X".into(),
            Box::new(BehavioralFormula::BoxAll(
                ActionPattern::Any,
                Box::new(BehavioralFormula::FixVar("X".into())),
            )),
        );
        assert!(alg.evaluate(&always, &BehavioralWorld::new(TestProc(0))));
        // νX. (done ∧ [−]X) — "done holds globally" — false (states 0,1 not done).
        let always_done = BehavioralFormula::Nu(
            "X".into(),
            Box::new(BehavioralFormula::And(
                Box::new(BehavioralFormula::Atom("done".into())),
                Box::new(BehavioralFormula::BoxAll(
                    ActionPattern::Any,
                    Box::new(BehavioralFormula::FixVar("X".into())),
                )),
            )),
        );
        assert!(!alg.evaluate(&always_done, &BehavioralWorld::new(TestProc(0))));
    }

    #[test]
    fn ctl_temporal_operators() {
        let alg = BehavioralAlgebra::<TestProc>::new(FactBase::new());
        let done = || BehavioralFormula::Atom("done".into());
        let s0 = || BehavioralWorld::new(TestProc(0));
        let s2 = || BehavioralWorld::new(TestProc(2));

        // EF done — done is reachable.
        assert!(alg.evaluate(&ef(done()), &s0()));
        // AF done — every (here, the single) maximal run reaches done.
        assert!(alg.evaluate(&af(done()), &s0()));
        // AG done — false (states 0,1 are not done) but holds at the done state.
        assert!(!alg.evaluate(&ag(done()), &s0()));
        assert!(alg.evaluate(&ag(done()), &s2()));
        // AG ¬bad — safety with no 'bad' states → true.
        let no_bad = ag(BehavioralFormula::Not(Box::new(BehavioralFormula::Atom(
            "bad".into(),
        ))));
        assert!(alg.evaluate(&no_bad, &s0()));
        // E(¬done U done) — some run stays ¬done until done.
        let until = eu(BehavioralFormula::Not(Box::new(done())), done());
        assert!(alg.evaluate(&until, &s0()));
        // AX over a terminal: AX ⊥ is vacuously true at the deadlock state 2.
        assert!(alg.evaluate(&ax(BehavioralFormula::Bot), &s2()));
        // EX (¬done) from state 0 — successor (state 1) is ¬done.
        assert!(alg.evaluate(&ex(BehavioralFormula::Not(Box::new(done()))), &s0()));
    }
}
