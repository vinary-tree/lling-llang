(** * WpdsPortability — stable identities and flat portable evidence

    This theory is independent of the protected WPDS implementation work.  It
    specifies the portability boundary that a later refinement must satisfy:
    caller-owned external rule keys, dense hot-path identifiers, exact replay
    identity, portable witness nodes, bounded cursor decoding, and explicit
    heap-resident teardown state.
*)

From Stdlib Require Import Arith.Arith Arith.PeanoNat Bool.Bool Lists.List.
From Stdlib Require Import Lists.ListDec Lia Wellfounded.Inclusion.
From Stdlib Require Import Wellfounded.Inverse_Image.
Import ListNotations.

(** ** Caller-owned external keys and dense identifiers *)

Definition external_rule_key : Type := nat.
Definition dense_rule_id : Type := nat.

Fixpoint index_of
    (key : external_rule_key) (keys : list external_rule_key)
    : option dense_rule_id :=
  match keys with
  | [] => None
  | head :: tail =>
      if Nat.eq_dec key head then Some 0
      else option_map S (index_of key tail)
  end.

Lemma index_of_sound : forall key keys dense,
  index_of key keys = Some dense -> nth_error keys dense = Some key.
Proof.
  intros key keys. induction keys as [|head tail IH]; intros dense Hindex.
  - discriminate.
  - simpl in Hindex. destruct (Nat.eq_dec key head) as [Hequal | Hneq].
    + subst head. inversion Hindex. reflexivity.
    + destruct (index_of key tail) as [tail_index |] eqn:Htail.
      * inversion Hindex. subst dense. simpl. now apply IH.
      * discriminate.
Qed.

Lemma index_of_complete : forall key keys dense,
  NoDup keys -> nth_error keys dense = Some key ->
  index_of key keys = Some dense.
Proof.
  intros key keys. induction keys as [|head tail IH]; intros dense Hnodup Hnth.
  - destruct dense; discriminate.
  - inversion Hnodup as [|head0 tail0 Hnotin Htail]; subst.
    destruct dense as [|tail_index].
    + simpl in Hnth. inversion Hnth. subst head.
      simpl. destruct (Nat.eq_dec key key); congruence.
    + simpl in Hnth. simpl. destruct (Nat.eq_dec key head) as [Hequal | Hneq].
      * subst head. exfalso. apply Hnotin.
        eapply nth_error_In. exact Hnth.
      * rewrite (IH tail_index Htail Hnth). reflexivity.
Qed.

Definition build_dense_map (keys : list external_rule_key)
    : option (list dense_rule_id) :=
  if NoDup_dec Nat.eq_dec keys
  then Some (seq 0 (length keys))
  else None.

Theorem duplicate_external_keys_are_rejected : forall keys,
  ~ NoDup keys -> build_dense_map keys = None.
Proof.
  intros keys Hduplicate. unfold build_dense_map.
  destruct (NoDup_dec Nat.eq_dec keys) as [Hnodup | Hnot].
  - exfalso. exact (Hduplicate Hnodup).
  - reflexivity.
Qed.

Theorem accepted_dense_map_is_contiguous : forall keys dense,
  build_dense_map keys = Some dense -> dense = seq 0 (length keys).
Proof.
  intros keys dense. unfold build_dense_map.
  destruct (NoDup_dec Nat.eq_dec keys); congruence.
Qed.

Theorem external_to_dense_is_injective : forall keys first second dense,
  NoDup keys ->
  index_of first keys = Some dense ->
  index_of second keys = Some dense ->
  first = second.
Proof.
  intros keys first second dense Hnodup Hfirst Hsecond.
  pose proof (index_of_sound first keys dense Hfirst) as Hnth_first.
  pose proof (index_of_sound second keys dense Hsecond) as Hnth_second.
  congruence.
Qed.

Theorem dense_to_external_round_trip : forall keys dense,
  NoDup keys -> dense < length keys ->
  exists key,
    nth_error keys dense = Some key /\ index_of key keys = Some dense.
