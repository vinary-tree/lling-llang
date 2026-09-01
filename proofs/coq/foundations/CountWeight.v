(** * Counting Semiring

    The counting (or "counting-paths") semiring is (ℕ, +, ×, 0, 1): a weight
    is a non-negative integer count of paths or derivations, added and
    multiplied as ordinary naturals. It is the exact-integer model
    corresponding to Rust [CountWeight] (src/semiring/basic/count.rs), which
    stores a `u64` and uses SATURATING arithmetic to avoid overflow panics.

    This file proves the semiring laws over the exact carrier ℕ (Coq [nat]),
    where every law is an ordinary arithmetic identity. The Rust
    representation coincides with this model exactly while unsaturated; the
    saturation boundary (a count reaching `u64::MAX`) is the concern of the
    ABI weight bridge (obligation #16, proofs/coq/abi/WeightBridge.v), not of
    the algebra — the counting semiring itself is over unbounded ℕ.

    Registry: proofs/doc/abi-invariants.tsv, LLING-SEMI-3.
*)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Classes.Morphisms.
From Stdlib Require Import Classes.RelationClasses.
From Stdlib Require Import Setoids.Setoid.
Require Import LlingLlang.foundations.Semiring.

(** The exact counting weight: a natural number. Equality is Leibniz. *)
Definition count := nat.

Definition count_eq (a b : count) : Prop := a = b.

#[global] Instance count_eq_Equivalence : Equivalence count_eq.
Proof. unfold count_eq; split; congruence. Qed.

Definition count_plus (a b : count) : count := a + b.
Definition count_times (a b : count) : count := a * b.
Definition count_zero : count := 0.
Definition count_one : count := 1.

#[global] Instance count_plus_proper :
  Proper (count_eq ==> count_eq ==> count_eq) count_plus.
Proof. unfold count_eq, Proper, respectful; intros; subst; reflexivity. Qed.

#[global] Instance count_times_proper :
  Proper (count_eq ==> count_eq ==> count_eq) count_times.
Proof. unfold count_eq, Proper, respectful; intros; subst; reflexivity. Qed.

(** Each semiring law as a named lemma (the house style: fields are supplied
    inline to the instance, not discharged in a Proof block). Every law is an
    ordinary [Arith] identity over ℕ. *)
Lemma count_plus_assoc : forall a b c : count,
  count_eq (count_plus (count_plus a b) c) (count_plus a (count_plus b c)).
Proof. unfold count_eq, count_plus; intros; symmetry; apply Nat.add_assoc. Qed.

Lemma count_plus_comm : forall a b : count,
  count_eq (count_plus a b) (count_plus b a).
Proof. unfold count_eq, count_plus; intros; apply Nat.add_comm. Qed.

Lemma count_plus_zero_l : forall a : count,
  count_eq (count_plus count_zero a) a.
Proof. unfold count_eq, count_plus, count_zero; intros; apply Nat.add_0_l. Qed.

Lemma count_times_assoc : forall a b c : count,
  count_eq (count_times (count_times a b) c) (count_times a (count_times b c)).
Proof. unfold count_eq, count_times; intros; symmetry; apply Nat.mul_assoc. Qed.

Lemma count_times_one_l : forall a : count,
  count_eq (count_times count_one a) a.
Proof. unfold count_eq, count_times, count_one; intros; apply Nat.mul_1_l. Qed.

Lemma count_times_one_r : forall a : count,
  count_eq (count_times a count_one) a.
Proof. unfold count_eq, count_times, count_one; intros; apply Nat.mul_1_r. Qed.

Lemma count_distr_l : forall a b c : count,
  count_eq (count_times a (count_plus b c))
           (count_plus (count_times a b) (count_times a c)).
Proof. unfold count_eq, count_times, count_plus; intros; apply Nat.mul_add_distr_l. Qed.

Lemma count_distr_r : forall a b c : count,
  count_eq (count_times (count_plus a b) c)
           (count_plus (count_times a c) (count_times b c)).
Proof. unfold count_eq, count_times, count_plus; intros; apply Nat.mul_add_distr_r. Qed.

Lemma count_zero_times_l : forall a : count,
  count_eq (count_times count_zero a) count_zero.
Proof. unfold count_eq, count_times, count_zero; intros; apply Nat.mul_0_l. Qed.

Lemma count_zero_times_r : forall a : count,
  count_eq (count_times a count_zero) count_zero.
Proof. unfold count_eq, count_times, count_zero; intros; apply Nat.mul_0_r. Qed.

(** The counting semiring. *)
#[global] Instance CountSemiring : Semiring count := {
  sr_eq := count_eq;
  sr_eq_equiv := count_eq_Equivalence;
  sr_plus := count_plus;
  sr_plus_proper := count_plus_proper;
  sr_times := count_times;
  sr_times_proper := count_times_proper;
  sr_zero := count_zero;
  sr_one := count_one;
  sr_plus_assoc := count_plus_assoc;
  sr_plus_comm := count_plus_comm;
  sr_plus_zero_l := count_plus_zero_l;
  sr_times_assoc := count_times_assoc;
  sr_times_one_l := count_times_one_l;
  sr_times_one_r := count_times_one_r;
  sr_distr_l := count_distr_l;
  sr_distr_r := count_distr_r;
  sr_zero_times_l := count_zero_times_l;
  sr_zero_times_r := count_zero_times_r
}.

(** The counting semiring is commutative in ⊗ and has no zero divisors among
    its additive structure — facts a shortest-count / path-count analysis
    relies on. *)
Theorem count_times_comm : forall a b : count,
  count_eq (count_times a b) (count_times b a).
Proof. unfold count_eq, count_times; intros; apply Nat.mul_comm. Qed.

Theorem count_plus_zero_sum_free : forall a b : count,
  count_eq (count_plus a b) count_zero -> count_eq a count_zero /\ count_eq b count_zero.
Proof.
  unfold count_eq, count_plus, count_zero; intros a b Hsum.
  apply Nat.eq_add_0 in Hsum. exact Hsum.
Qed.
