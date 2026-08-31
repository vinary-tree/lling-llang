(** * LazyExpansion — explicit lazy-state lifecycle contract

    A lazy state is never represented by an empty transition slice until a
    source has explicitly completed it as empty.  Normal expansion and retry
    are distinct operations; expansion has one owner; only that owner may
    finish an attempt; failures retry only when declared retryable;
    cancellation requires an explicit reset; and every observable terminal
    result is bound to the currently captured source snapshot.  The runtime
    control machine is finite and uses no call-stack-dependent recursion.
*)

From Stdlib Require Import Bool.Bool Lists.List Arith.PeanoNat.
Import ListNotations.

Inductive expansion_status : Type :=
| Unexpanded
| Expanding
| ExpandedEmpty
| ExpandedNonempty
| ExpansionFailed
| ExpansionCancelled.

Inductive expansion_observation : Type :=
| ObserveEmpty
| ObserveNonempty
| ObserveFailure
| ObserveCancellation.

Inductive completion_kind : Type :=
| CompleteWithEmpty
| CompleteWithNonempty.

Inductive begin_mode : Type :=
| BeginNormal
| BeginExplicitRetry.

Definition status_terminal (status : expansion_status) : bool :=
  match status with
  | ExpandedEmpty
  | ExpandedNonempty
  | ExpansionFailed
  | ExpansionCancelled => true
  | Unexpanded | Expanding => false
  end.

Definition status_cacheable (status : expansion_status) : bool :=
  match status with
  | ExpandedEmpty | ExpandedNonempty => true
  | _ => false
  end.

Definition observe_status
    (fresh : bool)
    (status : expansion_status) : option expansion_observation :=
  if fresh then
    match status with
    | ExpandedEmpty => Some ObserveEmpty
    | ExpandedNonempty => Some ObserveNonempty
    | ExpansionFailed => Some ObserveFailure
    | ExpansionCancelled => Some ObserveCancellation
    | Unexpanded | Expanding => None
    end
  else None.

Theorem unexpanded_is_not_observed_as_empty : forall fresh,
  observe_status fresh Unexpanded <> Some ObserveEmpty.
Proof. now destruct fresh. Qed.

Theorem expanding_is_not_observed_as_empty : forall fresh,
  observe_status fresh Expanding <> Some ObserveEmpty.
Proof. now destruct fresh. Qed.

Theorem empty_observation_requires_expanded_empty : forall fresh status,
  observe_status fresh status = Some ObserveEmpty ->
  fresh = true /\ status = ExpandedEmpty.
Proof. destruct fresh, status; simpl; intros; try discriminate; auto. Qed.

Theorem nonempty_observation_requires_expanded_nonempty : forall fresh status,
  observe_status fresh status = Some ObserveNonempty ->
  fresh = true /\ status = ExpandedNonempty.
Proof. destruct fresh, status; simpl; intros; try discriminate; auto. Qed.

Theorem failure_observation_requires_failed : forall fresh status,
  observe_status fresh status = Some ObserveFailure ->
  fresh = true /\ status = ExpansionFailed.
Proof. destruct fresh, status; simpl; intros; try discriminate; auto. Qed.

Theorem cancellation_observation_requires_cancelled : forall fresh status,
  observe_status fresh status = Some ObserveCancellation ->
  fresh = true /\ status = ExpansionCancelled.
Proof. destruct fresh, status; simpl; intros; try discriminate; auto. Qed.

Theorem stale_status_is_unobservable : forall status,
  observe_status false status = None.
Proof. reflexivity. Qed.

Theorem only_expanded_states_are_cacheable : forall status,
  status_cacheable status = true ->
  status = ExpandedEmpty \/ status = ExpandedNonempty.
Proof. destruct status; simpl; intros; try discriminate; auto. Qed.

Definition begin_authorized
    (mode : begin_mode)
    (status : expansion_status)
    (failure_retryable : bool) : bool :=
  match mode, status with
  | BeginNormal, Unexpanded => true
  | BeginExplicitRetry, ExpansionFailed => failure_retryable
  | _, _ => false
  end.

