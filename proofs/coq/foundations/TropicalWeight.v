(** * Tropical Semiring

    The tropical semiring (ℝ ∪ {+∞}, min, +, +∞, 0) is the standard
    choice for shortest-path problems in WFSTs.

    This corresponds to [TropicalWeight] in lling-llang's
    [src/semiring/tropical.rs].

    Operations:
    - ⊕ = min : Selects the best (minimum cost) of parallel paths
    - ⊗ = + : Accumulates costs along sequential transitions
    - 0̄ = +∞ : Represents unreachable states
    - 1̄ = 0 : Represents zero cost (free transitions)
*)

Require Import Coq.Reals.Reals.
Require Import Coq.micromega.Lra.
Require Import Coq.Classes.Morphisms.
Require Import Coq.Classes.RelationClasses.
Require Import Coq.Setoids.Setoid.
Require Import LlingLlang.foundations.Semiring.

Open Scope R_scope.

(** ** Tropical Weight Type *)

(** We model tropical weights as extended reals with +∞.
    For simplicity, we use an inductive type. *)
Inductive tropical : Type :=
  | Tropical_finite : R -> tropical
  | Tropical_inf : tropical.

(** Equality on tropical weights *)
Definition tropical_eq (a b : tropical) : Prop :=
  match a, b with
  | Tropical_finite x, Tropical_finite y => x = y
  | Tropical_inf, Tropical_inf => True
  | _, _ => False
  end.

(** tropical_eq is an equivalence relation *)
Lemma tropical_eq_refl : forall a : tropical, tropical_eq a a.
Proof.
  intro a; destruct a; simpl; auto.
Qed.

Lemma tropical_eq_sym : forall a b : tropical, tropical_eq a b -> tropical_eq b a.
Proof.
  intros a b H; destruct a, b; simpl in *; auto.
Qed.

Lemma tropical_eq_trans : forall a b c : tropical,
  tropical_eq a b -> tropical_eq b c -> tropical_eq a c.
Proof.
  intros a b c Hab Hbc.
  destruct a, b, c; simpl in *; try contradiction; auto.
  rewrite Hab; auto.
Qed.

#[global]
Instance tropical_eq_Equivalence : Equivalence tropical_eq := {
  Equivalence_Reflexive := tropical_eq_refl;
  Equivalence_Symmetric := tropical_eq_sym;
  Equivalence_Transitive := tropical_eq_trans
}.

(** ** Tropical Operations *)

(** Addition (⊕) is min *)
Definition tropical_plus (a b : tropical) : tropical :=
  match a, b with
  | Tropical_inf, _ => b
  | _, Tropical_inf => a
  | Tropical_finite x, Tropical_finite y => Tropical_finite (Rmin x y)
  end.

(** Multiplication (⊗) is + *)
Definition tropical_times (a b : tropical) : tropical :=
  match a, b with
  | Tropical_inf, _ => Tropical_inf
  | _, Tropical_inf => Tropical_inf
  | Tropical_finite x, Tropical_finite y => Tropical_finite (x + y)
  end.

(** Additive identity (0̄) is +∞ *)
Definition tropical_zero : tropical := Tropical_inf.

(** Multiplicative identity (1̄) is 0 *)
Definition tropical_one : tropical := Tropical_finite 0.

(** ** Proper Instances for Operations *)

#[global]
Instance tropical_plus_Proper :
  Proper (tropical_eq ==> tropical_eq ==> tropical_eq) tropical_plus.
Proof.
  unfold Proper, respectful.
  intros a1 a2 Ha b1 b2 Hb.
  destruct a1, a2, b1, b2; simpl in *; try contradiction; auto.
  rewrite Ha, Hb. reflexivity.
Qed.

#[global]
Instance tropical_times_Proper :
  Proper (tropical_eq ==> tropical_eq ==> tropical_eq) tropical_times.
Proof.
  unfold Proper, respectful.
  intros a1 a2 Ha b1 b2 Hb.
  destruct a1, a2, b1, b2; simpl in *; try contradiction; auto.
  rewrite Ha, Hb. reflexivity.
