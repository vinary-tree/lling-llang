(** * Semiring Properties

    Generic lemmas and properties that hold for semirings with
    certain additional structure.

    This module provides reusable lemmas that can be instantiated
    for specific semirings.
*)

Require Import Coq.Classes.Morphisms.
Require Import LlingLlang.foundations.Semiring.

(** ** Power Function *)

Notation "a ^ n" := (sr_power a n).

Section PowerProperties.
  Context {A : Type} `{Semiring A}.

  #[local]
  Instance sr_times_Proper_local :
    Proper (sr_eq ==> sr_eq ==> sr_eq) sr_times := sr_times_proper.

  (** a^0 = 1 *)
  Lemma sr_power_0 : forall a : A, a ^ 0 ≡ 𝟙.
  Proof.
    intro a. simpl. apply sr_eq_refl.
  Qed.

  (** a^1 = a *)
  Lemma sr_power_1 : forall a : A, a ^ 1 ≡ a.
  Proof.
    intro a. simpl.
    apply sr_times_one_r.
  Qed.

  (** 1^n = 1 *)
  Lemma sr_power_one : forall n : nat, 𝟙 ^ n ≡ (𝟙 : A).
  Proof.
    induction n.
    - apply sr_eq_refl.
    - simpl.
      eapply sr_eq_trans with (b := 𝟙 ⊗ (𝟙 : A)).
      + apply sr_times_proper; [apply sr_eq_refl | exact IHn].
      + apply sr_times_one_l.
  Qed.

  (** 0^(n+1) = 0 *)
  Lemma sr_power_zero : forall n : nat, (𝟘 : A) ^ (S n) ≡ 𝟘.
  Proof.
    intro n. simpl.
    apply sr_zero_times_l.
  Qed.

  (** a^(n+m) = a^n * a^m *)
  Lemma sr_power_add : forall (a : A) (n m : nat),
    a ^ (n + m) ≡ a ^ n ⊗ a ^ m.
  Proof.
    intros a n m.
    induction n.
    - simpl. apply sr_eq_sym. apply sr_times_one_l.
    - simpl.
      eapply sr_eq_trans with (b := a ⊗ (a ^ n ⊗ a ^ m)).
      + apply sr_times_proper; [apply sr_eq_refl | exact IHn].
      + apply sr_eq_sym. apply sr_times_assoc.
  Qed.

End PowerProperties.

(** ** Partial Sums *)

Section PartialStarProperties.
  Context {A : Type} `{Semiring A}.

  (** Unfolding lemma for partial star *)
  Lemma sr_partial_star_unfold : forall (a : A) (n : nat),
    sr_partial_star a (S n) ≡ 𝟙 ⊕ (a ⊗ sr_partial_star a n).
  Proof.
    intros a n. apply sr_eq_refl.
  Qed.

  (** For idempotent semirings, partial star stabilizes *)
  Context `{IdempotentSemiring A}.

  (** Adding the same term doesn't change an idempotent sum *)
  Lemma sr_idempotent_absorb : forall a b : A,
    sr_idempotent_le a b -> a ⊕ b ≡ a.
  Proof.
    intros a b Hle. exact Hle.
  Qed.

End PartialStarProperties.

(** ** Homomorphism *)

(** A semiring homomorphism preserves structure *)
Record SemiringHom {A B : Type} `{Semiring A} `{Semiring B} := {
  sh_map : A -> B;
  sh_zero : sr_eq (sh_map sr_zero) sr_zero;
  sh_one : sr_eq (sh_map sr_one) sr_one;
  sh_plus : forall a b, sr_eq (sh_map (sr_plus a b)) (sr_plus (sh_map a) (sh_map b));
  sh_times : forall a b, sr_eq (sh_map (sr_times a b)) (sr_times (sh_map a) (sh_map b));
}.

(** ** Natural Order in Idempotent Semirings *)

Section NaturalOrder.
  Context {A : Type} `{IdempotentSemiring A}.

  #[local]
  Instance sr_plus_Proper_natural_order :
    Proper (sr_eq ==> sr_eq ==> sr_eq) sr_plus := sr_plus_proper.

  (** The natural order a ≤ b := (a ⊕ b = a) is a partial order *)

  Lemma natural_order_refl : forall a : A, sr_idempotent_le a a.
  Proof.
    apply sr_idempotent_le_refl.
  Qed.

  Lemma natural_order_trans : forall a b c : A,
    sr_idempotent_le a b -> sr_idempotent_le b c -> sr_idempotent_le a c.
  Proof.
    intros a b c Hab Hbc.
    unfold sr_idempotent_le in *.
    (* a ⊕ c ≡ a *)
    (* We have: a ⊕ b ≡ a and b ⊕ c ≡ b *)
    (* a ⊕ c = (a ⊕ b) ⊕ c = a ⊕ (b ⊕ c) = a ⊕ b = a *)
    eapply sr_eq_trans with (b := (a ⊕ b) ⊕ c).
    - apply sr_plus_proper.
      + apply sr_eq_sym. exact Hab.
      + apply sr_eq_refl.
    - eapply sr_eq_trans with (b := a ⊕ (b ⊕ c)).
      + apply sr_plus_assoc.
      + eapply sr_eq_trans with (b := a ⊕ b).
        * apply sr_plus_proper; [apply sr_eq_refl | exact Hbc].
        * exact Hab.
  Qed.

  (** ⊕ is the meet (infimum) in the natural order *)
  Lemma natural_order_meet_lb1 : forall a b : A,
    sr_idempotent_le (a ⊕ b) a.
  Proof.
    apply sr_plus_contracts.
  Qed.

  Lemma natural_order_meet_lb2 : forall a b : A,
    sr_idempotent_le (a ⊕ b) b.
  Proof.
    intros a b.
    unfold sr_idempotent_le.
    eapply sr_eq_trans with (b := (b ⊕ a) ⊕ b).
    - apply sr_plus_proper; [apply sr_plus_comm | apply sr_eq_refl].
    - eapply sr_eq_trans with (b := b ⊕ a).
      + exact (sr_plus_contracts b a).
      + apply sr_plus_comm.
  Qed.

End NaturalOrder.
