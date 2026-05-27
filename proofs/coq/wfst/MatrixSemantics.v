(** * Matrix Semantics for WFST Closure

    This module connects the generic semiring matrix-closure surface to WFST
    adjacency matrices.  A transition filter determines which arcs contribute
    to the adjacency matrix, allowing later language-specific constructions to
    instantiate label-aware filters without changing the closure algebra.
*)

Require Import Coq.Bool.Bool.
Require Import Coq.Lists.List.
Require Import Coq.Arith.Arith.
Require Import Coq.micromega.Lia.
Require Import LlingLlang.foundations.Semiring.
Require Import LlingLlang.foundations.MatrixClosure.
Require Import LlingLlang.wfst.Definitions.
Require Import LlingLlang.wfst.Paths.

Import ListNotations.

Section WfstMatrixSemantics.
  Context {W : Type} `{Semiring W}.

  (** A transition occurrence names a concrete entry in a state's outgoing
      list.  This is intentionally finer than [Transition] equality: two equal
      transition records stored at different list indices are distinct
      occurrences and must both contribute to non-idempotent semiring sums. *)
  Record TransitionOccurrence := mkTransitionOccurrence {
    occ_source : StateId;
    occ_index : nat;
    occ_transition : Transition W;
  }.

  Definition OccurrencePath := list TransitionOccurrence.

  Definition occurrence_path_transitions
      (p : OccurrencePath) : @Path W :=
    map occ_transition p.

  Fixpoint occurrence_path_weight (p : OccurrencePath) : W :=
    match p with
    | [] => 𝟙
    | occ :: rest =>
        tr_weight (occ_transition occ) ⊗ occurrence_path_weight rest
    end.

  Fixpoint occurrence_path_weight_sum (paths : list OccurrencePath) : W :=
    match paths with
    | [] => 𝟘
    | path :: rest =>
        occurrence_path_weight path ⊕ occurrence_path_weight_sum rest
    end.

  Definition prepend_occurrence_paths
      (occ : TransitionOccurrence) (paths : list OccurrencePath)
      : list OccurrencePath :=
    map (fun path => occ :: path) paths.

  Definition transition_occurrence_in_wfst
      (fst : Wfst W) (occ : TransitionOccurrence) : Prop :=
    occ_source occ = tr_from (occ_transition occ) /\
    nth_error (get_outgoing fst (occ_source occ)) (occ_index occ) =
      Some (occ_transition occ).

  Lemma transition_occurrence_in_wfst_transition :
    forall fst occ,
      transition_occurrence_in_wfst fst occ ->
      transition_in_wfst fst (occ_transition occ).
  Proof.
    intros fst occ [Hsource Hnth].
    unfold transition_in_wfst.
    rewrite <- Hsource.
    eapply nth_error_In.
    exact Hnth.
  Qed.

  Lemma occurrence_path_transitions_in_wfst :
    forall fst p,
      Forall (transition_occurrence_in_wfst fst) p ->
      path_transitions_in_wfst fst (occurrence_path_transitions p).
  Proof.
    intros fst p Hocc.
    induction Hocc as [| occ rest Hocc Hrest IH].
    - constructor.
    - simpl. constructor.
      + apply transition_occurrence_in_wfst_transition.
        exact Hocc.
      + exact IH.
  Qed.

  Lemma transition_in_wfst_has_occurrence :
    forall fst t,
      transition_in_wfst fst t ->
      exists occ,
        occ_transition occ = t /\
        transition_occurrence_in_wfst fst occ.
  Proof.
    intros fst t Hin.
    unfold transition_in_wfst in Hin.
    apply In_nth_error in Hin.
    destruct Hin as [idx Hnth].
    exists (mkTransitionOccurrence (tr_from t) idx t).
    split; [reflexivity |].
    split; [reflexivity | exact Hnth].
  Qed.

  Lemma path_transitions_have_occurrence_path :
    forall fst p,
      path_transitions_in_wfst fst p ->
      exists occs,
        occurrence_path_transitions occs = p /\
        Forall (transition_occurrence_in_wfst fst) occs.
  Proof.
    intros fst p Hin.
    induction Hin as [| t rest Ht Hrest [occs [Hproj Hocc]]].
    - exists []. split; [reflexivity | constructor].
    - destruct (transition_in_wfst_has_occurrence fst t Ht)
        as [occ [Hocc_transition Hocc_in]].
      exists (occ :: occs).
      split.
      + simpl. rewrite Hocc_transition. rewrite Hproj. reflexivity.
      + constructor; assumption.
  Qed.

  Lemma accepting_path_has_occurrence_path :
    forall fst p,
      accepting_path fst p ->
      exists occs,
        occurrence_path_transitions occs = p /\
        Forall (transition_occurrence_in_wfst fst) occs.
  Proof.
    intros fst p Haccepts.
    apply path_transitions_have_occurrence_path.
    apply path_valid_in_wfst_transitions.
    exact Haccepts.
  Qed.

  Fixpoint transition_sum_to
      (keep : Transition W -> bool) (target : StateId)
      (transitions : list (Transition W)) : W :=
    match transitions with
    | [] => 𝟘
    | t :: rest =>
        (if andb (keep t) (Nat.eqb (tr_to t) target)
         then tr_weight t
         else 𝟘) ⊕ transition_sum_to keep target rest
    end.

  Definition outgoing_matrix_entry
      (fst : Wfst W) (keep : Transition W -> bool)
      (source target : StateId) : W :=
    match get_state fst source with
    | Some state => transition_sum_to keep target (ws_outgoing state)
    | None => 𝟘
    end.

  Definition wfst_adjacency_matrix
      (fst : Wfst W) (keep : Transition W -> bool) : Matrix W :=
    fun source target => outgoing_matrix_entry fst keep source target.

  Definition final_weight_or_zero (fst : Wfst W) (state : StateId) : W :=
    match final_weight fst state with
    | Some w => w
    | None => 𝟘
    end.

  Definition wfst_matrix_closure
      (fst : Wfst W) (keep : Transition W -> bool) (bound : nat)
      : Matrix W :=
    matrix_partial_star
      (wfst_num_states fst)
      (wfst_adjacency_matrix fst keep)
      bound.

  Definition wfst_matrix_stabilizes_at
      (fst : Wfst W) (keep : Transition W -> bool) (bound : nat) : Prop :=
    matrix_stabilizes_at
      (wfst_num_states fst)
      (wfst_adjacency_matrix fst keep)
      bound.

  Definition wfst_matrix_star_solution
      (fst : Wfst W) (keep : Transition W -> bool) (closure : Matrix W)
      : Prop :=
    matrix_star_solution
      (wfst_num_states fst)
      (wfst_adjacency_matrix fst keep)
      closure.

  Definition matrix_closed_path_weight
      (fst : Wfst W) (keep : Transition W -> bool) (bound : nat)
      (weight : W) : Prop :=
    wfst_well_formed fst /\
    weight ≡
      bounded_sum (wfst_num_states fst)
        (fun state =>
           wfst_matrix_closure fst keep bound (wfst_start fst) state
           ⊗ final_weight_or_zero fst state).

  Lemma transition_sum_to_empty : forall keep target,
    transition_sum_to keep target [] ≡ (𝟘 : W).
  Proof.
    intros keep target. simpl. apply sr_eq_refl.
  Qed.

  Lemma transition_sum_to_cons_keep : forall keep target t rest,
    keep t = true ->
    tr_to t = target ->
    transition_sum_to keep target (t :: rest) ≡
      tr_weight t ⊕ transition_sum_to keep target rest.
  Proof.
    intros keep target t rest Hkeep Htarget.
    simpl.
    rewrite Hkeep.
    rewrite Htarget.
    rewrite Nat.eqb_refl.
    apply sr_eq_refl.
  Qed.

  Lemma transition_sum_to_cons_drop : forall keep target t rest,
    keep t = false \/ tr_to t <> target ->
    transition_sum_to keep target (t :: rest) ≡
      transition_sum_to keep target rest.
  Proof.
    intros keep target t rest Hdrop.
    simpl.
    destruct Hdrop as [Hkeep | Htarget].
    - rewrite Hkeep. simpl. apply sr_plus_zero_l.
    - destruct (keep t); simpl.
      + destruct (Nat.eqb_spec (tr_to t) target).
        * contradiction.
        * apply sr_plus_zero_l.
      + apply sr_plus_zero_l.
  Qed.

  Lemma outgoing_matrix_entry_missing_source : forall fst keep source target,
    get_state fst source = None ->
    outgoing_matrix_entry fst keep source target ≡ (𝟘 : W).
  Proof.
    intros fst keep source target Hmissing.
    unfold outgoing_matrix_entry.
    rewrite Hmissing.
    apply sr_eq_refl.
  Qed.

  Lemma empty_wfst_adjacency_zero : forall keep,
    matrix_eq
      (wfst_adjacency_matrix (mkWfst [] NO_STATE 0 : Wfst W) keep)
      matrix_zero.
  Proof.
    intros keep source target.
    unfold wfst_adjacency_matrix, outgoing_matrix_entry, get_state, matrix_zero.
    destruct source; simpl; apply sr_eq_refl.
  Qed.

  Lemma wfst_matrix_stabilizes_star_solution : forall fst keep bound,
    wfst_matrix_stabilizes_at fst keep bound ->
    wfst_matrix_star_solution fst keep
      (wfst_matrix_closure fst keep bound).
  Proof.
    intros fst keep bound Hstable.
    unfold wfst_matrix_stabilizes_at,
      wfst_matrix_star_solution,
      wfst_matrix_closure in *.
    apply matrix_stabilizes_star_solution.
    exact Hstable.
  Qed.

  Lemma matrix_closed_path_weight_respects_sr_eq :
    forall fst keep bound w1 w2,
      w1 ≡ w2 ->
      matrix_closed_path_weight fst keep bound w1 ->
      matrix_closed_path_weight fst keep bound w2.
  Proof.
    intros fst keep bound w1 w2 Heq [Hwf Hweight].
    split; [exact Hwf |].
    eapply sr_eq_trans.
    - apply sr_eq_sym. exact Heq.
    - exact Hweight.
  Qed.

  Lemma empty_wfst_matrix_closed_path_weight : forall keep bound,
    matrix_closed_path_weight
      (mkWfst [] NO_STATE 0 : Wfst W)
      keep
      bound
      𝟘.
  Proof.
    intros keep bound.
    unfold matrix_closed_path_weight.
    split.
    - apply empty_wfst_well_formed.
    - simpl. apply sr_eq_refl.
  Qed.

  (** ** Product Matrix Semantics for Fixed Label Strings *)

  Definition product_input_bound (input : list Label) : nat :=
    S (length input).

  Definition product_output_bound (output : list Label) : nat :=
    S (length output).

  Definition product_position_count
      (input output : list Label) : nat :=
    product_input_bound input * product_output_bound output.

  Definition product_dim
      (fst : Wfst W) (input output : list Label) : nat :=
    wfst_num_states fst * product_position_count input output.

  Definition product_index
      (input output : list Label)
      (state input_pos output_pos : nat) : nat :=
    (state * product_input_bound input + input_pos) *
      product_output_bound output + output_pos.

  Definition product_state
      (input output : list Label) (idx : nat) : StateId :=
    idx / product_position_count input output.

  Definition product_input_pos
      (input output : list Label) (idx : nat) : nat :=
    (idx / product_output_bound output) mod product_input_bound input.

  Definition product_output_pos
      (output : list Label) (idx : nat) : nat :=
    idx mod product_output_bound output.

  Lemma product_input_bound_pos : forall input,
    product_input_bound input > 0.
  Proof.
    intro input. unfold product_input_bound. lia.
  Qed.

  Lemma product_output_bound_pos : forall output,
    product_output_bound output > 0.
  Proof.
    intro output. unfold product_output_bound. lia.
  Qed.

  Lemma product_position_count_pos : forall input output,
    product_position_count input output > 0.
  Proof.
    intros input output.
    unfold product_position_count.
    pose proof (product_input_bound_pos input).
    pose proof (product_output_bound_pos output).
    nia.
  Qed.

  Lemma product_index_lt_dim :
    forall fst input output state input_pos output_pos,
      state < wfst_num_states fst ->
      input_pos < product_input_bound input ->
      output_pos < product_output_bound output ->
      product_index input output state input_pos output_pos <
      product_dim fst input output.
  Proof.
    intros fst input output state input_pos output_pos
      Hstate Hinput Houtput.
    unfold product_index, product_dim, product_position_count.
    set (ib := product_input_bound input).
    set (ob := product_output_bound output).
    assert (Hib : ib > 0) by (subst ib; apply product_input_bound_pos).
    assert (Hob : ob > 0) by (subst ob; apply product_output_bound_pos).
    assert (Hidx :
      state * ib + input_pos + 1 <= (state + 1) * ib) by nia.
    assert (Hstep :
      (state * ib + input_pos) * ob + output_pos <
      (state * ib + input_pos + 1) * ob) by nia.
    assert (Hstate_step :
      state + 1 <= wfst_num_states fst) by lia.
    assert (Hle_input :
      (state * ib + input_pos + 1) * ob <=
      ((state + 1) * ib) * ob) by nia.
    assert (Hle_state :
      ((state + 1) * ib) * ob <=
      (wfst_num_states fst * ib) * ob) by nia.
    replace (wfst_num_states fst * (ib * ob))
      with ((wfst_num_states fst * ib) * ob) by nia.
    eapply Nat.lt_le_trans; [exact Hstep |].
    eapply Nat.le_trans; [exact Hle_input |].
    exact Hle_state.
  Qed.

  Lemma product_output_pos_index :
    forall input output state input_pos output_pos,
      output_pos < product_output_bound output ->
      product_output_pos output
        (product_index input output state input_pos output_pos) =
      output_pos.
  Proof.
    intros input output state input_pos output_pos Hout.
    unfold product_output_pos, product_index.
    replace
      ((state * product_input_bound input + input_pos) *
         product_output_bound output + output_pos)
      with
      (output_pos +
        (state * product_input_bound input + input_pos) *
          product_output_bound output) by lia.
    rewrite Nat.Div0.mod_add.
    apply Nat.mod_small.
    exact Hout.
  Qed.

  Lemma product_input_pos_index :
    forall input output state input_pos output_pos,
      input_pos < product_input_bound input ->
      output_pos < product_output_bound output ->
      product_input_pos input output
        (product_index input output state input_pos output_pos) =
      input_pos.
  Proof.
    intros input output state input_pos output_pos Hin Hout.
    unfold product_input_pos, product_index.
    rewrite Nat.div_add_l by
      (pose proof (product_output_bound_pos output); lia).
    rewrite Nat.div_small by exact Hout.
    rewrite Nat.add_0_r.
    replace (state * product_input_bound input + input_pos)
      with (input_pos + state * product_input_bound input) by lia.
    rewrite Nat.Div0.mod_add.
    apply Nat.mod_small.
    exact Hin.
  Qed.

  Lemma product_state_index :
    forall input output state input_pos output_pos,
      input_pos < product_input_bound input ->
      output_pos < product_output_bound output ->
      product_state input output
        (product_index input output state input_pos output_pos) =
      state.
  Proof.
    intros input output state input_pos output_pos Hin Hout.
    unfold product_state, product_index, product_position_count.
    replace
      ((state * product_input_bound input + input_pos) *
         product_output_bound output + output_pos)
      with
      (state * (product_input_bound input * product_output_bound output) +
        (input_pos * product_output_bound output + output_pos)) by nia.
    rewrite Nat.div_add_l by
      (pose proof (product_input_bound_pos input);
       pose proof (product_output_bound_pos output);
       nia).
    rewrite Nat.div_small by
      (pose proof (product_output_bound_pos output); nia).
    lia.
  Qed.

  Definition consume_label
      (label : option Label) (symbols : list Label) (pos : nat) : option nat :=
    match label with
    | None => Some pos
    | Some l =>
        match nth_error symbols pos with
        | Some actual => if Nat.eqb l actual then Some (S pos) else None
        | None => None
        end
    end.

  Fixpoint consume_path_labels
      (input output : list Label) (path : @Path W)
      (input_pos output_pos : nat) : option (nat * nat) :=
    match path with
    | [] => Some (input_pos, output_pos)
    | t :: rest =>
        match consume_label (tr_input t) input input_pos,
              consume_label (tr_output t) output output_pos with
        | Some next_input, Some next_output =>
            consume_path_labels input output rest next_input next_output
        | _, _ => None
        end
    end.

  Definition product_transition_matches
      (input output : list Label) (source target : nat)
      (t : Transition W) : bool :=
    let source_input := product_input_pos input output source in
    let source_output := product_output_pos output source in
    match consume_label (tr_input t) input source_input,
          consume_label (tr_output t) output source_output with
    | Some next_input, Some next_output =>
        Nat.eqb target
          (product_index input output (tr_to t) next_input next_output)
    | _, _ => false
    end.

  Definition product_matrix_step
      (fst : Wfst W) (input output : list Label)
      (source target : nat) (t : Transition W) : Prop :=
    transition_in_wfst fst t /\
    product_state input output source = tr_from t /\
    exists next_input next_output,
      consume_label (tr_input t) input
        (product_input_pos input output source) = Some next_input /\
      consume_label (tr_output t) output
        (product_output_pos output source) = Some next_output /\
      target =
        product_index input output (tr_to t) next_input next_output.

  Fixpoint product_matrix_walk
      (fst : Wfst W) (input output : list Label)
      (source : nat) (path : @Path W) (target : nat) : Prop :=
    match path with
    | [] => target = source
    | t :: rest =>
        exists next,
          product_matrix_step fst input output source next t /\
          product_matrix_walk fst input output next rest target
    end.

  Fixpoint product_transition_sum_to
      (input output : list Label) (source target : nat)
      (transitions : list (Transition W)) : W :=
    match transitions with
    | [] => 𝟘
    | t :: rest =>
        (if product_transition_matches input output source target t
         then tr_weight t
         else 𝟘) ⊕
        product_transition_sum_to input output source target rest
    end.

  Definition product_matrix_entry
      (fst : Wfst W) (input output : list Label)
      (source target : nat) : W :=
    let source_state := product_state input output source in
    match get_state fst source_state with
    | Some state =>
        product_transition_sum_to input output source target (ws_outgoing state)
    | None => 𝟘
    end.

  Definition wfst_product_matrix
      (fst : Wfst W) (input output : list Label) : Matrix W :=
    fun source target => product_matrix_entry fst input output source target.

  Definition wfst_product_matrix_closure
      (fst : Wfst W) (input output : list Label) (bound : nat)
      : Matrix W :=
    matrix_partial_star
      (product_dim fst input output)
      (wfst_product_matrix fst input output)
      bound.

  Definition wfst_product_matrix_walk_sum
      (fst : Wfst W) (input output : list Label) (bound : nat)
      (source target : nat) : W :=
    matrix_walk_sum
      (product_dim fst input output)
      (wfst_product_matrix fst input output)
      bound
      source
      target.

  Fixpoint product_transition_step_sum_to
      (dim : nat) (input output : list Label) (source : nat)
      (transitions : list (Transition W)) (cont : nat -> W) : W :=
    match transitions with
    | [] => 𝟘
    | t :: rest =>
        bounded_sum dim
          (fun next =>
             (if product_transition_matches input output source next t
              then tr_weight t
              else 𝟘) ⊗ cont next) ⊕
        product_transition_step_sum_to dim input output source rest cont
    end.

  Fixpoint product_transition_walk_sum
      (fst : Wfst W) (input output : list Label) (bound : nat)
      (source target : nat) : W :=
    match bound with
    | O => matrix_identity source target
    | S m =>
        matrix_identity source target ⊕
        match get_state fst (product_state input output source) with
        | Some state =>
            product_transition_step_sum_to
              (product_dim fst input output)
              input
              output
              source
              (ws_outgoing state)
              (fun next =>
                 product_transition_walk_sum
                   fst input output m next target)
        | None => 𝟘
        end
    end.

  Definition product_occurrence_matches
      (input output : list Label) (source target : nat)
      (occ : TransitionOccurrence) : bool :=
    Nat.eqb (occ_source occ) (product_state input output source) &&
    product_transition_matches input output source target
      (occ_transition occ).

  Definition product_occurrence_step_weight
      (input output : list Label) (source target : nat)
      (occ : TransitionOccurrence) : W :=
    if product_occurrence_matches input output source target occ
    then tr_weight (occ_transition occ)
    else 𝟘.

  Fixpoint occurrence_in_transition_suffix
      (source_state : StateId) (next_index : nat)
      (transitions : list (Transition W))
      (occ : TransitionOccurrence) : Prop :=
    match transitions with
    | [] => False
    | t :: rest =>
        occ = mkTransitionOccurrence source_state next_index t \/
        occurrence_in_transition_suffix
          source_state (S next_index) rest occ
    end.

  Definition product_occurrence_step
      (fst : Wfst W) (input output : list Label)
      (source target : nat) (occ : TransitionOccurrence) : Prop :=
    transition_occurrence_in_wfst fst occ /\
    occ_source occ = product_state input output source /\
    product_occurrence_matches input output source target occ = true.

  Fixpoint product_occurrence_walk
      (fst : Wfst W) (input output : list Label)
      (source : nat) (path : OccurrencePath) (target : nat) : Prop :=
    match path with
    | [] => target = source
    | occ :: rest =>
        exists next,
          product_occurrence_step fst input output source next occ /\
          product_occurrence_walk fst input output next rest target
    end.

  Fixpoint product_occurrence_target_paths_to
      (input output : list Label) (source : nat)
      (occ : TransitionOccurrence) (max_target : nat)
      (cont : nat -> list OccurrencePath) : list OccurrencePath :=
    match max_target with
    | O =>
        if product_occurrence_matches input output source 0 occ
        then prepend_occurrence_paths occ (cont 0)
        else []
    | S m =>
        product_occurrence_target_paths_to
          input output source occ m cont ++
        if product_occurrence_matches input output source (S m) occ
        then prepend_occurrence_paths occ (cont (S m))
        else []
    end.

  Definition product_occurrence_target_paths
      (dim : nat) (input output : list Label) (source : nat)
      (occ : TransitionOccurrence) (cont : nat -> list OccurrencePath)
      : list OccurrencePath :=
    match dim with
    | O => []
    | S max_target =>
        product_occurrence_target_paths_to
          input output source occ max_target cont
    end.

  Fixpoint product_occurrence_step_paths_to
      (dim : nat) (input output : list Label) (source : nat)
      (source_state : StateId) (next_index : nat)
      (transitions : list (Transition W))
      (cont : nat -> list OccurrencePath) : list OccurrencePath :=
    match transitions with
    | [] => []
    | t :: rest =>
        product_occurrence_target_paths
          dim input output source
          (mkTransitionOccurrence source_state next_index t)
          cont ++
        product_occurrence_step_paths_to
          dim input output source source_state (S next_index) rest cont
    end.

  Fixpoint product_occurrence_step_sum_to
      (dim : nat) (input output : list Label) (source : nat)
      (source_state : StateId) (next_index : nat)
      (transitions : list (Transition W)) (cont : nat -> W) : W :=
    match transitions with
    | [] => 𝟘
    | t :: rest =>
        bounded_sum dim
          (fun next =>
             product_occurrence_step_weight input output source next
               (mkTransitionOccurrence source_state next_index t) ⊗
             cont next) ⊕
        product_occurrence_step_sum_to
          dim input output source source_state (S next_index) rest cont
    end.

  Fixpoint product_occurrence_walk_sum
      (fst : Wfst W) (input output : list Label) (bound : nat)
      (source target : nat) : W :=
    match bound with
    | O => matrix_identity source target
    | S m =>
        matrix_identity source target ⊕
        match get_state fst (product_state input output source) with
        | Some state =>
            product_occurrence_step_sum_to
              (product_dim fst input output)
              input
              output
              source
              (product_state input output source)
              0
              (ws_outgoing state)
              (fun next =>
                 product_occurrence_walk_sum
                   fst input output m next target)
        | None => 𝟘
        end
    end.

  Fixpoint product_occurrence_walk_paths
      (fst : Wfst W) (input output : list Label) (bound : nat)
      (source target : nat) : list OccurrencePath :=
    match bound with
    | O =>
        if Nat.eqb source target then [[]] else []
    | S m =>
        (if Nat.eqb source target then [[]] else []) ++
        match get_state fst (product_state input output source) with
        | Some state =>
            product_occurrence_step_paths_to
              (product_dim fst input output)
              input
              output
              source
              (product_state input output source)
              0
              (ws_outgoing state)
              (fun next =>
                 product_occurrence_walk_paths
                   fst input output m next target)
        | None => []
        end
    end.

  Definition wfst_product_matrix_stabilizes_at
      (fst : Wfst W) (input output : list Label) (bound : nat) : Prop :=
    matrix_stabilizes_at
      (product_dim fst input output)
      (wfst_product_matrix fst input output)
      bound.

  Definition wfst_product_matrix_star_solution
      (fst : Wfst W) (input output : list Label) (closure : Matrix W)
      : Prop :=
    matrix_star_solution
      (product_dim fst input output)
      (wfst_product_matrix fst input output)
      closure.

  Definition product_start_index
      (fst : Wfst W) (input output : list Label) : nat :=
    product_index input output (wfst_start fst) 0 0.

  Lemma product_start_index_lt_dim :
    forall fst input output,
      is_valid_state fst (wfst_start fst) ->
      product_start_index fst input output < product_dim fst input output.
  Proof.
    intros fst input output Hstart.
    unfold product_start_index.
    apply product_index_lt_dim.
    - exact Hstart.
    - unfold product_input_bound. lia.
    - unfold product_output_bound. lia.
  Qed.

  Definition product_accepts_final_position
      (input output : list Label) (idx : nat) : bool :=
    Nat.eqb (product_input_pos input output idx) (length input) &&
    Nat.eqb (product_output_pos output idx) (length output).

  Definition product_final_weight_or_zero
      (fst : Wfst W) (input output : list Label) (idx : nat) : W :=
    if product_accepts_final_position input output idx
    then final_weight_or_zero fst (product_state input output idx)
    else 𝟘.

  Definition TargetOccurrencePath := (nat * OccurrencePath)%type.

  Definition target_occurrence_path_weight
      (fst : Wfst W) (input output : list Label)
      (entry : TargetOccurrencePath) : W :=
    match entry with
    | (target, path) =>
        occurrence_path_weight path ⊗
        product_final_weight_or_zero fst input output target
    end.

  Fixpoint target_occurrence_path_weight_sum
      (fst : Wfst W) (input output : list Label)
      (paths : list TargetOccurrencePath) : W :=
    match paths with
    | [] => 𝟘
    | path :: rest =>
        target_occurrence_path_weight fst input output path ⊕
        target_occurrence_path_weight_sum fst input output rest
    end.

  Definition attach_target_occurrence_paths
      (target : nat) (paths : list OccurrencePath)
      : list TargetOccurrencePath :=
    map (fun path => (target, path)) paths.

  Fixpoint product_occurrence_closed_paths_to
      (fst : Wfst W) (input output : list Label) (bound : nat)
      (max_target : nat) : list TargetOccurrencePath :=
    match max_target with
    | O =>
        attach_target_occurrence_paths 0
          (product_occurrence_walk_paths
            fst input output bound
            (product_start_index fst input output)
            0)
    | S m =>
        product_occurrence_closed_paths_to fst input output bound m ++
        attach_target_occurrence_paths (S m)
          (product_occurrence_walk_paths
            fst input output bound
            (product_start_index fst input output)
            (S m))
    end.

  Definition product_occurrence_closed_paths
      (fst : Wfst W) (input output : list Label) (bound : nat)
      : list TargetOccurrencePath :=
    match product_dim fst input output with
    | O => []
    | S max_target =>
        product_occurrence_closed_paths_to fst input output bound max_target
    end.

  Definition product_matrix_closed_path_weight
      (fst : Wfst W) (input output : list Label) (bound : nat)
      (weight : W) : Prop :=
    wfst_well_formed fst /\
    weight ≡
      bounded_sum (product_dim fst input output)
        (fun target =>
           wfst_product_matrix_closure fst input output bound
             (product_start_index fst input output) target
           ⊗ product_final_weight_or_zero fst input output target).

  Definition product_matrix_walk_sum_closed_path_weight
      (fst : Wfst W) (input output : list Label) (bound : nat)
      (weight : W) : Prop :=
    wfst_well_formed fst /\
    weight ≡
      bounded_sum (product_dim fst input output)
        (fun target =>
           wfst_product_matrix_walk_sum fst input output bound
             (product_start_index fst input output) target
           ⊗ product_final_weight_or_zero fst input output target).

  Definition product_transition_walk_sum_closed_path_weight
      (fst : Wfst W) (input output : list Label) (bound : nat)
      (weight : W) : Prop :=
    wfst_well_formed fst /\
    weight ≡
      bounded_sum (product_dim fst input output)
        (fun target =>
           product_transition_walk_sum fst input output bound
             (product_start_index fst input output) target
           ⊗ product_final_weight_or_zero fst input output target).

  Definition product_occurrence_walk_sum_closed_path_weight
      (fst : Wfst W) (input output : list Label) (bound : nat)
      (weight : W) : Prop :=
    wfst_well_formed fst /\
    weight ≡
      bounded_sum (product_dim fst input output)
        (fun target =>
           product_occurrence_walk_sum fst input output bound
             (product_start_index fst input output) target
           ⊗ product_final_weight_or_zero fst input output target).

  Definition product_occurrence_enumerator_closed_path_weight
      (fst : Wfst W) (input output : list Label) (bound : nat)
      (weight : W) : Prop :=
    wfst_well_formed fst /\
    weight ≡
      target_occurrence_path_weight_sum
        fst input output
        (product_occurrence_closed_paths fst input output bound).

  Lemma product_transition_sum_to_empty : forall input output source target,
    product_transition_sum_to input output source target [] ≡ (𝟘 : W).
  Proof.
    intros input output source target. simpl. apply sr_eq_refl.
  Qed.

  Lemma product_transition_sum_to_step_sum :
    forall dim input output source transitions cont,
      bounded_sum dim
        (fun next =>
           product_transition_sum_to input output source next transitions
           ⊗ cont next) ≡
      product_transition_step_sum_to
        dim input output source transitions cont.
  Proof.
    intros dim input output source transitions.
    induction transitions as [| t rest IH]; intro cont.
    - simpl.
      eapply sr_eq_trans.
      + apply bounded_sum_proper.
        intros next _.
        apply sr_zero_times_l.
      + apply bounded_sum_zero.
    - simpl.
      eapply sr_eq_trans.
      + apply bounded_sum_proper.
        intros next _.
        apply sr_distr_r.
      + eapply sr_eq_trans.
        * apply bounded_sum_plus.
        * apply sr_plus_proper.
          -- apply sr_eq_refl.
          -- apply IH.
  Qed.

  Lemma product_transition_step_sum_to_proper_cont :
    forall dim input output source transitions cont1 cont2,
      (forall next, next < dim -> cont1 next ≡ cont2 next) ->
      product_transition_step_sum_to
        dim input output source transitions cont1 ≡
      product_transition_step_sum_to
        dim input output source transitions cont2.
  Proof.
    intros dim input output source transitions.
    induction transitions as [| t rest IH]; intros cont1 cont2 Hcont.
    - simpl. apply sr_eq_refl.
    - simpl. apply sr_plus_proper.
      + apply bounded_sum_proper.
        intros next Hnext.
        apply sr_times_proper.
        * apply sr_eq_refl.
        * apply Hcont. exact Hnext.
      + apply IH. exact Hcont.
  Qed.

  Lemma product_occurrence_step_sum_to_transition :
    forall dim input output source source_index transitions cont,
      product_occurrence_step_sum_to
        dim input output source
        (product_state input output source)
        source_index transitions cont ≡
      product_transition_step_sum_to
        dim input output source transitions cont.
  Proof.
    intros dim input output source source_index transitions.
    revert source_index.
    induction transitions as [| t rest IH]; intros source_index cont.
    - simpl. apply sr_eq_refl.
    - simpl. apply sr_plus_proper.
      + apply bounded_sum_proper.
        intros next _.
        unfold product_occurrence_step_weight, product_occurrence_matches.
        simpl.
        rewrite Nat.eqb_refl.
        simpl.
        apply sr_eq_refl.
      + apply IH.
  Qed.

  Lemma occurrence_path_weight_sum_app :
    forall paths1 paths2,
      occurrence_path_weight_sum (paths1 ++ paths2) ≡
      occurrence_path_weight_sum paths1 ⊕
      occurrence_path_weight_sum paths2.
  Proof.
    intros paths1 paths2.
    induction paths1 as [| path rest IH].
    - simpl. apply sr_eq_sym. apply sr_plus_zero_l.
    - simpl.
      eapply sr_eq_trans with
        (b := occurrence_path_weight path ⊕
              (occurrence_path_weight_sum rest ⊕
               occurrence_path_weight_sum paths2)).
      + apply sr_plus_proper; [apply sr_eq_refl | exact IH].
      + apply sr_eq_sym. apply sr_plus_assoc.
  Qed.

  Lemma occurrence_path_weight_sum_prepend :
    forall occ paths,
      occurrence_path_weight_sum (prepend_occurrence_paths occ paths) ≡
      tr_weight (occ_transition occ) ⊗ occurrence_path_weight_sum paths.
  Proof.
    intros occ paths.
    induction paths as [| path rest IH].
    - simpl. apply sr_eq_sym. apply sr_zero_times_r.
    - simpl.
      eapply sr_eq_trans with
        (b := (tr_weight (occ_transition occ) ⊗
               occurrence_path_weight path) ⊕
              (tr_weight (occ_transition occ) ⊗
               occurrence_path_weight_sum rest)).
      + apply sr_plus_proper; [apply sr_eq_refl | exact IH].
      + apply sr_eq_sym. apply sr_distr_l.
  Qed.

  Lemma occurrence_path_weight_sum_target_paths_to :
    forall input output source occ max_target cont,
      occurrence_path_weight_sum
        (product_occurrence_target_paths_to
          input output source occ max_target cont) ≡
      matrix_sum_to
        (fun next =>
           product_occurrence_step_weight input output source next occ ⊗
           occurrence_path_weight_sum (cont next))
        max_target.
  Proof.
    intros input output source occ max_target.
    induction max_target as [| max_target IH]; intro cont.
    - simpl. unfold product_occurrence_step_weight.
      destruct (product_occurrence_matches input output source 0 occ).
      + apply occurrence_path_weight_sum_prepend.
      + simpl. apply sr_eq_sym. apply sr_zero_times_l.
    - simpl.
      eapply sr_eq_trans.
      + apply occurrence_path_weight_sum_app.
      + apply sr_plus_proper.
        * apply IH.
        * unfold product_occurrence_step_weight.
          destruct (product_occurrence_matches
            input output source (S max_target) occ).
          -- apply occurrence_path_weight_sum_prepend.
          -- simpl. apply sr_eq_sym. apply sr_zero_times_l.
  Qed.

  Lemma occurrence_path_weight_sum_target_paths :
    forall dim input output source occ cont,
      occurrence_path_weight_sum
        (product_occurrence_target_paths
          dim input output source occ cont) ≡
      bounded_sum dim
        (fun next =>
           product_occurrence_step_weight input output source next occ ⊗
           occurrence_path_weight_sum (cont next)).
  Proof.
    intros dim input output source occ cont.
    destruct dim as [| max_target].
    - simpl. apply sr_eq_refl.
    - simpl. apply occurrence_path_weight_sum_target_paths_to.
  Qed.

  Lemma occurrence_path_weight_sum_step_paths_to :
    forall dim input output source source_state next_index transitions cont,
      occurrence_path_weight_sum
        (product_occurrence_step_paths_to
          dim input output source source_state next_index transitions cont) ≡
      product_occurrence_step_sum_to
        dim input output source source_state next_index transitions
        (fun next => occurrence_path_weight_sum (cont next)).
  Proof.
    intros dim input output source source_state next_index transitions.
    revert next_index.
    induction transitions as [| t rest IH]; intros next_index cont.
    - simpl. apply sr_eq_refl.
    - simpl.
      eapply sr_eq_trans.
      + apply occurrence_path_weight_sum_app.
      + apply sr_plus_proper.
        * apply occurrence_path_weight_sum_target_paths.
        * apply IH.
  Qed.

  Lemma product_occurrence_step_sum_to_proper_cont :
    forall dim input output source source_state next_index transitions
      cont1 cont2,
      (forall next, next < dim -> cont1 next ≡ cont2 next) ->
      product_occurrence_step_sum_to
        dim input output source source_state next_index transitions cont1 ≡
      product_occurrence_step_sum_to
        dim input output source source_state next_index transitions cont2.
  Proof.
    intros dim input output source source_state next_index transitions.
    revert next_index.
    induction transitions as [| t rest IH]; intros next_index cont1 cont2 Hcont.
    - simpl. apply sr_eq_refl.
    - simpl. apply sr_plus_proper.
      + apply bounded_sum_proper.
        intros next Hnext.
        apply sr_times_proper.
        * apply sr_eq_refl.
        * apply Hcont. exact Hnext.
      + apply IH. exact Hcont.
  Qed.

  Lemma occurrence_path_weight_sum_identity_paths :
    forall source target,
      occurrence_path_weight_sum
        (if Nat.eqb source target then [[]] else []) ≡
      matrix_identity source target.
  Proof.
    intros source target.
    unfold matrix_identity.
    destruct (Nat.eqb source target).
    - simpl. apply sr_plus_zero_r.
    - simpl. apply sr_eq_refl.
  Qed.

  Lemma product_occurrence_walk_paths_sum :
    forall fst input output bound source target,
      occurrence_path_weight_sum
        (product_occurrence_walk_paths fst input output bound source target) ≡
      product_occurrence_walk_sum fst input output bound source target.
  Proof.
    intros fst input output bound.
    induction bound as [| bound IH]; intros source target.
    - simpl. apply occurrence_path_weight_sum_identity_paths.
    - simpl.
      eapply sr_eq_trans.
      + apply occurrence_path_weight_sum_app.
      + apply sr_plus_proper.
        * apply occurrence_path_weight_sum_identity_paths.
        * destruct (get_state fst (product_state input output source))
            as [state |].
          -- eapply sr_eq_trans.
             ++ apply occurrence_path_weight_sum_step_paths_to.
             ++ apply product_occurrence_step_sum_to_proper_cont.
                intros next _. apply IH.
          -- simpl. apply sr_eq_refl.
  Qed.

  Lemma target_occurrence_path_weight_sum_app :
    forall fst input output paths1 paths2,
      target_occurrence_path_weight_sum
        fst input output (paths1 ++ paths2) ≡
      target_occurrence_path_weight_sum fst input output paths1 ⊕
      target_occurrence_path_weight_sum fst input output paths2.
  Proof.
    intros fst input output paths1 paths2.
    induction paths1 as [| path rest IH].
    - simpl. apply sr_eq_sym. apply sr_plus_zero_l.
    - simpl.
      eapply sr_eq_trans with
        (b := target_occurrence_path_weight fst input output path ⊕
              (target_occurrence_path_weight_sum fst input output rest ⊕
               target_occurrence_path_weight_sum fst input output paths2)).
      + apply sr_plus_proper; [apply sr_eq_refl | exact IH].
      + apply sr_eq_sym. apply sr_plus_assoc.
  Qed.

  Lemma target_occurrence_path_weight_sum_attach :
    forall fst input output target paths,
      target_occurrence_path_weight_sum
        fst input output
        (attach_target_occurrence_paths target paths) ≡
      occurrence_path_weight_sum paths ⊗
      product_final_weight_or_zero fst input output target.
  Proof.
    intros fst input output target paths.
    induction paths as [| path rest IH].
    - simpl. apply sr_eq_sym. apply sr_zero_times_l.
    - simpl.
      eapply sr_eq_trans with
        (b := (occurrence_path_weight path ⊗
               product_final_weight_or_zero fst input output target) ⊕
              (occurrence_path_weight_sum rest ⊗
               product_final_weight_or_zero fst input output target)).
      + apply sr_plus_proper; [apply sr_eq_refl | exact IH].
      + apply sr_eq_sym. apply sr_distr_r.
  Qed.

  Lemma target_occurrence_path_weight_sum_closed_paths_to :
    forall fst input output bound max_target,
      target_occurrence_path_weight_sum
        fst input output
        (product_occurrence_closed_paths_to
          fst input output bound max_target) ≡
      matrix_sum_to
        (fun target =>
           occurrence_path_weight_sum
             (product_occurrence_walk_paths
               fst input output bound
               (product_start_index fst input output)
               target) ⊗
           product_final_weight_or_zero fst input output target)
        max_target.
  Proof.
    intros fst input output bound max_target.
    induction max_target as [| max_target IH].
    - simpl. apply target_occurrence_path_weight_sum_attach.
    - simpl.
      eapply sr_eq_trans.
      + apply target_occurrence_path_weight_sum_app.
      + apply sr_plus_proper.
        * exact IH.
        * apply target_occurrence_path_weight_sum_attach.
  Qed.

  Lemma target_occurrence_path_weight_sum_closed_paths :
    forall fst input output bound,
      target_occurrence_path_weight_sum
        fst input output
        (product_occurrence_closed_paths fst input output bound) ≡
      bounded_sum (product_dim fst input output)
        (fun target =>
           occurrence_path_weight_sum
             (product_occurrence_walk_paths
               fst input output bound
               (product_start_index fst input output)
               target) ⊗
           product_final_weight_or_zero fst input output target).
  Proof.
    intros fst input output bound.
    unfold product_occurrence_closed_paths.
    destruct (product_dim fst input output) as [| max_target].
    - simpl. apply sr_eq_refl.
    - simpl. apply target_occurrence_path_weight_sum_closed_paths_to.
  Qed.

  Lemma consume_epsilon : forall symbols pos,
    consume_label None symbols pos = Some pos.
  Proof.
    intros symbols pos. reflexivity.
  Qed.

  Lemma consume_label_some : forall symbols pos label,
    nth_error symbols pos = Some label ->
    consume_label (Some label) symbols pos = Some (S pos).
  Proof.
    intros symbols pos label Hnth.
    unfold consume_label.
    rewrite Hnth.
    rewrite Nat.eqb_refl.
    reflexivity.
  Qed.

  Lemma consume_label_within_bound : forall symbols label pos next,
    pos < S (length symbols) ->
    consume_label label symbols pos = Some next ->
    next < S (length symbols).
  Proof.
    intros symbols label pos next Hpos Hconsume.
    destruct label as [label |].
    - unfold consume_label in Hconsume.
      destruct (nth_error symbols pos) as [actual |] eqn:Hnth;
        try discriminate.
      destruct (Nat.eqb label actual); inversion Hconsume; subst.
      apply Nat.lt_succ_r.
      apply nth_error_Some.
      rewrite Hnth. discriminate.
    - unfold consume_label in Hconsume.
      inversion Hconsume. subst.
      exact Hpos.
  Qed.

  Lemma product_accepts_final_position_index :
    forall input output state,
      product_accepts_final_position input output
        (product_index input output state (length input) (length output)) =
      true.
  Proof.
    intros input output state.
    unfold product_accepts_final_position.
    rewrite product_input_pos_index by
      (unfold product_input_bound, product_output_bound; lia).
    rewrite product_output_pos_index by
      (unfold product_output_bound; lia).
    rewrite Nat.eqb_refl.
    simpl.
    rewrite Nat.eqb_refl.
    reflexivity.
  Qed.

  Lemma product_final_weight_or_zero_index :
    forall fst input output state,
      product_final_weight_or_zero fst input output
        (product_index input output state (length input) (length output)) ≡
      final_weight_or_zero fst state.
  Proof.
    intros fst input output state.
    unfold product_final_weight_or_zero.
    rewrite product_accepts_final_position_index.
    rewrite product_state_index by
      (unfold product_input_bound, product_output_bound; lia).
    apply sr_eq_refl.
  Qed.

  Lemma product_matrix_step_matches :
    forall fst input output source target t,
      product_matrix_step fst input output source target t ->
      product_transition_matches input output source target t = true.
  Proof.
    intros fst input output source target t
      [_ [_ [next_input [next_output [Hinput [Houtput Htarget]]]]]].
    unfold product_transition_matches.
    rewrite Hinput.
    rewrite Houtput.
    rewrite Htarget.
    apply Nat.eqb_refl.
  Qed.

  Lemma product_matrix_step_outgoing :
    forall fst input output source target t,
      product_matrix_step fst input output source target t ->
      In t (get_outgoing fst (product_state input output source)).
  Proof.
    intros fst input output source target t [Hin [Hsource _]].
    unfold transition_in_wfst in Hin.
    rewrite Hsource.
    exact Hin.
  Qed.

  Lemma product_matrix_walk_empty :
    forall fst input output source,
      product_matrix_walk fst input output source [] source.
  Proof.
    intros fst input output source. reflexivity.
  Qed.

  Lemma product_matrix_walk_cons :
    forall fst input output source mid target t rest,
      product_matrix_step fst input output source mid t ->
      product_matrix_walk fst input output mid rest target ->
      product_matrix_walk fst input output source (t :: rest) target.
  Proof.
    intros fst input output source mid target t rest Hstep Hwalk.
    simpl.
    exists mid. split; assumption.
  Qed.

  Lemma product_matrix_walk_app :
    forall fst input output source mid target p1 p2,
      product_matrix_walk fst input output source p1 mid ->
      product_matrix_walk fst input output mid p2 target ->
      product_matrix_walk fst input output source (p1 ++ p2) target.
  Proof.
    intros fst input output source mid target p1.
    revert source mid target.
    induction p1 as [| t rest IH]; intros source mid target p2 Hwalk1 Hwalk2.
    - simpl in *. subst. exact Hwalk2.
    - simpl in *.
      destruct Hwalk1 as [next [Hstep Hrest]].
      exists next. split.
      + exact Hstep.
      + eapply IH; eauto.
  Qed.

  Lemma product_matrix_walk_of_connected_path :
    forall fst input output source_state input_pos output_pos
      path final_input final_output,
      path_connects_from source_state path ->
      path_transitions_in_wfst fst path ->
      input_pos < product_input_bound input ->
      output_pos < product_output_bound output ->
      consume_path_labels input output path input_pos output_pos =
        Some (final_input, final_output) ->
      product_matrix_walk fst input output
        (product_index input output source_state input_pos output_pos)
        path
        (product_index input output
          (path_end_state_from source_state path) final_input final_output).
  Proof.
    intros fst input output source_state input_pos output_pos path.
    revert source_state input_pos output_pos.
    induction path as [| t rest IH];
      intros source_state input_pos output_pos final_input final_output
        Hconnect Hin Hinput_bound Houtput_bound Hconsume.
    - simpl in Hconsume. inversion Hconsume. subst.
      simpl. reflexivity.
    - simpl in Hconnect.
      destruct Hconnect as [Hfrom Hrest_connect].
      inversion Hin as [| t' rest' Ht_in Hrest_in]; subst.
      simpl in Hconsume.
      destruct (consume_label (tr_input t) input input_pos) as
        [next_input |] eqn:Hconsume_input; try discriminate.
      destruct (consume_label (tr_output t) output output_pos) as
        [next_output |] eqn:Hconsume_output; try discriminate.
      simpl.
      exists (product_index input output (tr_to t) next_input next_output).
      split.
      + unfold product_matrix_step.
        split; [exact Ht_in |].
        split.
        * rewrite product_state_index by assumption.
          reflexivity.
        * exists next_input, next_output.
          rewrite product_input_pos_index by assumption.
          rewrite product_output_pos_index by assumption.
          split; [exact Hconsume_input |].
          split; [exact Hconsume_output | reflexivity].
      + apply IH.
        * exact Hrest_connect.
        * exact Hrest_in.
        * eapply consume_label_within_bound; eauto.
        * eapply consume_label_within_bound; eauto.
        * exact Hconsume.
  Qed.

  Lemma accepting_path_product_matrix_walk :
    forall fst input output path,
      accepting_path fst path ->
      consume_path_labels input output path 0 0 =
        Some (length input, length output) ->
      product_matrix_walk fst input output
        (product_start_index fst input output)
        path
        (product_index input output
          (path_end_state_from (wfst_start fst) path)
          (length input)
          (length output)).
  Proof.
    intros fst input output path Haccepts Hconsume.
    unfold product_start_index.
    apply product_matrix_walk_of_connected_path.
    - apply path_valid_in_wfst_connects_from_start.
      exact Haccepts.
    - apply path_valid_in_wfst_transitions.
      exact Haccepts.
    - unfold product_input_bound. lia.
    - unfold product_output_bound. lia.
    - exact Hconsume.
  Qed.

  Lemma wfst_product_matrix_stabilizes_star_solution :
    forall fst input output bound,
      wfst_product_matrix_stabilizes_at fst input output bound ->
      wfst_product_matrix_star_solution fst input output
        (wfst_product_matrix_closure fst input output bound).
  Proof.
    intros fst input output bound Hstable.
    unfold wfst_product_matrix_stabilizes_at,
      wfst_product_matrix_star_solution,
      wfst_product_matrix_closure in *.
    apply matrix_stabilizes_star_solution.
    exact Hstable.
  Qed.

  Lemma wfst_product_matrix_closure_walk_sum :
    forall fst input output bound source target,
      wfst_product_matrix_closure fst input output bound source target ≡
      wfst_product_matrix_walk_sum fst input output bound source target.
  Proof.
    intros fst input output bound source target.
    unfold wfst_product_matrix_closure, wfst_product_matrix_walk_sum.
    apply matrix_partial_star_walk_sum.
  Qed.

  Lemma wfst_product_matrix_walk_sum_zero :
    forall fst input output source target,
      wfst_product_matrix_walk_sum fst input output 0 source target ≡
      matrix_identity source target.
  Proof.
    intros fst input output source target.
    unfold wfst_product_matrix_walk_sum.
    apply matrix_walk_sum_zero.
  Qed.

  Lemma wfst_product_matrix_walk_sum_unfold :
    forall fst input output bound source target,
      wfst_product_matrix_walk_sum fst input output (S bound) source target ≡
        matrix_identity source target ⊕
        bounded_sum (product_dim fst input output)
          (fun next =>
             wfst_product_matrix fst input output source next ⊗
             wfst_product_matrix_walk_sum
               fst input output bound next target).
  Proof.
    intros fst input output bound source target.
    unfold wfst_product_matrix_walk_sum.
    apply matrix_walk_sum_unfold.
  Qed.

  Lemma product_transition_walk_sum_zero :
    forall fst input output source target,
      product_transition_walk_sum fst input output 0 source target ≡
      matrix_identity source target.
  Proof.
    intros fst input output source target. simpl. apply sr_eq_refl.
  Qed.

  Lemma product_transition_walk_sum_unfold :
    forall fst input output bound source target,
      product_transition_walk_sum fst input output (S bound) source target ≡
        matrix_identity source target ⊕
        match get_state fst (product_state input output source) with
        | Some state =>
            product_transition_step_sum_to
              (product_dim fst input output)
              input
              output
              source
              (ws_outgoing state)
              (fun next =>
                 product_transition_walk_sum
                   fst input output bound next target)
        | None => 𝟘
        end.
  Proof.
    intros fst input output bound source target.
    simpl. apply sr_eq_refl.
  Qed.

  Lemma product_occurrence_walk_sum_zero :
    forall fst input output source target,
      product_occurrence_walk_sum fst input output 0 source target ≡
      matrix_identity source target.
  Proof.
    intros fst input output source target. simpl. apply sr_eq_refl.
  Qed.

  Lemma product_occurrence_walk_sum_unfold :
    forall fst input output bound source target,
      product_occurrence_walk_sum fst input output (S bound) source target ≡
        matrix_identity source target ⊕
        match get_state fst (product_state input output source) with
        | Some state =>
            product_occurrence_step_sum_to
              (product_dim fst input output)
              input
              output
              source
              (product_state input output source)
              0
              (ws_outgoing state)
              (fun next =>
                 product_occurrence_walk_sum
                   fst input output bound next target)
        | None => 𝟘
        end.
  Proof.
    intros fst input output bound source target.
    simpl. apply sr_eq_refl.
  Qed.

  Lemma product_transition_walk_sum_occurrence_expansion :
    forall fst input output bound source target,
      product_transition_walk_sum fst input output bound source target ≡
      product_occurrence_walk_sum fst input output bound source target.
  Proof.
    intros fst input output bound.
    induction bound as [| bound IH]; intros source target.
    - simpl. apply sr_eq_refl.
    - simpl. apply sr_plus_proper.
      + apply sr_eq_refl.
      + destruct (get_state fst (product_state input output source))
          as [state |].
        * eapply sr_eq_trans.
          -- apply product_transition_step_sum_to_proper_cont.
             intros next _. apply IH.
          -- apply sr_eq_sym.
             apply product_occurrence_step_sum_to_transition.
        * apply sr_eq_refl.
  Qed.

  Lemma wfst_product_matrix_walk_sum_transition_expansion :
    forall fst input output bound source target,
      wfst_product_matrix_walk_sum fst input output bound source target ≡
      product_transition_walk_sum fst input output bound source target.
  Proof.
    intros fst input output bound.
    induction bound as [| bound IH]; intros source target.
    - unfold wfst_product_matrix_walk_sum.
      simpl. apply sr_eq_refl.
    - eapply sr_eq_trans.
      + apply wfst_product_matrix_walk_sum_unfold.
      + simpl.
        apply sr_plus_proper.
        * apply sr_eq_refl.
        * unfold wfst_product_matrix, product_matrix_entry.
          destruct (get_state fst (product_state input output source))
            as [state |].
          -- eapply sr_eq_trans.
             ++ apply bounded_sum_proper.
                intros next _.
                apply sr_times_proper.
                ** apply sr_eq_refl.
                ** apply IH.
             ++ apply product_transition_sum_to_step_sum.
          -- eapply sr_eq_trans.
             ++ apply bounded_sum_proper.
                intros next _.
                apply sr_zero_times_l.
             ++ apply bounded_sum_zero.
  Qed.

  Lemma product_matrix_closed_path_weight_walk_sum_equiv :
    forall fst input output bound weight,
      product_matrix_closed_path_weight fst input output bound weight <->
      product_matrix_walk_sum_closed_path_weight fst input output bound weight.
  Proof.
    intros fst input output bound weight.
    unfold product_matrix_closed_path_weight,
      product_matrix_walk_sum_closed_path_weight.
    split.
    - intros [Hwf Hweight].
      split; [exact Hwf |].
      eapply sr_eq_trans.
      + exact Hweight.
      + apply bounded_sum_proper.
        intros target _.
        apply sr_times_proper.
        * apply wfst_product_matrix_closure_walk_sum.
        * apply sr_eq_refl.
    - intros [Hwf Hweight].
      split; [exact Hwf |].
      eapply sr_eq_trans.
      + exact Hweight.
      + apply sr_eq_sym.
        apply bounded_sum_proper.
        intros target _.
        apply sr_times_proper.
        * apply wfst_product_matrix_closure_walk_sum.
        * apply sr_eq_refl.
  Qed.

  Lemma product_matrix_walk_sum_closed_path_weight_transition_equiv :
    forall fst input output bound weight,
      product_matrix_walk_sum_closed_path_weight fst input output bound weight <->
      product_transition_walk_sum_closed_path_weight
        fst input output bound weight.
  Proof.
    intros fst input output bound weight.
    unfold product_matrix_walk_sum_closed_path_weight,
      product_transition_walk_sum_closed_path_weight.
    split.
    - intros [Hwf Hweight].
      split; [exact Hwf |].
      eapply sr_eq_trans.
      + exact Hweight.
      + apply bounded_sum_proper.
        intros target _.
        apply sr_times_proper.
        * apply wfst_product_matrix_walk_sum_transition_expansion.
        * apply sr_eq_refl.
    - intros [Hwf Hweight].
      split; [exact Hwf |].
      eapply sr_eq_trans.
      + exact Hweight.
      + apply sr_eq_sym.
        apply bounded_sum_proper.
        intros target _.
        apply sr_times_proper.
        * apply wfst_product_matrix_walk_sum_transition_expansion.
        * apply sr_eq_refl.
  Qed.

  Lemma product_transition_walk_sum_closed_path_weight_occurrence_equiv :
    forall fst input output bound weight,
      product_transition_walk_sum_closed_path_weight
        fst input output bound weight <->
      product_occurrence_walk_sum_closed_path_weight
        fst input output bound weight.
  Proof.
    intros fst input output bound weight.
    unfold product_transition_walk_sum_closed_path_weight,
      product_occurrence_walk_sum_closed_path_weight.
    split.
    - intros [Hwf Hweight].
      split; [exact Hwf |].
      eapply sr_eq_trans.
      + exact Hweight.
      + apply bounded_sum_proper.
        intros target _.
        apply sr_times_proper.
        * apply product_transition_walk_sum_occurrence_expansion.
        * apply sr_eq_refl.
    - intros [Hwf Hweight].
      split; [exact Hwf |].
      eapply sr_eq_trans.
      + exact Hweight.
      + apply sr_eq_sym.
        apply bounded_sum_proper.
        intros target _.
        apply sr_times_proper.
        * apply product_transition_walk_sum_occurrence_expansion.
        * apply sr_eq_refl.
  Qed.

  Lemma product_occurrence_walk_sum_closed_path_weight_enumerator_equiv :
    forall fst input output bound weight,
      product_occurrence_walk_sum_closed_path_weight
        fst input output bound weight <->
      product_occurrence_enumerator_closed_path_weight
        fst input output bound weight.
  Proof.
    intros fst input output bound weight.
    unfold product_occurrence_walk_sum_closed_path_weight,
      product_occurrence_enumerator_closed_path_weight.
    split.
    - intros [Hwf Hweight].
      split; [exact Hwf |].
      eapply sr_eq_trans.
      + exact Hweight.
      + eapply sr_eq_trans.
        * apply bounded_sum_proper.
          intros target _.
          apply sr_times_proper.
          -- apply sr_eq_sym.
             apply product_occurrence_walk_paths_sum.
          -- apply sr_eq_refl.
        * apply sr_eq_sym.
          apply target_occurrence_path_weight_sum_closed_paths.
    - intros [Hwf Hweight].
      split; [exact Hwf |].
      eapply sr_eq_trans.
      + exact Hweight.
      + eapply sr_eq_trans.
        * apply target_occurrence_path_weight_sum_closed_paths.
        * apply bounded_sum_proper.
          intros target _.
          apply sr_times_proper.
          -- apply product_occurrence_walk_paths_sum.
          -- apply sr_eq_refl.
  Qed.

  Lemma product_matrix_closed_path_weight_transition_equiv :
    forall fst input output bound weight,
      product_matrix_closed_path_weight fst input output bound weight <->
      product_transition_walk_sum_closed_path_weight
        fst input output bound weight.
  Proof.
    intros fst input output bound weight.
    split; intro Hclosed.
    - apply product_matrix_walk_sum_closed_path_weight_transition_equiv.
      apply product_matrix_closed_path_weight_walk_sum_equiv.
      exact Hclosed.
    - apply product_matrix_closed_path_weight_walk_sum_equiv.
      apply product_matrix_walk_sum_closed_path_weight_transition_equiv.
      exact Hclosed.
  Qed.

  Lemma product_matrix_closed_path_weight_occurrence_equiv :
    forall fst input output bound weight,
      product_matrix_closed_path_weight fst input output bound weight <->
      product_occurrence_walk_sum_closed_path_weight
        fst input output bound weight.
  Proof.
    intros fst input output bound weight.
    split; intro Hclosed.
    - apply product_transition_walk_sum_closed_path_weight_occurrence_equiv.
      apply product_matrix_closed_path_weight_transition_equiv.
      exact Hclosed.
    - apply product_matrix_closed_path_weight_transition_equiv.
      apply product_transition_walk_sum_closed_path_weight_occurrence_equiv.
      exact Hclosed.
  Qed.

  Lemma product_matrix_closed_path_weight_occurrence_enumerator_equiv :
    forall fst input output bound weight,
      product_matrix_closed_path_weight fst input output bound weight <->
      product_occurrence_enumerator_closed_path_weight
        fst input output bound weight.
  Proof.
    intros fst input output bound weight.
    split; intro Hclosed.
    - apply product_occurrence_walk_sum_closed_path_weight_enumerator_equiv.
      apply product_matrix_closed_path_weight_occurrence_equiv.
      exact Hclosed.
    - apply product_matrix_closed_path_weight_occurrence_equiv.
      apply product_occurrence_walk_sum_closed_path_weight_enumerator_equiv.
      exact Hclosed.
  Qed.

  Lemma product_matrix_closed_path_weight_respects_sr_eq :
    forall fst input output bound w1 w2,
      w1 ≡ w2 ->
      product_matrix_closed_path_weight fst input output bound w1 ->
      product_matrix_closed_path_weight fst input output bound w2.
  Proof.
    intros fst input output bound w1 w2 Heq [Hwf Hweight].
    split; [exact Hwf |].
    eapply sr_eq_trans.
    - apply sr_eq_sym. exact Heq.
    - exact Hweight.
  Qed.

  (** ** Occurrence-path enumerator exactness (soundness + completeness)

      The weighted aggregate of [product_occurrence_walk_paths] is already
      proved equal to [product_occurrence_walk_sum].  The lemmas below give the
      complementary *structural* characterization: the enumerated list contains
      exactly the bounded product-occurrence walks (membership soundness and
      completeness), and the list is duplicate-free.  These remove the last
      formal bridge caveat without changing any runtime code. *)

  (** Membership in one candidate-target branch of the inner enumerator. *)
  Lemma in_prepend_if_matches :
    forall input output source k occ cont p,
      In p (if product_occurrence_matches input output source k occ
            then prepend_occurrence_paths occ (cont k) else [])
      <->
      product_occurrence_matches input output source k occ = true /\
      exists tail, In tail (cont k) /\ p = occ :: tail.
  Proof.
    intros input output source k occ cont p.
    destruct (product_occurrence_matches input output source k occ).
    - unfold prepend_occurrence_paths. rewrite in_map_iff. split.
      + intros [x [Heq Hinx]]. split; [reflexivity |].
        exists x. split; [exact Hinx | symmetry; exact Heq].
      + intros [_ [tail [Hintail Hpeq]]].
        exists tail. split; [symmetry; exact Hpeq | exact Hintail].
    - simpl. split.
      + intro Hf. destruct Hf.
      + intros [Hc _]. discriminate.
  Qed.

  (** Under well-formedness, a transition stored in a state's outgoing list
      reports that state as its source. *)
  Lemma get_outgoing_tr_from :
    forall (fst : Wfst W) (s : StateId) (t : Transition W),
      wfst_well_formed fst ->
      In t (get_outgoing fst s) ->
      tr_from t = s.
  Proof.
    intros fst s t Hwf Hin.
    destruct Hwf as [_ [Hstates [Hidx _]]].
    unfold get_outgoing in Hin.
    destruct (get_state fst s) as [state |] eqn:Hget; [| destruct Hin].
    rewrite Forall_forall in Hstates.
    assert (Hsw : state_well_formed fst state)
      by (apply Hstates; eapply get_state_in_states; exact Hget).
    destruct Hsw as [_ Hout].
    rewrite Forall_forall in Hout.
    specialize (Hout t Hin).
    destruct Hout as [Hfrom _].
    rewrite Hfrom.
    exact (Hidx s state Hget).
  Qed.

  (** Inverting the boolean product transition match into its step witnesses. *)
  Lemma product_transition_matches_inv :
    forall input output source target t,
      product_transition_matches input output source target t = true ->
      exists ni no,
        consume_label (tr_input t) input
          (product_input_pos input output source) = Some ni /\
        consume_label (tr_output t) output
          (product_output_pos output source) = Some no /\
        target = product_index input output (tr_to t) ni no.
  Proof.
    intros input output source target t Hmatch.
    unfold product_transition_matches in Hmatch.
    cbv zeta in Hmatch.
    destruct (consume_label (tr_input t) input
                (product_input_pos input output source)) as [ni |] eqn:Hi;
      destruct (consume_label (tr_output t) output
                  (product_output_pos output source)) as [no |] eqn:Ho;
      try discriminate Hmatch.
    apply Nat.eqb_eq in Hmatch.
    exists ni, no.
    split; [reflexivity |].
    split; [reflexivity | exact Hmatch].
  Qed.

  Lemma product_input_pos_lt_bound :
    forall input output source,
      product_input_pos input output source < product_input_bound input.
  Proof.
    intros input output source.
    unfold product_input_pos.
    apply Nat.mod_upper_bound.
    pose proof (product_input_bound_pos input). lia.
  Qed.

  Lemma product_output_pos_lt_bound :
    forall output source,
      product_output_pos output source < product_output_bound output.
  Proof.
    intros output source.
    unfold product_output_pos.
    apply Nat.mod_upper_bound.
    pose proof (product_output_bound_pos output). lia.
  Qed.

  (** A matched product target is always within the finite product carrier. *)
  Lemma product_transition_matches_target_lt_dim :
    forall fst input output source target t,
      wfst_well_formed fst ->
      transition_in_wfst fst t ->
      product_transition_matches input output source target t = true ->
      target < product_dim fst input output.
  Proof.
    intros fst input output source target t Hwf Hin Hmatch.
    apply product_transition_matches_inv in Hmatch.
    destruct Hmatch as [ni [no [Hi [Ho Htarget]]]].
    subst target.
    apply product_index_lt_dim.
    - assert (Htwf : transition_well_formed fst t)
        by (apply transition_in_wfst_well_formed; assumption).
      destruct Htwf as [_ Hto]. exact Hto.
    - eapply consume_label_within_bound; [| exact Hi].
      apply product_input_pos_lt_bound.
    - eapply consume_label_within_bound; [| exact Ho].
      apply product_output_pos_lt_bound.
  Qed.

  (** Membership in the candidate-target enumerator. *)
  Lemma in_product_occurrence_target_paths_to :
    forall input output source occ max_target cont p,
      In p (product_occurrence_target_paths_to
              input output source occ max_target cont)
      <->
      exists next tail,
        next <= max_target /\
        product_occurrence_matches input output source next occ = true /\
        In tail (cont next) /\
        p = occ :: tail.
  Proof.
    intros input output source occ max_target cont.
    induction max_target as [| m IH]; intro p.
    - simpl. rewrite in_prepend_if_matches. split.
      + intros [Hm [tail [Hintail Hpeq]]].
        exists 0, tail.
        split; [lia |]. split; [exact Hm |].
        split; [exact Hintail | exact Hpeq].
      + intros [next [tail [Hle [Hm [Hintail Hpeq]]]]].
        assert (next = 0) as -> by lia.
        split; [exact Hm |].
        exists tail. split; [exact Hintail | exact Hpeq].
    - simpl. rewrite in_app_iff. rewrite IH. rewrite in_prepend_if_matches.
      split.
      + intros [Hleft | Hright].
        * destruct Hleft as [next [tail [Hle [Hm [Hintail Hpeq]]]]].
          exists next, tail.
          split; [lia |]. split; [exact Hm |].
          split; [exact Hintail | exact Hpeq].
        * destruct Hright as [Hm [tail [Hintail Hpeq]]].
          exists (S m), tail.
          split; [lia |]. split; [exact Hm |].
          split; [exact Hintail | exact Hpeq].
      + intros [next [tail [Hle [Hm [Hintail Hpeq]]]]].
        destruct (Nat.eq_dec next (S m)) as [Heqn | Hneq].
        * right. subst next. split; [exact Hm |].
          exists tail. split; [exact Hintail | exact Hpeq].
        * left. exists next, tail.
          split; [lia |]. split; [exact Hm |].
          split; [exact Hintail | exact Hpeq].
  Qed.

  Lemma in_product_occurrence_target_paths :
    forall dim input output source occ cont p,
      In p (product_occurrence_target_paths dim input output source occ cont)
      <->
      exists next tail,
        next < dim /\
        product_occurrence_matches input output source next occ = true /\
        In tail (cont next) /\
        p = occ :: tail.
  Proof.
    intros dim input output source occ cont p.
    destruct dim as [| max_target].
    - simpl. split.
      + intro Hf. destruct Hf.
      + intros [next [tail [Hlt _]]]. lia.
    - cbn [product_occurrence_target_paths].
      rewrite in_product_occurrence_target_paths_to. split.
      + intros [next [tail [Hle [Hm [Hintail Hpeq]]]]].
        exists next, tail. split; [lia |]. split; [exact Hm |].
        split; [exact Hintail | exact Hpeq].
      + intros [next [tail [Hlt [Hm [Hintail Hpeq]]]]].
        exists next, tail. split; [lia |]. split; [exact Hm |].
        split; [exact Hintail | exact Hpeq].
  Qed.

  (** Membership in the outgoing-transition step enumerator, with the running
      index threaded as [next_index + k] where [k] is the list position. *)
  Lemma in_product_occurrence_step_paths_to :
    forall dim input output source source_state next_index transitions cont p,
      In p (product_occurrence_step_paths_to
              dim input output source source_state next_index transitions cont)
      <->
      exists k t target tail,
        nth_error transitions k = Some t /\
        product_occurrence_matches input output source target
          (mkTransitionOccurrence source_state (next_index + k) t) = true /\
        target < dim /\
        In tail (cont target) /\
        p = mkTransitionOccurrence source_state (next_index + k) t :: tail.
  Proof.
    intros dim input output source source_state next_index transitions cont p.
    revert next_index p.
    induction transitions as [| t0 rest IH]; intros next_index p.
    - simpl. split.
      + intro Hf. destruct Hf.
      + intros [k [t [target [tail [Hnth _]]]]].
        destruct k; simpl in Hnth; discriminate.
    - simpl. rewrite in_app_iff.
      rewrite in_product_occurrence_target_paths.
      rewrite IH.
      split.
      + intros [Hleft | Hright].
        * destruct Hleft as [target [tail [Hlt [Hm [Hintail Hpeq]]]]].
          exists 0, t0, target, tail.
          replace (next_index + 0) with next_index by lia.
          split; [reflexivity |].
          split; [exact Hm |]. split; [exact Hlt |].
          split; [exact Hintail | exact Hpeq].
        * destruct Hright as
            [k' [t [target [tail [Hnth [Hm [Hlt [Hintail Hpeq]]]]]]]].
          exists (S k'), t, target, tail.
          replace (next_index + S k') with (S next_index + k') by lia.
          split; [exact Hnth |].
          split; [exact Hm |]. split; [exact Hlt |].
          split; [exact Hintail | exact Hpeq].
      + intros [k [t [target [tail [Hnth [Hm [Hlt [Hintail Hpeq]]]]]]]].
        destruct k as [| k'].
        * left.
          simpl in Hnth. injection Hnth as Heq. subst t0.
          replace (next_index + 0) with next_index in Hm, Hpeq by lia.
          exists target, tail.
          split; [exact Hlt |]. split; [exact Hm |].
          split; [exact Hintail | exact Hpeq].
        * right.
          simpl in Hnth.
          replace (next_index + S k') with (S next_index + k') in Hm, Hpeq
            by lia.
          exists k', t, target, tail.
          split; [exact Hnth |]. split; [exact Hm |]. split; [exact Hlt |].
          split; [exact Hintail | exact Hpeq].
  Qed.

  (** Connect codex's scaffolding predicate to the [nth_error]+offset form, so
      it is no longer dead code. *)
  Lemma occurrence_in_transition_suffix_iff_nth_error :
    forall transitions source_state next_index occ,
      occurrence_in_transition_suffix source_state next_index transitions occ
      <->
      exists k t,
        nth_error transitions k = Some t /\
        occ = mkTransitionOccurrence source_state (next_index + k) t.
  Proof.
    intros transitions source_state next_index occ.
    revert next_index.
    induction transitions as [| t0 rest IH]; intro next_index.
    - simpl. split.
      + intro Hf. destruct Hf.
      + intros [k [t [Hnth _]]]. destruct k; simpl in Hnth; discriminate.
    - simpl. split.
      + intros [Heq | Hsuffix].
        * exists 0, t0.
          replace (next_index + 0) with next_index by lia.
          split; [reflexivity | exact Heq].
        * apply IH in Hsuffix.
          destruct Hsuffix as [k' [t [Hnth Heq]]].
          exists (S k'), t.
          replace (next_index + S k') with (S next_index + k') by lia.
          split; [exact Hnth | exact Heq].
      + intros [k [t [Hnth Heq]]].
        destruct k as [| k'].
        * left. simpl in Hnth. injection Hnth as Hnth. subst t0.
          replace (next_index + 0) with next_index in Heq by lia.
          exact Heq.
        * right. simpl in Hnth. apply IH.
          exists k', t.
          replace (next_index + S k') with (S next_index + k') in Heq by lia.
          split; [exact Hnth | exact Heq].
  Qed.

  (** Bridge: an enumerated occurrence is a genuine product-occurrence step. *)
  Lemma product_occurrence_step_of_enum :
    forall (fst : Wfst W) input output source state k (t : Transition W) target,
      wfst_well_formed fst ->
      get_state fst (product_state input output source) = Some state ->
      nth_error (ws_outgoing state) k = Some t ->
      product_occurrence_matches input output source target
        (mkTransitionOccurrence (product_state input output source) k t) = true ->
      product_occurrence_step fst input output source target
        (mkTransitionOccurrence (product_state input output source) k t).
  Proof.
    intros fst input output source state k t target Hwf Hget Hnth Hmatch.
    unfold product_occurrence_step. split; [split |].
    - cbn [occ_source occ_transition]. symmetry.
      apply get_outgoing_tr_from with (fst := fst) (s := product_state input output source).
      + exact Hwf.
      + unfold get_outgoing. rewrite Hget. eapply nth_error_In. exact Hnth.
    - cbn [occ_source occ_index occ_transition].
      unfold get_outgoing. rewrite Hget. exact Hnth.
    - split.
      + cbn [occ_source]. reflexivity.
      + exact Hmatch.
  Qed.

  (** Soundness: every enumerated occurrence path is a bounded product walk. *)
  Lemma product_occurrence_walk_paths_sound :
    forall fst input output bound source target p,
      wfst_well_formed fst ->
      In p (product_occurrence_walk_paths fst input output bound source target) ->
      product_occurrence_walk fst input output source p target /\
      length p <= bound.
  Proof.
    intros fst input output bound.
    induction bound as [| m IH]; intros source target p Hwf Hin.
    - simpl in Hin. destruct (Nat.eqb source target) eqn:Heq.
      + simpl in Hin. destruct Hin as [Hp | []].
        subst p. apply Nat.eqb_eq in Heq.
        split; [simpl; symmetry; exact Heq | simpl; lia].
      + simpl in Hin. destruct Hin.
    - simpl in Hin. apply in_app_iff in Hin.
      destruct Hin as [Hid | Hstep].
      + destruct (Nat.eqb source target) eqn:Heq.
        * simpl in Hid. destruct Hid as [Hp | []].
          subst p. apply Nat.eqb_eq in Heq.
          split; [simpl; symmetry; exact Heq | simpl; lia].
        * simpl in Hid. destruct Hid.
      + destruct (get_state fst (product_state input output source))
          as [state |] eqn:Hget; [| destruct Hstep].
        apply in_product_occurrence_step_paths_to in Hstep.
        destruct Hstep as [k [t [mid [tail [Hnth [Hm [Hlt [Hintail Hpeq]]]]]]]].
        destruct (IH mid target tail Hwf Hintail) as [Hwalk Hlen].
        subst p. split.
        * simpl. exists mid. split.
          -- apply product_occurrence_step_of_enum with (state := state).
             ++ exact Hwf.
             ++ exact Hget.
             ++ exact Hnth.
             ++ exact Hm.
          -- exact Hwalk.
        * simpl. lia.
  Qed.

  (** Completeness: every bounded product walk is enumerated. *)
  Lemma product_occurrence_walk_paths_complete :
    forall fst input output bound source target p,
      wfst_well_formed fst ->
      product_occurrence_walk fst input output source p target ->
      length p <= bound ->
      In p (product_occurrence_walk_paths fst input output bound source target).
  Proof.
    intros fst input output bound.
    induction bound as [| m IH]; intros source target p Hwf Hwalk Hlen.
    - destruct p as [| occ tail]; [| simpl in Hlen; lia].
      simpl in Hwalk.
      assert (Hst : source = target) by (symmetry; exact Hwalk).
      simpl. rewrite Hst, Nat.eqb_refl. apply in_eq.
    - destruct p as [| occ tail].
      + simpl in Hwalk.
        assert (Hst : source = target) by (symmetry; exact Hwalk).
        simpl. rewrite Hst, Nat.eqb_refl. apply in_or_app. left. apply in_eq.
      + simpl in Hwalk. destruct Hwalk as [mid [Hstep Hwalkrest]].
        simpl in Hlen. assert (Hlen' : length tail <= m) by lia.
        destruct occ as [osrc oidx otr].
        destruct Hstep as [Hocc [Hsrc Hmatch]].
        unfold transition_occurrence_in_wfst in Hocc.
        cbn [occ_source occ_index occ_transition] in Hocc.
        destruct Hocc as [Hfrom Hnth].
        cbn [occ_source] in Hsrc.
        assert (Htin : transition_in_wfst fst otr).
        { unfold transition_in_wfst. rewrite <- Hfrom.
          eapply nth_error_In. exact Hnth. }
        assert (Hptm : product_transition_matches input output source mid otr = true).
        { unfold product_occurrence_matches in Hmatch.
          cbn [occ_source occ_transition] in Hmatch.
          apply andb_true_iff in Hmatch. destruct Hmatch as [_ Hp]. exact Hp. }
        rewrite Hsrc in Hnth.
        unfold get_outgoing in Hnth.
        simpl.
        destruct (get_state fst (product_state input output source))
          as [state |]; [| destruct oidx; simpl in Hnth; discriminate].
        apply in_or_app. right.
        rewrite in_product_occurrence_step_paths_to.
        exists oidx, otr, mid, tail.
        rewrite Hsrc in Hmatch.
        split; [exact Hnth |].
        split; [exact Hmatch |].
        split; [apply product_transition_matches_target_lt_dim
                  with (source := source) (t := otr);
                  [exact Hwf | exact Htin | exact Hptm] |].
        split; [apply IH; [exact Hwf | exact Hwalkrest | exact Hlen'] |].
        rewrite Hsrc. reflexivity.
  Qed.

  (** Exactness: the enumerated list is exactly the bounded product walks. *)
  Theorem product_occurrence_walk_paths_exact :
    forall fst input output bound source target p,
      wfst_well_formed fst ->
      (In p (product_occurrence_walk_paths fst input output bound source target)
       <->
       product_occurrence_walk fst input output source p target /\
       length p <= bound).
  Proof.
    intros fst input output bound source target p Hwf. split.
    - intro Hin. apply product_occurrence_walk_paths_sound; assumption.
    - intros [Hwalk Hlen].
      apply product_occurrence_walk_paths_complete; assumption.
  Qed.

  (** ** Duplicate-freedom of the occurrence enumerators

      The enumerated lists are duplicate-free.  This is purely structural (it
      needs no well-formedness): distinct list positions yield distinct
      occurrence indices, and for a fixed occurrence at most one product target
      matches. *)

  Lemma NoDup_single : forall (A : Type) (x : A), NoDup (x :: []).
  Proof. intros A x. constructor; [ simpl; tauto | constructor ]. Qed.

  Lemma NoDup_app_intro :
    forall (A : Type) (l1 l2 : list A),
      NoDup l1 -> NoDup l2 ->
      (forall x, In x l1 -> In x l2 -> False) ->
      NoDup (l1 ++ l2).
  Proof.
    intros A l1 l2 H1 H2 Hdisj.
    induction l1 as [| a l1 IH].
    - exact H2.
    - inversion H1 as [| a' l1' Hnotin Hnd]; subst.
      simpl. apply NoDup_cons.
      + rewrite in_app_iff. intros [Hin1 | Hin2].
        * contradiction.
        * exact (Hdisj a (or_introl eq_refl) Hin2).
      + apply IH.
        * exact Hnd.
        * intros x Hx1 Hx2. exact (Hdisj x (or_intror Hx1) Hx2).
  Qed.

  Lemma NoDup_prepend_occurrence_paths :
    forall occ l, NoDup l -> NoDup (prepend_occurrence_paths occ l).
  Proof.
    intros occ l Hnd. unfold prepend_occurrence_paths.
    induction Hnd as [| x l Hnotin Hnd IH].
    - simpl. constructor.
    - simpl. apply NoDup_cons.
      + rewrite in_map_iff. intros [y [Heq Hiny]].
        injection Heq as Heq. subst y. contradiction.
      + exact IH.
  Qed.

  Lemma NoDup_attach_target_occurrence_paths :
    forall target paths,
      NoDup paths -> NoDup (attach_target_occurrence_paths target paths).
  Proof.
    intros target paths Hnd. unfold attach_target_occurrence_paths.
    induction Hnd as [| x l Hnotin Hnd IH].
    - simpl. constructor.
    - simpl. apply NoDup_cons.
      + rewrite in_map_iff. intros [y [Heq Hiny]].
        inversion Heq. subst y. contradiction.
      + exact IH.
  Qed.

  (** A given occurrence matches at most one product target. *)
  Lemma product_occurrence_matches_unique_target :
    forall input output source t1 t2 occ,
      product_occurrence_matches input output source t1 occ = true ->
      product_occurrence_matches input output source t2 occ = true ->
      t1 = t2.
  Proof.
    intros input output source t1 t2 occ Hm1 Hm2.
    unfold product_occurrence_matches in Hm1, Hm2.
    apply andb_true_iff in Hm1. destruct Hm1 as [_ Hp1].
    apply andb_true_iff in Hm2. destruct Hm2 as [_ Hp2].
    apply product_transition_matches_inv in Hp1.
    apply product_transition_matches_inv in Hp2.
    destruct Hp1 as [ni1 [no1 [Hi1 [Ho1 Ht1]]]].
    destruct Hp2 as [ni2 [no2 [Hi2 [Ho2 Ht2]]]].
    rewrite Hi1 in Hi2. injection Hi2 as Hi2. subst ni2.
    rewrite Ho1 in Ho2. injection Ho2 as Ho2. subst no2.
    rewrite Ht1, Ht2. reflexivity.
  Qed.

  Lemma product_occurrence_target_paths_to_NoDup :
    forall input output source occ max_target cont,
      (forall next, NoDup (cont next)) ->
      NoDup (product_occurrence_target_paths_to
               input output source occ max_target cont).
  Proof.
    intros input output source occ max_target cont Hcont.
    induction max_target as [| m IH].
    - simpl. destruct (product_occurrence_matches input output source 0 occ).
      + apply NoDup_prepend_occurrence_paths. apply Hcont.
      + constructor.
    - simpl. apply NoDup_app_intro.
      + exact IH.
      + destruct (product_occurrence_matches input output source (S m) occ).
        * apply NoDup_prepend_occurrence_paths. apply Hcont.
        * constructor.
      + intros x Hx1 Hx2.
        apply in_product_occurrence_target_paths_to in Hx1.
        destruct Hx1 as [next1 [tail1 [Hle1 [Hm1 [_ _]]]]].
        apply in_prepend_if_matches in Hx2.
        destruct Hx2 as [HmS [_ _]].
        assert (next1 = S m).
        { apply product_occurrence_matches_unique_target
            with (input := input) (output := output) (source := source) (occ := occ);
            [exact Hm1 | exact HmS]. }
        lia.
  Qed.

  Lemma product_occurrence_target_paths_NoDup :
    forall dim input output source occ cont,
      (forall next, NoDup (cont next)) ->
      NoDup (product_occurrence_target_paths dim input output source occ cont).
  Proof.
    intros dim input output source occ cont Hcont.
    destruct dim as [| max_target].
    - simpl. constructor.
    - cbn [product_occurrence_target_paths].
      apply product_occurrence_target_paths_to_NoDup. exact Hcont.
  Qed.

  Lemma product_occurrence_step_paths_to_NoDup :
    forall dim input output source source_state next_index transitions cont,
      (forall next, NoDup (cont next)) ->
      NoDup (product_occurrence_step_paths_to
               dim input output source source_state next_index transitions cont).
  Proof.
    intros dim input output source source_state next_index transitions cont Hcont.
    revert next_index.
    induction transitions as [| t0 rest IH]; intro next_index.
    - simpl. constructor.
    - simpl. apply NoDup_app_intro.
      + apply product_occurrence_target_paths_NoDup. exact Hcont.
      + apply IH.
      + intros x Hx1 Hx2.
        apply in_product_occurrence_target_paths in Hx1.
        destruct Hx1 as [next1 [tail1 [_ [_ [_ Heq1]]]]].
        apply in_product_occurrence_step_paths_to in Hx2.
        destruct Hx2 as [k2 [t2 [target2 [tail2 [_ [_ [_ [_ Heq2]]]]]]]].
        rewrite Heq1 in Heq2. injection Heq2. intros. lia.
  Qed.

  Lemma product_occurrence_walk_paths_NoDup :
    forall fst input output bound source target,
      NoDup (product_occurrence_walk_paths fst input output bound source target).
  Proof.
    intros fst input output bound.
    induction bound as [| m IH]; intros source target.
    - simpl. destruct (Nat.eqb source target).
      + apply NoDup_single.
      + constructor.
    - simpl. apply NoDup_app_intro.
      + destruct (Nat.eqb source target).
        * apply NoDup_single.
        * constructor.
      + destruct (get_state fst (product_state input output source)) as [state |].
        * apply product_occurrence_step_paths_to_NoDup. intro next. apply IH.
        * constructor.
      + intros x Hx1 Hx2.
        destruct (Nat.eqb source target).
        * simpl in Hx1. destruct Hx1 as [Hx1 | Hx1]; [| destruct Hx1].
          subst x.
          destruct (get_state fst (product_state input output source))
            as [state |]; [| destruct Hx2].
          apply in_product_occurrence_step_paths_to in Hx2.
          destruct Hx2 as [k [t [target0 [tail [_ [_ [_ [_ Heq]]]]]]]].
          discriminate Heq.
        * destruct Hx1.
  Qed.

  (** ** Closed-path enumerator exactness and duplicate-freedom *)

  Lemma in_attach_target_occurrence_paths :
    forall target paths entry,
      In entry (attach_target_occurrence_paths target paths)
      <-> exists path, entry = (target, path) /\ In path paths.
  Proof.
    intros target paths entry. unfold attach_target_occurrence_paths.
    rewrite in_map_iff. split.
    - intros [path [Heq Hin]]. exists path. split; [symmetry; exact Heq | exact Hin].
    - intros [path [Heq Hin]]. exists path. split; [symmetry; exact Heq | exact Hin].
  Qed.

  Lemma in_product_occurrence_closed_paths_to :
    forall fst input output bound max_target tgt p,
      In (tgt, p) (product_occurrence_closed_paths_to fst input output bound max_target)
      <->
      tgt <= max_target /\
      In p (product_occurrence_walk_paths fst input output bound
              (product_start_index fst input output) tgt).
  Proof.
    intros fst input output bound max_target tgt p.
    induction max_target as [| m IH].
    - simpl. rewrite in_attach_target_occurrence_paths. split.
      + intros [path [Heq Hin]]. injection Heq as Htgt Hpath. subst tgt p.
        split; [lia | exact Hin].
      + intros [Hle Hin]. assert (tgt = 0) as -> by lia.
        exists p. split; [reflexivity | exact Hin].
    - simpl. rewrite in_app_iff. rewrite IH.
      rewrite in_attach_target_occurrence_paths. split.
      + intros [[Hle Hin] | [path [Heq Hin]]].
        * split; [lia | exact Hin].
        * injection Heq as Htgt Hpath. subst tgt p.
          split; [lia | exact Hin].
      + intros [Hle Hin].
        destruct (Nat.eq_dec tgt (S m)) as [Heqn | Hneq].
        * right. subst tgt. exists p. split; [reflexivity | exact Hin].
        * left. split; [lia | exact Hin].
  Qed.

  Theorem product_occurrence_closed_paths_exact :
    forall fst input output bound tgt p,
      wfst_well_formed fst ->
      (In (tgt, p) (product_occurrence_closed_paths fst input output bound)
       <->
       tgt < product_dim fst input output /\
       product_occurrence_walk fst input output
         (product_start_index fst input output) p tgt /\
       length p <= bound).
  Proof.
    intros fst input output bound tgt p Hwf.
    unfold product_occurrence_closed_paths.
    destruct (product_dim fst input output) as [| max_target].
    - split.
      + intro Hf. destruct Hf.
      + intros [Hlt _]. lia.
    - rewrite in_product_occurrence_closed_paths_to.
      rewrite (product_occurrence_walk_paths_exact fst input output bound
                 (product_start_index fst input output) tgt p Hwf).
      split.
      + intros [Hle [Hwalk Hlen]]. split; [lia | split; [exact Hwalk | exact Hlen]].
      + intros [Hlt [Hwalk Hlen]]. split; [lia | split; [exact Hwalk | exact Hlen]].
  Qed.

  Lemma product_occurrence_closed_paths_NoDup :
    forall fst input output bound,
      NoDup (product_occurrence_closed_paths fst input output bound).
  Proof.
    intros fst input output bound.
    unfold product_occurrence_closed_paths.
    destruct (product_dim fst input output) as [| max_target].
    - constructor.
    - induction max_target as [| m IH].
      + simpl. apply NoDup_attach_target_occurrence_paths.
        apply product_occurrence_walk_paths_NoDup.
      + simpl. apply NoDup_app_intro.
        * exact IH.
        * apply NoDup_attach_target_occurrence_paths.
          apply product_occurrence_walk_paths_NoDup.
        * intros [t1 p1] Hx1 Hx2.
          apply in_product_occurrence_closed_paths_to in Hx1.
          destruct Hx1 as [Hle1 _].
          apply in_attach_target_occurrence_paths in Hx2.
          destruct Hx2 as [path [Heq2 _]].
          inversion Heq2. subst. lia.
  Qed.

  (** Packaged exactness predicate, mirroring [exact_matching_accepting_paths]
      in Language.v: the enumerated list is duplicate-free and its membership is
      exactly the bounded product-occurrence walks. *)
  Definition exact_product_occurrence_walk_paths
      (fst : Wfst W) (input output : list Label) (bound : nat)
      (source target : nat) (paths : list OccurrencePath) : Prop :=
    NoDup paths /\
    forall p,
      In p paths <->
      product_occurrence_walk fst input output source p target /\
      length p <= bound.

  Theorem product_occurrence_walk_paths_is_exact :
    forall fst input output bound source target,
      wfst_well_formed fst ->
      exact_product_occurrence_walk_paths fst input output bound source target
        (product_occurrence_walk_paths fst input output bound source target).
  Proof.
    intros fst input output bound source target Hwf.
    unfold exact_product_occurrence_walk_paths. split.
    - apply product_occurrence_walk_paths_NoDup.
    - intro p. apply product_occurrence_walk_paths_exact. exact Hwf.
  Qed.

  (** ** Grounding the product semantics in independent accepting-path oracles

      The weighted equivalence chain above is internally consistent but
      self-referential: both the matrix side and the enumerator side bottom out
      in [product_transition_matches]/[consume_label].  The lemmas below remove
      that self-reference by relating the product-occurrence walks to genuinely
      independent notions — [accepting_path] (pure connectivity + arc
      membership) and [accepting_path_weight] — neither of which mentions
      [product_transition_matches] or [product_index]. *)

  (** H1: a connected-from path is valid (the converse of the existing
      valid-implies-connected direction). *)
  Lemma path_connects_from_path_valid :
    forall (p : @Path W) (source : StateId),
      path_connects_from source p -> path_valid p.
  Proof.
    induction p as [| t1 rest IH]; intros source Hconn.
    - exact I.
    - destruct Hconn as [Hfrom1 Hconn'].
      destruct rest as [| t2 rest'].
      + exact I.
      + simpl. split.
        * destruct Hconn' as [Hfrom2 _]. symmetry. exact Hfrom2.
        * apply IH with (source := tr_to t1). exact Hconn'.
  Qed.

  (** A.1: converse of [product_matrix_step_matches]. *)
  Lemma product_matrix_step_of_matches :
    forall fst input output source target t,
      transition_in_wfst fst t ->
      product_state input output source = tr_from t ->
      product_transition_matches input output source target t = true ->
      product_matrix_step fst input output source target t.
  Proof.
    intros fst input output source target t Hin Hsrc Hmatch.
    unfold product_matrix_step.
    split; [exact Hin |].
    split; [exact Hsrc |].
    apply product_transition_matches_inv in Hmatch.
    destruct Hmatch as [ni [no [Hi [Ho Htgt]]]].
    exists ni, no. split; [exact Hi |]. split; [exact Ho | exact Htgt].
  Qed.

  (** B.6: occurrence-path weight equals the plain path weight of its
      projected transitions. *)
  Lemma occurrence_path_weight_is_path_weight :
    forall occs,
      occurrence_path_weight occs ≡ path_weight (occurrence_path_transitions occs).
  Proof.
    induction occs as [| occ rest IH].
    - simpl. apply sr_eq_refl.
    - simpl. apply sr_times_proper; [apply sr_eq_refl | exact IH].
  Qed.

  (** Zero companion: non-(position-)accepting targets contribute 𝟘. *)
  Lemma target_occurrence_path_weight_zero_when_not_final_position :
    forall fst input output target occs,
      product_accepts_final_position input output target = false ->
      target_occurrence_path_weight fst input output (target, occs) ≡ 𝟘.
  Proof.
    intros fst input output target occs Hpos.
    cbn [target_occurrence_path_weight].
    unfold product_final_weight_or_zero.
    rewrite Hpos.
    apply sr_zero_times_r.
  Qed.

  (** A.2 (crux): a product-occurrence walk reads out into a genuinely connected
      WFST path that consumes the labels exactly — the reverse of
      [product_matrix_walk_of_connected_path], read backwards through the
      decoders.  This relates the product/label encoding to [path_connects_from]
      and [consume_path_labels] without any further appeal to
      [product_transition_matches]. *)
  Lemma product_occurrence_walk_connects_path :
    forall fst input output,
      wfst_well_formed fst ->
      forall source p target,
        product_input_pos input output source < product_input_bound input ->
        product_output_pos output source < product_output_bound output ->
        product_occurrence_walk fst input output source p target ->
        path_connects_from (product_state input output source)
          (occurrence_path_transitions p) /\
        Forall (transition_occurrence_in_wfst fst) p /\
        consume_path_labels input output (occurrence_path_transitions p)
          (product_input_pos input output source)
          (product_output_pos output source)
          = Some (product_input_pos input output target,
                  product_output_pos output target) /\
        path_end_state_from (product_state input output source)
          (occurrence_path_transitions p)
          = product_state input output target.
  Proof.
    intros fst input output Hwf source p target Hib Hob Hwalk.
    revert source Hib Hob Hwalk.
    induction p as [| occ rest IH]; intros source Hib Hob Hwalk.
    - cbn [product_occurrence_walk] in Hwalk. subst target.
      cbn [occurrence_path_transitions map path_connects_from
           path_end_state_from consume_path_labels].
      split; [exact I |].
      split; [constructor |].
      split; reflexivity.
    - cbn [product_occurrence_walk] in Hwalk.
      destruct Hwalk as [mid [Hstep Hrest]].
      destruct Hstep as [Hocc [Hsrceq Hmatch]].
      unfold product_occurrence_matches in Hmatch.
      apply andb_true_iff in Hmatch. destruct Hmatch as [_ Hptm].
      apply product_transition_matches_inv in Hptm.
      destruct Hptm as [ni [no [Hi [Ho Hmideq]]]].
      assert (Hni : ni < product_input_bound input)
        by (eapply consume_label_within_bound; [exact Hib | exact Hi]).
      assert (Hno : no < product_output_bound output)
        by (eapply consume_label_within_bound; [exact Hob | exact Ho]).
      assert (Hmid_ip : product_input_pos input output mid = ni)
        by (rewrite Hmideq; apply product_input_pos_index; assumption).
      assert (Hmid_op : product_output_pos output mid = no)
        by (rewrite Hmideq; apply product_output_pos_index; assumption).
      assert (Hmid_st :
        product_state input output mid = tr_to (occ_transition occ))
        by (rewrite Hmideq; apply product_state_index; assumption).
      assert (Hib_mid :
        product_input_pos input output mid < product_input_bound input)
        by (rewrite Hmid_ip; exact Hni).
      assert (Hob_mid :
        product_output_pos output mid < product_output_bound output)
        by (rewrite Hmid_op; exact Hno).
      specialize (IH mid Hib_mid Hob_mid Hrest).
      destruct IH as [IHconn [IHforall [IHconsume IHend]]].
      assert (Hfromt :
        tr_from (occ_transition occ) = product_state input output source).
      { unfold transition_occurrence_in_wfst in Hocc.
        destruct Hocc as [Hf _]. rewrite <- Hf. exact Hsrceq. }
      split.
      + cbn [occurrence_path_transitions map path_connects_from].
        split; [exact Hfromt |].
        rewrite Hmid_st in IHconn. exact IHconn.
      + split.
        * constructor; [exact Hocc | exact IHforall].
        * split.
          -- cbn [occurrence_path_transitions map consume_path_labels].
             rewrite Hi, Ho.
             rewrite Hmid_ip, Hmid_op in IHconsume.
             exact IHconsume.
          -- cbn [occurrence_path_transitions map path_end_state_from].
             rewrite Hmid_st in IHend. exact IHend.
  Qed.

  (** A.4a: structural recovery at the start index for a position-accepting
      target — a real connected, in-WFST path consuming the full strings. *)
  Lemma product_occurrence_closed_walk_recovers_path :
    forall fst input output p target,
      wfst_well_formed fst ->
      product_occurrence_walk fst input output
        (product_start_index fst input output) p target ->
      product_accepts_final_position input output target = true ->
      path_connects_from (wfst_start fst) (occurrence_path_transitions p) /\
      path_transitions_in_wfst fst (occurrence_path_transitions p) /\
      path_end_state_from (wfst_start fst) (occurrence_path_transitions p)
        = product_state input output target /\
      consume_path_labels input output (occurrence_path_transitions p) 0 0
        = Some (length input, length output).
  Proof.
    intros fst input output p target Hwf Hwalk Hfinal.
    assert (Hpip0 :
      product_input_pos input output (product_start_index fst input output) = 0).
    { unfold product_start_index. apply product_input_pos_index;
        [apply product_input_bound_pos | apply product_output_bound_pos]. }
    assert (Hpop0 :
      product_output_pos output (product_start_index fst input output) = 0).
    { unfold product_start_index. apply product_output_pos_index.
      apply product_output_bound_pos. }
    assert (Hpst0 :
      product_state input output (product_start_index fst input output)
        = wfst_start fst).
    { unfold product_start_index. apply product_state_index;
        [apply product_input_bound_pos | apply product_output_bound_pos]. }
    assert (Hbi :
      product_input_pos input output (product_start_index fst input output)
        < product_input_bound input)
      by (rewrite Hpip0; apply product_input_bound_pos).
    assert (Hbo :
      product_output_pos output (product_start_index fst input output)
        < product_output_bound output)
      by (rewrite Hpop0; apply product_output_bound_pos).
    pose proof (product_occurrence_walk_connects_path fst input output Hwf
      (product_start_index fst input output) p target Hbi Hbo Hwalk)
      as [Hconn [Hforall [Hconsume Hend]]].
    rewrite Hpst0 in Hconn, Hend.
    rewrite Hpip0, Hpop0 in Hconsume.
    unfold product_accepts_final_position in Hfinal.
    apply andb_true_iff in Hfinal. destruct Hfinal as [Hf1 Hf2].
    apply Nat.eqb_eq in Hf1. apply Nat.eqb_eq in Hf2.
    rewrite Hf1, Hf2 in Hconsume.
    split; [exact Hconn |].
    split; [apply occurrence_path_transitions_in_wfst; exact Hforall |].
    split; [exact Hend | exact Hconsume].
  Qed.

  (** A.4b: a position-accepting walk landing on a *final* WFST state recovers a
      genuine [accepting_path].  The finality hypothesis is required (D1):
      position-acceptance alone does not imply state finality.  The matching
      (label-transduction) half is assembled in Language.v. *)
  Lemma product_occurrence_closed_walk_accepting :
    forall fst input output p target,
      wfst_well_formed fst ->
      product_occurrence_walk fst input output
        (product_start_index fst input output) p target ->
      product_accepts_final_position input output target = true ->
      is_final fst (product_state input output target) = true ->
      accepting_path fst (occurrence_path_transitions p).
  Proof.
    intros fst input output p target Hwf Hwalk Hfinal Hisfinal.
    pose proof (product_occurrence_closed_walk_recovers_path fst input output p
      target Hwf Hwalk Hfinal) as [Hconn [Htrans [Hend _]]].
    unfold accepting_path, path_valid_in_wfst.
    split; [| split].
    - apply path_connects_from_path_valid with (source := wfst_start fst).
      exact Hconn.
    - exact Htrans.
    - destruct (occurrence_path_transitions p) as [| t q'] eqn:Hq.
      + simpl in Hend. rewrite Hend. exact Hisfinal.
      + split.
        * simpl in Hconn. destruct Hconn as [Hfrom _]. exact Hfrom.
        * rewrite path_end_state_from_cons_last with (default := t) in Hend.
          rewrite Hend. exact Hisfinal.
  Qed.

  (** ** Reverse inclusion support: real accepting paths are enumerated

      Helpers for proving every accepting transducing path appears in the
      product-occurrence closed-path enumeration (the completeness direction
      complementing [product_occurrence_closed_walk_accepting]). *)

  (** H5: a final state is a valid (in-range) state. *)
  Lemma is_final_state_valid :
    forall (fst : Wfst W) (s : StateId),
      wfst_well_formed fst ->
      is_final fst s = true ->
      s < wfst_num_states fst.
  Proof.
    intros fst s Hwf Hfin.
    destruct Hwf as [_ [_ [_ Hlen]]].
    unfold is_final in Hfin.
    destruct (get_state fst s) as [state |] eqn:Hget; [| discriminate].
    unfold get_state in Hget.
    assert (Hsome : nth_error (wfst_states fst) s <> None)
      by (rewrite Hget; discriminate).
    apply nth_error_Some in Hsome.
    rewrite Hlen in Hsome.
    exact Hsome.
  Qed.

  (** H6: an accepting path ends at a valid state. *)
  Lemma accepting_path_end_state_valid :
    forall (fst : Wfst W) (p : @Path W),
      wfst_well_formed fst ->
      accepting_path fst p ->
      path_end_state_from (wfst_start fst) p < wfst_num_states fst.
  Proof.
    intros fst p Hwf Hacc.
    unfold accepting_path, path_valid_in_wfst in Hacc.
    destruct Hacc as [_ [_ Hfinal]].
    destruct p as [| t rest].
    - simpl. apply is_final_state_valid; [exact Hwf | exact Hfinal].
    - destruct Hfinal as [_ Hisfin].
      rewrite path_end_state_from_cons_last with (default := t).
      apply is_final_state_valid; [exact Hwf | exact Hisfin].
  Qed.

  (** H3: lift a product-matrix walk to an occurrence walk along a given
      occurrence projection. *)
  Lemma product_matrix_walk_to_occurrence_walk :
    forall fst input output occs source target,
      Forall (transition_occurrence_in_wfst fst) occs ->
      product_matrix_walk fst input output source
        (occurrence_path_transitions occs) target ->
      product_occurrence_walk fst input output source occs target.
  Proof.
    intros fst input output occs.
    induction occs as [| occ rest IH]; intros source target Hforall Hwalk.
    - cbn [occurrence_path_transitions map product_matrix_walk] in Hwalk.
      cbn [product_occurrence_walk]. exact Hwalk.
    - cbn [occurrence_path_transitions map product_matrix_walk] in Hwalk.
      destruct Hwalk as [next [Hstep Hrest]].
      pose proof (Forall_inv Hforall) as Hocc.
      pose proof (Forall_inv_tail Hforall) as Htail.
      assert (Hsrc : occ_source occ = product_state input output source).
      { unfold transition_occurrence_in_wfst in Hocc.
        destruct Hocc as [Hfrom _].
        destruct Hstep as [_ [Hstsrc _]].
        rewrite Hfrom. symmetry. exact Hstsrc. }
      assert (Hptm : product_transition_matches input output source next
                       (occ_transition occ) = true)
        by (eapply product_matrix_step_matches; exact Hstep).
      cbn [product_occurrence_walk].
      exists next. split.
      + unfold product_occurrence_step. split; [exact Hocc |].
        split; [exact Hsrc |].
        unfold product_occurrence_matches. apply andb_true_intro. split.
        * apply Nat.eqb_eq. exact Hsrc.
        * exact Hptm.
      + apply IH; [exact Htail | exact Hrest].
  Qed.

End WfstMatrixSemantics.