Qed.

(** ** Tropical Semiring Axioms *)

(** min is associative *)
Lemma tropical_plus_assoc : forall a b c : tropical,
  tropical_eq (tropical_plus (tropical_plus a b) c)
              (tropical_plus a (tropical_plus b c)).
Proof.
  intros a b c.
  destruct a, b, c; simpl; auto.
  symmetry. apply Rmin_assoc.
Qed.

(** min is commutative *)
Lemma tropical_plus_comm : forall a b : tropical,
  tropical_eq (tropical_plus a b) (tropical_plus b a).
Proof.
  intros a b.
  destruct a, b; simpl; auto.
  apply Rmin_comm.
Qed.

(** +∞ is left identity for min *)
Lemma tropical_plus_zero_l : forall a : tropical,
  tropical_eq (tropical_plus tropical_zero a) a.
Proof.
  intro a; destruct a; simpl; auto.
Qed.

(** + is associative *)
Lemma tropical_times_assoc : forall a b c : tropical,
  tropical_eq (tropical_times (tropical_times a b) c)
              (tropical_times a (tropical_times b c)).
Proof.
  intros a b c.
  destruct a, b, c; simpl; auto.
  unfold tropical_eq. lra.
Qed.

(** 0 is left identity for + *)
Lemma tropical_times_one_l : forall a : tropical,
  tropical_eq (tropical_times tropical_one a) a.
Proof.
  intro a; destruct a; simpl; auto.
  unfold tropical_eq. lra.
Qed.

(** 0 is right identity for + *)
Lemma tropical_times_one_r : forall a : tropical,
  tropical_eq (tropical_times a tropical_one) a.
Proof.
  intro a; destruct a; simpl; auto.
  unfold tropical_eq. lra.
Qed.

(** + left-distributes over min *)
Lemma tropical_distr_l : forall a b c : tropical,
  tropical_eq (tropical_times a (tropical_plus b c))
              (tropical_plus (tropical_times a b) (tropical_times a c)).
Proof.
  intros a b c.
  destruct a, b, c; simpl; auto.
  - (* All finite: a * min(b,c) = min(a*b, a*c) *)
    unfold tropical_eq.
    unfold Rmin.
    destruct (Rle_dec r0 r1);
    destruct (Rle_dec (r + r0) (r + r1)); lra.
Qed.

(** + right-distributes over min *)
Lemma tropical_distr_r : forall a b c : tropical,
  tropical_eq (tropical_times (tropical_plus a b) c)
              (tropical_plus (tropical_times a c) (tropical_times b c)).
Proof.
  intros a b c.
  destruct a, b, c; simpl; auto.
  - (* All finite *)
    unfold tropical_eq.
    unfold Rmin.
    destruct (Rle_dec r r0);
    destruct (Rle_dec (r + r1) (r0 + r1)); lra.
Qed.

(** +∞ left-annihilates *)
Lemma tropical_zero_times_l : forall a : tropical,
  tropical_eq (tropical_times tropical_zero a) tropical_zero.
Proof.
  intro a; destruct a; simpl; auto.
Qed.

(** +∞ right-annihilates *)
Lemma tropical_zero_times_r : forall a : tropical,
  tropical_eq (tropical_times a tropical_zero) tropical_zero.
Proof.
  intro a; destruct a; simpl; auto.
Qed.

(** ** Semiring Instance *)

#[global]
Instance TropicalSemiring : Semiring tropical := {
  sr_eq := tropical_eq;
  sr_eq_equiv := tropical_eq_Equivalence;
  sr_plus := tropical_plus;
  sr_plus_proper := tropical_plus_Proper;
  sr_times := tropical_times;
  sr_times_proper := tropical_times_Proper;
  sr_zero := tropical_zero;
  sr_one := tropical_one;
  sr_plus_assoc := tropical_plus_assoc;
  sr_plus_comm := tropical_plus_comm;
  sr_plus_zero_l := tropical_plus_zero_l;
  sr_times_assoc := tropical_times_assoc;
  sr_times_one_l := tropical_times_one_l;
  sr_times_one_r := tropical_times_one_r;
  sr_distr_l := tropical_distr_l;
  sr_distr_r := tropical_distr_r;
  sr_zero_times_l := tropical_zero_times_l;
  sr_zero_times_r := tropical_zero_times_r;
}.

