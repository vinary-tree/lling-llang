(** * Semiring Foundations

    This module defines the algebraic structure of semirings and proves
    basic properties that hold for all semirings.

    A semiring (K, ⊕, ⊗, 0̄, 1̄) consists of:
    - A set K with elements called "weights"
    - An associative, commutative addition ⊕ with identity 0̄
    - An associative multiplication ⊗ with identity 1̄
    - Multiplication distributes over addition
    - 0̄ annihilates under multiplication

    This corresponds directly to the [Semiring] trait in lling-llang's
    [src/semiring/traits.rs].
*)

Require Import Coq.Arith.Arith.
Require Import Coq.Classes.Morphisms.
Require Import Coq.Classes.RelationClasses.
Require Import Coq.Setoids.Setoid.

(** ** Semiring Type Class *)

Class Semiring (A : Type) := {
  (** Weights can be compared for equality *)
  sr_eq : A -> A -> Prop;
  sr_eq_equiv :> Equivalence sr_eq;

  (** Addition operation (⊕) *)
  sr_plus : A -> A -> A;
  sr_plus_proper : Proper (sr_eq ==> sr_eq ==> sr_eq) sr_plus;

  (** Multiplication operation (⊗) *)
  sr_times : A -> A -> A;
  sr_times_proper : Proper (sr_eq ==> sr_eq ==> sr_eq) sr_times;

  (** Additive identity (0̄) *)
  sr_zero : A;

  (** Multiplicative identity (1̄) *)
  sr_one : A;

  (** Semiring law: ⊕ is associative *)
  sr_plus_assoc : forall a b c : A,
    sr_eq (sr_plus (sr_plus a b) c) (sr_plus a (sr_plus b c));

  (** Semiring law: ⊕ is commutative *)
  sr_plus_comm : forall a b : A,
    sr_eq (sr_plus a b) (sr_plus b a);

  (** Semiring law: 0̄ is left identity for ⊕ *)
  sr_plus_zero_l : forall a : A,
    sr_eq (sr_plus sr_zero a) a;

  (** Semiring law: ⊗ is associative *)
  sr_times_assoc : forall a b c : A,
    sr_eq (sr_times (sr_times a b) c) (sr_times a (sr_times b c));

  (** Semiring law: 1̄ is left identity for ⊗ *)
  sr_times_one_l : forall a : A,
    sr_eq (sr_times sr_one a) a;

  (** Semiring law: 1̄ is right identity for ⊗ *)
  sr_times_one_r : forall a : A,
    sr_eq (sr_times a sr_one) a;

  (** Semiring law: ⊗ left-distributes over ⊕ *)
  sr_distr_l : forall a b c : A,
    sr_eq (sr_times a (sr_plus b c)) (sr_plus (sr_times a b) (sr_times a c));

  (** Semiring law: ⊗ right-distributes over ⊕ *)
  sr_distr_r : forall a b c : A,
    sr_eq (sr_times (sr_plus a b) c) (sr_plus (sr_times a c) (sr_times b c));

  (** Semiring law: 0̄ left-annihilates *)
  sr_zero_times_l : forall a : A,
    sr_eq (sr_times sr_zero a) sr_zero;

  (** Semiring law: 0̄ right-annihilates *)
  sr_zero_times_r : forall a : A,
    sr_eq (sr_times a sr_zero) sr_zero;
}.

(** Infix notations for semiring operations *)
Notation "a ⊕ b" := (sr_plus a b) (at level 50, left associativity).
Notation "a ⊗ b" := (sr_times a b) (at level 40, left associativity).
Notation "𝟘" := sr_zero.
Notation "𝟙" := sr_one.
Notation "a ≡ b" := (sr_eq a b) (at level 70, no associativity).

(** ** Derived Properties *)