Proof.
  intros keys dense Hnodup Hrange.
  destruct (nth_error keys dense) as [key |] eqn:Hnth.
  - exists key. split; auto. now apply index_of_complete.
  - apply nth_error_None in Hnth. lia.
Qed.

Theorem same_snapshot_key_tape_has_stable_dense_ids : forall keys key first second,
  NoDup keys ->
  index_of key keys = Some first ->
  index_of key keys = Some second ->
  first = second.
Proof. intros. congruence. Qed.

(** ** Exact replay identity *)

Record replay_identity : Type := {
  rule_snapshot_id : nat;
  context_digest : nat;
  query_digest : nat;
  semantic_digest : nat;
  codec_profile_id : nat
}.

Definition same_replay_identity
    (expected observed : replay_identity) : bool :=
  Nat.eqb (rule_snapshot_id expected) (rule_snapshot_id observed)
  && Nat.eqb (context_digest expected) (context_digest observed)
  && Nat.eqb (query_digest expected) (query_digest observed)
  && Nat.eqb (semantic_digest expected) (semantic_digest observed)
  && Nat.eqb (codec_profile_id expected) (codec_profile_id observed).

Theorem same_replay_identity_exact : forall expected observed,
  same_replay_identity expected observed = true <->
  rule_snapshot_id expected = rule_snapshot_id observed /\
  context_digest expected = context_digest observed /\
  query_digest expected = query_digest observed /\
  semantic_digest expected = semantic_digest observed /\
  codec_profile_id expected = codec_profile_id observed.
Proof.
  intros expected observed. unfold same_replay_identity.
  repeat rewrite andb_true_iff. repeat rewrite Nat.eqb_eq. tauto.
Qed.

Theorem stale_rule_snapshot_rejects_replay : forall expected observed,
  rule_snapshot_id expected <> rule_snapshot_id observed ->
  same_replay_identity expected observed = false.
Proof.
  intros expected observed Hneq.
  destruct (same_replay_identity expected observed) eqn:Hsame; auto.
  apply same_replay_identity_exact in Hsame. tauto.
Qed.

Theorem context_mismatch_rejects_replay : forall expected observed,
  context_digest expected <> context_digest observed ->
  same_replay_identity expected observed = false.
Proof.
  intros expected observed Hneq.
  destruct (same_replay_identity expected observed) eqn:Hsame; auto.
  apply same_replay_identity_exact in Hsame. tauto.
Qed.

Theorem query_mismatch_rejects_replay : forall expected observed,
  query_digest expected <> query_digest observed ->
  same_replay_identity expected observed = false.
Proof.
  intros expected observed Hneq.
  destruct (same_replay_identity expected observed) eqn:Hsame; auto.
  apply same_replay_identity_exact in Hsame. tauto.
Qed.

Theorem semantic_mismatch_rejects_replay : forall expected observed,
  semantic_digest expected <> semantic_digest observed ->
  same_replay_identity expected observed = false.
Proof.
  intros expected observed Hneq.
  destruct (same_replay_identity expected observed) eqn:Hsame; auto.
  apply same_replay_identity_exact in Hsame. tauto.
Qed.

Theorem codec_profile_mismatch_rejects_replay : forall expected observed,
  codec_profile_id expected <> codec_profile_id observed ->
  same_replay_identity expected observed = false.
Proof.
  intros expected observed Hneq.
  destruct (same_replay_identity expected observed) eqn:Hsame; auto.
  apply same_replay_identity_exact in Hsame. tauto.
Qed.

Record replay_request : Type := {
  expected_identity : replay_identity;
  observed_identity : replay_identity;
  checksum_valid : bool;
  canonical_payload : bool;
  within_budget : bool;
  cancellation_requested : bool;
  witness_valid : bool
}.

Definition replay_admitted (request : replay_request) : bool :=
  same_replay_identity
    (expected_identity request) (observed_identity request)
  && checksum_valid request
  && canonical_payload request
  && within_budget request
  && negb (cancellation_requested request)
  && witness_valid request.