(** ** Additional Tropical Properties *)

(** min is idempotent *)
Lemma tropical_plus_idempotent : forall a : tropical,
  tropical_eq (tropical_plus a a) a.
Proof.
  intro a; destruct a; simpl; auto.
  unfold tropical_eq. apply Rmin_left. lra.
Qed.

(** Tropical semiring is idempotent *)
#[global]
Instance TropicalIdempotent : IdempotentSemiring tropical := {
  sr_plus_idempotent := tropical_plus_idempotent
}.

(** + is commutative *)
Lemma tropical_times_comm : forall a b : tropical,
  tropical_eq (tropical_times a b) (tropical_times b a).
Proof.
  intros a b.
  destruct a, b; simpl; auto.
  unfold tropical_eq. lra.
Qed.

(** Tropical semiring has commutative multiplication *)
#[global]
Instance TropicalCommutative : CommutativeTimesSemiring tropical := {
  sr_times_comm := tropical_times_comm
}.

(** min(a,b) = +∞ implies a = +∞ and b = +∞ *)
Lemma tropical_zero_sum_free : forall a b : tropical,
  tropical_eq (tropical_plus a b) tropical_zero ->
  tropical_eq a tropical_zero /\ tropical_eq b tropical_zero.
Proof.
  intros a b H.
  destruct a, b; simpl in *; try contradiction; auto.
Qed.

(** Tropical semiring is zero-sum-free *)
#[global]
Instance TropicalZeroSumFree : ZeroSumFreeSemiring tropical := {
  sr_zero_sum_free := tropical_zero_sum_free
}.

(** ** Tropical Order *)

(** Natural order on tropical: a ≤ b iff a ⊕ b = a (min selects a) *)
Definition tropical_le (a b : tropical) : Prop :=
  tropical_eq (tropical_plus a b) a.

(** The order is total *)
Lemma tropical_le_total : forall a b : tropical,
  tropical_le a b \/ tropical_le b a.
Proof.
  intros a b.
  destruct a, b; unfold tropical_le; simpl.
  - (* Both finite *)
    unfold tropical_eq.
    destruct (Rle_dec r r0).
    + left. apply Rmin_left. assumption.
    + right. apply Rmin_left. lra.
  - (* a finite, b inf *)
    left. reflexivity.
  - (* a inf, b finite *)
    right. reflexivity.
  - (* Both inf *)
    left. reflexivity.
Qed.

(** The order is antisymmetric *)
Lemma tropical_le_antisym : forall a b : tropical,
  tropical_le a b -> tropical_le b a -> tropical_eq a b.
Proof.
  intros a b Hab Hba.
  destruct a, b; unfold tropical_le in *; simpl in *.
  - (* Both finite *)
    unfold tropical_eq in *.
    unfold Rmin in Hab, Hba.
    destruct (Rle_dec r r0);
    destruct (Rle_dec r0 r); lra.
  - (* a finite, b inf *)
    unfold tropical_eq in *. contradiction.
  - (* a inf, b finite *)
    unfold tropical_eq in *. contradiction.
  - (* Both inf *)
    reflexivity.
Qed.

(** The order is transitive *)
Lemma tropical_le_trans : forall a b c : tropical,
  tropical_le a b -> tropical_le b c -> tropical_le a c.
Proof.
  intros a b c Hab Hbc.
  destruct a as [x|], b as [y|], c as [z|];
    unfold tropical_le in *; simpl in *.
  - unfold tropical_eq in *.
    unfold Rmin in Hab, Hbc |- *.
    destruct (Rle_dec x y);
    destruct (Rle_dec y z);
    destruct (Rle_dec x z); lra.
  - reflexivity.
  - contradiction.
  - reflexivity.
  - contradiction.
  - contradiction.
  - contradiction.
  - reflexivity.