Definition can_begin
    (mode : begin_mode)
    (cancellation_requested : bool)
    (status : expansion_status)
    (failure_retryable : bool) : bool :=
  negb cancellation_requested &&
  begin_authorized mode status failure_retryable.

Theorem normal_begin_requires_unexpanded : forall cancelled status retryable,
  can_begin BeginNormal cancelled status retryable = true ->
  cancelled = false /\ status = Unexpanded.
Proof.
  destruct cancelled, status; simpl; intros; try discriminate; auto.
Qed.

Theorem retry_begin_requires_retryable_failure : forall cancelled status retryable,
  can_begin BeginExplicitRetry cancelled status retryable = true ->
  cancelled = false /\ status = ExpansionFailed /\ retryable = true.
Proof.
  destruct cancelled, status, retryable; simpl; intros; try discriminate; auto.
Qed.

Theorem normal_begin_cannot_retry_failure : forall cancelled retryable,
  can_begin BeginNormal cancelled ExpansionFailed retryable = false.
Proof. now destruct cancelled, retryable. Qed.

Theorem nonretryable_failure_is_terminal : forall cancelled,
  can_begin BeginExplicitRetry cancelled ExpansionFailed false = false.
Proof. now destruct cancelled. Qed.

Theorem cancellation_blocks_begin : forall mode status retryable,
  can_begin mode true status retryable = false.
Proof. now destruct mode, status, retryable. Qed.

Theorem expanding_blocks_second_begin : forall mode cancelled retryable,
  can_begin mode cancelled Expanding retryable = false.
Proof. now destruct mode, cancelled, retryable. Qed.

Theorem cancelled_requires_explicit_reset : forall mode cancelled retryable,
  can_begin mode cancelled ExpansionCancelled retryable = false.
Proof. now destruct mode, cancelled, retryable. Qed.

Record expansion_state : Type := expansion_state_value {
  current_snapshot : nat;
  entry_snapshot : nat;
  state_status : expansion_status;
  active_owner : option nat;
  attempt_count : nat;
  failure_retryable : bool
}.

Definition snapshot_fresh (state : expansion_state) : bool :=
  Nat.eqb (entry_snapshot state) (current_snapshot state).

Definition owner_consistent (state : expansion_state) : bool :=
  match state_status state, active_owner state with
  | Expanding, Some _ => true
  | Unexpanded, None
  | ExpandedEmpty, None
  | ExpandedNonempty, None
  | ExpansionFailed, None
  | ExpansionCancelled, None => true
  | _, _ => false
  end.

Definition retry_consistent (state : expansion_state) : bool :=
  if failure_retryable state then
    match state_status state with
    | ExpansionFailed => true
    | _ => false
    end
  else true.

Definition terminal_fresh (state : expansion_state) : bool :=
  if status_terminal (state_status state)
  then snapshot_fresh state
  else true.

Definition state_well_formed (state : expansion_state) : bool :=
  owner_consistent state && retry_consistent state && terminal_fresh state.

Definition begin_attempt
    (mode : begin_mode)
    (worker : nat)
    (cancellation_requested : bool)
    (state : expansion_state) : option expansion_state :=
  if can_begin mode cancellation_requested
       (state_status state) (failure_retryable state) then
    Some (expansion_state_value
      (current_snapshot state)
      (current_snapshot state)
      Expanding
      (Some worker)
      (S (attempt_count state))
      false)
  else None.

Definition cancel_before_begin
    (mode : begin_mode)
    (cancellation_requested : bool)
    (state : expansion_state) : option expansion_state :=
  if cancellation_requested &&
       begin_authorized mode (state_status state) (failure_retryable state) then
    Some (expansion_state_value
      (current_snapshot state)
      (current_snapshot state)
      ExpansionCancelled
      None
      (attempt_count state)
      false)
  else None.

