(** * ProviderResult — non-promoting provider result algebra

    This provider-neutral model fixes the three observable completion classes
    before any adapter implementation exists.  Payload mapping is functorial
    only over the payload: it cannot change status, limitations, checkpoints,
    or cache eligibility.  Composition is the meet of assurance claims, so an
    incomplete or approximate input can never manufacture exactness.
*)

From Stdlib Require Import Bool.Bool Lists.List Arith.PeanoNat.
Import ListNotations.

Inductive result_status : Type :=
| CompleteExact
| CompleteApproximate
| Incomplete.

Inductive incomplete_reason : Type :=
| Cancelled
| DeadlineExceeded
| ResourceLimit
| ProviderFailed
| UnsupportedCapability
| OutOfDistribution
| Ambiguous.

Record approximation : Type := approximation_value {
  approximation_method : nat;
  approximation_limitations : list nat;
  approximation_bound : option nat
}.

Definition approximation_valid (value : approximation) : bool :=
  negb (Nat.eqb (length (approximation_limitations value)) 0).

Inductive provider_result (Payload Checkpoint : Type) : Type :=
| result_exact : Payload -> provider_result Payload Checkpoint
| result_approximate :
    Payload -> approximation -> provider_result Payload Checkpoint
| result_incomplete :
    incomplete_reason -> option Payload -> option Checkpoint ->
    provider_result Payload Checkpoint.

Arguments result_exact {Payload Checkpoint} _.
Arguments result_approximate {Payload Checkpoint} _ _.
Arguments result_incomplete {Payload Checkpoint} _ _ _.

Definition status_of {Payload Checkpoint}
    (value : provider_result Payload Checkpoint) : result_status :=
  match value with
  | result_exact _ => CompleteExact
  | result_approximate _ _ => CompleteApproximate
  | result_incomplete _ _ _ => Incomplete
  end.

Definition limitations_of {Payload Checkpoint}
    (value : provider_result Payload Checkpoint) : list nat :=
  match value with
  | result_approximate _ approximation =>
      approximation_limitations approximation
  | _ => []
  end.

Definition checkpoint_of {Payload Checkpoint}
    (value : provider_result Payload Checkpoint) : option Checkpoint :=
  match value with
  | result_incomplete _ _ checkpoint => checkpoint
  | _ => None
  end.

Definition map_result {A B Checkpoint}
    (map_payload : A -> B)
    (value : provider_result A Checkpoint) : provider_result B Checkpoint :=
  match value with
  | result_exact payload => result_exact (map_payload payload)
  | result_approximate payload approximation =>
      result_approximate (map_payload payload) approximation
  | result_incomplete reason partial checkpoint =>
      result_incomplete reason (option_map map_payload partial) checkpoint
  end.

Definition result_valid {Payload Checkpoint}
    (value : provider_result Payload Checkpoint) : bool :=
  match value with
  | result_exact _ => true
  | result_approximate _ approximation => approximation_valid approximation
  | result_incomplete _ _ _ => true
  end.

Definition status_cacheable (status : result_status) : bool :=
  match status with
  | CompleteExact | CompleteApproximate => true
  | Incomplete => false
  end.

Theorem valid_approximation_has_limitation : forall value,
  approximation_valid value = true ->
  approximation_limitations value <> [].
Proof.
  intros [method limitations bound] valid; simpl in *.
  intro empty; subst; discriminate.
Qed.

Theorem map_preserves_status : forall (A B Checkpoint : Type)
    (map_payload : A -> B) (value : provider_result A Checkpoint),
  status_of (map_result map_payload value) = status_of value.
Proof. intros A B Checkpoint map_payload []; reflexivity. Qed.

Theorem map_preserves_limitations : forall (A B Checkpoint : Type)
    (map_payload : A -> B) (value : provider_result A Checkpoint),
  limitations_of (map_result map_payload value) = limitations_of value.
Proof. intros A B Checkpoint map_payload []; reflexivity. Qed.

Theorem map_preserves_checkpoint : forall (A B Checkpoint : Type)
    (map_payload : A -> B) (value : provider_result A Checkpoint),
  checkpoint_of (map_result map_payload value) = checkpoint_of value.
Proof. intros A B Checkpoint map_payload []; reflexivity. Qed.

Theorem map_preserves_validity : forall (A B Checkpoint : Type)
    (map_payload : A -> B) (value : provider_result A Checkpoint),
  result_valid (map_result map_payload value) = result_valid value.
Proof. intros A B Checkpoint map_payload []; reflexivity. Qed.

Theorem map_preserves_cache_eligibility : forall (A B Checkpoint : Type)
    (map_payload : A -> B) (value : provider_result A Checkpoint),
  status_cacheable (status_of (map_result map_payload value)) =
  status_cacheable (status_of value).
