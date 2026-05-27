(** * Weighted Languages

    Definitions for the weighted language recognized by a WFST.

    The weighted language L(A) of a WFST A assigns a weight to each
    input/output string pair, computed as the semiring sum of all
    accepting path weights.
*)

Require Import Coq.Lists.List.
Require Import Coq.Arith.Arith.
Require Import Coq.micromega.Lia.
Require Import LlingLlang.foundations.Semiring.
Require Import LlingLlang.wfst.Definitions.
Require Import LlingLlang.wfst.Paths.
Require Import LlingLlang.wfst.MatrixSemantics.

Import ListNotations.

(** ** String Type *)

Section Language.
  Context {W : Type} `{Semiring W}.

  (** A string is a list of labels (None represents epsilon) *)
  Definition LabelString := list (option Label).

  (** Remove epsilon labels from a string *)
  Fixpoint remove_epsilons (s : LabelString) : list Label :=
    match s with
    | [] => []
    | None :: rest => remove_epsilons rest
    | Some l :: rest => l :: remove_epsilons rest
    end.

  Definition append_consumed_label
      (prefix : list Label) (label : option Label) : list Label :=
    match label with
    | None => prefix
    | Some l => prefix ++ [l]
    end.

  (** ** Language Definition *)

  (** The weight of a string pair (input, output) in a WFST
      is the sum of weights of all accepting paths with those labels.

      Note: This is a specification-level definition. Computing it
      requires enumerating all paths, which may be infinite for cyclic WFSTs. *)

  (** Paths that match a given input/output string pair *)
  Definition path_matches (p : @Path W) (input output : LabelString) : Prop :=
    remove_epsilons (@path_input W p) = remove_epsilons input /\
    remove_epsilons (@path_output W p) = remove_epsilons output.

  (** Final-state contribution for an accepting path. *)
  Definition path_final_weight (fst : Wfst W) (p : @Path W) : option W :=
    match p with
    | [] => final_weight fst (wfst_start fst)
    | t :: _ =>
        match last p t with
        | t_last => final_weight fst (tr_to t_last)
        end
    end.

  (** Full accepting-path weight, including the final weight. *)
  Definition accepting_path_weight (fst : Wfst W) (p : @Path W) : W :=
    match path_final_weight fst p with
    | Some fw => path_weight p ⊗ fw
    | None => 𝟘
    end.

  Definition path_final_weight_or_zero (fst : Wfst W) (p : @Path W) : W :=
    match path_final_weight fst p with
    | Some fw => fw
    | None => 𝟘
    end.

	  (** ** Language Equivalence *)

  Definition PathSet := list (@Path W).

  (** Sum the full weights of a finite set/list of accepting paths. *)
  Definition language_weight_over (fst : Wfst W) (paths : PathSet) : W :=
    fold_right (fun p acc => accepting_path_weight fst p ⊕ acc) 𝟘 paths.

  (** A finite path set is exact for an input/output pair when it contains
      precisely the matching accepting paths, with no duplicates.  This is the
      finite/acyclic language surface; cyclic language semantics need an
      explicit closure/star model rather than pretending TLC-style enumeration
      is finite. *)
  Definition exact_matching_accepting_paths
      (fst : Wfst W) (input output : LabelString) (paths : PathSet) : Prop :=
    NoDup paths /\
    forall p : @Path W,
      In p paths <-> accepting_path fst p /\ path_matches p input output.

  Lemma exact_matching_accepting_paths_sound :
    forall fst input output paths p,
      exact_matching_accepting_paths fst input output paths ->
      In p paths ->
      accepting_path fst p /\ path_matches p input output.
  Proof.
    intros fst input output paths p [_ Hexact] Hin.
    apply Hexact. exact Hin.
  Qed.

  Lemma exact_matching_accepting_paths_complete :
    forall fst input output paths p,
      exact_matching_accepting_paths fst input output paths ->
      accepting_path fst p /\ path_matches p input output ->
      In p paths.
  Proof.
    intros fst input output paths p [_ Hexact] Hmatch.
    apply Hexact. exact Hmatch.
  Qed.

  (** Weighted language relation for finite path enumerations. *)
  Definition language_weight
      (fst : Wfst W) (input output : LabelString) (weight : W) : Prop :=
    wfst_well_formed fst /\
    exists paths : PathSet,
      exact_matching_accepting_paths fst input output paths /\
      weight ≡ language_weight_over fst paths.

  (** Bounded approximations for cyclic WFSTs.  These are finite by
      construction and can be used as a checked closure surface when the
      aggregate stabilizes after some path-length bound. *)
  Definition exact_bounded_matching_accepting_paths
      (fst : Wfst W) (input output : LabelString) (max_len : nat)
      (paths : PathSet) : Prop :=
    NoDup paths /\
    forall p : @Path W,
      In p paths <->
      accepting_path fst p /\ path_matches p input output /\ length p <= max_len.

  Definition bounded_language_weight
      (fst : Wfst W) (input output : LabelString) (max_len : nat)
      (weight : W) : Prop :=
    wfst_well_formed fst /\
    exists paths : PathSet,
      exact_bounded_matching_accepting_paths fst input output max_len paths /\
      weight ≡ language_weight_over fst paths.

  Definition stable_bounded_language_weight
      (fst : Wfst W) (input output : LabelString) (max_len : nat)
      (weight : W) : Prop :=
    bounded_language_weight fst input output max_len weight /\
    forall larger_len : nat,
      max_len <= larger_len ->
      bounded_language_weight fst input output larger_len weight.

  Definition closed_language_weight
      (fst : Wfst W) (input output : LabelString) (weight : W) : Prop :=
    exists max_len : nat,
      stable_bounded_language_weight fst input output max_len weight.

  (** The state-adjacency matrix closure directly models epsilon-only cyclic
      closure: since no labels are consumed, the original WFST state space is
      enough.  For arbitrary strings, the product matrix below tracks WFST
      state together with input/output positions. *)
  Definition epsilon_transition_filter (t : Transition W) : bool :=
    match tr_input t, tr_output t with
    | None, None => true
    | _, _ => false
    end.

  Definition epsilon_label_pair
      (input output : LabelString) : Prop :=
    remove_epsilons input = [] /\
    remove_epsilons output = [].

  Definition matrix_language_weight
      (fst : Wfst W) (input output : LabelString) (weight : W) : Prop :=
    epsilon_label_pair input output /\
    exists bound : nat,
      wfst_matrix_stabilizes_at fst epsilon_transition_filter bound /\
      matrix_closed_path_weight fst epsilon_transition_filter bound weight.

  Definition product_matrix_language_weight
      (fst : Wfst W) (input output : LabelString) (weight : W) : Prop :=
    exists bound : nat,
      wfst_product_matrix_stabilizes_at
        fst (remove_epsilons input) (remove_epsilons output) bound /\
      product_matrix_closed_path_weight
        fst (remove_epsilons input) (remove_epsilons output) bound weight.

  (** Language facts used by algorithm specs.  A weighted-language fact may
      come from an exact finite enumeration, a stable bounded closure witness,
      an epsilon-closure matrix witness, or a label-position product-matrix
      witness. *)
  Definition weighted_language_weight
      (fst : Wfst W) (input output : LabelString) (weight : W) : Prop :=
    language_weight fst input output weight \/
    closed_language_weight fst input output weight \/
    matrix_language_weight fst input output weight \/
    product_matrix_language_weight fst input output weight.

  (** Algorithm-level language equivalence is intentionally not a bare
      iff over possibly undefined finite enumerations.  Requiring a witness for
      every input/output pair prevents cyclic languages outside the finite or
      stable-closure surface from making two machines equivalent vacuously. *)
  Definition weighted_language_defined (fst : Wfst W) : Prop :=
    forall input output : LabelString,
      exists weight : W, weighted_language_weight fst input output weight.

  Lemma exact_bounded_matching_accepting_paths_sound :
    forall fst input output max_len paths p,
      exact_bounded_matching_accepting_paths fst input output max_len paths ->
      In p paths ->
      accepting_path fst p /\ path_matches p input output /\ length p <= max_len.
  Proof.
    intros fst input output max_len paths p [_ Hexact] Hin.
    apply Hexact. exact Hin.
  Qed.

  Lemma exact_bounded_matching_accepting_paths_complete :
    forall fst input output max_len paths p,
      exact_bounded_matching_accepting_paths fst input output max_len paths ->
      accepting_path fst p /\ path_matches p input output /\ length p <= max_len ->
      In p paths.
  Proof.
    intros fst input output max_len paths p [_ Hexact] Hmatch.
    apply Hexact. exact Hmatch.
  Qed.

  Lemma language_weight_over_empty : forall fst,
    language_weight_over fst [] ≡ (𝟘 : W).
  Proof.
    intro fst. unfold language_weight_over. simpl. apply sr_eq_refl.
  Qed.

  Lemma nth_error_app_singleton_length :
    forall (prefix : list Label) label rest,
      nth_error (prefix ++ label :: rest) (length prefix) = Some label.
  Proof.
    induction prefix as [| x prefix IH]; intros label rest.
    - reflexivity.
    - simpl. apply IH.
  Qed.

  Lemma append_consumed_label_app_remove_epsilons :
    forall prefix label rest,
      prefix ++ remove_epsilons (label :: rest) =
      append_consumed_label prefix label ++ remove_epsilons rest.
  Proof.
    intros prefix label rest.
    destruct label as [label |].
    - unfold append_consumed_label. simpl.
      rewrite <- app_assoc. reflexivity.
    - unfold append_consumed_label. simpl.
      reflexivity.
  Qed.

  Lemma consume_label_append_consumed_label :
    forall prefix label rest,
      consume_label label
        (append_consumed_label prefix label ++ rest)
        (length prefix) =
      Some (length (append_consumed_label prefix label)).
  Proof.
    intros prefix label rest.
    destruct label as [label |].
    - unfold append_consumed_label, consume_label.
      replace ((prefix ++ [label]) ++ rest)
        with (prefix ++ label :: rest) by
        (rewrite <- app_assoc; reflexivity).
      rewrite nth_error_app_singleton_length.
      rewrite Nat.eqb_refl.
      rewrite length_app. simpl. f_equal. lia.
    - unfold append_consumed_label, consume_label.
      reflexivity.
  Qed.

  Lemma consume_path_labels_remove_epsilons_from :
    forall path input_prefix output_prefix,
      consume_path_labels
        (input_prefix ++ remove_epsilons (@path_input W path))
        (output_prefix ++ remove_epsilons (@path_output W path))
        path
        (length input_prefix)
        (length output_prefix) =
      Some
        (length (input_prefix ++ remove_epsilons (@path_input W path)),
         length (output_prefix ++ remove_epsilons (@path_output W path))).
  Proof.
    induction path as [| t rest IH]; intros input_prefix output_prefix.
    - cbn [path_input path_output remove_epsilons consume_path_labels].
      repeat rewrite app_nil_r. reflexivity.
    - replace (input_prefix ++ remove_epsilons (@path_input W (t :: rest)))
        with
        (append_consumed_label input_prefix (tr_input t) ++
          remove_epsilons (@path_input W rest))
        by (symmetry; cbn [path_input remove_epsilons];
            apply append_consumed_label_app_remove_epsilons).
      replace (output_prefix ++ remove_epsilons (@path_output W (t :: rest)))
        with
        (append_consumed_label output_prefix (tr_output t) ++
          remove_epsilons (@path_output W rest))
        by (symmetry; cbn [path_output remove_epsilons];
            apply append_consumed_label_app_remove_epsilons).
      cbn [consume_path_labels].
      rewrite consume_label_append_consumed_label.
      rewrite consume_label_append_consumed_label.
      apply IH.
  Qed.

  Lemma consume_path_labels_path_matches :
    forall path input output,
      path_matches path input output ->
      consume_path_labels
        (remove_epsilons input)
        (remove_epsilons output)
        path 0 0 =
      Some (length (remove_epsilons input), length (remove_epsilons output)).
  Proof.
    intros path input output [Hinput Houtput].
    pose proof
      (consume_path_labels_remove_epsilons_from path [] []) as Hconsume.
    simpl in Hconsume.
    rewrite <- Hinput.
    rewrite <- Houtput.
    exact Hconsume.
  Qed.

  Lemma accepting_path_matches_product_matrix_walk :
    forall fst input output path,
      accepting_path fst path ->
      path_matches path input output ->
      product_matrix_walk
        fst (remove_epsilons input) (remove_epsilons output)
        (product_start_index
          fst (remove_epsilons input) (remove_epsilons output))
        path
        (product_index
          (remove_epsilons input)
          (remove_epsilons output)
          (path_end_state_from (wfst_start fst) path)
          (length (remove_epsilons input))
          (length (remove_epsilons output))).
  Proof.
    intros fst input output path Haccepts Hmatches.
    apply accepting_path_product_matrix_walk.
    - exact Haccepts.
    - apply consume_path_labels_path_matches.
      exact Hmatches.
  Qed.

  Definition occurrence_matching_accepting_path
      (fst : Wfst W) (input output : LabelString)
      (occs : OccurrencePath) : Prop :=
    Forall (transition_occurrence_in_wfst fst) occs /\
    accepting_path fst (occurrence_path_transitions occs) /\
    path_matches (occurrence_path_transitions occs) input output.

  Definition occurrence_accepting_path_weight
      (fst : Wfst W) (occs : OccurrencePath) : W :=
    accepting_path_weight fst (occurrence_path_transitions occs).

  Lemma matching_accepting_path_has_occurrence_path :
    forall fst input output path,
      accepting_path fst path ->
      path_matches path input output ->
      exists occs,
        occurrence_path_transitions occs = path /\
        occurrence_matching_accepting_path fst input output occs /\
        occurrence_accepting_path_weight fst occs ≡
          accepting_path_weight fst path.
  Proof.
    intros fst input output path Haccepts Hmatches.
    destruct (accepting_path_has_occurrence_path fst path Haccepts)
      as [occs [Hproject Hocc]].
    exists occs.
    split; [exact Hproject |].
    split.
    - unfold occurrence_matching_accepting_path.
      rewrite Hproject.
      split; [exact Hocc | split; assumption].
    - unfold occurrence_accepting_path_weight.
      rewrite Hproject.
      apply sr_eq_refl.
  Qed.

  Lemma path_final_weight_end_state_from :
    forall fst path,
      path_final_weight fst path =
      final_weight fst (path_end_state_from (wfst_start fst) path).
  Proof.
    intros fst path.
    destruct path as [| t rest].
    - reflexivity.
    - cbn [path_final_weight].
      rewrite path_end_state_from_cons_last with (default := t).
      reflexivity.
  Qed.

  Lemma path_final_weight_or_zero_end_state :
    forall fst path,
      path_final_weight_or_zero fst path ≡
      final_weight_or_zero fst
        (path_end_state_from (wfst_start fst) path).
  Proof.
    intros fst path.
    unfold path_final_weight_or_zero, final_weight_or_zero.
    rewrite path_final_weight_end_state_from.
    destruct (final_weight fst (path_end_state_from (wfst_start fst) path));
      apply sr_eq_refl.
  Qed.

  Lemma accepting_path_weight_factor : forall fst path,
    accepting_path_weight fst path ≡
      path_weight path ⊗ path_final_weight_or_zero fst path.
  Proof.
    intros fst path.
    unfold accepting_path_weight, path_final_weight_or_zero.
    destruct (path_final_weight fst path).
    - apply sr_eq_refl.
    - apply sr_eq_sym. apply sr_zero_times_r.
  Qed.

  Lemma product_final_weight_or_zero_path_endpoint :
    forall fst input output path,
      product_final_weight_or_zero
        fst (remove_epsilons input) (remove_epsilons output)
        (product_index
          (remove_epsilons input)
          (remove_epsilons output)
          (path_end_state_from (wfst_start fst) path)
          (length (remove_epsilons input))
          (length (remove_epsilons output))) ≡
      path_final_weight_or_zero fst path.
  Proof.
    intros fst input output path.
    eapply sr_eq_trans.
    - apply product_final_weight_or_zero_index.
    - apply sr_eq_sym.
      apply path_final_weight_or_zero_end_state.
  Qed.

  Lemma language_weight_over_cons : forall fst p paths,
    language_weight_over fst (p :: paths) ≡
    accepting_path_weight fst p ⊕ language_weight_over fst paths.
  Proof.
    intros fst p paths. unfold language_weight_over. simpl. apply sr_eq_refl.
  Qed.

  Lemma language_weight_over_app : forall fst paths1 paths2,
    language_weight_over fst (paths1 ++ paths2) ≡
    language_weight_over fst paths1 ⊕ language_weight_over fst paths2.
  Proof.
    intros fst paths1 paths2.
    induction paths1 as [| p rest IH].
    - simpl. apply sr_eq_sym. apply sr_plus_zero_l.
    - simpl.
      eapply sr_eq_trans with
        (b := accepting_path_weight fst p ⊕
              (language_weight_over fst rest ⊕
               language_weight_over fst paths2)).
      + apply sr_plus_proper; [apply sr_eq_refl | exact IH].
      + apply sr_eq_sym. apply sr_plus_assoc.
  Qed.

  Lemma language_weight_exact : forall fst input output paths,
    wfst_well_formed fst ->
    exact_matching_accepting_paths fst input output paths ->
    language_weight fst input output (language_weight_over fst paths).
  Proof.
    intros fst input output paths Hwf Hexact.
    split; [exact Hwf |].
    exists paths. split; [exact Hexact | apply sr_eq_refl].
  Qed.

  Lemma language_weight_respects_sr_eq : forall fst input output w1 w2,
    w1 ≡ w2 ->
    language_weight fst input output w1 ->
    language_weight fst input output w2.
  Proof.
    intros fst input output w1 w2 Heq [Hwf [paths [Hexact Hweight]]].
    split; [exact Hwf |].
    exists paths. split; [exact Hexact |].
    eapply sr_eq_trans.
    - apply sr_eq_sym. exact Heq.
    - exact Hweight.
  Qed.

  Lemma bounded_language_weight_exact : forall fst input output max_len paths,
    wfst_well_formed fst ->
    exact_bounded_matching_accepting_paths fst input output max_len paths ->
    bounded_language_weight fst input output max_len
      (language_weight_over fst paths).
  Proof.
    intros fst input output max_len paths Hwf Hexact.
    split; [exact Hwf |].
    exists paths. split; [exact Hexact | apply sr_eq_refl].
  Qed.

  Lemma bounded_language_weight_respects_sr_eq :
    forall fst input output max_len w1 w2,
      w1 ≡ w2 ->
      bounded_language_weight fst input output max_len w1 ->
      bounded_language_weight fst input output max_len w2.
  Proof.
    intros fst input output max_len w1 w2 Heq [Hwf [paths [Hexact Hweight]]].
    split; [exact Hwf |].
    exists paths. split; [exact Hexact |].
    eapply sr_eq_trans.
    - apply sr_eq_sym. exact Heq.
    - exact Hweight.
  Qed.

  Lemma stable_bounded_language_weight_bounded :
    forall fst input output max_len weight,
      stable_bounded_language_weight fst input output max_len weight ->
      bounded_language_weight fst input output max_len weight.
  Proof.
    intros fst input output max_len weight [Hbounded _]. exact Hbounded.
  Qed.

  Lemma stable_bounded_language_weight_closed :
    forall fst input output max_len weight,
      stable_bounded_language_weight fst input output max_len weight ->
      closed_language_weight fst input output weight.
  Proof.
    intros fst input output max_len weight Hstable.
    exists max_len. exact Hstable.
  Qed.

  Lemma exact_matching_paths_bounded_when_all_lengths_le :
    forall fst input output paths max_len,
      exact_matching_accepting_paths fst input output paths ->
      (forall p : @Path W,
         accepting_path fst p ->
         path_matches p input output ->
         length p <= max_len) ->
      exact_bounded_matching_accepting_paths
        fst input output max_len paths.
  Proof.
    intros fst input output paths max_len [Hnodup Hexact] Hbound.
    split.
    - exact Hnodup.
    - intro p. split.
      + intro Hin.
        apply Hexact in Hin.
        destruct Hin as [Hacc Hmatch].
        split; [exact Hacc | split; [exact Hmatch |]].
        apply Hbound; assumption.
      + intros [Hacc [Hmatch _]].
        apply Hexact. split; assumption.
  Qed.

  Lemma finite_language_weight_stable_from_length_bound :
    forall fst input output paths max_len,
      wfst_well_formed fst ->
      exact_matching_accepting_paths fst input output paths ->
      (forall p : @Path W,
         accepting_path fst p ->
         path_matches p input output ->
         length p <= max_len) ->
      stable_bounded_language_weight fst input output max_len
        (language_weight_over fst paths).
  Proof.
    intros fst input output paths max_len Hwf Hexact Hbound.
    split.
    - apply bounded_language_weight_exact.
      + exact Hwf.
      + eapply exact_matching_paths_bounded_when_all_lengths_le; eauto.
    - intros larger_len Hle.
      apply bounded_language_weight_exact.
      + exact Hwf.
      + eapply exact_matching_paths_bounded_when_all_lengths_le.
        * exact Hexact.
        * intros p Hacc Hmatch.
          eapply Nat.le_trans.
          -- apply Hbound; assumption.
          -- exact Hle.
  Qed.

  Lemma matrix_language_weight_intro : forall fst input output bound weight,
    epsilon_label_pair input output ->
    wfst_matrix_stabilizes_at fst epsilon_transition_filter bound ->
    matrix_closed_path_weight fst epsilon_transition_filter bound weight ->
    matrix_language_weight fst input output weight.
  Proof.
    intros fst input output bound weight Heps Hstable Hmatrix.
    split.
    - exact Heps.
    - exists bound. split; assumption.
  Qed.

  Lemma matrix_language_weight_star_solution :
    forall fst input output weight,
      matrix_language_weight fst input output weight ->
      exists closure,
        wfst_matrix_star_solution fst epsilon_transition_filter closure.
  Proof.
    intros fst input output weight [_ [bound [Hstable _]]].
    exists (wfst_matrix_closure fst epsilon_transition_filter bound).
    apply wfst_matrix_stabilizes_star_solution.
    exact Hstable.
  Qed.

  Lemma matrix_language_weight_respects_sr_eq :
    forall fst input output w1 w2,
      w1 ≡ w2 ->
      matrix_language_weight fst input output w1 ->
      matrix_language_weight fst input output w2.
  Proof.
    intros fst input output w1 w2 Heq [Heps [bound [Hstable Hmatrix]]].
    split.
    - exact Heps.
    - exists bound. split.
      + exact Hstable.
      + eapply matrix_closed_path_weight_respects_sr_eq; eauto.
  Qed.

  Lemma weighted_language_weight_matrix : forall fst input output weight,
    matrix_language_weight fst input output weight ->
    weighted_language_weight fst input output weight.
  Proof.
    intros fst input output weight Hmatrix.
    right. right. left. exact Hmatrix.
  Qed.

  Lemma product_matrix_language_weight_intro :
    forall fst input output bound weight,
      wfst_product_matrix_stabilizes_at
        fst (remove_epsilons input) (remove_epsilons output) bound ->
      product_matrix_closed_path_weight
        fst (remove_epsilons input) (remove_epsilons output) bound weight ->
      product_matrix_language_weight fst input output weight.
  Proof.
    intros fst input output bound weight Hstable Hmatrix.
    exists bound. split; assumption.
  Qed.

  Lemma product_matrix_language_weight_star_solution :
    forall fst input output weight,
      product_matrix_language_weight fst input output weight ->
      exists closure,
        wfst_product_matrix_star_solution
          fst (remove_epsilons input) (remove_epsilons output) closure.
  Proof.
    intros fst input output weight [bound [Hstable _]].
    exists (wfst_product_matrix_closure
      fst (remove_epsilons input) (remove_epsilons output) bound).
    apply wfst_product_matrix_stabilizes_star_solution.
    exact Hstable.
  Qed.

  Lemma product_matrix_language_weight_walk_sum_closed :
    forall fst input output weight,
      product_matrix_language_weight fst input output weight ->
      exists bound : nat,
        wfst_product_matrix_stabilizes_at
          fst (remove_epsilons input) (remove_epsilons output) bound /\
        product_matrix_walk_sum_closed_path_weight
          fst (remove_epsilons input) (remove_epsilons output) bound weight.
  Proof.
    intros fst input output weight [bound [Hstable Hmatrix]].
    exists bound.
    split; [exact Hstable |].
    apply product_matrix_closed_path_weight_walk_sum_equiv.
    exact Hmatrix.
  Qed.

  Lemma product_matrix_language_weight_transition_closed :
    forall fst input output weight,
      product_matrix_language_weight fst input output weight ->
      exists bound : nat,
        wfst_product_matrix_stabilizes_at
          fst (remove_epsilons input) (remove_epsilons output) bound /\
        product_transition_walk_sum_closed_path_weight
          fst (remove_epsilons input) (remove_epsilons output) bound weight.
  Proof.
    intros fst input output weight [bound [Hstable Hmatrix]].
    exists bound.
    split; [exact Hstable |].
    apply product_matrix_closed_path_weight_transition_equiv.
    exact Hmatrix.
  Qed.

  Lemma product_matrix_language_weight_occurrence_closed :
    forall fst input output weight,
      product_matrix_language_weight fst input output weight ->
      exists bound : nat,
        wfst_product_matrix_stabilizes_at
          fst (remove_epsilons input) (remove_epsilons output) bound /\
        product_occurrence_walk_sum_closed_path_weight
          fst (remove_epsilons input) (remove_epsilons output) bound weight.
  Proof.
    intros fst input output weight [bound [Hstable Hmatrix]].
    exists bound.
    split; [exact Hstable |].
    apply product_matrix_closed_path_weight_occurrence_equiv.
    exact Hmatrix.
  Qed.

  Lemma product_matrix_language_weight_occurrence_enumerator :
    forall fst input output weight,
      product_matrix_language_weight fst input output weight ->
      exists bound : nat,
        wfst_product_matrix_stabilizes_at
          fst (remove_epsilons input) (remove_epsilons output) bound /\
        product_occurrence_enumerator_closed_path_weight
          fst (remove_epsilons input) (remove_epsilons output) bound weight.
  Proof.
    intros fst input output weight [bound [Hstable Hmatrix]].
    exists bound.
    split; [exact Hstable |].
    apply product_matrix_closed_path_weight_occurrence_enumerator_equiv.
    exact Hmatrix.
  Qed.

  Lemma product_matrix_language_weight_respects_sr_eq :
    forall fst input output w1 w2,
      w1 ≡ w2 ->
      product_matrix_language_weight fst input output w1 ->
      product_matrix_language_weight fst input output w2.
  Proof.
    intros fst input output w1 w2 Heq [bound [Hstable Hmatrix]].
    exists bound. split.
    - exact Hstable.
    - eapply product_matrix_closed_path_weight_respects_sr_eq; eauto.
  Qed.

  Lemma weighted_language_weight_product_matrix :
    forall fst input output weight,
      product_matrix_language_weight fst input output weight ->
      weighted_language_weight fst input output weight.
  Proof.
    intros fst input output weight Hmatrix.
    right. right. right. exact Hmatrix.
  Qed.

  (** One-way path simulation over accepting paths.  This is useful for some
      structural arguments, but it is not weighted-language equivalence for
      non-idempotent semirings because duplicate paths contribute additional
      weight to the aggregate language. *)
  Definition path_language_simulates (fst1 fst2 : Wfst W) : Prop :=
    forall input output : LabelString,
      forall p1 : @Path W,
        accepting_path fst1 p1 -> path_matches p1 input output ->
        exists p2 : @Path W,
          accepting_path fst2 p2 /\
	          path_matches p2 input output /\
	          accepting_path_weight fst1 p1 ≡ accepting_path_weight fst2 p2.

  Definition path_language_equiv (fst1 fst2 : Wfst W) : Prop :=
    path_language_simulates fst1 fst2 /\ path_language_simulates fst2 fst1.

  (** Two WFSTs are language-equivalent when both have a defined weighted
      language surface and assign the same relation to every input/output
      string pair. *)
  Definition language_equiv (fst1 fst2 : Wfst W) : Prop :=
    weighted_language_defined fst1 /\
    weighted_language_defined fst2 /\
    forall input output weight,
      weighted_language_weight fst1 input output weight <->
      weighted_language_weight fst2 input output weight.

  (** Language equivalence is reflexive *)
  Lemma language_equiv_refl : forall fst : Wfst W,
    weighted_language_defined fst ->
    language_equiv fst fst.
  Proof.
    unfold language_equiv.
    intros fst Hdefined.
    split; [exact Hdefined |].
    split; [exact Hdefined |].
    intros input output weight. split; intro Hlang; exact Hlang.
  Qed.

  (** Language equivalence is symmetric *)
  Lemma language_equiv_sym : forall fst1 fst2 : Wfst W,
    language_equiv fst1 fst2 -> language_equiv fst2 fst1.
  Proof.
    unfold language_equiv.
    intros fst1 fst2 [Hdef1 [Hdef2 Heq]].
    split; [exact Hdef2 |].
    split; [exact Hdef1 |].
    intros input output weight.
    specialize (Heq input output weight).
    split; intro Hlang; apply Heq; exact Hlang.
  Qed.

  (** Language equivalence is transitive *)
  Lemma language_equiv_trans : forall fst1 fst2 fst3 : Wfst W,
    language_equiv fst1 fst2 -> language_equiv fst2 fst3 -> language_equiv fst1 fst3.
  Proof.
    unfold language_equiv.
    intros fst1 fst2 fst3 [Hdef1 [_ H12]] [_ [Hdef3 H23]].
    split; [exact Hdef1 |].
    split; [exact Hdef3 |].
    intros input output weight.
    specialize (H12 input output weight).
    specialize (H23 input output weight).
    split; intro Hlang.
    - apply H23. apply H12. exact Hlang.
    - apply H12. apply H23. exact Hlang.
  Qed.

  Lemma path_language_equiv_refl : forall fst : Wfst W,
    path_language_equiv fst fst.
  Proof.
    unfold path_language_equiv, path_language_simulates.
    intro fst. split.
    - intros input output p Hacc Hmatch.
      exists p. split; [exact Hacc | split; [exact Hmatch | apply sr_eq_refl]].
    - intros input output p Hacc Hmatch.
      exists p. split; [exact Hacc | split; [exact Hmatch | apply sr_eq_refl]].
  Qed.

  (** ** Acceptor Language *)

  (** For acceptors (input = output), language simplifies to weighted strings *)
  Definition acceptor_language (fst : Wfst W) (s : LabelString) : Prop :=
    exists p : @Path W,
      accepting_path fst p /\
      path_matches p s s.

  (** ** Prop-level independent characterization (label transduction)

      The reverse of [consume_path_labels_path_matches]: a successful
      label-consumption forces the path's epsilon-collapsed labels to equal the
      strings.  [path_matches] uses neither [consume_label] nor
      [product_transition_matches], so this grounds the product label semantics
      in a genuinely independent oracle. *)

  Lemma skipn_nth_error_cons :
    forall (A : Type) (l : list A) n x,
      nth_error l n = Some x -> skipn n l = x :: skipn (S n) l.
  Proof.
    induction l as [| a l IH]; intros n x Hnth.
    - destruct n; simpl in Hnth; discriminate.
    - destruct n as [| n'].
      + simpl in Hnth. injection Hnth as Hax. subst a. reflexivity.
      + simpl in Hnth. simpl. apply IH. exact Hnth.
  Qed.

  (** H2: prefix-threaded converse of consumption — the consumed segment of each
      string equals the path's epsilon-collapsed labels. *)
  Lemma consume_path_labels_some_prefix :
    forall (p : @Path W) full_input full_output ip op fi fo,
      consume_path_labels full_input full_output p ip op = Some (fi, fo) ->
      firstn (fi - ip) (skipn ip full_input) = remove_epsilons (@path_input W p) /\
      firstn (fo - op) (skipn op full_output) = remove_epsilons (@path_output W p) /\
      ip <= fi /\ op <= fo.
  Proof.
    induction p as [| t rest IH];
      intros full_input full_output ip op fi fo Hconsume.
    - cbn [consume_path_labels] in Hconsume. injection Hconsume as Hfi Hfo.
      subst fi fo.
      cbn [path_input path_output map remove_epsilons].
      rewrite !Nat.sub_diag. cbn [firstn].
      split; [reflexivity |]. split; [reflexivity |]. split; lia.
    - cbn [consume_path_labels] in Hconsume.
      destruct (consume_label (tr_input t) full_input ip) as [ni |] eqn:Hci;
        [| discriminate].
      destruct (consume_label (tr_output t) full_output op) as [no |] eqn:Hco;
        [| discriminate].
      specialize (IH full_input full_output ni no fi fo Hconsume).
      destruct IH as [IHin [IHout [IHle1 IHle2]]].
      assert (Hinput :
        firstn (fi - ip) (skipn ip full_input)
          = remove_epsilons (@path_input W (t :: rest)) /\ ip <= fi).
      { cbn [path_input map remove_epsilons].
        destruct (tr_input t) as [li |].
        - cbn [consume_label] in Hci.
          destruct (nth_error full_input ip) as [actual |] eqn:Hnth;
            [| discriminate].
          destruct (Nat.eqb li actual) eqn:Heqb; [| discriminate].
          injection Hci as Hci. subst ni.
          apply Nat.eqb_eq in Heqb. subst actual.
          split.
          + rewrite (skipn_nth_error_cons _ full_input ip li Hnth).
            replace (fi - ip) with (S (fi - S ip)) by lia.
            cbn [firstn]. rewrite IHin. reflexivity.
          + lia.
        - cbn [consume_label] in Hci. injection Hci as Hci. subst ni.
          split.
          + exact IHin.
          + lia. }
      assert (Houtput :
        firstn (fo - op) (skipn op full_output)
          = remove_epsilons (@path_output W (t :: rest)) /\ op <= fo).
      { cbn [path_output map remove_epsilons].
        destruct (tr_output t) as [lo |].
        - cbn [consume_label] in Hco.
          destruct (nth_error full_output op) as [actual |] eqn:Hnth;
            [| discriminate].
          destruct (Nat.eqb lo actual) eqn:Heqb; [| discriminate].
          injection Hco as Hco. subst no.
          apply Nat.eqb_eq in Heqb. subst actual.
          split.
          + rewrite (skipn_nth_error_cons _ full_output op lo Hnth).
            replace (fo - op) with (S (fo - S op)) by lia.
            cbn [firstn]. rewrite IHout. reflexivity.
          + lia.
        - cbn [consume_label] in Hco. injection Hco as Hco. subst no.
          split.
          + exact IHout.
          + lia. }
      destruct Hinput as [Hinput_eq Hip_le].
      destruct Houtput as [Houtput_eq Hop_le].
      split; [exact Hinput_eq |]. split; [exact Houtput_eq |].
      split; [exact Hip_le | exact Hop_le].
  Qed.

  (** A.3: full-string consumption recovers [path_matches]. *)
  Lemma path_matches_of_consume_path_labels :
    forall (p : @Path W) input output,
      consume_path_labels (remove_epsilons input) (remove_epsilons output) p 0 0
        = Some (length (remove_epsilons input), length (remove_epsilons output)) ->
      path_matches p input output.
  Proof.
    intros p input output Hconsume.
    apply consume_path_labels_some_prefix in Hconsume.
    destruct Hconsume as [Hin [Hout _]].
    cbn [skipn] in Hin, Hout.
    rewrite Nat.sub_0_r in Hin, Hout.
    rewrite firstn_all in Hin, Hout.
    unfold path_matches. split.
    - symmetry. exact Hin.
    - symmetry. exact Hout.
  Qed.

  (** A.5 (forward inclusion): every position-accepting closed occurrence path
      landing on a final state is a genuine accepting path that transduces the
      strings.  The finality side-condition is required (D1). *)
  Lemma product_occurrence_closed_path_accepting_matching :
    forall fst input output bound tgt p,
      wfst_well_formed fst ->
      product_accepts_final_position
        (remove_epsilons input) (remove_epsilons output) tgt = true ->
      is_final fst
        (product_state (remove_epsilons input) (remove_epsilons output) tgt) = true ->
      In (tgt, p) (product_occurrence_closed_paths fst
        (remove_epsilons input) (remove_epsilons output) bound) ->
      accepting_path fst (occurrence_path_transitions p) /\
      path_matches (occurrence_path_transitions p) input output.
  Proof.
    intros fst input output bound tgt p Hwf Hpos Hisfinal Hin.
    pose proof (proj1 (product_occurrence_closed_paths_exact fst
      (remove_epsilons input) (remove_epsilons output) bound tgt p Hwf) Hin)
      as [_ [Hwalk _]].
    split.
    - apply (product_occurrence_closed_walk_accepting fst
        (remove_epsilons input) (remove_epsilons output) p tgt
        Hwf Hwalk Hpos Hisfinal).
    - pose proof (product_occurrence_closed_walk_recovers_path fst
        (remove_epsilons input) (remove_epsilons output) p tgt Hwf Hwalk Hpos)
        as [_ [_ [_ Hconsume]]].
      apply path_matches_of_consume_path_labels. exact Hconsume.
  Qed.

  (** ** Grounding the product weight in independent accepting-path weights

      The product-matrix weighted chain bottoms out in [product_transition_matches].
      The results below re-express the product weight as a sum of the independent
      [accepting_path_weight] over genuinely transducing closed paths, removing
      the self-reference on the weight axis.  Multiplicity is preserved: the sum
      ranges over the occurrence-keyed [product_occurrence_closed_paths], NOT the
      duplicate-collapsing [PathSet] of [language_weight_over]. *)

  (** B.7: per-element grounding. *)
  Lemma target_occurrence_path_weight_is_accepting_weight :
    forall fst input output target occs,
      wfst_well_formed fst ->
      product_occurrence_walk fst input output
        (product_start_index fst input output) occs target ->
      product_accepts_final_position input output target = true ->
      target_occurrence_path_weight fst input output (target, occs)
        ≡ accepting_path_weight fst (occurrence_path_transitions occs).
  Proof.
    intros fst input output target occs Hwf Hwalk Hfinal.
    pose proof (product_occurrence_closed_walk_recovers_path fst input output occs
      target Hwf Hwalk Hfinal) as [_ [_ [Hend _]]].
    cbn [target_occurrence_path_weight].
    unfold product_final_weight_or_zero. rewrite Hfinal.
    rewrite <- Hend.
    eapply sr_eq_trans with
      (b := path_weight (occurrence_path_transitions occs)
            ⊗ path_final_weight_or_zero fst (occurrence_path_transitions occs)).
    - apply sr_times_proper.
      + apply occurrence_path_weight_is_path_weight.
      + apply sr_eq_sym. apply path_final_weight_or_zero_end_state.
    - apply sr_eq_sym. apply accepting_path_weight_factor.
  Qed.

  (** The grounded sum over the occurrence-keyed closed-path list. *)
  Definition closed_accepting_path_weight_sum
      (fst : Wfst W) (input output : list Label)
      (entries : list TargetOccurrencePath) : W :=
    fold_right
      (fun (e : TargetOccurrencePath) acc =>
         match e with
         | (_, occs) => accepting_path_weight fst (occurrence_path_transitions occs)
         end ⊕ acc)
      𝟘 entries.

  (** B.8a: list-level congruence. *)
  Lemma target_occurrence_path_weight_sum_grounds :
    forall fst input output entries,
      wfst_well_formed fst ->
      Forall (fun e => match e with
                       | (tgt, occs) =>
                           product_occurrence_walk fst input output
                             (product_start_index fst input output) occs tgt
                       end) entries ->
      target_occurrence_path_weight_sum fst input output entries
      ≡ closed_accepting_path_weight_sum fst input output
          (filter (fun e => match e with
                            | (tgt, _) =>
                                product_accepts_final_position input output tgt
                            end) entries).
  Proof.
    intros fst input output entries Hwf.
    induction entries as [| e rest IH]; intros Hforall.
    - cbn. apply sr_eq_refl.
    - destruct e as [tgt occs].
      pose proof (Forall_inv Hforall) as Hhead. cbn in Hhead.
      pose proof (Forall_inv_tail Hforall) as Htail.
      cbn [target_occurrence_path_weight_sum filter].
      destruct (product_accepts_final_position input output tgt) eqn:Hfin.
      + cbn [closed_accepting_path_weight_sum].
        apply sr_plus_proper.
        * apply target_occurrence_path_weight_is_accepting_weight.
          -- exact Hwf.
          -- exact Hhead.
          -- exact Hfin.
        * apply IH. exact Htail.
      + eapply sr_eq_trans with
          (b := 𝟘 ⊕ target_occurrence_path_weight_sum fst input output rest).
        * apply sr_plus_proper.
          -- apply target_occurrence_path_weight_zero_when_not_final_position.
             exact Hfin.
          -- apply sr_eq_refl.
        * eapply sr_eq_trans with
            (b := target_occurrence_path_weight_sum fst input output rest).
          -- apply sr_plus_zero_l.
          -- apply IH. exact Htail.
  Qed.

  (** B.8 (headline): the occurrence-enumerator closed-path weight equals a sum
      of independent [accepting_path_weight]s over genuinely transducing closed
      paths. *)
  Theorem product_occurrence_enumerator_weight_is_accepting_path_sum :
    forall fst input output bound weight,
      product_occurrence_enumerator_closed_path_weight fst input output bound weight ->
      weight ≡
        closed_accepting_path_weight_sum fst input output
          (filter (fun e => match e with
                            | (tgt, _) =>
                                product_accepts_final_position input output tgt
                            end)
                  (product_occurrence_closed_paths fst input output bound)).
  Proof.
    intros fst input output bound weight Henum.
    destruct Henum as [Hwf Hweq].
    eapply sr_eq_trans; [exact Hweq |].
    apply target_occurrence_path_weight_sum_grounds.
    - exact Hwf.
    - apply Forall_forall. intros e Hin. destruct e as [tgt occs].
      pose proof
        (proj1 (product_occurrence_closed_paths_exact fst input output bound
                  tgt occs Hwf) Hin) as [_ [Hwalk _]].
      exact Hwalk.
  Qed.

  (** Headline corollary: the product-matrix language weight is a sum of
      independent accepting-path weights over genuine transducing paths. *)
  Corollary product_matrix_language_weight_is_accepting_path_sum :
    forall fst input output weight,
      product_matrix_language_weight fst input output weight ->
      exists bound,
        weight ≡
          closed_accepting_path_weight_sum fst
            (remove_epsilons input) (remove_epsilons output)
            (filter (fun e => match e with
                              | (tgt, _) =>
                                  product_accepts_final_position
                                    (remove_epsilons input)
                                    (remove_epsilons output) tgt
                              end)
                    (product_occurrence_closed_paths fst
                      (remove_epsilons input) (remove_epsilons output) bound)).
  Proof.
    intros fst input output weight Hpm.
    apply product_matrix_language_weight_occurrence_enumerator in Hpm.
    destruct Hpm as [bound [_ Henum]].
    exists bound.
    apply product_occurrence_enumerator_weight_is_accepting_path_sum.
    exact Henum.
  Qed.

End Language.

(** ** Language Operations *)

Section LanguageOperations.
  Context {W : Type} `{Semiring W}.

  (** Union of languages: L(A ∪ B)(w) = L(A)(w) ⊕ L(B)(w) *)
  (* Specification: if a string is in either language, its weight is the sum *)

  (** Concatenation of languages: L(A · B)(uv) = L(A)(u) ⊗ L(B)(v) *)
  (* Specification: weights compose for concatenated strings *)

  (** Kleene closure: the weight is the sum over n >= 0 of L(A)^n(w). *)
  (* Specification: sum over all decompositions into A-strings *)

End LanguageOperations.
