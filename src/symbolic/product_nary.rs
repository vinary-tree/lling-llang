//! N-ary product and sum (coproduct) effective Boolean algebras — the
//! combinators that close the algebra family over the *structured* type
//! constructors:
//!
//! - [`NaryProductAlgebra`] — tuples / records: a value is a fixed-arity tuple
//!   `(x_0, …, x_{k-1})`, each component drawn from its own field algebra. The
//!   fields are **independent** (no shared variable), so satisfiability factors
//!   per field. Generalizes the 2-ary
//!   [`ProductAlgebra`](crate::symbolic::ProductAlgebra).
//! - [`SumAlgebra`] — variants / enums / grammar alternation: a value is a
//!   tagged payload `(tag, payload)`, the payload drawn from variant `tag`'s
//!   algebra.
//!
//! Both are generic over the element algebra `A: BooleanAlgebra`. Instantiating
//! at `A = AnyAlgebra` (the uniform recursive carrier) gives heterogeneous
//! tuples/variants (each field/variant a different sort). The predicate types
//! are parameterized by the *inner predicate type* `P = A::Predicate` rather
//! than `A`, so `derive(Eq, Hash)` works without spurious `A: Eq` bounds.

use std::fmt;
use std::hash::{Hash, Hasher};

use super::BooleanAlgebra;

// ══════════════════════════════════════════════════════════════════════════════
// N-ary product (tuples / records)
// ══════════════════════════════════════════════════════════════════════════════

/// A predicate over a tuple whose components have inner-predicate type `P`.
pub enum NaryProductPred<P> {
    /// Satisfied by every tuple.
    True,
    /// Satisfied by no tuple.
    False,
    /// Component `i` satisfies the inner predicate.
    Field(usize, P),
    /// Conjunction.
    And(Box<NaryProductPred<P>>, Box<NaryProductPred<P>>),
    /// Disjunction.
    Or(Box<NaryProductPred<P>>, Box<NaryProductPred<P>>),
    /// Negation.
    Not(Box<NaryProductPred<P>>),
}

impl<P: Clone> Clone for NaryProductPred<P> {
    fn clone(&self) -> Self {
        enum Task<'a, P> {
            Clone(&'a NaryProductPred<P>),
            And,
            Or,
            Not,
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(predicate) => match predicate {
                    NaryProductPred::True => values.push(NaryProductPred::True),
                    NaryProductPred::False => values.push(NaryProductPred::False),
                    NaryProductPred::Field(index, inner) => {
                        values.push(NaryProductPred::Field(*index, inner.clone()));
                    }
                    NaryProductPred::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    NaryProductPred::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    NaryProductPred::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Clone(inner));
                    }
                },
                Task::And | Task::Or => {
                    let right = values.pop().expect("right n-ary product clone is present");
                    let left = values.pop().expect("left n-ary product clone is present");
                    values.push(if matches!(task, Task::And) {
                        NaryProductPred::And(Box::new(left), Box::new(right))
                    } else {
                        NaryProductPred::Or(Box::new(left), Box::new(right))
                    });
                }
                Task::Not => {
                    let inner = values
                        .pop()
                        .expect("negated n-ary product clone is present");
                    values.push(NaryProductPred::Not(Box::new(inner)));
                }
            }
        }
        values
            .pop()
            .expect("the root n-ary product produces one clone")
    }
}

impl<P: PartialEq> PartialEq for NaryProductPred<P> {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (NaryProductPred::True, NaryProductPred::True)
                | (NaryProductPred::False, NaryProductPred::False) => {}
                (NaryProductPred::Field(li, lp), NaryProductPred::Field(ri, rp))
                    if li == ri && lp == rp => {}
                (NaryProductPred::And(ll, lr), NaryProductPred::And(rl, rr))
                | (NaryProductPred::Or(ll, lr), NaryProductPred::Or(rl, rr)) => {
                    pending.push((lr, rr));
                    pending.push((ll, rl));
                }
                (NaryProductPred::Not(left), NaryProductPred::Not(right)) => {
                    pending.push((left, right));
                }
                _ => return false,
            }
        }
        true
    }
}

