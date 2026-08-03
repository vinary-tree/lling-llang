(** * Arctic (max-plus) Semiring

    The arctic semiring is (R union {-infinity}, max, +, -infinity, 0).
    It is the exact-real model corresponding to Rust [ArcticWeight].  Rust
    floating-point operations are tested against this model with an explicit
    roundoff envelope; this file never assumes IEEE-754 associativity.
*)

Require Import Coq.Reals.Reals.
Require Import Coq.micromega.Lra.
Require Import Coq.Classes.Morphisms.
Require Import Coq.Classes.RelationClasses.
Require Import LlingLlang.foundations.Semiring.

Open Scope R_scope.

Inductive arctic : Type :=
  | Arctic_finite : R -> arctic
  | Arctic_neg_inf : arctic.

Definition arctic_eq (a b : arctic) : Prop :=
  match a, b with
  | Arctic_finite x, Arctic_finite y => x = y
  | Arctic_neg_inf, Arctic_neg_inf => True
  | _, _ => False
  end.

Lemma arctic_eq_refl : forall a, arctic_eq a a.
Proof. intros [x|]; simpl; auto. Qed.

Lemma arctic_eq_sym : forall a b, arctic_eq a b -> arctic_eq b a.
Proof. intros [x|] [y|]; simpl; auto. Qed.

Lemma arctic_eq_trans : forall a b c,
  arctic_eq a b -> arctic_eq b c -> arctic_eq a c.
Proof.
  intros [x|] [y|] [z|]; simpl; try contradiction; intros; subst; auto.
Qed.

#[global] Instance arctic_eq_Equivalence : Equivalence arctic_eq := {
  Equivalence_Reflexive := arctic_eq_refl;
  Equivalence_Symmetric := arctic_eq_sym;
  Equivalence_Transitive := arctic_eq_trans
}.

Definition arctic_plus (a b : arctic) : arctic :=
  match a, b with
  | Arctic_neg_inf, _ => b
  | _, Arctic_neg_inf => a
  | Arctic_finite x, Arctic_finite y => Arctic_finite (Rmax x y)
  end.

Definition arctic_times (a b : arctic) : arctic :=
  match a, b with
  | Arctic_neg_inf, _ => Arctic_neg_inf
  | _, Arctic_neg_inf => Arctic_neg_inf
  | Arctic_finite x, Arctic_finite y => Arctic_finite (x + y)
  end.

Definition arctic_zero := Arctic_neg_inf.
Definition arctic_one := Arctic_finite 0.

#[global] Instance arctic_plus_Proper :
  Proper (arctic_eq ==> arctic_eq ==> arctic_eq) arctic_plus.
Proof.
  unfold Proper, respectful.
  intros [a|] [a'|] Ha [b|] [b'|] Hb; simpl in *;
    try contradiction; subst; auto.
Qed.

#[global] Instance arctic_times_Proper :
  Proper (arctic_eq ==> arctic_eq ==> arctic_eq) arctic_times.
Proof.
  unfold Proper, respectful.
  intros [a|] [a'|] Ha [b|] [b'|] Hb; simpl in *;
    try contradiction; subst; auto.
Qed.

Lemma arctic_plus_assoc : forall a b c,
  arctic_eq (arctic_plus (arctic_plus a b) c)
            (arctic_plus a (arctic_plus b c)).
Proof.
  intros [a|] [b|] [c|]; simpl; auto.
  symmetry. apply Rmax_assoc.
Qed.

Lemma arctic_plus_comm : forall a b,
  arctic_eq (arctic_plus a b) (arctic_plus b a).
Proof.
  intros [a|] [b|]; simpl; auto. apply Rmax_comm.
Qed.

Lemma arctic_plus_zero_l : forall a,
  arctic_eq (arctic_plus arctic_zero a) a.
Proof. intros [a|]; simpl; auto. Qed.

Lemma arctic_times_assoc : forall a b c,
  arctic_eq (arctic_times (arctic_times a b) c)
            (arctic_times a (arctic_times b c)).
Proof. intros [a|] [b|] [c|]; simpl; auto; lra. Qed.