Theorem admitted_replay_has_exact_identity : forall request,
  replay_admitted request = true ->
  same_replay_identity
    (expected_identity request) (observed_identity request) = true.
Proof.
  intros request Hadmitted. unfold replay_admitted in Hadmitted.
  apply andb_true_iff in Hadmitted as [Hthrough_cancel _].
  apply andb_true_iff in Hthrough_cancel as [Hthrough_budget _].
  apply andb_true_iff in Hthrough_budget as [Hthrough_payload _].
  apply andb_true_iff in Hthrough_payload as [Hthrough_checksum _].
  apply andb_true_iff in Hthrough_checksum as [Hidentity _].
  exact Hidentity.
Qed.

Theorem malformed_payload_never_publishes : forall request,
  canonical_payload request = false -> replay_admitted request = false.
Proof.
  intros request Hmalformed. unfold replay_admitted.
  rewrite Hmalformed. now repeat rewrite andb_false_r.
Qed.

Theorem checksum_failure_never_publishes : forall request,
  checksum_valid request = false -> replay_admitted request = false.
Proof.
  intros request Hchecksum. unfold replay_admitted.
  rewrite Hchecksum. now repeat rewrite andb_false_r.
Qed.

Theorem budget_exhaustion_never_publishes : forall request,
  within_budget request = false -> replay_admitted request = false.
Proof.
  intros request Hbudget. unfold replay_admitted.
  rewrite Hbudget. now repeat rewrite andb_false_r.
Qed.

Theorem cancellation_never_publishes : forall request,
  cancellation_requested request = true -> replay_admitted request = false.
Proof.
  intros request Hcancelled. unfold replay_admitted.
  rewrite Hcancelled. simpl. now repeat rewrite andb_false_r.
Qed.

Theorem invalid_witness_never_publishes : forall request,
  witness_valid request = false -> replay_admitted request = false.
Proof.
  intros request Hinvalid. unfold replay_admitted.
  now rewrite Hinvalid, andb_false_r.
Qed.

(** ** Portable, premises-first witness tapes *)

Record portable_proof_node : Type := {
  portable_fact : nat;
  portable_rule : option external_rule_key;
  portable_premises : list nat
}.

Definition key_resolves
    (keys : list external_rule_key) (node : portable_proof_node) : bool :=
  match portable_rule node with
  | None => true
  | Some key =>
      match index_of key keys with Some _ => true | None => false end
  end.

Definition premises_precede
    (index : nat) (node : portable_proof_node) : Prop :=
  Forall (fun premise => premise < index) (portable_premises node).

Definition portable_witness_valid
    (keys : list external_rule_key) (nodes : list portable_proof_node) : Prop :=
  NoDup keys /\
  forall index node,
    nth_error nodes index = Some node ->
    premises_precede index node /\ key_resolves keys node = true.

Theorem portable_witness_rejects_unknown_rule : forall keys nodes index node key,
  portable_witness_valid keys nodes ->
  nth_error nodes index = Some node ->
  portable_rule node = Some key ->
  index_of key keys = None -> False.
Proof.
  intros keys nodes index node key Hvalid Hnode Hrule Hmissing.
  destruct Hvalid as [_ Hall].
  specialize (Hall index node Hnode) as [_ Hkey].
  unfold key_resolves in Hkey. rewrite Hrule, Hmissing in Hkey. discriminate.
Qed.

Theorem every_portable_premise_is_earlier : forall keys nodes index node premise,
  portable_witness_valid keys nodes ->
  nth_error nodes index = Some node ->
  In premise (portable_premises node) -> premise < index.
Proof.
  intros keys nodes index node premise Hvalid Hnode Hin.
  destruct Hvalid as [_ Hall].
  specialize (Hall index node Hnode) as [Horder _].
  eapply Forall_forall in Horder; eauto.
Qed.