impl<P: Eq> Eq for NaryProductPred<P> {}

impl<P: Hash> Hash for NaryProductPred<P> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut pending = vec![self];
        while let Some(predicate) = pending.pop() {
            std::mem::discriminant(predicate).hash(state);
            match predicate {
                NaryProductPred::True | NaryProductPred::False => {}
                NaryProductPred::Field(index, inner) => {
                    index.hash(state);
                    inner.hash(state);
                }
                NaryProductPred::And(left, right) | NaryProductPred::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                NaryProductPred::Not(inner) => pending.push(inner),
            }
        }
    }
}

impl<P: fmt::Debug> fmt::Debug for NaryProductPred<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a, P> {
            Pred(&'a NaryProductPred<P>),
            Text(&'static str),
        }
        let mut events = vec![Event::Pred(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Pred(predicate) => match predicate {
                    NaryProductPred::True => write!(f, "True")?,
                    NaryProductPred::False => write!(f, "False")?,
                    NaryProductPred::Field(index, inner) => {
                        write!(f, "Field({index:?}, {inner:?})")?;
                    }
                    NaryProductPred::And(left, right) | NaryProductPred::Or(left, right) => {
                        write!(
                            f,
                            "{}(",
                            if matches!(predicate, NaryProductPred::And(_, _)) {
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
                    NaryProductPred::Not(inner) => {
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

impl<P> Drop for NaryProductPred<P> {
    fn drop(&mut self) {
        fn drain<P>(predicate: &mut NaryProductPred<P>, pending: &mut Vec<NaryProductPred<P>>) {
            match predicate {
                NaryProductPred::And(left, right) | NaryProductPred::Or(left, right) => {
                    pending.push(std::mem::replace(&mut **right, NaryProductPred::True));
                    pending.push(std::mem::replace(&mut **left, NaryProductPred::True));
                }
                NaryProductPred::Not(inner) => {
                    pending.push(std::mem::replace(&mut **inner, NaryProductPred::True));
                }
                NaryProductPred::True | NaryProductPred::False | NaryProductPred::Field(_, _) => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut predicate) = pending.pop() {
            drain(&mut predicate, &mut pending);
        }
    }
}

/// The effective Boolean algebra of fixed-arity tuples with independent fields.
#[derive(Clone, Debug)]
pub struct NaryProductAlgebra<A: BooleanAlgebra> {
    /// One algebra per tuple position; `fields.len()` is the arity.
    pub fields: Vec<A>,
}

impl<A: BooleanAlgebra> NaryProductAlgebra<A> {
    /// Construct an algebra over tuples of the given field algebras.
    pub fn new(fields: Vec<A>) -> Self {
        NaryProductAlgebra { fields }
    }

    /// The tuple arity.
    pub fn arity(&self) -> usize {
        self.fields.len()
    }

    /// Negation-normal form: push `Not` down to the field leaves using each
    /// field algebra's `not`. Out-of-range field indices are treated as the
    /// unsatisfiable atom (so a positive occurrence is `False`, a negated one is
    /// `True`).
    fn nnf(
        &self,
        p: &NaryProductPred<A::Predicate>,
        negate: bool,
    ) -> NaryProductPred<A::Predicate> {
        use NaryProductPred::*;
        enum Task<'a, P> {
            Visit(&'a NaryProductPred<P>, bool),
            Binary { conjunction: bool },
        }
        let mut tasks = vec![Task::Visit(p, negate)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Visit(predicate, polarity) => match predicate {
                    True => values.push(if polarity { False } else { True }),
                    False => values.push(if polarity { True } else { False }),
                    Field(index, field_predicate) => {
                        values.push(if *index >= self.fields.len() {
                            if polarity {
                                True
                            } else {
                                False
                            }
                        } else if polarity {
                            Field(*index, self.fields[*index].not(field_predicate))
                        } else {
                            Field(*index, field_predicate.clone())
                        });
                    }
                    And(left, right) => {
                        tasks.push(Task::Binary {
                            conjunction: !polarity,
                        });
                        tasks.push(Task::Visit(right, polarity));
                        tasks.push(Task::Visit(left, polarity));
                    }
                    Or(left, right) => {
                        tasks.push(Task::Binary {
                            conjunction: polarity,
                        });
                        tasks.push(Task::Visit(right, polarity));
                        tasks.push(Task::Visit(left, polarity));
                    }
                    Not(inner) => tasks.push(Task::Visit(inner, !polarity)),
                },
                Task::Binary { conjunction } => {
                    let right = values.pop().expect("right NNF operand is present");
                    let left = values.pop().expect("left NNF operand is present");
                    values.push(if conjunction {
                        And(Box::new(left), Box::new(right))
                    } else {
                        Or(Box::new(left), Box::new(right))
                    });
                }
            }
        }
        values.pop().expect("the root predicate produces one NNF")
    }

    /// Disjunctive normal form over a `Not`-free predicate: a list of disjuncts,
    /// each a list of `(field, predicate)` atoms.
    fn to_dnf(&self, p: &NaryProductPred<A::Predicate>) -> Vec<Vec<(usize, A::Predicate)>> {
        use NaryProductPred::*;
        enum Task<'a, P> {
            Convert(&'a NaryProductPred<P>),
            And,
            Or,
        }
        let mut tasks = vec![Task::Convert(p)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Convert(predicate) => match predicate {
                    True => values.push(vec![Vec::new()]),
                    False => values.push(Vec::new()),
                    Field(index, field_predicate) => {
                        values.push(vec![vec![(*index, field_predicate.clone())]]);
                    }
                    Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Convert(right));
                        tasks.push(Task::Convert(left));
                    }
                    And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Convert(right));
                        tasks.push(Task::Convert(left));
                    }
                    Not(_) => unreachable!("to_dnf expects NNF (no Not)"),
                },
                Task::Or => {
                    let right = values.pop().expect("right DNF is present");
                    let mut left = values.pop().expect("left DNF is present");
                    left.extend(right);
                    values.push(left);
                }
                Task::And => {
                    let right = values.pop().expect("right DNF is present");
                    let left = values.pop().expect("left DNF is present");
                    let mut output = Vec::with_capacity(left.len().saturating_mul(right.len()));
                    for left_conjunction in &left {
                        for right_conjunction in &right {
                            let mut conjunction = Vec::with_capacity(
                                left_conjunction.len() + right_conjunction.len(),
                            );
                            conjunction.extend(left_conjunction.iter().cloned());
                            conjunction.extend(right_conjunction.iter().cloned());
                            output.push(conjunction);
                        }
                    }
                    values.push(output);
                }
            }
        }
        values.pop().expect("the root predicate produces one DNF")
    }

    /// Collapse a disjunct's atoms into a per-field conjoined predicate
    /// (`None` for unconstrained fields). Returns `None` if any field is
    /// unsatisfiable (so the whole disjunct is unsatisfiable).
    fn field_constraints(
        &self,
        disjunct: &[(usize, A::Predicate)],
    ) -> Option<Vec<Option<A::Predicate>>> {
        let mut acc: Vec<Option<A::Predicate>> = vec![None; self.fields.len()];
        for (i, pi) in disjunct {
            if *i >= self.fields.len() {
                return None; // out-of-range atom never holds
            }
            acc[*i] = Some(match acc[*i].take() {
                Some(prev) => self.fields[*i].and(&prev, pi),
                None => pi.clone(),
            });
        }
        Some(acc)
    }
}

impl<A: BooleanAlgebra> BooleanAlgebra for NaryProductAlgebra<A> {
    type Predicate = NaryProductPred<A::Predicate>;
    type Domain = Vec<A::Domain>;

    fn true_pred(&self) -> Self::Predicate {
        NaryProductPred::True
    }

    fn false_pred(&self) -> Self::Predicate {
        NaryProductPred::False
    }

    fn and(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        match (a, b) {
            (NaryProductPred::False, _) | (_, NaryProductPred::False) => NaryProductPred::False,
            (NaryProductPred::True, x) | (x, NaryProductPred::True) => x.clone(),
            _ => NaryProductPred::And(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn or(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        match (a, b) {
            (NaryProductPred::True, _) | (_, NaryProductPred::True) => NaryProductPred::True,
            (NaryProductPred::False, x) | (x, NaryProductPred::False) => x.clone(),
            _ => NaryProductPred::Or(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn not(&self, a: &Self::Predicate) -> Self::Predicate {
        NaryProductPred::Not(Box::new(a.clone()))
    }

    fn is_satisfiable(&self, a: &Self::Predicate) -> bool {
        let nnf = self.nnf(a, false);
        for disjunct in self.to_dnf(&nnf) {
            if let Some(constraints) = self.field_constraints(&disjunct) {
                let all_sat = constraints.iter().enumerate().all(|(i, c)| match c {
                    Some(pred) => self.fields[i].is_satisfiable(pred),
                    None => true, // unconstrained field — satisfiable if its domain is nonempty
                });
                // An unconstrained field needs a witness of its universe; if the
                // field's domain is empty the tuple is unsatisfiable. Check via
                // true_pred satisfiability.
                let universe_ok = constraints.iter().enumerate().all(|(i, c)| {
                    c.is_some() || self.fields[i].is_satisfiable(&self.fields[i].true_pred())
                });
                if all_sat && universe_ok {
                    return true;
                }
            }
        }
        false
    }

    fn witness(&self, a: &Self::Predicate) -> Option<Self::Domain> {
        let nnf = self.nnf(a, false);
        for disjunct in self.to_dnf(&nnf) {
            let Some(constraints) = self.field_constraints(&disjunct) else {
                continue;
            };
            let mut tuple = Vec::with_capacity(self.fields.len());
            let mut ok = true;
            for (i, c) in constraints.iter().enumerate() {
                let pred = match c {
                    Some(pred) => pred.clone(),
                    None => self.fields[i].true_pred(),
                };
                match self.fields[i].witness(&pred) {
                    Some(v) => tuple.push(v),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                return Some(tuple);
            }
        }
        None
    }

    fn evaluate(&self, pred: &Self::Predicate, elem: &Self::Domain) -> bool {
        enum Frame<'a, P> {
            Eval(&'a NaryProductPred<P>),
            Not,
            AndRight(&'a NaryProductPred<P>),
            OrRight(&'a NaryProductPred<P>),
        }
        let mut frames = vec![Frame::Eval(pred)];
        let mut result = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Eval(predicate) => match predicate {
                    NaryProductPred::True => result = Some(true),
                    NaryProductPred::False => result = Some(false),
                    NaryProductPred::Field(index, field_predicate) => {
                        result = Some(match (self.fields.get(*index), elem.get(*index)) {
                            (Some(field), Some(value)) => field.evaluate(field_predicate, value),
                            _ => false,
                        });
                    }
                    NaryProductPred::And(left, right) => {
                        frames.push(Frame::AndRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    NaryProductPred::Or(left, right) => {
                        frames.push(Frame::OrRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    NaryProductPred::Not(inner) => {
                        frames.push(Frame::Not);
                        frames.push(Frame::Eval(inner));
                    }
                },
                Frame::Not => result = Some(!result.take().expect("negated product is evaluated")),
                Frame::AndRight(right) => {
                    if result
                        .take()
                        .expect("left product conjunction is evaluated")
                    {
                        frames.push(Frame::Eval(right));
                    } else {
                        result = Some(false);
                    }
                }
                Frame::OrRight(right) => {
                    if result
                        .take()
                        .expect("left product disjunction is evaluated")
                    {
                        result = Some(true);
                    } else {
                        frames.push(Frame::Eval(right));
                    }
                }
            }
        }
        result.expect("the root product predicate produces one result")
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Sum (coproduct / variants)
// ══════════════════════════════════════════════════════════════════════════════

/// A tagged value: variant `tag`, carrying `payload` of variant `tag`'s domain.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SumValue<D> {
    /// Which variant.
    pub tag: usize,
    /// The variant's payload.
    pub payload: D,
}

/// A predicate over a tagged value.
pub enum SumPred<P> {
    /// Satisfied by every value.
    True,
    /// Satisfied by no value.
    False,
    /// `tag == i` and the payload satisfies the inner predicate.
    InVariant(usize, P),
    /// `tag == i` (payload unconstrained).
    TagIs(usize),
    /// Conjunction.
    And(Box<SumPred<P>>, Box<SumPred<P>>),
    /// Disjunction.
    Or(Box<SumPred<P>>, Box<SumPred<P>>),
    /// Negation.
    Not(Box<SumPred<P>>),
}

impl<P: Clone> Clone for SumPred<P> {
    fn clone(&self) -> Self {
        enum Task<'a, P> {
            Clone(&'a SumPred<P>),
            And,
            Or,
            Not,
        }
        let mut tasks = vec![Task::Clone(self)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Clone(predicate) => match predicate {
                    SumPred::True => values.push(SumPred::True),
                    SumPred::False => values.push(SumPred::False),
                    SumPred::InVariant(tag, inner) => {
                        values.push(SumPred::InVariant(*tag, inner.clone()));
                    }
                    SumPred::TagIs(tag) => values.push(SumPred::TagIs(*tag)),
                    SumPred::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    SumPred::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Clone(right));
                        tasks.push(Task::Clone(left));
                    }
                    SumPred::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Clone(inner));
                    }
                },
                Task::And | Task::Or => {
                    let right = values.pop().expect("right sum clone is present");
                    let left = values.pop().expect("left sum clone is present");
                    values.push(if matches!(task, Task::And) {
                        SumPred::And(Box::new(left), Box::new(right))
                    } else {
                        SumPred::Or(Box::new(left), Box::new(right))
                    });
                }
                Task::Not => {
                    let inner = values.pop().expect("negated sum clone is present");
                    values.push(SumPred::Not(Box::new(inner)));
                }
            }
        }
        values
            .pop()
            .expect("the root sum predicate produces one clone")
    }
}

impl<P: PartialEq> PartialEq for SumPred<P> {
    fn eq(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (SumPred::True, SumPred::True) | (SumPred::False, SumPred::False) => {}
                (SumPred::InVariant(lt, lp), SumPred::InVariant(rt, rp))
                    if lt == rt && lp == rp => {}
                (SumPred::TagIs(left), SumPred::TagIs(right)) if left == right => {}
                (SumPred::And(ll, lr), SumPred::And(rl, rr))
                | (SumPred::Or(ll, lr), SumPred::Or(rl, rr)) => {
                    pending.push((lr, rr));
                    pending.push((ll, rl));
                }
                (SumPred::Not(left), SumPred::Not(right)) => pending.push((left, right)),
                _ => return false,
            }
        }
        true
    }
}

impl<P: Eq> Eq for SumPred<P> {}

impl<P: Hash> Hash for SumPred<P> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut pending = vec![self];
        while let Some(predicate) = pending.pop() {
            std::mem::discriminant(predicate).hash(state);
            match predicate {
                SumPred::True | SumPred::False => {}
                SumPred::InVariant(tag, inner) => {
                    tag.hash(state);
                    inner.hash(state);
                }
                SumPred::TagIs(tag) => tag.hash(state),
                SumPred::And(left, right) | SumPred::Or(left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                SumPred::Not(inner) => pending.push(inner),
            }
        }
    }
}

impl<P: fmt::Debug> fmt::Debug for SumPred<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Event<'a, P> {
            Pred(&'a SumPred<P>),
            Text(&'static str),
        }
        let mut events = vec![Event::Pred(self)];
        while let Some(event) = events.pop() {
            match event {
                Event::Text(text) => write!(f, "{text}")?,
                Event::Pred(predicate) => match predicate {
                    SumPred::True => write!(f, "True")?,
                    SumPred::False => write!(f, "False")?,
                    SumPred::InVariant(tag, inner) => {
                        write!(f, "InVariant({tag:?}, {inner:?})")?;
                    }
                    SumPred::TagIs(tag) => write!(f, "TagIs({tag:?})")?,
                    SumPred::And(left, right) | SumPred::Or(left, right) => {
                        write!(
                            f,
                            "{}(",
                            if matches!(predicate, SumPred::And(_, _)) {
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
                    SumPred::Not(inner) => {
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

impl<P> Drop for SumPred<P> {
    fn drop(&mut self) {
        fn drain<P>(predicate: &mut SumPred<P>, pending: &mut Vec<SumPred<P>>) {
            match predicate {
                SumPred::And(left, right) | SumPred::Or(left, right) => {
                    pending.push(std::mem::replace(&mut **right, SumPred::True));
                    pending.push(std::mem::replace(&mut **left, SumPred::True));
                }
                SumPred::Not(inner) => {
                    pending.push(std::mem::replace(&mut **inner, SumPred::True));
                }
                SumPred::True | SumPred::False | SumPred::InVariant(_, _) | SumPred::TagIs(_) => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut predicate) = pending.pop() {
            drain(&mut predicate, &mut pending);
        }
    }
}

/// The effective Boolean algebra of tagged unions.
#[derive(Clone, Debug)]
pub struct SumAlgebra<A: BooleanAlgebra> {
    /// One algebra per variant; `variants.len()` is the number of tags.
    pub variants: Vec<A>,
}

impl<A: BooleanAlgebra> SumAlgebra<A> {
    /// Construct an algebra over a tagged union of the given variant algebras.
    pub fn new(variants: Vec<A>) -> Self {
        SumAlgebra { variants }
    }

    /// The number of variants.
    pub fn num_variants(&self) -> usize {
        self.variants.len()
    }

    /// Project a predicate onto variant `tag`, yielding an inner predicate for
    /// `variants[tag]`. (Mirrors the per-sort fold of the many-sorted carrier.)
    fn project(&self, p: &SumPred<A::Predicate>, tag: usize) -> A::Predicate {
        let alg = &self.variants[tag];
        enum Task<'a, P> {
            Project(&'a SumPred<P>),
            And,
            Or,
            Not,
        }
        let mut tasks = vec![Task::Project(p)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Project(predicate) => match predicate {
                    SumPred::True => values.push(alg.true_pred()),
                    SumPred::False => values.push(alg.false_pred()),
                    SumPred::InVariant(index, inner) => values.push(if *index == tag {
                        inner.clone()
                    } else {
                        alg.false_pred()
                    }),
                    SumPred::TagIs(index) => values.push(if *index == tag {
                        alg.true_pred()
                    } else {
                        alg.false_pred()
                    }),
                    SumPred::And(left, right) => {
                        tasks.push(Task::And);
                        tasks.push(Task::Project(right));
                        tasks.push(Task::Project(left));
                    }
                    SumPred::Or(left, right) => {
                        tasks.push(Task::Or);
                        tasks.push(Task::Project(right));
                        tasks.push(Task::Project(left));
                    }
                    SumPred::Not(inner) => {
                        tasks.push(Task::Not);
                        tasks.push(Task::Project(inner));
                    }
                },
                Task::And | Task::Or => {
                    let right = values.pop().expect("right sum projection is present");
                    let left = values.pop().expect("left sum projection is present");
                    values.push(if matches!(task, Task::And) {
                        alg.and(&left, &right)
                    } else {
                        alg.or(&left, &right)
                    });
                }
                Task::Not => {
                    let inner = values.pop().expect("negated sum projection is present");
                    values.push(alg.not(&inner));
                }
            }
        }
        values
            .pop()
            .expect("the root sum projection produces one predicate")
    }
}

impl<A: BooleanAlgebra> BooleanAlgebra for SumAlgebra<A> {
    type Predicate = SumPred<A::Predicate>;
    type Domain = SumValue<A::Domain>;

    fn true_pred(&self) -> Self::Predicate {
        SumPred::True
    }

    fn false_pred(&self) -> Self::Predicate {
        SumPred::False
    }

    fn and(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        match (a, b) {
            (SumPred::False, _) | (_, SumPred::False) => SumPred::False,
            (SumPred::True, x) | (x, SumPred::True) => x.clone(),
            _ => SumPred::And(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn or(&self, a: &Self::Predicate, b: &Self::Predicate) -> Self::Predicate {
        match (a, b) {
            (SumPred::True, _) | (_, SumPred::True) => SumPred::True,
            (SumPred::False, x) | (x, SumPred::False) => x.clone(),
            _ => SumPred::Or(Box::new(a.clone()), Box::new(b.clone())),
        }
    }

    fn not(&self, a: &Self::Predicate) -> Self::Predicate {
        SumPred::Not(Box::new(a.clone()))
    }

    fn is_satisfiable(&self, a: &Self::Predicate) -> bool {
        (0..self.variants.len()).any(|tag| {
            let projected = self.project(a, tag);
            self.variants[tag].is_satisfiable(&projected)
        })
    }

    fn witness(&self, a: &Self::Predicate) -> Option<Self::Domain> {
        for tag in 0..self.variants.len() {
            let projected = self.project(a, tag);
            if let Some(payload) = self.variants[tag].witness(&projected) {
                return Some(SumValue { tag, payload });
            }
        }
        None
    }

    fn evaluate(&self, pred: &Self::Predicate, elem: &Self::Domain) -> bool {
        enum Frame<'a, P> {
            Eval(&'a SumPred<P>),
            Not,
            AndRight(&'a SumPred<P>),
            OrRight(&'a SumPred<P>),
        }
        let mut frames = vec![Frame::Eval(pred)];
        let mut result = None;
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Eval(predicate) => match predicate {
                    SumPred::True => result = Some(true),
                    SumPred::False => result = Some(false),
                    SumPred::InVariant(index, inner) => {
                        result = Some(
                            *index == elem.tag
                                && self
                                    .variants
                                    .get(elem.tag)
                                    .is_some_and(|alg| alg.evaluate(inner, &elem.payload)),
                        );
                    }
                    SumPred::TagIs(index) => result = Some(*index == elem.tag),
                    SumPred::And(left, right) => {
                        frames.push(Frame::AndRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    SumPred::Or(left, right) => {
                        frames.push(Frame::OrRight(right));
                        frames.push(Frame::Eval(left));
                    }
                    SumPred::Not(inner) => {
                        frames.push(Frame::Not);
                        frames.push(Frame::Eval(inner));
                    }
                },
                Frame::Not => result = Some(!result.take().expect("negated sum is evaluated")),
                Frame::AndRight(right) => {
                    if result.take().expect("left sum conjunction is evaluated") {
                        frames.push(Frame::Eval(right));
                    } else {
                        result = Some(false);
                    }
                }
                Frame::OrRight(right) => {
                    if result.take().expect("left sum disjunction is evaluated") {
                        result = Some(true);
                    } else {
                        frames.push(Frame::Eval(right));
                    }
                }
            }
        }
        result.expect("the root sum predicate produces one result")
    }
}

#[cfg(test)]
mod tests {
    use super::super::{IntervalAlgebra, IntervalPred};
    use super::*;

    fn field(lo: i64, hi: i64) -> NaryProductPred<IntervalPred> {
        NaryProductPred::Field(0, IntervalPred::Range(lo, hi))
    }

    #[test]
    fn product_independent_fields() {
        let alg = NaryProductAlgebra::new(vec![
            IntervalAlgebra::new(0, 100),
            IntervalAlgebra::new(0, 100),
        ]);
        // component 0 in [10,50) AND component 1 in [30,70)
        let p = alg.and(
            &NaryProductPred::Field(0, IntervalPred::Range(10, 50)),
            &NaryProductPred::Field(1, IntervalPred::Range(30, 70)),
        );
        assert!(alg.is_satisfiable(&p));
        assert!(alg.evaluate(&p, &vec![20, 40]));
        assert!(!alg.evaluate(&p, &vec![20, 10])); // field 1 fails
        assert!(!alg.evaluate(&p, &vec![5, 40])); // field 0 fails
        let w = alg.witness(&p).expect("nonempty");
        assert!(alg.evaluate(&p, &w));
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn product_negation_distributes_into_fields() {
        let alg = NaryProductAlgebra::new(vec![IntervalAlgebra::new(0, 100)]);
        let p = field(10, 20);
        let np = alg.not(&p);
        assert!(!alg.evaluate(&np, &vec![15]));
        assert!(alg.evaluate(&np, &vec![5]));
        assert!(alg.evaluate(&np, &vec![25]));
        // p ∧ ¬p unsat
        assert!(!alg.is_satisfiable(&alg.and(&p, &np)));
    }

    #[test]
    fn product_arity_mismatch_rejected() {
        let alg = NaryProductAlgebra::new(vec![
            IntervalAlgebra::new(0, 100),
            IntervalAlgebra::new(0, 100),
        ]);
        // A tuple shorter than a referenced field position is not satisfied.
        let p = NaryProductPred::Field(1, IntervalPred::True);
        assert!(alg.evaluate(&p, &vec![5, 7])); // component 1 present
        assert!(!alg.evaluate(&p, &vec![5])); // no component 1 → false
                                              // out-of-range field reference is never satisfied
        let oob = NaryProductPred::Field(5, IntervalPred::True);
        assert!(!alg.is_satisfiable(&oob));
        assert!(!alg.evaluate(&oob, &vec![1, 2]));
    }

    #[test]
    fn sum_per_variant_projection() {
        let alg = SumAlgebra::new(vec![
            IntervalAlgebra::new(0, 100),
            IntervalAlgebra::new(0, 100),
        ]);
        // variant 0 with payload in [10,20), OR variant 1 (any payload)
        let p = alg.or(
            &SumPred::InVariant(0, IntervalPred::Range(10, 20)),
            &SumPred::TagIs(1),
        );
        assert!(alg.is_satisfiable(&p));
        assert!(alg.evaluate(
            &p,
            &SumValue {
                tag: 0,
                payload: 15
            }
        ));
        assert!(!alg.evaluate(
            &p,
            &SumValue {
                tag: 0,
                payload: 25
            }
        ));
        assert!(alg.evaluate(
            &p,
            &SumValue {
                tag: 1,
                payload: 99
            }
        ));
        let w = alg.witness(&p).expect("nonempty");
        assert!(alg.evaluate(&p, &w));
    }

    #[test]
    fn sum_unsatisfiable_variant() {
        let alg = SumAlgebra::new(vec![IntervalAlgebra::new(0, 100)]);
        // variant 0 payload in empty range → unsat
        let p = SumPred::InVariant(0, IntervalPred::Range(50, 50));
        assert!(!alg.is_satisfiable(&p));
        // reference to a nonexistent tag → unsat
        let p2 = SumPred::TagIs(7);
        assert!(!alg.is_satisfiable(&p2));
    }

    #[test]
    fn sum_negation() {
        let alg = SumAlgebra::new(vec![
            IntervalAlgebra::new(0, 100),
            IntervalAlgebra::new(0, 100),
        ]);
        let tag0 = SumPred::TagIs(0);
        let not_tag0 = alg.not(&tag0);
        // not-tag0 is satisfiable (variant 1 witnesses it).
        assert!(alg.is_satisfiable(&not_tag0));
        assert!(alg.evaluate(&not_tag0, &SumValue { tag: 1, payload: 5 }));
        assert!(!alg.evaluate(&not_tag0, &SumValue { tag: 0, payload: 5 }));
        // tag0 ∧ ¬tag0 unsat
        assert!(!alg.is_satisfiable(&alg.and(&tag0, &not_tag0)));
    }
}
