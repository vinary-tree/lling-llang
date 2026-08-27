(** * Signed Tropical Semiring

    The signed tropical semiring is (ℝ ∪ {+∞}, min, +, +∞, 0) where the finite
    part ranges over ALL reals — positive costs AND negative rewards. It is the
    semiring model of Rust [SignedTropicalWeight]
    (src/semiring/signed/signed_tropical.rs), whose `plus` is `min`, `times` is
    `+`, `zero` is `+∞`, and `one` is `0`.

    Algebraically this is the same min-plus structure as [TropicalWeight]
    (proofs/coq/foundations/TropicalWeight.v); the only difference is
    intended range — the plain tropical weight is validated to reject negative
    finite values, while the signed weight admits them. The Coq carrier `R`
    already ranges over all reals, so the semiring laws are proved identically.

    **The −∞ caveat (a documented non-semiring value).** Rust
    `SignedTropicalWeight` can also HOLD `-∞` ("an infinitely good reward",
    which the source itself flags as a special/error case). `-∞` is NOT part
    of this semiring: with `+∞` as the annihilating zero, the zero-annihilation
    law needs `(+∞) ⊗ x = +∞` for every `x`, but the Rust `times` computes
    `x.value() + other.value()`, and `(+∞) + (-∞) = NaN` in IEEE-754 — a value
    outside the carrier. This model therefore ranges over `ℝ ∪ {+∞}` only; the
    representability and rejection of `-∞` at the ABI is the weight bridge's
    concern (obligation #16, and the tropical ingestion fix already rejects it
    on the composition path). This file never assumes IEEE-754 associativity —
    it reasons over exact extended reals.

    Registry: proofs/doc/abi-invariants.tsv, LLING-SEMI-2.
*)

From Stdlib Require Import Reals.Reals.
From Stdlib Require Import micromega.Lra.
From Stdlib Require Import Classes.Morphisms.
From Stdlib Require Import Classes.RelationClasses.
Require Import LlingLlang.foundations.Semiring.

Open Scope R_scope.

(** A signed tropical weight: a finite real (of either sign) or +∞. *)
Inductive stropical : Type :=
  | STropical_finite : R -> stropical
  | STropical_inf : stropical.

Definition stropical_eq (a b : stropical) : Prop :=
  match a, b with
  | STropical_finite x, STropical_finite y => x = y
  | STropical_inf, STropical_inf => True
  | _, _ => False
  end.

Lemma stropical_eq_refl : forall a, stropical_eq a a.
Proof. intro a; destruct a; simpl; auto. Qed.

Lemma stropical_eq_sym : forall a b, stropical_eq a b -> stropical_eq b a.
Proof. intros a b H; destruct a, b; simpl in *; auto. Qed.

Lemma stropical_eq_trans : forall a b c,
  stropical_eq a b -> stropical_eq b c -> stropical_eq a c.
Proof.
  intros a b c Hab Hbc; destruct a, b, c; simpl in *; try contradiction; auto.
  rewrite Hab; auto.
Qed.

#[global] Instance stropical_eq_Equivalence : Equivalence stropical_eq := {
  Equivalence_Reflexive := stropical_eq_refl;
  Equivalence_Symmetric := stropical_eq_sym;
  Equivalence_Transitive := stropical_eq_trans
}.

Definition stropical_plus (a b : stropical) : stropical :=
  match a, b with
  | STropical_inf, _ => b
  | _, STropical_inf => a
  | STropical_finite x, STropical_finite y => STropical_finite (Rmin x y)
  end.

Definition stropical_times (a b : stropical) : stropical :=
  match a, b with
  | STropical_inf, _ => STropical_inf
  | _, STropical_inf => STropical_inf
  | STropical_finite x, STropical_finite y => STropical_finite (x + y)
  end.

Definition stropical_zero : stropical := STropical_inf.
Definition stropical_one : stropical := STropical_finite 0.

#[global] Instance stropical_plus_Proper :
  Proper (stropical_eq ==> stropical_eq ==> stropical_eq) stropical_plus.
Proof.
  unfold Proper, respectful; intros a1 a2 Ha b1 b2 Hb.
  destruct a1, a2, b1, b2; simpl in *; try contradiction; auto.
  rewrite Ha, Hb; reflexivity.
Qed.

#[global] Instance stropical_times_Proper :
  Proper (stropical_eq ==> stropical_eq ==> stropical_eq) stropical_times.