Section PortableReplaySoundness.
  Variable node_of : nat -> option portable_proof_node.
  Variable Meaning : nat -> Prop.
  Variable RuleKnown : external_rule_key -> Prop.

  Definition proof_semantic (proof : nat) : Prop :=
    exists node, node_of proof = Some node /\ Meaning (portable_fact node).

  Record local_replay_certificate : Type := {
    portable_local_sound : forall proof node,
      node_of proof = Some node ->
      (forall key, portable_rule node = Some key -> RuleKnown key) ->
      (forall premise, In premise (portable_premises node) ->
        proof_semantic premise) ->
      Meaning (portable_fact node)
  }.

  Inductive portable_replay : list nat -> list nat -> list nat -> Prop :=
  | PortableReplayDone : forall established,
      portable_replay [] established established
  | PortableReplayStep : forall proof node remaining established final,
      node_of proof = Some node ->
      ~ In proof established ->
      (forall key, portable_rule node = Some key -> RuleKnown key) ->
      (forall premise, In premise (portable_premises node) ->
        In premise established) ->
      portable_replay remaining (proof :: established) final ->
      portable_replay (proof :: remaining) established final.

  Theorem portable_replay_preserves_soundness :
    forall (certificate : local_replay_certificate) pending established final,
      portable_replay pending established final ->
      Forall proof_semantic established ->
      Forall proof_semantic final.
  Proof.
    intros certificate pending established final Hreplay.
    induction Hreplay as
      [established0
      | proof node remaining established0 final0 Hnode Hfresh Hknown Hpremises
        Htail IH].
    - auto.
    - intros Hsemantic. apply IH. constructor.
      + exists node. split; auto.
        eapply portable_local_sound; eauto.
        intros premise Hin.
        eapply Forall_forall in Hsemantic.
        * exact Hsemantic.
        * now apply Hpremises.
      + exact Hsemantic.
  Qed.

  Theorem replayed_portable_root_is_semantic :
    forall (certificate : local_replay_certificate) witness root final,
      portable_replay witness [] final ->
      In root final -> proof_semantic root.
  Proof.
    intros certificate witness root final Hreplay Hroot.
    pose proof (portable_replay_preserves_soundness certificate
      witness [] final Hreplay (Forall_nil _)) as Hall.
    eapply Forall_forall in Hall; eauto.
  Qed.
End PortableReplaySoundness.

(** ** Deterministic portable codec boundary *)

Section DeterministicPortableCodec.
  Variable encode_portable :
    replay_identity -> list external_rule_key -> list portable_proof_node -> list nat.

  Definition portable_encoding
      (identity : replay_identity)
      (keys : list external_rule_key)
      (nodes : list portable_proof_node)
      (bytes : list nat) : Prop :=
    encode_portable identity keys nodes = bytes.

  Theorem portable_encoding_is_deterministic :
    forall identity keys nodes first second,
      portable_encoding identity keys nodes first ->
      portable_encoding identity keys nodes second ->
      first = second.
  Proof. intros identity keys nodes first second Hfirst Hsecond. congruence. Qed.
End DeterministicPortableCodec.

(** ** Bounded flat decoding and explicit heap continuations *)

Inductive decoder_phase : Type := DecodeHeader | DecodeMap | DecodeNodes | DecodeDone.

Record flat_decoder : Type := {
  input_bytes : nat;
  cursor : nat;
  node_budget : nat;
  edge_budget : nat;
  nodes_used : nat;
  edges_used : nat;
  continuation_tape : list nat;
  output_tape : list nat;
  phase : decoder_phase
}.

Definition decoder_well_formed (state : flat_decoder) : Prop :=
  cursor state <= input_bytes state /\
  nodes_used state <= node_budget state /\
  edges_used state <= edge_budget state /\
  length (continuation_tape state) <= node_budget state /\
  length (output_tape state) <= node_budget state.