Lemma arctic_times_one_l : forall a,
  arctic_eq (arctic_times arctic_one a) a.
Proof. intros [a|]; simpl; auto; lra. Qed.

Lemma arctic_times_one_r : forall a,
  arctic_eq (arctic_times a arctic_one) a.
Proof. intros [a|]; simpl; auto; lra. Qed.

Lemma arctic_distr_l : forall a b c,
  arctic_eq (arctic_times a (arctic_plus b c))
            (arctic_plus (arctic_times a b) (arctic_times a c)).
Proof.
  intros [a|] [b|] [c|]; simpl; auto.
  unfold Rmax.
  destruct (Rle_dec b c); destruct (Rle_dec (a + b) (a + c)); lra.
Qed.

Lemma arctic_distr_r : forall a b c,
  arctic_eq (arctic_times (arctic_plus a b) c)
            (arctic_plus (arctic_times a c) (arctic_times b c)).
Proof.
  intros [a|] [b|] [c|]; simpl; auto.
  unfold Rmax.
  destruct (Rle_dec a b); destruct (Rle_dec (a + c) (b + c)); lra.
Qed.

Lemma arctic_zero_times_l : forall a,
  arctic_eq (arctic_times arctic_zero a) arctic_zero.
Proof. intros [a|]; simpl; auto. Qed.

Lemma arctic_zero_times_r : forall a,
  arctic_eq (arctic_times a arctic_zero) arctic_zero.
Proof. intros [a|]; simpl; auto. Qed.

#[global] Instance ArcticSemiring : Semiring arctic := {
  sr_eq := arctic_eq;
  sr_eq_equiv := arctic_eq_Equivalence;
  sr_plus := arctic_plus;
  sr_plus_proper := arctic_plus_Proper;
  sr_times := arctic_times;
  sr_times_proper := arctic_times_Proper;
  sr_zero := arctic_zero;
  sr_one := arctic_one;
  sr_plus_assoc := arctic_plus_assoc;
  sr_plus_comm := arctic_plus_comm;
  sr_plus_zero_l := arctic_plus_zero_l;
  sr_times_assoc := arctic_times_assoc;
  sr_times_one_l := arctic_times_one_l;
  sr_times_one_r := arctic_times_one_r;
  sr_distr_l := arctic_distr_l;
  sr_distr_r := arctic_distr_r;
  sr_zero_times_l := arctic_zero_times_l;
  sr_zero_times_r := arctic_zero_times_r
}.

Lemma arctic_plus_idempotent : forall a,
  arctic_eq (arctic_plus a a) a.
Proof.
  intros [a|]; simpl; auto. apply Rmax_left. lra.
Qed.

#[global] Instance ArcticIdempotent : IdempotentSemiring arctic := {
  sr_plus_idempotent := arctic_plus_idempotent
}.

Lemma arctic_times_comm : forall a b,
  arctic_eq (arctic_times a b) (arctic_times b a).
Proof. intros [a|] [b|]; simpl; auto; lra. Qed.

#[global] Instance ArcticCommutative : CommutativeTimesSemiring arctic := {
  sr_times_comm := arctic_times_comm
}.

Lemma arctic_zero_sum_free : forall a b,
  arctic_eq (arctic_plus a b) arctic_zero ->
  arctic_eq a arctic_zero /\ arctic_eq b arctic_zero.
Proof. intros [a|] [b|]; simpl; try contradiction; auto. Qed.

#[global] Instance ArcticZeroSumFree : ZeroSumFreeSemiring arctic := {
  sr_zero_sum_free := arctic_zero_sum_free
}.

(** The Rust natural_less relation treats a numerically larger score as the
    preferred ("less") weight, hence [a <=_A b] iff max(a,b)=a. *)
Definition arctic_le (a b : arctic) : Prop :=
  arctic_eq (arctic_plus a b) a.

Lemma arctic_le_refl : forall a, arctic_le a a.
Proof. intro a; apply arctic_plus_idempotent. Qed.