Definition finish_status (kind : completion_kind) : expansion_status :=
  match kind with
  | CompleteWithEmpty => ExpandedEmpty
  | CompleteWithNonempty => ExpandedNonempty
  end.

Definition complete_attempt
    (worker : nat)
    (kind : completion_kind)
    (state : expansion_state) : option expansion_state :=
  match state_status state, active_owner state with
  | Expanding, Some owner =>
      if Nat.eqb owner worker && snapshot_fresh state then
        Some (expansion_state_value
          (current_snapshot state)
          (entry_snapshot state)
          (finish_status kind)
          None
          (attempt_count state)
          false)
      else None
  | _, _ => None
  end.

Definition fail_attempt
    (worker : nat)
    (retryable : bool)
    (state : expansion_state) : option expansion_state :=
  match state_status state, active_owner state with
  | Expanding, Some owner =>
      if Nat.eqb owner worker && snapshot_fresh state then
        Some (expansion_state_value
          (current_snapshot state)
          (entry_snapshot state)
          ExpansionFailed
          None
          (attempt_count state)
          retryable)
      else None
  | _, _ => None
  end.

Definition cancel_attempt
    (worker : nat)
    (state : expansion_state) : option expansion_state :=
  match state_status state, active_owner state with
  | Expanding, Some owner =>
      if Nat.eqb owner worker && snapshot_fresh state then
        Some (expansion_state_value
          (current_snapshot state)
          (entry_snapshot state)
          ExpansionCancelled
          None
          (attempt_count state)
          false)
      else None
  | _, _ => None
  end.

Definition reset_cancelled (state : expansion_state) : option expansion_state :=
  match state_status state with
  | ExpansionCancelled =>
      Some (expansion_state_value
        (current_snapshot state)
        (current_snapshot state)
        Unexpanded
        None
        (attempt_count state)
        false)
  | _ => None
  end.

Definition reset_failed (state : expansion_state) : option expansion_state :=
  match state_status state with
  | ExpansionFailed =>
      Some (expansion_state_value
        (current_snapshot state)
        (current_snapshot state)
        Unexpanded
        None
        (attempt_count state)
        false)
  | _ => None
  end.

Definition rebind_snapshot
    (snapshot : nat)
    (_ : expansion_state) : expansion_state :=
  expansion_state_value snapshot snapshot Unexpanded None 0 false.

Theorem begin_attempt_has_single_owner : forall mode worker cancelled state begun,
  begin_attempt mode worker cancelled state = Some begun ->
  state_status begun = Expanding /\
  active_owner begun = Some worker.
Proof.
  intros mode worker cancelled
    [current entry status owner attempts retryable] begun result.
  unfold begin_attempt in result; simpl in result.
  destruct (can_begin mode cancelled status retryable); try discriminate.
  inversion result; auto.
Qed.

Theorem begin_attempt_increments_count : forall mode worker cancelled state begun,
  begin_attempt mode worker cancelled state = Some begun ->
  attempt_count begun = S (attempt_count state).
Proof.
  intros mode worker cancelled
    [current entry status owner attempts retryable] begun result.
  unfold begin_attempt in result; simpl in result.
  destruct (can_begin mode cancelled status retryable); try discriminate.
  now inversion result.
Qed.

Theorem begin_attempt_captures_current_snapshot : forall mode worker cancelled state begun,
  begin_attempt mode worker cancelled state = Some begun ->
  entry_snapshot begun = current_snapshot begun.
Proof.
  intros mode worker cancelled
    [current entry status owner attempts retryable] begun result.
  unfold begin_attempt in result; simpl in result.
  destruct (can_begin mode cancelled status retryable); try discriminate.
  now inversion result.
Qed.

Theorem begin_attempt_is_well_formed : forall mode worker cancelled state begun,
  begin_attempt mode worker cancelled state = Some begun ->
  state_well_formed begun = true.
Proof.
  intros mode worker cancelled
    [current entry status owner attempts retryable] begun result.
  unfold begin_attempt in result; simpl in result.
  destruct (can_begin mode cancelled status retryable); try discriminate.
  inversion result; reflexivity.