Proof. intros; now rewrite map_preserves_status. Qed.

Theorem map_identity : forall (A Checkpoint : Type)
    (value : provider_result A Checkpoint),
  map_result (fun payload => payload) value = value.
Proof.
  intros A Checkpoint [payload | payload approximation | reason partial checkpoint];
    simpl; try reflexivity.
  destruct partial; reflexivity.
Qed.

Theorem map_composition : forall (A B C Checkpoint : Type)
    (first : A -> B) (second : B -> C)
    (value : provider_result A Checkpoint),
  map_result second (map_result first value) =
  map_result (fun payload => second (first payload)) value.
Proof.
  intros A B C Checkpoint first second
    [payload | payload approximation | reason partial checkpoint];
    simpl; try reflexivity.
  destruct partial; reflexivity.
Qed.

Definition compose_status (left right : result_status) : result_status :=
  match left, right with
  | Incomplete, _ | _, Incomplete => Incomplete
  | CompleteExact, CompleteExact => CompleteExact
  | _, _ => CompleteApproximate
  end.

Lemma compose_status_left_identity : forall value,
  compose_status CompleteExact value = value.
Proof. now destruct value. Qed.

Lemma compose_status_right_identity : forall value,
  compose_status value CompleteExact = value.
Proof. now destruct value. Qed.

Lemma compose_status_associative : forall first second third,
  compose_status (compose_status first second) third =
  compose_status first (compose_status second third).
Proof. now destruct first, second, third. Qed.

Theorem composed_exact_has_only_exact_inputs : forall left right,
  compose_status left right = CompleteExact ->
  left = CompleteExact /\ right = CompleteExact.
Proof. destruct left, right; simpl; intros; try discriminate; auto. Qed.

Theorem composed_incomplete_is_absorbing_left : forall right,
  compose_status Incomplete right = Incomplete.
Proof. now destruct right. Qed.

Theorem composed_incomplete_is_absorbing_right : forall left,
  compose_status left Incomplete = Incomplete.
Proof. now destruct left. Qed.

Theorem incomplete_is_not_cacheable :
  status_cacheable Incomplete = false.
Proof. reflexivity. Qed.

Record result_metadata : Type := result_metadata_value {
  metadata_status : result_status;
  metadata_limitations : list nat
}.

Definition compose_metadata
    (left right : result_metadata) : result_metadata :=
  result_metadata_value
    (compose_status (metadata_status left) (metadata_status right))
    (metadata_limitations left ++ metadata_limitations right).

Lemma compose_metadata_left_identity : forall value,
  compose_metadata (result_metadata_value CompleteExact []) value = value.
Proof.
  intros [status limitations]; destruct status; reflexivity.
Qed.

Lemma compose_metadata_right_identity : forall value,
  compose_metadata value (result_metadata_value CompleteExact []) = value.
Proof.
  intros [status limitations]; unfold compose_metadata; simpl.
  rewrite app_nil_r; now destruct status.
Qed.

Theorem compose_metadata_associative : forall first second third,
  compose_metadata (compose_metadata first second) third =
  compose_metadata first (compose_metadata second third).
Proof.
  intros [first_status first_limitations]
         [second_status second_limitations]
         [third_status third_limitations].
  unfold compose_metadata; simpl.
  rewrite compose_status_associative, app_assoc; reflexivity.
Qed.

Theorem composed_metadata_exact_has_only_exact_inputs : forall left right,
  metadata_status (compose_metadata left right) = CompleteExact ->
  metadata_status left = CompleteExact /\
  metadata_status right = CompleteExact.
Proof.
  intros [left_status left_limitations] [right_status right_limitations].
  simpl; exact (composed_exact_has_only_exact_inputs left_status right_status).
Qed.

Theorem compose_metadata_retains_limitations : forall left right,
  metadata_limitations (compose_metadata left right) =
  metadata_limitations left ++ metadata_limitations right.
Proof. reflexivity. Qed.

Inductive result_control_phase : Type :=
| ClassifyStatus
| ValidateLimitations
| BindCheckpoint
| DecideCache
| ResultAccepted
| ResultRejected.

Theorem result_control_is_finite : forall phase,
  phase = ClassifyStatus \/
  phase = ValidateLimitations \/
  phase = BindCheckpoint \/
  phase = DecideCache \/
  phase = ResultAccepted \/
  phase = ResultRejected.
Proof. destruct phase; auto 6. Qed.

Print Assumptions map_preserves_status.
Print Assumptions map_composition.
Print Assumptions composed_exact_has_only_exact_inputs.
Print Assumptions compose_metadata_associative.
Print Assumptions result_control_is_finite.