Definition decode_advance
    (state : flat_decoder) (width add_nodes add_edges : nat)
    : option flat_decoder :=
  if (Nat.leb (cursor state + width) (input_bytes state)
      && Nat.leb (nodes_used state + add_nodes) (node_budget state)
      && Nat.leb (edges_used state + add_edges) (edge_budget state))%bool
  then Some
    {| input_bytes := input_bytes state;
       cursor := cursor state + width;
       node_budget := node_budget state;
       edge_budget := edge_budget state;
       nodes_used := nodes_used state + add_nodes;
       edges_used := edges_used state + add_edges;
       continuation_tape := continuation_tape state;
       output_tape := output_tape state;
       phase := phase state |}
  else None.

Theorem successful_decode_advance_preserves_bounds : forall before after width nodes edges,
  decoder_well_formed before ->
  decode_advance before width nodes edges = Some after ->
  decoder_well_formed after.
Proof.
  intros before after width nodes edges Hwell Hstep.
  unfold decode_advance in Hstep.
  destruct ((Nat.leb (cursor before + width) (input_bytes before)
    && Nat.leb (nodes_used before + nodes) (node_budget before)
    && Nat.leb (edges_used before + edges) (edge_budget before))%bool)
    eqn:Hchecks; try discriminate.
  inversion Hstep. subst after. unfold decoder_well_formed in *; simpl.
  repeat rewrite andb_true_iff in Hchecks.
  repeat rewrite Nat.leb_le in Hchecks.
  destruct Hwell as [_ [_ [_ [Hframes Houtput]]]].
  destruct Hchecks as [[Hcursor Hnodes] Hedges].
  auto.
Qed.

Theorem successful_positive_decode_advance_progresses : forall before after width nodes edges,
  0 < width ->
  decode_advance before width nodes edges = Some after ->
  cursor before < cursor after.
Proof.
  intros before after width nodes edges Hpositive Hstep.
  unfold decode_advance in Hstep.
  destruct ((Nat.leb (cursor before + width) (input_bytes before)
    && Nat.leb (nodes_used before + nodes) (node_budget before)
    && Nat.leb (edges_used before + edges) (edge_budget before))%bool);
    try discriminate.
  inversion Hstep. simpl. lia.
Qed.

Definition decode_precedes (after before : flat_decoder) : Prop :=
  exists width nodes edges,
    0 < width /\ decode_advance before width nodes edges = Some after.

Definition decode_measure (state : flat_decoder) : nat :=
  input_bytes state - cursor state.

Lemma decode_precedes_decreases : forall after before,
  decoder_well_formed before ->
  decode_precedes after before ->
  decode_measure after < decode_measure before.
Proof.
  intros after before Hwell [width [nodes [edges [Hpositive Hstep]]]].
  pose proof (successful_positive_decode_advance_progresses
    before after width nodes edges Hpositive Hstep) as Hprogress.
  pose proof (successful_decode_advance_preserves_bounds
    before after width nodes edges Hwell Hstep) as Hafter.
  unfold decoder_well_formed in Hwell, Hafter.
  unfold decode_measure. destruct Hwell as [Hbefore _].
  destruct Hafter as [Hafter _].
  assert (input_bytes after = input_bytes before) as Hsame.
  { unfold decode_advance in Hstep.
    destruct ((Nat.leb (cursor before + width) (input_bytes before)
      && Nat.leb (nodes_used before + nodes) (node_budget before)
      && Nat.leb (edges_used before + edges) (edge_budget before))%bool);
      try discriminate.
    inversion Hstep. reflexivity. }
  rewrite Hsame. lia.
Qed.

(** The decoder relation is well-founded on admitted states.  The executable
    refinement keeps [decoder_well_formed] as a loop invariant; this theorem
    supplies its decreasing measure. *)
Theorem bounded_decoder_has_no_infinite_positive_cursor_trace :
  forall initial,
    decoder_well_formed initial ->
    Acc decode_precedes initial.
Proof.
  intros initial Hwell.
  remember (decode_measure initial) as measure eqn:Hmeasure.
  revert initial Hwell Hmeasure.
  induction measure using lt_wf_ind.
  intros initial Hwell Hmeasure. constructor.
  intros after Hstep.
  apply (H (decode_measure after)). rewrite Hmeasure.
  - eapply decode_precedes_decreases; eauto.
  - destruct Hstep as [width [nodes [edges [_ Hadvance]]]].
    eapply successful_decode_advance_preserves_bounds
      with (width := width) (nodes := nodes) (edges := edges); eauto.
  - reflexivity.
