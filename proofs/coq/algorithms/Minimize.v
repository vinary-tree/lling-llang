(** * Minimization Support Lemmas

    This module proves properties of the equivalence and partition helpers used
    when specifying minimization. It does not assert minimality for an
    unspecified executable minimization relation.
*)

Require Import Coq.Lists.List.
Require Import Coq.Arith.Arith.
Require Import LlingLlang.foundations.Semiring.
Require Import LlingLlang.wfst.Definitions.
Require Import LlingLlang.wfst.Paths.
Require Import LlingLlang.wfst.Language.

Import ListNotations.

(** ** State Equivalence *)

Section StateEquivalence.
  Context {W : Type} `{Semiring W}.

  Definition states_equivalent (fst : Wfst W) (s1 s2 : StateId) : Prop :=
    final_weight fst s1 = final_weight fst s2 /\
    get_outgoing fst s1 = get_outgoing fst s2.

  Lemma states_equiv_refl : forall fst s,
    states_equivalent fst s s.
  Proof.
    intros fst s. unfold states_equivalent. split; reflexivity.
  Qed.

  Lemma states_equiv_sym : forall fst s1 s2,
    states_equivalent fst s1 s2 ->
    states_equivalent fst s2 s1.
  Proof.
    intros fst s1 s2 [Hfinal Hout].
    unfold states_equivalent. split; symmetry; assumption.
  Qed.

  Lemma states_equiv_trans : forall fst s1 s2 s3,
    states_equivalent fst s1 s2 ->
    states_equivalent fst s2 s3 ->
    states_equivalent fst s1 s3.
  Proof.
    intros fst s1 s2 s3 [Hfinal12 Hout12] [Hfinal23 Hout23].
    unfold states_equivalent. split.
    - transitivity (final_weight fst s2); assumption.
    - transitivity (get_outgoing fst s2); assumption.
  Qed.

End StateEquivalence.

(** ** Partitions *)

Section Minimization.
  Context {W : Type} `{Semiring W}.

  Definition EquivClass := list StateId.
  Definition Partition := list EquivClass.

  Definition initial_partition (fst : Wfst W) : Partition :=
    let finals := filter (fun s => is_final fst s)
                         (seq 0 (wfst_num_states fst)) in
    let non_finals := filter (fun s => negb (is_final fst s))
                             (seq 0 (wfst_num_states fst)) in
    [finals; non_finals].

  Definition refine_partition (fst : Wfst W) (part : Partition) : Partition :=
    flat_map (fun block => [block]) part.

  Lemma initial_partition_has_two_blocks : forall fst,
    length (initial_partition fst) = 2.
  Proof.
    intro fst. unfold initial_partition. simpl. reflexivity.
  Qed.

  Lemma refine_partition_identity : forall fst part,
    refine_partition fst part = part.
  Proof.
    intros fst part. unfold refine_partition.
    induction part as [| block rest IH].
    - reflexivity.
    - simpl. rewrite IH. reflexivity.
  Qed.

  Definition preserves_language (fst min_fst : Wfst W) : Prop :=
    language_equiv fst min_fst.

  Lemma identity_minimize_preserves_determinism : forall fst min_fst : Wfst W,
    min_fst = fst ->
    wfst_deterministic fst ->
    wfst_deterministic min_fst.
  Proof.
    intros fst min_fst Heq Hdet. subst. exact Hdet.
  Qed.

  Lemma identity_minimize_preserves_language : forall fst min_fst : Wfst W,
    min_fst = fst ->
    preserves_language fst min_fst.
  Proof.
    intros fst min_fst Heq. subst.
    unfold preserves_language. apply language_equiv_refl.
  Qed.

End Minimization.

(** ** Weighted Minimization Preconditions *)

Section WeightedMinimization.
  Context {W : Type} `{DivisibleSemiring W}.

  Definition push_weights_spec (fst pushed : Wfst W) : Prop :=
    language_equiv fst pushed.

  Lemma push_weights_identity_spec : forall fst,
    push_weights_spec fst fst.
  Proof.
    intro fst. unfold push_weights_spec. apply language_equiv_refl.
  Qed.

End WeightedMinimization.

(** ** Idempotence for Identity Relation *)

Section MinimizationIdempotence.
  Context {W : Type} `{Semiring W}.

  Lemma identity_minimize_idempotent : forall fst min1 min2 : Wfst W,
    min1 = fst ->
    min2 = min1 ->
    wfst_num_states min1 = wfst_num_states min2.
  Proof.
    intros fst min1 min2 H1 H2. subst. reflexivity.
  Qed.

End MinimizationIdempotence.