Proof.
  unfold Proper, respectful; intros a1 a2 Ha b1 b2 Hb.
  destruct a1, a2, b1, b2; simpl in *; try contradiction; auto.
  rewrite Ha, Hb; reflexivity.
Qed.

Lemma stropical_plus_assoc : forall a b c : stropical,
  stropical_eq (stropical_plus (stropical_plus a b) c)
               (stropical_plus a (stropical_plus b c)).
Proof. intros a b c; destruct a, b, c; simpl; auto. symmetry; apply Rmin_assoc. Qed.

Lemma stropical_plus_comm : forall a b : stropical,
  stropical_eq (stropical_plus a b) (stropical_plus b a).
Proof. intros a b; destruct a, b; simpl; auto. apply Rmin_comm. Qed.

Lemma stropical_plus_zero_l : forall a : stropical,
  stropical_eq (stropical_plus stropical_zero a) a.
Proof. intro a; destruct a; simpl; auto. Qed.

Lemma stropical_times_assoc : forall a b c : stropical,
  stropical_eq (stropical_times (stropical_times a b) c)
               (stropical_times a (stropical_times b c)).
Proof. intros a b c; destruct a, b, c; simpl; auto. unfold stropical_eq; lra. Qed.

Lemma stropical_times_one_l : forall a : stropical,
  stropical_eq (stropical_times stropical_one a) a.
Proof. intro a; destruct a; simpl; auto. unfold stropical_eq; lra. Qed.

Lemma stropical_times_one_r : forall a : stropical,
  stropical_eq (stropical_times a stropical_one) a.
Proof. intro a; destruct a; simpl; auto. unfold stropical_eq; lra. Qed.

Lemma stropical_distr_l : forall a b c : stropical,
  stropical_eq (stropical_times a (stropical_plus b c))
               (stropical_plus (stropical_times a b) (stropical_times a c)).
Proof.
  intros a b c; destruct a, b, c; simpl; auto.
  unfold stropical_eq, Rmin.
  destruct (Rle_dec r0 r1); destruct (Rle_dec (r + r0) (r + r1)); lra.
Qed.

Lemma stropical_distr_r : forall a b c : stropical,
  stropical_eq (stropical_times (stropical_plus a b) c)
               (stropical_plus (stropical_times a c) (stropical_times b c)).
Proof.
  intros a b c; destruct a, b, c; simpl; auto.
  unfold stropical_eq, Rmin.
  destruct (Rle_dec r r0); destruct (Rle_dec (r + r1) (r0 + r1)); lra.
Qed.

Lemma stropical_zero_times_l : forall a : stropical,
  stropical_eq (stropical_times stropical_zero a) stropical_zero.
Proof. intro a; destruct a; simpl; auto. Qed.

Lemma stropical_zero_times_r : forall a : stropical,
  stropical_eq (stropical_times a stropical_zero) stropical_zero.
Proof. intro a; destruct a; simpl; auto. Qed.

#[global] Instance SignedTropicalSemiring : Semiring stropical := {
  sr_eq := stropical_eq;
  sr_eq_equiv := stropical_eq_Equivalence;
  sr_plus := stropical_plus;
  sr_plus_proper := stropical_plus_Proper;
  sr_times := stropical_times;
  sr_times_proper := stropical_times_Proper;
  sr_zero := stropical_zero;
  sr_one := stropical_one;
  sr_plus_assoc := stropical_plus_assoc;
  sr_plus_comm := stropical_plus_comm;
  sr_plus_zero_l := stropical_plus_zero_l;
  sr_times_assoc := stropical_times_assoc;
  sr_times_one_l := stropical_times_one_l;
  sr_times_one_r := stropical_times_one_r;
  sr_distr_l := stropical_distr_l;
  sr_distr_r := stropical_distr_r;
  sr_zero_times_l := stropical_zero_times_l;
  sr_zero_times_r := stropical_zero_times_r
}.

(** ⊕ = min is idempotent (parallel-path selection is a fixpoint), and the
    signed weight admits genuinely negative finite rewards that the plain
    tropical weight rejects — the distinguishing witness. *)
Theorem stropical_plus_idempotent : forall a : stropical,
  stropical_eq (stropical_plus a a) a.
Proof. intro a; destruct a; simpl; auto. unfold stropical_eq; apply Rmin_left; lra. Qed.

Theorem stropical_admits_negative_finite :
  stropical_eq (stropical_times (STropical_finite (-1)) (STropical_finite (-2)))
               (STropical_finite (-3)).
Proof. unfold stropical_eq; simpl; lra. Qed.
