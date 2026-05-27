(** * Minimization Specification and Partial Correctness Lemmas

    This module proves residual right-language state equivalence, partition
    helper facts, and the correctness of the identity result as a minimization
    baseline for deterministic WFSTs.
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

  Definition path_starts_at_state (s : StateId) (p : @Path W) : Prop :=
    match p with
    | [] => True
    | t :: _ => tr_from t = s
    end.

  Definition path_ends_in_final_from_state
      (fst : Wfst W) (s : StateId) (p : @Path W) : Prop :=
    match p with
    | [] => is_final fst s = true
    | t :: _ =>
        match last p t with
        | t_last => is_final fst (tr_to t_last) = true
        end
    end.

  Definition path_valid_from_state
      (fst : Wfst W) (s : StateId) (p : @Path W) : Prop :=
    path_valid p /\
    path_transitions_in_wfst fst p /\
    path_starts_at_state s p /\
    path_ends_in_final_from_state fst s p.

  Definition residual_path_weight
      (fst : Wfst W) (s : StateId) (p : @Path W) : W :=
    match p with
    | [] =>
        match final_weight fst s with
        | Some fw => fw
        | None => 𝟘
        end
    | _ => accepting_path_weight fst p
    end.

  Definition state_residual_weight_over
      (fst : Wfst W) (s : StateId) (paths : list (@Path W)) : W :=
    fold_right (fun p acc => residual_path_weight fst s p ⊕ acc) 𝟘 paths.

  Definition exact_state_residual_paths
      (fst : Wfst W) (s : StateId) (input output : LabelString)
      (paths : list (@Path W)) : Prop :=
    NoDup paths /\
    forall p : @Path W,
      In p paths <->
      path_valid_from_state fst s p /\ path_matches p input output.

  Definition state_residual_weight
      (fst : Wfst W) (s : StateId)
      (input output : LabelString) (weight : W) : Prop :=
    exists paths : list (@Path W),
      exact_state_residual_paths fst s input output paths /\
      weight ≡ state_residual_weight_over fst s paths.

  Definition states_equivalent (fst : Wfst W) (s1 s2 : StateId) : Prop :=
    forall input output weight,
      state_residual_weight fst s1 input output weight <->
      state_residual_weight fst s2 input output weight.

  Lemma states_equiv_refl : forall fst s,
    states_equivalent fst s s.
  Proof.
    unfold states_equivalent.
    intros fst s input output weight. split; intro Hres; exact Hres.
  Qed.

  Lemma states_equiv_sym : forall fst s1 s2,
    states_equivalent fst s1 s2 ->
    states_equivalent fst s2 s1.
  Proof.
    unfold states_equivalent.
    intros fst s1 s2 Heq input output weight.
    specialize (Heq input output weight).
    split; intro Hres; apply Heq; exact Hres.
  Qed.

  Lemma states_equiv_trans : forall fst s1 s2 s3,
    states_equivalent fst s1 s2 ->
    states_equivalent fst s2 s3 ->
    states_equivalent fst s1 s3.
  Proof.
    unfold states_equivalent.
    intros fst s1 s2 s3 H12 H23 input output weight.
    specialize (H12 input output weight).
    specialize (H23 input output weight).
    split; intro Hres.
    - apply H23. apply H12. exact Hres.
    - apply H12. apply H23. exact Hres.
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

  Definition minimize_correct (fst min_fst : Wfst W) : Prop :=
    wfst_well_formed fst /\
    wfst_deterministic fst /\
    weighted_language_defined fst /\
    wfst_well_formed min_fst /\
    weighted_language_defined min_fst /\
    wfst_deterministic min_fst /\
    preserves_language fst min_fst /\
    wfst_num_states min_fst <= wfst_num_states fst.

  Lemma identity_minimize_correct : forall fst : Wfst W,
    wfst_well_formed fst ->
    wfst_deterministic fst ->
    weighted_language_defined fst ->
    minimize_correct fst fst.
  Proof.
    intros fst Hwf Hdet Hdefined.
    unfold minimize_correct, preserves_language.
    split.
    - exact Hwf.
    - split.
      + exact Hdet.
      + split.
        * exact Hdefined.
        * split.
          -- exact Hwf.
          -- split.
             ++ exact Hdefined.
             ++ split.
                ** exact Hdet.
                ** split.
                   --- apply language_equiv_refl. exact Hdefined.
                   --- apply le_n.
  Qed.

  Lemma identity_minimize_preserves_determinism : forall fst min_fst : Wfst W,
    min_fst = fst ->
    wfst_deterministic fst ->
    wfst_deterministic min_fst.
  Proof.
    intros fst min_fst Heq Hdet. subst. exact Hdet.
  Qed.

  Lemma identity_minimize_preserves_language : forall fst min_fst : Wfst W,
    min_fst = fst ->
    weighted_language_defined fst ->
    preserves_language fst min_fst.
  Proof.
    intros fst min_fst Heq Hdefined. subst.
    unfold preserves_language. apply language_equiv_refl. exact Hdefined.
  Qed.

End Minimization.

(** ** Weighted Minimization Preconditions *)

Section WeightedMinimization.
  Context {W : Type} `{DivisibleSemiring W}.

  Definition push_weights_spec (fst pushed : Wfst W) : Prop :=
    wfst_well_formed fst /\
    weighted_language_defined fst /\
    wfst_well_formed pushed /\
    weighted_language_defined pushed /\
    language_equiv fst pushed.

  Lemma push_weights_identity_spec : forall fst,
    wfst_well_formed fst ->
    weighted_language_defined fst ->
    push_weights_spec fst fst.
  Proof.
    intros fst Hwf Hdefined.
    unfold push_weights_spec.
    split.
    - exact Hwf.
    - split.
      + exact Hdefined.
      + split.
        * exact Hwf.
        * split.
          -- exact Hdefined.
          -- apply language_equiv_refl. exact Hdefined.
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
