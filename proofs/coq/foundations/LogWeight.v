(** * Log-Weight Algebra

    Rust's [LogWeight] stores negative logarithms for numerical stability.
    The algebraic semiring laws are simplest to prove in the corresponding
    probability-mass domain, where:

    - [⊕] is probability addition,
    - [⊗] is probability multiplication,
    - [0̄] is zero probability,
    - [1̄] is probability one.

    The negative-log implementation is a representation of this algebra on
    positive finite probabilities; this file proves the algebraic laws directly
    for the exact real-valued mass model and does not assert floating-point or
    logarithm identities as unproved assumptions.
*)

Require Import Coq.Reals.Reals.
Require Import Coq.Classes.Morphisms.
Require Import Coq.Classes.RelationClasses.
Require Import Coq.setoid_ring.Ring.
Require Import LlingLlang.foundations.Semiring.

Open Scope R_scope.

(** ** Log Weight Type *)

Inductive log_weight : Type :=
  | Log_mass : R -> log_weight.

Definition log_value (w : log_weight) : R :=
  match w with
  | Log_mass x => x
  end.

Definition log_eq (a b : log_weight) : Prop :=
  log_value a = log_value b.

Lemma log_eq_refl : forall a : log_weight, log_eq a a.
Proof.
  intro a. destruct a. reflexivity.
Qed.

Lemma log_eq_sym : forall a b : log_weight, log_eq a b -> log_eq b a.
Proof.
  intros a b H. unfold log_eq in *. symmetry. exact H.
Qed.

Lemma log_eq_trans : forall a b c : log_weight,
  log_eq a b -> log_eq b c -> log_eq a c.
Proof.
  intros a b c Hab Hbc. unfold log_eq in *. transitivity (log_value b); auto.
Qed.

#[global]
Instance log_eq_Equivalence : Equivalence log_eq := {
  Equivalence_Reflexive := log_eq_refl;
  Equivalence_Symmetric := log_eq_sym;
  Equivalence_Transitive := log_eq_trans
}.

(** ** Operations *)

Definition log_plus (a b : log_weight) : log_weight :=
  Log_mass (log_value a + log_value b).

Definition log_times (a b : log_weight) : log_weight :=
  Log_mass (log_value a * log_value b).

Definition log_zero : log_weight := Log_mass 0.
Definition log_one : log_weight := Log_mass 1.

#[global]
Instance log_plus_Proper :
  Proper (log_eq ==> log_eq ==> log_eq) log_plus.
Proof.
  unfold Proper, respectful, log_eq, log_plus, log_value.
  intros [a1] [a2] Ha [b1] [b2] Hb.
  simpl in *. subst. reflexivity.
Qed.

#[global]
Instance log_times_Proper :
  Proper (log_eq ==> log_eq ==> log_eq) log_times.
Proof.
  unfold Proper, respectful, log_eq, log_times, log_value.
  intros [a1] [a2] Ha [b1] [b2] Hb.
  simpl in *. subst. reflexivity.
Qed.

(** ** Semiring Laws *)

Lemma log_plus_assoc : forall a b c : log_weight,
  log_eq (log_plus (log_plus a b) c) (log_plus a (log_plus b c)).
Proof.
  intros [a] [b] [c]. unfold log_eq, log_plus, log_value. simpl. ring.
Qed.

Lemma log_plus_comm : forall a b : log_weight,
  log_eq (log_plus a b) (log_plus b a).
Proof.
  intros [a] [b]. unfold log_eq, log_plus, log_value. simpl. ring.
Qed.

Lemma log_plus_zero_l : forall a : log_weight,
  log_eq (log_plus log_zero a) a.
Proof.
  intros [a]. unfold log_eq, log_plus, log_zero, log_value. simpl. ring.
Qed.

Lemma log_times_assoc : forall a b c : log_weight,
  log_eq (log_times (log_times a b) c) (log_times a (log_times b c)).
Proof.
  intros [a] [b] [c]. unfold log_eq, log_times, log_value. simpl. ring.
Qed.

Lemma log_times_one_l : forall a : log_weight,
  log_eq (log_times log_one a) a.
Proof.
  intros [a]. unfold log_eq, log_times, log_one, log_value. simpl. ring.
Qed.

Lemma log_times_one_r : forall a : log_weight,
  log_eq (log_times a log_one) a.
Proof.
  intros [a]. unfold log_eq, log_times, log_one, log_value. simpl. ring.
Qed.

Lemma log_distr_l : forall a b c : log_weight,
  log_eq (log_times a (log_plus b c))
         (log_plus (log_times a b) (log_times a c)).
Proof.
  intros [a] [b] [c].
  unfold log_eq, log_times, log_plus, log_value. simpl. ring.
Qed.

Lemma log_distr_r : forall a b c : log_weight,
  log_eq (log_times (log_plus a b) c)
         (log_plus (log_times a c) (log_times b c)).
Proof.
  intros [a] [b] [c].
  unfold log_eq, log_times, log_plus, log_value. simpl. ring.
Qed.

Lemma log_zero_times_l : forall a : log_weight,
  log_eq (log_times log_zero a) log_zero.
Proof.
  intros [a]. unfold log_eq, log_times, log_zero, log_value. simpl. ring.
Qed.

Lemma log_zero_times_r : forall a : log_weight,
  log_eq (log_times a log_zero) log_zero.
Proof.
  intros [a]. unfold log_eq, log_times, log_zero, log_value. simpl. ring.
Qed.

#[global]
Instance LogSemiring : Semiring log_weight := {
  sr_eq := log_eq;
  sr_eq_equiv := log_eq_Equivalence;
  sr_plus := log_plus;
  sr_plus_proper := log_plus_Proper;
  sr_times := log_times;
  sr_times_proper := log_times_Proper;
  sr_zero := log_zero;
  sr_one := log_one;
  sr_plus_assoc := log_plus_assoc;
  sr_plus_comm := log_plus_comm;
  sr_plus_zero_l := log_plus_zero_l;
  sr_times_assoc := log_times_assoc;
  sr_times_one_l := log_times_one_l;
  sr_times_one_r := log_times_one_r;
  sr_distr_l := log_distr_l;
  sr_distr_r := log_distr_r;
  sr_zero_times_l := log_zero_times_l;
  sr_zero_times_r := log_zero_times_r;
}.

(** ** Additional Proven Structure *)

Lemma log_times_comm : forall a b : log_weight,
  log_eq (log_times a b) (log_times b a).
Proof.
  intros [a] [b]. unfold log_eq, log_times, log_value. simpl. ring.
Qed.

#[global]
Instance LogCommutative : CommutativeTimesSemiring log_weight := {
  sr_times_comm := log_times_comm
}.