Qed.

Theorem pre_cancel_is_ownerless_and_does_not_attempt :
  forall mode state cancelled,
    cancel_before_begin mode true state = Some cancelled ->
    state_status cancelled = ExpansionCancelled /\
    active_owner cancelled = None /\
    attempt_count cancelled = attempt_count state.
Proof.
  intros mode [current entry status owner attempts retryable] cancelled result.
  unfold cancel_before_begin in result; simpl in result.
  destruct (begin_authorized mode status retryable); try discriminate.
  inversion result; auto.
Qed.

Theorem wrong_owner_cannot_complete : forall owner other kind state,
  active_owner state = Some owner ->
  owner <> other ->
  complete_attempt other kind state = None.
Proof.
  intros owner other kind
    [current entry status active attempts retryable] owner_is different.
  simpl in owner_is; subst active.
  unfold complete_attempt; simpl.
  destruct status; try reflexivity.
  apply Nat.eqb_neq in different; now rewrite different.
Qed.

Theorem stale_attempt_cannot_complete : forall worker kind state,
  snapshot_fresh state = false ->
  complete_attempt worker kind state = None.
Proof.
  intros worker kind
    [current entry status owner attempts retryable] stale.
  unfold snapshot_fresh in stale; simpl in stale.
  unfold complete_attempt; simpl.
  unfold snapshot_fresh; simpl.
  destruct status; try reflexivity; destruct owner; try reflexivity.
  now rewrite stale, andb_false_r.
Qed.

Theorem completion_is_fresh_and_ownerless : forall worker kind state completed,
  complete_attempt worker kind state = Some completed ->
  snapshot_fresh completed = true /\
  active_owner completed = None.
Proof.
  intros worker kind
    [current entry status owner attempts retryable] completed result.
  unfold complete_attempt in result; simpl in result.
  unfold snapshot_fresh in result; simpl in result.
  destruct status; try discriminate; destruct owner; try discriminate.
  destruct (n =? worker) eqn:owns; simpl in result; try discriminate.
  destruct (entry =? current) eqn:fresh; try discriminate.
  inversion result; subst; split.
  - unfold snapshot_fresh; simpl; exact fresh.
  - reflexivity.
Qed.

Theorem completion_classifies_empty_exactly : forall worker state completed,
  complete_attempt worker CompleteWithEmpty state = Some completed ->
  state_status completed = ExpandedEmpty /\
  status_cacheable (state_status completed) = true.
Proof.
  intros worker
    [current entry status owner attempts retryable] completed result.
  unfold complete_attempt in result; simpl in result.
  unfold snapshot_fresh in result; simpl in result.
  destruct status; try discriminate; destruct owner; try discriminate.
  destruct (n =? worker); simpl in result; try discriminate.
  destruct (entry =? current); try discriminate.
  inversion result; auto.
Qed.

Theorem completion_classifies_nonempty_exactly : forall worker state completed,
  complete_attempt worker CompleteWithNonempty state = Some completed ->
  state_status completed = ExpandedNonempty /\
  status_cacheable (state_status completed) = true.
Proof.
  intros worker
    [current entry status owner attempts retryable] completed result.
  unfold complete_attempt in result; simpl in result.
  unfold snapshot_fresh in result; simpl in result.
  destruct status; try discriminate; destruct owner; try discriminate.
  destruct (n =? worker); simpl in result; try discriminate.
  destruct (entry =? current); try discriminate.
  inversion result; auto.
Qed.

Theorem failed_attempt_is_ownerless : forall worker retryable state failed,
  fail_attempt worker retryable state = Some failed ->
  state_status failed = ExpansionFailed /\
  active_owner failed = None /\
  failure_retryable failed = retryable.
Proof.
  intros worker retryable
    [current entry status owner attempts old_retry] failed result.
  unfold fail_attempt in result; simpl in result.
  unfold snapshot_fresh in result; simpl in result.
  destruct status; try discriminate; destruct owner; try discriminate.
  destruct (n =? worker); simpl in result; try discriminate.
  destruct (entry =? current); try discriminate.
  inversion result; auto.
