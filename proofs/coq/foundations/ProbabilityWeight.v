(** * Probability Semiring

    The probability semiring is (ℝ≥0, +, ×, 0, 1): weights are non-negative
    reals combined by ordinary addition and multiplication. It is the
    exact-real model corresponding to Rust [ProbabilityWeight]
    (src/semiring/basic/probability.rs), which stores an `f64` clamped to
    [0, ∞).

    The semiring laws are the ordinary real-arithmetic laws, discharged by
    [ring]/[lra]; the defining feature of THIS semiring versus the real ring
    is the non-negativity of the carrier, so this file additionally proves
    that ℝ≥0 is CLOSED under ⊕ and ⊗ and contains 0̄ and 1̄ — i.e. the
    non-negative reals form a genuine sub-semiring, which is what makes
    probability weights well-defined. The f64 clamp is the ABI weight
    bridge's concern (obligation #16); this file never assumes IEEE-754
    associativity — it reasons over exact ℝ.

    Registry: proofs/doc/abi-invariants.tsv, LLING-SEMI-1.
*)

Require Import Coq.Reals.Reals.
Require Import Coq.micromega.Lra.
Require Import Coq.Classes.RelationClasses.
Require Import Coq.Classes.Morphisms.
Require Import Coq.setoid_ring.Ring.
Require Import LlingLlang.foundations.Semiring.

Open Scope R_scope.

(** The exact probability weight: a real number (the non-negativity invariant
    is a property, proved closed below, not a carrier restriction — mirroring
    how the Rust type is an f64 with a clamp). Equality is Leibniz on R. *)
Notation prob := R.

Definition prob_eq (a b : prob) : Prop := a = b.

#[global] Instance prob_eq_Equivalence : Equivalence prob_eq.
Proof. unfold prob_eq; split; congruence. Qed.

Definition prob_plus (a b : prob) : prob := Rplus a b.
Definition prob_times (a b : prob) : prob := Rmult a b.
Definition prob_zero : prob := 0.
Definition prob_one : prob := 1.

#[global] Instance prob_plus_proper :
  Proper (prob_eq ==> prob_eq ==> prob_eq) prob_plus.
Proof. unfold prob_eq, Proper, respectful; intros; subst; reflexivity. Qed.

#[global] Instance prob_times_proper :
  Proper (prob_eq ==> prob_eq ==> prob_eq) prob_times.
Proof. unfold prob_eq, Proper, respectful; intros; subst; reflexivity. Qed.

(** ** Semiring laws (ordinary real arithmetic) *)

Lemma prob_plus_assoc : forall a b c : prob,
  prob_eq (prob_plus (prob_plus a b) c) (prob_plus a (prob_plus b c)).
Proof. unfold prob_eq, prob_plus; intros; ring. Qed.

Lemma prob_plus_comm : forall a b : prob,
  prob_eq (prob_plus a b) (prob_plus b a).
Proof. unfold prob_eq, prob_plus; intros; ring. Qed.

Lemma prob_plus_zero_l : forall a : prob,
  prob_eq (prob_plus prob_zero a) a.
Proof. unfold prob_eq, prob_plus, prob_zero; intros; ring. Qed.

Lemma prob_times_assoc : forall a b c : prob,
  prob_eq (prob_times (prob_times a b) c) (prob_times a (prob_times b c)).
Proof. unfold prob_eq, prob_times; intros; ring. Qed.

Lemma prob_times_one_l : forall a : prob,
  prob_eq (prob_times prob_one a) a.
Proof. unfold prob_eq, prob_times, prob_one; intros; ring. Qed.

Lemma prob_times_one_r : forall a : prob,
  prob_eq (prob_times a prob_one) a.
Proof. unfold prob_eq, prob_times, prob_one; intros; ring. Qed.

Lemma prob_distr_l : forall a b c : prob,
  prob_eq (prob_times a (prob_plus b c))
          (prob_plus (prob_times a b) (prob_times a c)).
Proof. unfold prob_eq, prob_times, prob_plus; intros; ring. Qed.

Lemma prob_distr_r : forall a b c : prob,
  prob_eq (prob_times (prob_plus a b) c)
          (prob_plus (prob_times a c) (prob_times b c)).
Proof. unfold prob_eq, prob_times, prob_plus; intros; ring. Qed.

Lemma prob_zero_times_l : forall a : prob,
  prob_eq (prob_times prob_zero a) prob_zero.
Proof. unfold prob_eq, prob_times, prob_zero; intros; ring. Qed.

Lemma prob_zero_times_r : forall a : prob,
  prob_eq (prob_times a prob_zero) prob_zero.
Proof. unfold prob_eq, prob_times, prob_zero; intros; ring. Qed.

#[global] Instance ProbabilitySemiring : Semiring prob := {
  sr_eq := prob_eq;
  sr_eq_equiv := prob_eq_Equivalence;
  sr_plus := prob_plus;
  sr_plus_proper := prob_plus_proper;
  sr_times := prob_times;
  sr_times_proper := prob_times_proper;
  sr_zero := prob_zero;
  sr_one := prob_one;
  sr_plus_assoc := prob_plus_assoc;
  sr_plus_comm := prob_plus_comm;
  sr_plus_zero_l := prob_plus_zero_l;
  sr_times_assoc := prob_times_assoc;
  sr_times_one_l := prob_times_one_l;
  sr_times_one_r := prob_times_one_r;
  sr_distr_l := prob_distr_l;
  sr_distr_r := prob_distr_r;
  sr_zero_times_l := prob_zero_times_l;
  sr_zero_times_r := prob_zero_times_r
}.

(** ** Non-negativity: ℝ≥0 is a sub-semiring *)

Definition prob_nonneg (a : prob) : Prop := 0 <= a.

Theorem prob_zero_nonneg : prob_nonneg prob_zero.
Proof. unfold prob_nonneg, prob_zero; lra. Qed.

Theorem prob_one_nonneg : prob_nonneg prob_one.
Proof. unfold prob_nonneg, prob_one; lra. Qed.

Theorem prob_plus_nonneg_closed : forall a b : prob,
  prob_nonneg a -> prob_nonneg b -> prob_nonneg (prob_plus a b).
Proof. unfold prob_nonneg, prob_plus; intros; lra. Qed.

Theorem prob_times_nonneg_closed : forall a b : prob,
  prob_nonneg a -> prob_nonneg b -> prob_nonneg (prob_times a b).
Proof.
  unfold prob_nonneg, prob_times; intros a b Ha Hb.
  now apply Rmult_le_pos.
Qed.