Qed.

(** The order is reflexive. *)
Lemma tropical_le_refl : forall a : tropical, tropical_le a a.
Proof.
  intro a. unfold tropical_le.
  apply tropical_plus_idempotent.
Qed.

(** Tropical addition is monotone in the natural order. *)
Lemma tropical_le_plus_compat : forall a b c : tropical,
  tropical_le a b -> tropical_le (tropical_plus a c) (tropical_plus b c).
Proof.
  intros a b c Hab.
  destruct a as [x|], b as [y|], c as [z|];
    unfold tropical_le in *; simpl in *.
  - unfold tropical_eq in *.
    assert (Hle : x <= y).
    { unfold Rmin in Hab. destruct (Rle_dec x y); lra. }
    apply Rmin_left.
    unfold Rmin.
    destruct (Rle_dec x z);
    destruct (Rle_dec y z); lra.
  - exact Hab.
  - unfold tropical_eq.
    apply Rmin_left.
    unfold Rmin. destruct (Rle_dec x z); lra.
  - reflexivity.
  - contradiction.
  - contradiction.
  - unfold tropical_eq. apply Rmin_left. lra.
  - reflexivity.
Qed.

(** Tropical multiplication is monotone in the natural order. *)
Lemma tropical_le_times_compat : forall a b c : tropical,
  tropical_le a b -> tropical_le (tropical_times a c) (tropical_times b c).
Proof.
  intros a b c Hab.
  destruct a as [x|], b as [y|], c as [z|];
    unfold tropical_le in *; simpl in *.
  - unfold tropical_eq in *.
    assert (Hle : x <= y).
    { unfold Rmin in Hab. destruct (Rle_dec x y); lra. }
    apply Rmin_left. lra.
  - reflexivity.
  - reflexivity.
  - reflexivity.
  - contradiction.
  - contradiction.
  - reflexivity.
  - reflexivity.
Qed.

(** Tropical weights form a totally ordered semiring under the natural order. *)
#[global]
Instance TropicalTotallyOrdered : TotallyOrderedSemiring tropical := {
  sr_le := tropical_le;
  sr_le_refl := tropical_le_refl;
  sr_le_total := tropical_le_total;
  sr_le_antisym := tropical_le_antisym;
  sr_le_trans := tropical_le_trans;
  sr_le_plus_compat := tropical_le_plus_compat;
  sr_le_times_compat := tropical_le_times_compat;
}.

(** ** Kleene Star for Tropical Semiring *)

(** For tropical semiring with non-negative weights:
    a* = min(0, a, 2a, 3a, ...) = 0 when a >= 0 *)
Definition tropical_star (a : tropical) : option tropical :=
  match a with
  | Tropical_inf => Some tropical_one  (* ∞* = 0 *)
  | Tropical_finite x =>
      if Rle_dec 0 x then Some tropical_one  (* x >= 0: star = 0 *)
      else None  (* x < 0: diverges to -∞ *)
  end.

(** Star satisfies the fixed-point equation for non-negative weights *)
Lemma tropical_star_unfold : forall a astar : tropical,
  tropical_star a = Some astar ->
  tropical_eq astar (tropical_plus tropical_one (tropical_times a astar)).
Proof.
  intros a astar Hstar.
  destruct a; simpl in Hstar.
  - (* Finite *)
    destruct (Rle_dec 0 r); try discriminate.
    inversion Hstar; subst; clear Hstar.
    simpl. unfold tropical_eq.
    (* We need to show: 0 = min(0, r + 0) = min(0, r) *)
    rewrite Rplus_0_r.
    symmetry. apply Rmin_left. assumption.
  - (* Infinity *)
    inversion Hstar; subst; clear Hstar.
    simpl. reflexivity.
Qed.

(** Tropical semiring is a star semiring *)
#[global]
Instance TropicalStar : StarSemiring tropical := {
  sr_star := tropical_star;
  sr_star_unfold := tropical_star_unfold
}.