Lemma arctic_le_total : forall a b, arctic_le a b \/ arctic_le b a.
Proof.
  intros [a|] [b|]; unfold arctic_le; simpl; auto.
  destruct (Rle_dec a b).
  - right. apply Rmax_left. assumption.
  - left. apply Rmax_left. lra.
Qed.

Lemma arctic_le_antisym : forall a b,
  arctic_le a b -> arctic_le b a -> arctic_eq a b.
Proof.
  intros [a|] [b|]; unfold arctic_le; simpl; try contradiction; auto.
  unfold Rmax. destruct (Rle_dec a b); destruct (Rle_dec b a); lra.
Qed.

Lemma arctic_le_trans : forall a b c,
  arctic_le a b -> arctic_le b c -> arctic_le a c.
Proof.
  intros [a|] [b|] [c|]; unfold arctic_le; simpl; try contradiction; auto.
  unfold Rmax.
  destruct (Rle_dec a b); destruct (Rle_dec b c);
  destruct (Rle_dec a c); lra.
Qed.

Lemma arctic_le_plus_compat : forall a b c,
  arctic_le a b -> arctic_le (arctic_plus a c) (arctic_plus b c).
Proof.
  intros [a|] [b|] [c|] Hab; unfold arctic_le in *; simpl in *.
  - assert (Hba : b <= a).
    { rewrite <- Hab. apply Rmax_r. }
    apply Rmax_left. apply Rmax_lub.
    + eapply Rle_trans; [exact Hba | apply Rmax_l].
    + apply Rmax_r.
  - exact Hab.
  - apply Rmax_left. apply Rmax_r.
  - reflexivity.
  - contradiction.
  - contradiction.
  - apply Rmax_left. lra.
  - exact I.
Qed.

Lemma arctic_le_times_compat : forall a b c,
  arctic_le a b -> arctic_le (arctic_times a c) (arctic_times b c).
Proof.
  intros [a|] [b|] [c|] Hab; unfold arctic_le in *; simpl in *;
    try contradiction; auto.
  unfold Rmax in *.
  destruct (Rle_dec a b); destruct (Rle_dec (a+c) (b+c)); lra.
Qed.

#[global] Instance ArcticTotallyOrdered : TotallyOrderedSemiring arctic := {
  sr_le := arctic_le;
  sr_le_refl := arctic_le_refl;
  sr_le_total := arctic_le_total;
  sr_le_antisym := arctic_le_antisym;
  sr_le_trans := arctic_le_trans;
  sr_le_plus_compat := arctic_le_plus_compat;
  sr_le_times_compat := arctic_le_times_compat
}.

Definition arctic_star (a : arctic) : option arctic :=
  match a with
  | Arctic_neg_inf => Some arctic_one
  | Arctic_finite x => if Rle_dec x 0 then Some arctic_one else None
  end.

Lemma arctic_star_unfold : forall a astar,
  arctic_star a = Some astar ->
  arctic_eq astar (arctic_plus arctic_one (arctic_times a astar)).
Proof.
  intros [a|] astar Hstar; simpl in Hstar.
  - destruct (Rle_dec a 0); try discriminate.
    inversion Hstar; subst; clear Hstar. simpl.
    rewrite Rplus_0_r. symmetry. apply Rmax_left. assumption.
  - inversion Hstar; subst; simpl; auto.
Qed.

#[global] Instance ArcticStar : StarSemiring arctic := {
  sr_star := arctic_star;
  sr_star_unfold := arctic_star_unfold
}.

(** A positive cycle has no finite max-plus closure. *)
Lemma arctic_positive_cycle_has_no_star : forall a,
  0 < a -> arctic_star (Arctic_finite a) = None.
Proof.
  intros a Ha. simpl. destruct (Rle_dec a 0); [lra | reflexivity].
Qed.

(** Exact transition deltas telescope: adding successive score differences
    reconstructs the final score.  This is the algebraic fact used by
    FzfStateSource's Arctic arc weights. *)
Lemma arctic_delta_telescope : forall s0 s1 s2,
  arctic_times (Arctic_finite (s1 - s0))
    (Arctic_finite (s2 - s1)) = Arctic_finite (s2 - s0).
Proof. intros; simpl; f_equal; lra. Qed.