Qed.

Theorem nonretryable_failure_cannot_begin : forall worker state failed retry,
  fail_attempt worker false state = Some failed ->
  begin_attempt BeginExplicitRetry retry false failed = None.
Proof.
  intros worker state failed retry result.
  apply failed_attempt_is_ownerless in result.
  destruct result as [status [_ nonretryable]].
  unfold begin_attempt; rewrite status, nonretryable; reflexivity.
Qed.

Theorem cancelled_attempt_is_ownerless : forall worker state cancelled,
  cancel_attempt worker state = Some cancelled ->
  state_status cancelled = ExpansionCancelled /\
  active_owner cancelled = None.
Proof.
  intros worker
    [current entry status owner attempts retryable] cancelled result.
  unfold cancel_attempt in result; simpl in result.
  unfold snapshot_fresh in result; simpl in result.
  destruct status; try discriminate; destruct owner; try discriminate.
  destruct (n =? worker); simpl in result; try discriminate.
  destruct (entry =? current); try discriminate.
  inversion result; auto.
Qed.

Theorem reset_cancelled_is_explicitly_unexpanded : forall state reset,
  reset_cancelled state = Some reset ->
  state_status reset = Unexpanded /\
  active_owner reset = None /\
  snapshot_fresh reset = true.
Proof.
  intros [current entry status owner attempts retryable] reset result.
  unfold reset_cancelled in result; simpl in result.
  destruct status; try discriminate.
  inversion result; subst; repeat split; try reflexivity.
  unfold snapshot_fresh; simpl; apply Nat.eqb_refl.
Qed.

Theorem reset_failed_is_explicitly_unexpanded : forall state reset,
  reset_failed state = Some reset ->
  state_status reset = Unexpanded /\
  active_owner reset = None /\
  failure_retryable reset = false.
Proof.
  intros [current entry status owner attempts retryable] reset result.
  unfold reset_failed in result; simpl in result.
  destruct status; try discriminate.
  inversion result; auto.
Qed.

Theorem rebind_invalidates_prior_lifecycle : forall snapshot state,
  current_snapshot (rebind_snapshot snapshot state) = snapshot /\
  entry_snapshot (rebind_snapshot snapshot state) = snapshot /\
  state_status (rebind_snapshot snapshot state) = Unexpanded /\
  active_owner (rebind_snapshot snapshot state) = None /\
  attempt_count (rebind_snapshot snapshot state) = 0 /\
  state_well_formed (rebind_snapshot snapshot state) = true.
Proof. intros; repeat split; reflexivity. Qed.

Inductive lazy_expansion_control_phase : Type :=
| CheckSnapshot
| CheckCancellation
| ClaimExpansion
| InvokeSource
| ClassifyCompletion
| RecordFailure
| PublishObservation
| ResetLifecycle.

Theorem lazy_expansion_control_is_finite : forall phase,
  phase = CheckSnapshot \/
  phase = CheckCancellation \/
  phase = ClaimExpansion \/
  phase = InvokeSource \/
  phase = ClassifyCompletion \/
  phase = RecordFailure \/
  phase = PublishObservation \/
  phase = ResetLifecycle.
Proof. destruct phase; auto 8. Qed.

Print Assumptions unexpanded_is_not_observed_as_empty.
Print Assumptions empty_observation_requires_expanded_empty.
Print Assumptions retry_begin_requires_retryable_failure.
Print Assumptions begin_attempt_has_single_owner.
Print Assumptions wrong_owner_cannot_complete.
Print Assumptions stale_attempt_cannot_complete.
Print Assumptions completion_is_fresh_and_ownerless.
Print Assumptions nonretryable_failure_cannot_begin.
Print Assumptions reset_cancelled_is_explicitly_unexpanded.
Print Assumptions rebind_invalidates_prior_lifecycle.