Section SemiringProperties.
  Context {A : Type} `{Semiring A}.

  #[local]
  Instance sr_plus_Proper_idempotent :
    Proper (sr_eq ==> sr_eq ==> sr_eq) sr_plus := sr_plus_proper.

  #[local]
  Instance sr_times_Proper_local :
    Proper (sr_eq ==> sr_eq ==> sr_eq) sr_times := sr_times_proper.

  Lemma sr_eq_refl : forall a : A, a ≡ a.
  Proof.
    intro a. apply (@Equivalence_Reflexive A sr_eq sr_eq_equiv).
  Qed.

  Lemma sr_eq_sym : forall a b : A, a ≡ b -> b ≡ a.
  Proof.
    intros a b Heq.
    eapply (@Equivalence_Symmetric A sr_eq sr_eq_equiv); exact Heq.
  Qed.

  Lemma sr_eq_trans : forall a b c : A, a ≡ b -> b ≡ c -> a ≡ c.
  Proof.
    intros a b c Hab Hbc.
    eapply (@Equivalence_Transitive A sr_eq sr_eq_equiv); eauto.
  Qed.

  (** 0̄ is also a right identity for ⊕ (derived from commutativity) *)
  Lemma sr_plus_zero_r : forall a : A,
    a ⊕ 𝟘 ≡ a.
  Proof.
    intro a.
    eapply sr_eq_trans with (b := 𝟘 ⊕ a).
    - apply sr_plus_comm.
    - apply sr_plus_zero_l.
  Qed.

  (** ⊕ can be reassociated left *)
  Lemma sr_plus_assoc_l : forall a b c : A,
    a ⊕ (b ⊕ c) ≡ (a ⊕ b) ⊕ c.
  Proof.
    intros a b c.
    apply sr_eq_sym.
    apply sr_plus_assoc.
  Qed.

  (** ⊗ can be reassociated left *)
  Lemma sr_times_assoc_l : forall a b c : A,
    a ⊗ (b ⊗ c) ≡ (a ⊗ b) ⊗ c.
  Proof.
    intros a b c.
    apply sr_eq_sym.
    apply sr_times_assoc.
  Qed.

  (** Double application of ⊕ with 𝟘 *)
  Lemma sr_plus_zero_zero : 𝟘 ⊕ 𝟘 ≡ (𝟘 : A).
  Proof.
    apply sr_plus_zero_l.
  Qed.

  (** Double application of ⊗ with 𝟙 *)
  Lemma sr_times_one_one : 𝟙 ⊗ 𝟙 ≡ (𝟙 : A).
  Proof.
    apply sr_times_one_l.
  Qed.

End SemiringProperties.

(** ** Idempotent Semiring *)

(** An idempotent semiring satisfies a ⊕ a = a.
    This corresponds to [IdempotentSemiring] in lling-llang. *)
Class IdempotentSemiring (A : Type) `{Semiring A} := {
  sr_plus_idempotent : forall a : A, a ⊕ a ≡ a
}.

(** ** Commutative Semiring *)

(** A commutative semiring has commutative multiplication.
    This corresponds to [CommutativeTimesSemiring] in lling-llang. *)
Class CommutativeTimesSemiring (A : Type) `{Semiring A} := {
  sr_times_comm : forall a b : A, a ⊗ b ≡ b ⊗ a
}.

(** ** Zero-Sum-Free Semiring *)

(** A zero-sum-free semiring has the property that a ⊕ b = 0̄ implies a = b = 0̄.
    This corresponds to [ZeroSumFreeSemiring] in lling-llang. *)
Class ZeroSumFreeSemiring (A : Type) `{Semiring A} := {
  sr_zero_sum_free : forall a b : A, a ⊕ b ≡ 𝟘 -> a ≡ 𝟘 /\ b ≡ 𝟘
}.

(** ** Divisible Semiring *)

(** A divisible semiring has a division operation.
    This corresponds to [DivisibleSemiring] in lling-llang. *)
Class DivisibleSemiring (A : Type) `{Semiring A} := {
  sr_divide : A -> A -> option A;

  (** Division inverts multiplication when divisor is non-zero *)
  sr_divide_spec : forall a b c : A,
    ~(b ≡ 𝟘) ->
    sr_divide (a ⊗ b) b = Some c ->
    c ≡ a
}.

(** ** Weakly Left-Divisible Semiring *)

(** A weakly left-divisible semiring supports the quotient used by weighted
    determinization. This matches the standard WFST requirement: for
    [x ⊕ y <> 0], one can choose a residual [z] such that
    [x = z ⊗ (x ⊕ y)]. *)
Class WeaklyLeftDivisibleSemiring (A : Type) `{Semiring A} := {
  sr_left_divide : A -> A -> option A;

  sr_left_divide_spec : forall a divisor quotient : A,
    ~(divisor ≡ 𝟘) ->
    sr_left_divide a divisor = Some quotient ->
    quotient ⊗ divisor ≡ a
}.

(** ** Star Semiring *)

(** A star semiring has a Kleene closure operation.
    This corresponds to [StarSemiring] in lling-llang. *)
