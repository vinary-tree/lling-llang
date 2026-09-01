(** * Boolean Semiring

    The boolean semiring is ({false, true}, ∨, ∧, false, true): reachability
    weights where ⊕ is disjunction and ⊗ is conjunction. It is the model of
    the interop [VtWeightDomain::BooleanF64] domain (values exactly 0.0 or
    1.0); lling-llang has no dedicated Rust `BooleanWeight` struct — the
    boolean domain crosses the ABI as an f64 restricted to {0, 1}, and the
    ABI weight bridge (obligation #16) maps that f64 representation onto this
    exact two-element carrier.

    Every law decides by case analysis over the two elements. Qed-clean.

    Registry: proofs/doc/abi-invariants.tsv, LLING-SEMI-4.
*)

From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Classes.Morphisms.
From Stdlib Require Import Classes.RelationClasses.
From Stdlib Require Import Setoids.Setoid.
Require Import LlingLlang.foundations.Semiring.

(** The exact boolean weight. Equality is Leibniz on [bool]. *)
Notation boolw := bool.

Definition boolw_eq (a b : boolw) : Prop := a = b.

#[global] Instance boolw_eq_Equivalence : Equivalence boolw_eq.
Proof. unfold boolw_eq; split; congruence. Qed.

Definition boolw_plus (a b : boolw) : boolw := orb a b.
Definition boolw_times (a b : boolw) : boolw := andb a b.
Definition boolw_zero : boolw := false.
Definition boolw_one : boolw := true.

#[global] Instance boolw_plus_proper :
  Proper (boolw_eq ==> boolw_eq ==> boolw_eq) boolw_plus.
Proof. unfold boolw_eq, Proper, respectful; intros; subst; reflexivity. Qed.

#[global] Instance boolw_times_proper :
  Proper (boolw_eq ==> boolw_eq ==> boolw_eq) boolw_times.
Proof. unfold boolw_eq, Proper, respectful; intros; subst; reflexivity. Qed.

(** Every semiring law decides by [destruct] over the (at most three) boolean
    arguments. *)
Lemma boolw_plus_assoc : forall a b c : boolw,
  boolw_eq (boolw_plus (boolw_plus a b) c) (boolw_plus a (boolw_plus b c)).
Proof. unfold boolw_eq, boolw_plus; intros [] [] []; reflexivity. Qed.

Lemma boolw_plus_comm : forall a b : boolw,
  boolw_eq (boolw_plus a b) (boolw_plus b a).
Proof. unfold boolw_eq, boolw_plus; intros [] []; reflexivity. Qed.

Lemma boolw_plus_zero_l : forall a : boolw,
  boolw_eq (boolw_plus boolw_zero a) a.
Proof. unfold boolw_eq, boolw_plus, boolw_zero; intros []; reflexivity. Qed.

Lemma boolw_times_assoc : forall a b c : boolw,
  boolw_eq (boolw_times (boolw_times a b) c) (boolw_times a (boolw_times b c)).
Proof. unfold boolw_eq, boolw_times; intros [] [] []; reflexivity. Qed.

Lemma boolw_times_one_l : forall a : boolw,
  boolw_eq (boolw_times boolw_one a) a.
Proof. unfold boolw_eq, boolw_times, boolw_one; intros []; reflexivity. Qed.

Lemma boolw_times_one_r : forall a : boolw,
  boolw_eq (boolw_times a boolw_one) a.
Proof. unfold boolw_eq, boolw_times, boolw_one; intros []; reflexivity. Qed.

Lemma boolw_distr_l : forall a b c : boolw,
  boolw_eq (boolw_times a (boolw_plus b c))
           (boolw_plus (boolw_times a b) (boolw_times a c)).
Proof. unfold boolw_eq, boolw_times, boolw_plus; intros [] [] []; reflexivity. Qed.

Lemma boolw_distr_r : forall a b c : boolw,
  boolw_eq (boolw_times (boolw_plus a b) c)
           (boolw_plus (boolw_times a c) (boolw_times b c)).
Proof. unfold boolw_eq, boolw_times, boolw_plus; intros [] [] []; reflexivity. Qed.

Lemma boolw_zero_times_l : forall a : boolw,
  boolw_eq (boolw_times boolw_zero a) boolw_zero.
Proof. unfold boolw_eq, boolw_times, boolw_zero; intros []; reflexivity. Qed.

Lemma boolw_zero_times_r : forall a : boolw,
  boolw_eq (boolw_times a boolw_zero) boolw_zero.
Proof. unfold boolw_eq, boolw_times, boolw_zero; intros []; reflexivity. Qed.

#[global] Instance BooleanSemiring : Semiring boolw := {
  sr_eq := boolw_eq;
  sr_eq_equiv := boolw_eq_Equivalence;
  sr_plus := boolw_plus;
  sr_plus_proper := boolw_plus_proper;
  sr_times := boolw_times;
  sr_times_proper := boolw_times_proper;
  sr_zero := boolw_zero;
  sr_one := boolw_one;
  sr_plus_assoc := boolw_plus_assoc;
  sr_plus_comm := boolw_plus_comm;
  sr_plus_zero_l := boolw_plus_zero_l;
  sr_times_assoc := boolw_times_assoc;
  sr_times_one_l := boolw_times_one_l;
  sr_times_one_r := boolw_times_one_r;
  sr_distr_l := boolw_distr_l;
  sr_distr_r := boolw_distr_r;
  sr_zero_times_l := boolw_zero_times_l;
  sr_zero_times_r := boolw_zero_times_r
}.

(** The boolean semiring is idempotent in ⊕ (reachability is a fixpoint) and
    commutative in ⊗ — the two facts a boolean reachability closure relies on. *)
Theorem boolw_plus_idempotent : forall a : boolw,
  boolw_eq (boolw_plus a a) a.
Proof. unfold boolw_eq, boolw_plus; intros []; reflexivity. Qed.

Theorem boolw_times_comm : forall a b : boolw,
  boolw_eq (boolw_times a b) (boolw_times b a).
Proof. unfold boolw_eq, boolw_times; intros [] []; reflexivity. Qed.

(** The ABI representation law: the boolean domain crosses the wire as an f64
    in exactly {0, 1}. Modeled here as the two carrier values' canonical
    encodings; the bridge (#16) proves the f64 round trip. *)
Definition boolw_encode (a : boolw) : nat := if a then 1 else 0.

Theorem boolw_encode_injective : forall a b : boolw,
  boolw_encode a = boolw_encode b -> a = b.
Proof. intros [] []; simpl; congruence. Qed.
