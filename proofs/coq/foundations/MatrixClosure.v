(** * Matrix Closure over Semirings

    Generic finite-matrix operations used to state cyclic WFST closure
    semantics.  The construction is dimension-indexed at the operation level:
    matrix multiplication sums over the finite index set [0, dim).
*)

Require Import Coq.Arith.Arith.
Require Import Coq.Classes.Morphisms.
Require Import Coq.Classes.RelationClasses.
Require Import Coq.micromega.Lia.
Require Import LlingLlang.foundations.Semiring.

(** A matrix is represented extensionally.  Operations that need a finite
    carrier take an explicit [dim] argument and only sum over indices below it. *)
Definition Matrix (W : Type) := nat -> nat -> W.

Section MatrixClosure.
  Context {W : Type} `{Semiring W}.

  Definition matrix_eq (a b : Matrix W) : Prop :=
    forall i j, a i j ≡ b i j.

  #[global]
  Instance matrix_eq_equiv : Equivalence matrix_eq.
  Proof.
    split.
    - intros a i j. apply sr_eq_refl.
    - intros a b Hab i j. apply sr_eq_sym. apply Hab.
    - intros a b c Hab Hbc i j.
      eapply sr_eq_trans; [apply Hab | apply Hbc].
  Qed.

  Definition matrix_zero : Matrix W :=
    fun _ _ => 𝟘.

  Definition matrix_identity : Matrix W :=
    fun i j => if Nat.eqb i j then 𝟙 else 𝟘.

  Definition matrix_plus (a b : Matrix W) : Matrix W :=
    fun i j => a i j ⊕ b i j.

  #[global]
  Instance matrix_plus_Proper :
    Proper (matrix_eq ==> matrix_eq ==> matrix_eq) matrix_plus.
  Proof.
    intros a a' Ha b b' Hb i j.
    unfold matrix_plus.
    apply sr_plus_proper; [apply Ha | apply Hb].
  Qed.

  Lemma matrix_plus_zero_l : forall a,
    matrix_eq (matrix_plus matrix_zero a) a.
  Proof.
    intros a i j.
    unfold matrix_plus, matrix_zero.
    apply sr_plus_zero_l.
  Qed.

  Lemma matrix_plus_zero_r : forall a,
    matrix_eq (matrix_plus a matrix_zero) a.
  Proof.
    intros a i j.
    unfold matrix_plus, matrix_zero.
    apply sr_plus_zero_r.
  Qed.

  Lemma matrix_plus_assoc : forall a b c,
    matrix_eq
      (matrix_plus (matrix_plus a b) c)
      (matrix_plus a (matrix_plus b c)).
  Proof.
    intros a b c i j.
    unfold matrix_plus.
    apply sr_plus_assoc.
  Qed.

  Lemma matrix_plus_comm : forall a b,
    matrix_eq (matrix_plus a b) (matrix_plus b a).
  Proof.
    intros a b i j.
    unfold matrix_plus.
    apply sr_plus_comm.
  Qed.

  Fixpoint matrix_sum_to (f : nat -> W) (max_idx : nat) : W :=
    match max_idx with
    | O => f O
    | S m => matrix_sum_to f m ⊕ f (S m)
    end.

  Definition bounded_sum (dim : nat) (f : nat -> W) : W :=
    match dim with
    | O => 𝟘
    | S max_idx => matrix_sum_to f max_idx
    end.

  Lemma matrix_sum_to_proper : forall f g max_idx,
    (forall k, (k <= max_idx)%nat -> f k ≡ g k) ->
    matrix_sum_to f max_idx ≡ matrix_sum_to g max_idx.
  Proof.
    intros f g max_idx.
    induction max_idx as [| max_idx IH]; intro Hpoint.
    - simpl. apply Hpoint. lia.
    - simpl.
      apply sr_plus_proper.
      + apply IH. intros k Hk. apply Hpoint. lia.
      + apply Hpoint. lia.
  Qed.

  Lemma bounded_sum_proper : forall dim f g,
    (forall k, (k < dim)%nat -> f k ≡ g k) ->
    bounded_sum dim f ≡ bounded_sum dim g.
  Proof.
    intros dim f g Hpoint.
    destruct dim as [| max_idx].
    - simpl. apply sr_eq_refl.
    - simpl. apply matrix_sum_to_proper.
      intros k Hk. apply Hpoint. lia.
  Qed.

  Lemma matrix_sum_to_zero : forall max_idx,
    matrix_sum_to (fun _ : nat => (𝟘 : W)) max_idx ≡ 𝟘.
  Proof.
    induction max_idx as [| max_idx IH].
    - simpl. apply sr_eq_refl.
    - simpl.
      eapply sr_eq_trans with (b := 𝟘 ⊕ (𝟘 : W)).
      + apply sr_plus_proper; [exact IH | apply sr_eq_refl].
      + apply sr_plus_zero_l.
  Qed.

  Lemma bounded_sum_zero : forall dim,
    bounded_sum dim (fun _ : nat => (𝟘 : W)) ≡ 𝟘.
  Proof.
    intro dim.
    destruct dim as [| max_idx].
    - simpl. apply sr_eq_refl.
    - simpl. apply matrix_sum_to_zero.
  Qed.

  Lemma sr_plus_interchange : forall a b c d : W,
    (a ⊕ b) ⊕ (c ⊕ d) ≡ (a ⊕ c) ⊕ (b ⊕ d).
  Proof.
    intros a b c d.
    eapply sr_eq_trans.
    - apply sr_plus_assoc.
    - eapply sr_eq_trans.
      + apply sr_plus_proper.
        * apply sr_eq_refl.
        * apply sr_eq_sym. apply sr_plus_assoc.
      + eapply sr_eq_trans.
        * apply sr_plus_proper.
          -- apply sr_eq_refl.
          -- apply sr_plus_proper.
             ++ apply sr_plus_comm.
             ++ apply sr_eq_refl.
        * eapply sr_eq_trans.
          -- apply sr_plus_proper.
             ++ apply sr_eq_refl.
             ++ apply sr_plus_assoc.
          -- apply sr_eq_sym. apply sr_plus_assoc.
  Qed.

  Lemma matrix_sum_to_plus : forall max_idx f g,
    matrix_sum_to (fun k => f k ⊕ g k) max_idx ≡
    matrix_sum_to f max_idx ⊕ matrix_sum_to g max_idx.
  Proof.
    intros max_idx f g.
    induction max_idx as [| max_idx IH].
    - simpl. apply sr_eq_refl.
    - simpl.
      eapply sr_eq_trans.
      + apply sr_plus_proper; [exact IH | apply sr_eq_refl].
      + apply sr_plus_interchange.
  Qed.

  Lemma bounded_sum_plus : forall dim f g,
    bounded_sum dim (fun k => f k ⊕ g k) ≡
    bounded_sum dim f ⊕ bounded_sum dim g.
  Proof.
    intros dim f g.
    destruct dim as [| max_idx].
    - simpl. apply sr_eq_sym. apply sr_plus_zero_l.
    - simpl. apply matrix_sum_to_plus.
  Qed.

  Definition matrix_times (dim : nat) (a b : Matrix W) : Matrix W :=
    fun i j => bounded_sum dim (fun k => a i k ⊗ b k j).

  #[global]
  Instance matrix_times_Proper : forall dim,
    Proper (matrix_eq ==> matrix_eq ==> matrix_eq) (matrix_times dim).
  Proof.
    intros dim a a' Ha b b' Hb i j.
    unfold matrix_times.
    apply bounded_sum_proper.
    intros k _.
    apply sr_times_proper; [apply Ha | apply Hb].
  Qed.

  Lemma matrix_times_zero_l : forall dim a,
    matrix_eq (matrix_times dim matrix_zero a) matrix_zero.
  Proof.
    intros dim a i j.
    unfold matrix_times, matrix_zero.
    eapply sr_eq_trans.
    - apply bounded_sum_proper.
      intros k _. apply sr_zero_times_l.
    - apply bounded_sum_zero.
  Qed.

  Lemma matrix_times_zero_r : forall dim a,
    matrix_eq (matrix_times dim a matrix_zero) matrix_zero.
  Proof.
    intros dim a i j.
    unfold matrix_times, matrix_zero.
    eapply sr_eq_trans.
    - apply bounded_sum_proper.
      intros k _. apply sr_zero_times_r.
    - apply bounded_sum_zero.
  Qed.

  Fixpoint matrix_power (dim : nat) (a : Matrix W) (n : nat) : Matrix W :=
    match n with
    | O => matrix_identity
    | S m => matrix_times dim a (matrix_power dim a m)
    end.

  Fixpoint matrix_partial_star (dim : nat) (a : Matrix W) (n : nat)
      : Matrix W :=
    match n with
    | O => matrix_identity
    | S m =>
        matrix_plus matrix_identity
          (matrix_times dim a (matrix_partial_star dim a m))
    end.

  Fixpoint matrix_walk_sum
      (dim : nat) (a : Matrix W) (bound source target : nat) : W :=
    match bound with
    | O => matrix_identity source target
    | S m =>
        matrix_identity source target ⊕
        bounded_sum dim
          (fun next =>
             a source next ⊗ matrix_walk_sum dim a m next target)
    end.

  Definition matrix_stabilizes_at
      (dim : nat) (a : Matrix W) (k : nat) : Prop :=
    matrix_eq
      (matrix_partial_star dim a (S k))
      (matrix_partial_star dim a k).

  Definition matrix_star_solution
      (dim : nat) (a astar : Matrix W) : Prop :=
    matrix_eq astar
      (matrix_plus matrix_identity (matrix_times dim a astar)).

  Lemma matrix_power_zero : forall dim a,
    matrix_eq (matrix_power dim a 0) matrix_identity.
  Proof.
    intros dim a i j. simpl. apply sr_eq_refl.
  Qed.

  Lemma matrix_partial_star_zero : forall dim a,
    matrix_eq (matrix_partial_star dim a 0) matrix_identity.
  Proof.
    intros dim a i j. simpl. apply sr_eq_refl.
  Qed.

  Lemma matrix_partial_star_unfold : forall dim a n,
    matrix_eq
      (matrix_partial_star dim a (S n))
      (matrix_plus matrix_identity
        (matrix_times dim a (matrix_partial_star dim a n))).
  Proof.
    intros dim a n i j. simpl. apply sr_eq_refl.
  Qed.

  Lemma matrix_walk_sum_zero : forall dim a source target,
    matrix_walk_sum dim a 0 source target ≡
    matrix_identity source target.
  Proof.
    intros dim a source target. simpl. apply sr_eq_refl.
  Qed.

  Lemma matrix_walk_sum_unfold : forall dim a n source target,
    matrix_walk_sum dim a (S n) source target ≡
      matrix_identity source target ⊕
      bounded_sum dim
        (fun next =>
           a source next ⊗ matrix_walk_sum dim a n next target).
  Proof.
    intros dim a n source target. simpl. apply sr_eq_refl.
  Qed.

  Lemma matrix_partial_star_walk_sum : forall dim a bound,
    matrix_eq
      (matrix_partial_star dim a bound)
      (fun source target => matrix_walk_sum dim a bound source target).
  Proof.
    intros dim a bound.
    induction bound as [| bound IH].
    - intros source target. simpl. apply sr_eq_refl.
    - intros source target.
      simpl.
      unfold matrix_plus, matrix_times.
      apply sr_plus_proper.
      + apply sr_eq_refl.
      + apply bounded_sum_proper.
        intros next _.
        apply sr_times_proper.
        * apply sr_eq_refl.
        * apply IH.
  Qed.

  Lemma matrix_stabilizes_star_solution : forall dim a k,
    matrix_stabilizes_at dim a k ->
    matrix_star_solution dim a (matrix_partial_star dim a k).
  Proof.
    intros dim a k Hstable.
    unfold matrix_star_solution, matrix_stabilizes_at in *.
    intros i j.
    apply sr_eq_sym.
    apply Hstable.
  Qed.

  Lemma matrix_star_solution_unfold : forall dim a astar,
    matrix_star_solution dim a astar ->
    matrix_eq astar
      (matrix_plus matrix_identity (matrix_times dim a astar)).
  Proof.
    intros dim a astar Hsolution.
    exact Hsolution.
  Qed.

End MatrixClosure.