Class StarSemiring (A : Type) `{Semiring A} := {
  sr_star : A -> option A;

  (** Star satisfies the fixed-point equation: a* = 1 ⊕ a ⊗ a* *)
  sr_star_unfold : forall a astar : A,
    sr_star a = Some astar ->
    astar ≡ 𝟙 ⊕ (a ⊗ astar)
}.

(** ** k-Closed Semiring *)

(** Define a^n for semiring elements. *)
Fixpoint sr_power {A : Type} `{Semiring A} (a : A) (n : nat) : A :=
  match n with
  | O => sr_one
  | S m => sr_times a (sr_power a m)
  end.

(** Define partial star: 1 + a + a^2 + ... + a^n. *)
Fixpoint sr_partial_star {A : Type} `{Semiring A} (a : A) (n : nat) : A :=
  match n with
  | O => sr_one
  | S m => sr_plus sr_one (sr_times a (sr_partial_star a m))
  end.

(** A k-closed semiring has bounded convergence for star.
    This corresponds to [KClosedSemiring] in lling-llang. *)
Class KClosedSemiring (A : Type) `{Semiring A} := {
  sr_closure_bound : option nat;

  (** If there is a uniform bound [k], then the partial star stabilizes after
      one additional unfolding. *)
  sr_k_closed : match sr_closure_bound with
    | Some k => forall a : A,
        sr_partial_star a (S k) ≡ sr_partial_star a k
    | None => True
    end
}.

(** ** Totally Ordered Semiring *)

(** A totally ordered semiring has a total order compatible with operations.
    This corresponds to [TotallyOrderedSemiring] in lling-llang. *)
Class TotallyOrderedSemiring (A : Type) `{Semiring A} := {
  sr_le : A -> A -> Prop;
  sr_le_refl : forall a : A, sr_le a a;
  sr_le_total : forall a b : A, sr_le a b \/ sr_le b a;
  sr_le_antisym : forall a b : A, sr_le a b -> sr_le b a -> a ≡ b;
  sr_le_trans : forall a b c : A, sr_le a b -> sr_le b c -> sr_le a c;

  (** Order is compatible with ⊕ *)
  sr_le_plus_compat : forall a b c : A, sr_le a b -> sr_le (a ⊕ c) (b ⊕ c);

  (** Order is compatible with right multiplication. Concrete instances may
      restrict their carrier so this monotonicity law is valid. *)
  sr_le_times_compat : forall a b c : A,
    sr_le a b -> sr_le (a ⊗ c) (b ⊗ c)
}.

Notation "a ≤ b" := (sr_le a b) (at level 70, no associativity).

(** ** Properties of Idempotent Semirings *)

Section IdempotentProperties.
  Context {A : Type} `{IdempotentSemiring A}.

  #[local]
  Instance sr_plus_Proper_local :
    Proper (sr_eq ==> sr_eq ==> sr_eq) sr_plus := sr_plus_proper.

  (** In an idempotent semiring, ⊕ defines a partial order *)
  Definition sr_idempotent_le (a b : A) : Prop := a ⊕ b ≡ a.

  (** The idempotent order is reflexive *)
  Lemma sr_idempotent_le_refl : forall a : A, sr_idempotent_le a a.
  Proof.
    intro a.
    unfold sr_idempotent_le.
    apply sr_plus_idempotent.
  Qed.

  (** In an idempotent semiring, a ⊕ b ≤ a (under the natural order) *)
  Lemma sr_plus_contracts : forall a b : A,
    sr_idempotent_le (a ⊕ b) a.
  Proof.
    intros a b.
    unfold sr_idempotent_le.
    eapply sr_eq_trans with (b := a ⊕ (b ⊕ a)).
    - apply sr_plus_assoc.
    - eapply sr_eq_trans with (b := a ⊕ (a ⊕ b)).
      + apply sr_plus_proper.
        * apply (@Equivalence_Reflexive A sr_eq sr_eq_equiv).
        * apply sr_plus_comm.
      + eapply sr_eq_trans with (b := (a ⊕ a) ⊕ b).
        * apply sr_eq_sym. apply sr_plus_assoc.
        * apply sr_plus_proper.
          -- apply sr_plus_idempotent.
          -- apply (@Equivalence_Reflexive A sr_eq sr_eq_equiv).
  Qed.

End IdempotentProperties.

(** ** Properties of Commutative Semirings *)

Section CommutativeProperties.
  Context {A : Type} `{CommutativeTimesSemiring A}.

  (** In a commutative semiring, left and right distribution are equivalent *)
  Lemma sr_distr_comm : forall a b c : A,
    (a ⊕ b) ⊗ c ≡ c ⊗ (a ⊕ b).
  Proof.
    intros a b c.
    apply sr_times_comm.
  Qed.

End CommutativeProperties.