Qed.

Record teardown_machine : Type := {
  teardown_frames : list nat;
  teardown_outputs : list nat;
  teardown_nodes : list nat
}.

Definition teardown_measure (machine : teardown_machine) : nat :=
  length (teardown_frames machine)
  + length (teardown_outputs machine)
  + length (teardown_nodes machine).

Inductive teardown_step : teardown_machine -> teardown_machine -> Prop :=
| TeardownFrame : forall head tail outputs nodes,
    teardown_step
      {| teardown_frames := head :: tail;
         teardown_outputs := outputs;
         teardown_nodes := nodes |}
      {| teardown_frames := tail;
         teardown_outputs := outputs;
         teardown_nodes := nodes |}
| TeardownOutput : forall head tail nodes,
    teardown_step
      {| teardown_frames := [];
         teardown_outputs := head :: tail;
         teardown_nodes := nodes |}
      {| teardown_frames := [];
         teardown_outputs := tail;
         teardown_nodes := nodes |}
| TeardownNode : forall head tail,
    teardown_step
      {| teardown_frames := [];
         teardown_outputs := [];
         teardown_nodes := head :: tail |}
      {| teardown_frames := [];
         teardown_outputs := [];
         teardown_nodes := tail |}.

Theorem teardown_step_decreases : forall before after,
  teardown_step before after ->
  teardown_measure after < teardown_measure before.
Proof.
  intros before after Hstep. destruct Hstep; unfold teardown_measure; simpl.
  all: apply Nat.le_refl.
Qed.

Theorem teardown_is_well_founded : well_founded (fun after before => teardown_step before after).
Proof.
  eapply wf_incl.
  - intros after before Hstep.
    exact (teardown_step_decreases before after Hstep).
  - apply wf_inverse_image. exact lt_wf.
Qed.

(** ** Representation-aware work and space contracts *)

Record portability_shape : Type := {
  shape_rules : nat;
  shape_transitions : nat;
  shape_proof_nodes : nat;
  shape_premise_edges : nat;
  shape_pending_deltas : nat;
  shape_encoded_bytes : nat
}.

Definition fixed_key_bytes : nat := 16.

Definition radix_map_work (shape : portability_shape) : nat :=
  fixed_key_bytes * shape_rules shape.

Definition replay_work (shape : portability_shape) : nat :=
  shape_proof_nodes shape + shape_premise_edges shape + shape_rules shape.

Definition codec_work (shape : portability_shape) : nat :=
  shape_encoded_bytes shape + shape_proof_nodes shape
  + shape_premise_edges shape + shape_rules shape.

Definition explicit_heap_items (shape : portability_shape) : nat :=
  shape_rules shape + shape_transitions shape + shape_proof_nodes shape
  + shape_premise_edges shape + shape_pending_deltas shape.

Theorem fixed_width_radix_mapping_is_linear : forall shape,
  radix_map_work shape = 16 * shape_rules shape.
Proof. reflexivity. Qed.

Theorem flat_replay_is_linear_in_admitted_graph : forall shape,
  replay_work shape =
    shape_proof_nodes shape + shape_premise_edges shape + shape_rules shape.
Proof. reflexivity. Qed.

Theorem flat_codec_is_linear_in_bytes_and_records : forall shape,
  codec_work shape =
    shape_encoded_bytes shape + shape_proof_nodes shape
    + shape_premise_edges shape + shape_rules shape.
Proof. reflexivity. Qed.

Theorem iterative_teardown_is_linear_in_heap_items : forall shape,
  explicit_heap_items shape =
    shape_rules shape + shape_transitions shape + shape_proof_nodes shape
    + shape_premise_edges shape + shape_pending_deltas shape.
Proof. reflexivity. Qed.
