(**
  RegresSpec-derived contracts for the provider-neutral Vinary foundations.

  This model deliberately separates six boundaries that production code must
  keep separate: canonical wire encoding, analysis-artifact graphs, runtime
  supervision, requirements history, assurance authority, and documentation
  evidence.  Every transition over input-shaped data is represented by a
  heap-work measure and a constant native-frame bound.
*)

From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Lia.

Import ListNotations.

(** * Canonical wire profile and identity domains *)

Inductive json_number : Type :=
| JsonInteger (magnitude : nat)
| JsonFiniteDecimal (coefficient scale : nat)
| JsonNegativeZero
| JsonNonFinite.

Inductive canonical_outcome : Type :=
| CanonicalSuccess (bytes : list nat)
| CanonicalMalformed
| CanonicalBudgetExceeded
| CanonicalNumberRejected.

Definition admitted_number (number : json_number) : bool :=
  match number with
  | JsonInteger _ | JsonFiniteDecimal _ _ | JsonNegativeZero => true
  | JsonNonFinite => false
  end.

Definition canonicalize_number (number : json_number) : canonical_outcome :=
  match number with
  | JsonInteger magnitude => CanonicalSuccess [0; magnitude]
  | JsonFiniteDecimal coefficient scale =>
      CanonicalSuccess [1; coefficient; scale]
  | JsonNegativeZero => CanonicalSuccess [0; 0]
  | JsonNonFinite => CanonicalNumberRejected
  end.

Theorem non_finite_numbers_are_rejected :
  canonicalize_number JsonNonFinite = CanonicalNumberRejected.
Proof. reflexivity. Qed.

Theorem negative_zero_has_the_zero_encoding :
  canonicalize_number JsonNegativeZero = canonicalize_number (JsonInteger 0).
Proof. reflexivity. Qed.

Inductive identity_domain : Type :=
| WireSchemaDomain
| CanonicalContentDomain
| RuntimeManifestDomain
| EvidenceDomain.

Record domain_separated_identity : Type := identity_value {
  identity_tag : identity_domain;
  identity_payload : nat
}.

Definition wire_schema_identity (fingerprint : nat) : domain_separated_identity :=
  identity_value WireSchemaDomain fingerprint.

Definition canonical_content_identity (digest : nat) : domain_separated_identity :=
  identity_value CanonicalContentDomain digest.

Theorem schema_and_content_identity_are_distinct : forall value,
  wire_schema_identity value <> canonical_content_identity value.
Proof. intros value equality; discriminate equality. Qed.

Inductive sink_result : Type :=
| SinkAccepted (bytes : list nat) (remaining : nat)
| SinkRejected (original : list nat) (remaining : nat).

Definition write_chunk
    (remaining : nat) (current chunk : list nat) : sink_result :=
  if Nat.leb (length chunk) remaining
  then SinkAccepted (current ++ chunk) (remaining - length chunk)
  else SinkRejected current remaining.

Theorem rejected_chunk_is_atomic : forall remaining current chunk,
  length chunk > remaining ->
  write_chunk remaining current chunk = SinkRejected current remaining.
Proof.
  intros remaining current chunk exceeds.
  unfold write_chunk.
  destruct (Nat.leb (length chunk) remaining) eqn:bounded.
  - apply Nat.leb_le in bounded. lia.
  - reflexivity.
Qed.

Fixpoint emit_chunks (chunks : list (list nat)) : list nat :=
  match chunks with
  | [] => []
  | chunk :: rest => chunk ++ emit_chunks rest
  end.

Theorem streaming_and_buffered_emission_agree : forall prefix suffix,
  emit_chunks (prefix ++ suffix) = emit_chunks prefix ++ emit_chunks suffix.
Proof.
  induction prefix as [|chunk rest hypothesis]; intros suffix; simpl.
  - reflexivity.
  - rewrite hypothesis, app_assoc. reflexivity.
Qed.

Record wire_machine_state : Type := wire_machine {
  wire_heap_frames : nat;
  wire_native_frames : nat
}.

Definition wire_machine_step (state : wire_machine_state) : wire_machine_state :=
  wire_machine (Nat.pred (wire_heap_frames state)) (wire_native_frames state).

Fixpoint run_wire_machine
    (steps : nat) (state : wire_machine_state) : wire_machine_state :=
  match steps with
  | 0 => state
  | S rest => run_wire_machine rest (wire_machine_step state)
  end.

Theorem wire_machine_native_stack_is_constant : forall steps state,
  wire_native_frames state = 1 ->
  wire_native_frames (run_wire_machine steps state) = 1.
Proof.
  induction steps as [|rest hypothesis]; intros state bounded; simpl.
  - exact bounded.
  - apply hypothesis. destruct state as [heap native]. exact bounded.
Qed.

Theorem malformed_and_budget_outcomes_are_not_success :
  CanonicalMalformed <> CanonicalSuccess [] /\
  CanonicalBudgetExceeded <> CanonicalSuccess [].
Proof. split; discriminate. Qed.

(** * Neutral analysis-artifact graph *)

Inductive completion_axis : Type := Complete | Incomplete.
Inductive strength_axis : Type := UnknownStrength | UnderApproximation
  | OverApproximation | ExactStrength.
Inductive authority_axis : Type := CandidateAuthority | ReviewedAuthority
  | CertifiedAuthority.
Inductive revision_axis : Type := WorkingRevision | ImmutableRevision.

Record graph_epistemic_state : Type := graph_epistemic {
  graph_completion : completion_axis;
  graph_strength : strength_axis;
  graph_authority : authority_axis;
  graph_revision : revision_axis
}.

Theorem graph_epistemic_axes_are_orthogonal :
  exists left right,
    graph_completion left = graph_completion right /\
    graph_strength left <> graph_strength right /\
    graph_authority left = graph_authority right /\
    graph_revision left = graph_revision right.
Proof.
  exists (graph_epistemic Complete ExactStrength ReviewedAuthority ImmutableRevision).
  exists (graph_epistemic Complete OverApproximation ReviewedAuthority ImmutableRevision).
  repeat split; discriminate || reflexivity.
Qed.

Record role_edge : Type := role_edge_value {
  role_identifier : nat;
  role_target : nat
}.

Record relation_node : Type := relation_node_value {
  relation_identifier : nat;
  relation_roles : list role_edge
}.

Definition lower_relation (relation : relation_node) : list role_edge :=
  relation_roles relation.

Theorem relation_lowering_preserves_every_role : forall relation role,
  In role (relation_roles relation) -> In role (lower_relation relation).
Proof. intros relation role present; exact present. Qed.

Definition dialect_conforms
    (required_fields supplied_fields : list nat) : Prop :=
  forall field, In field required_fields -> In field supplied_fields.

Theorem empty_neutral_dialect_requires_no_application_fields : forall fields,
  dialect_conforms [] fields.
Proof. unfold dialect_conforms; intros fields field impossible; inversion impossible. Qed.

Record graph_snapshot : Type := graph_snapshot_value {
  graph_digest : nat;
  graph_payload : list nat
}.

Record graph_patch : Type := graph_patch_value {
  patch_base_digest : nat;
  patch_payload : list nat
}.

Inductive graph_patch_result : Type :=
| PatchCommitted (snapshot : graph_snapshot)
| PatchRejected (original : graph_snapshot).

Definition apply_graph_patch
    (base : graph_snapshot) (patch : graph_patch) : graph_patch_result :=
  if Nat.eqb (graph_digest base) (patch_base_digest patch)
  then PatchCommitted
         (graph_snapshot_value
            (S (graph_digest base))
            (graph_payload base ++ patch_payload patch))
  else PatchRejected base.

Theorem stale_graph_patch_is_atomic : forall base patch,
  graph_digest base <> patch_base_digest patch ->
  apply_graph_patch base patch = PatchRejected base.
Proof.
  intros base patch stale.
  unfold apply_graph_patch.
  destruct (Nat.eqb (graph_digest base) (patch_base_digest patch)) eqn:matches.
  - apply Nat.eqb_eq in matches. exfalso. exact (stale matches).
  - reflexivity.
Qed.

Definition strength_rank (strength : strength_axis) : nat :=
  match strength with
  | UnknownStrength => 0
  | UnderApproximation => 1
  | OverApproximation => 2
  | ExactStrength => 3
  end.

Definition project_strength (source requested : strength_axis) : strength_axis :=
  if Nat.leb (strength_rank requested) (strength_rank source)
  then requested
  else source.

Theorem projection_never_strengthens : forall source requested,
  strength_rank (project_strength source requested) <= strength_rank source.
Proof.
  intros source requested.
  unfold project_strength.
  destruct (Nat.leb (strength_rank requested) (strength_rank source)) eqn:bounded.
  - apply Nat.leb_le in bounded; exact bounded.
  - apply Nat.le_refl.
Qed.

Inductive jsonl_outcome : Type := JsonlComplete | JsonlMalformed | JsonlBudget.

Definition ingest_jsonl (records limit : nat) : jsonl_outcome :=
  if Nat.leb records limit then JsonlComplete else JsonlBudget.

Theorem jsonl_limit_exhaustion_is_not_completion : forall records limit,
  records > limit -> ingest_jsonl records limit = JsonlBudget.
Proof.
  intros records limit exceeds.
  unfold ingest_jsonl.
  destruct (Nat.leb records limit) eqn:bounded.
  - apply Nat.leb_le in bounded. lia.
  - reflexivity.
Qed.

(** * Runtime result, locks, supervision, output, and resume *)

Inductive precision_axis : Type := ExactPrecision | ApproximatePrecision.
Inductive availability_axis : Type := Produced | Unavailable.
Inductive applicability_axis : Type := Applicable | NotApplicable | ApplicabilityUnknown.
Inductive integrity_axis : Type := IntegrityVerified | IntegrityUnknown | IntegrityFailed.
Inductive termination_axis : Type := Terminated | Cancelled | TimedOut | Failed.

Record runtime_result : Type := runtime_result_value {
  runtime_precision : precision_axis;
  runtime_completion : completion_axis;
  runtime_applicability : applicability_axis;
  runtime_availability : availability_axis;
  runtime_integrity : integrity_axis;
  runtime_termination : termination_axis
}.

Definition cache_admissible (result : runtime_result) : bool :=
  match runtime_completion result, runtime_availability result,
        runtime_integrity result, runtime_termination result with
  | Complete, Produced, IntegrityVerified, Terminated => true
  | _, _, _, _ => false
  end.

Theorem incomplete_result_is_not_cacheable : forall precision applicability availability integrity termination,
  cache_admissible
    (runtime_result_value precision Incomplete applicability availability integrity termination) = false.
Proof. reflexivity. Qed.

Record input_locks : Type := input_locks_value {
  executable_lock : nat;
  model_lock : nat;
  data_lock : nat;
  schema_lock : nat;
  environment_lock : nat;
  seed_lock : nat
}.

Definition locks_compatible (expected observed : input_locks) : Prop :=
  expected = observed.

Definition exact_release_admissible
    (result : runtime_result) (expected observed : input_locks) : Prop :=
  runtime_precision result = ExactPrecision /\
  runtime_completion result = Complete /\
  runtime_integrity result = IntegrityVerified /\
  runtime_termination result = Terminated /\
  locks_compatible expected observed.

Theorem exact_release_binds_every_input_lock : forall result expected observed,
  exact_release_admissible result expected observed -> expected = observed.
Proof. intros result expected observed [_ [_ [_ [_ locks]]]]; exact locks. Qed.

Record checkpoint : Type := checkpoint_value {
  checkpoint_schema : nat;
  checkpoint_locks : input_locks;
  checkpoint_payload : list nat
}.

Definition resume_compatible (checkpoint_value : checkpoint) (current : input_locks) : Prop :=
  checkpoint_locks checkpoint_value = current.

Theorem stale_checkpoint_cannot_resume : forall saved current,
  checkpoint_locks saved <> current -> ~ resume_compatible saved current.
Proof. unfold resume_compatible; intros saved current stale compatible; exact (stale compatible). Qed.

Inductive spill_location : Type := RepositoryBacked | TemporaryMemoryFilesystem.
Inductive output_route : Type := InMemory | Spill (location : spill_location).

Definition route_output (bytes memory_cap : nat) : output_route :=
  if Nat.leb bytes memory_cap then InMemory else Spill RepositoryBacked.

Theorem overflow_output_never_uses_tmpfs : forall bytes cap,
  route_output bytes cap <> Spill TemporaryMemoryFilesystem.
Proof.
  intros bytes cap; unfold route_output.
  destruct (Nat.leb bytes cap); discriminate.
Qed.

Record process_tree_state : Type := process_tree_state_value {
  pending_processes : nat;
  process_native_frames : nat
}.

Definition terminate_process_step (state : process_tree_state) : process_tree_state :=
  process_tree_state_value
    (Nat.pred (pending_processes state))
    (process_native_frames state).

Fixpoint run_process_termination
    (steps : nat) (state : process_tree_state) : process_tree_state :=
  match steps with
  | 0 => state
  | S rest => run_process_termination rest (terminate_process_step state)
  end.

Theorem process_termination_decreases_pending_work : forall state,
  pending_processes state > 0 ->
  pending_processes (terminate_process_step state) < pending_processes state.
Proof. intros [pending native] positive; simpl in *; lia. Qed.

Theorem process_termination_native_stack_is_constant : forall steps state,
  process_native_frames state = 1 ->
  process_native_frames (run_process_termination steps state) = 1.
Proof.
  induction steps as [|rest hypothesis]; intros state bounded; simpl.
  - exact bounded.
  - apply hypothesis. destruct state as [pending native]. exact bounded.
Qed.

(** * Requirements history and lossless source accounting *)

Record requirement_version : Type := requirement_version_value {
  stable_requirement_id : nat;
  requirement_revision : nat;
  requirement_payload : nat;
  requirement_tombstoned : bool
}.

Definition revise_requirement
    (requirement : requirement_version) (payload : nat) : requirement_version :=
  requirement_version_value
    (stable_requirement_id requirement)
    (S (requirement_revision requirement))
    payload
    false.

Definition tombstone_requirement (requirement : requirement_version) : requirement_version :=
  requirement_version_value
    (stable_requirement_id requirement)
    (S (requirement_revision requirement))
    (requirement_payload requirement)
    true.

Theorem revision_preserves_stable_requirement_identity : forall requirement payload,
  stable_requirement_id (revise_requirement requirement payload) =
  stable_requirement_id requirement.
Proof. intros [identifier revision content retired] payload; reflexivity. Qed.

Theorem revision_strictly_advances : forall requirement payload,
  requirement_revision (revise_requirement requirement payload) >
  requirement_revision requirement.
Proof. intros [identifier revision content retired] payload; simpl; lia. Qed.

Theorem tombstone_is_not_active : forall requirement,
  requirement_tombstoned (tombstone_requirement requirement) = true.
Proof. intros [identifier revision content retired]; reflexivity. Qed.

Fixpoint account_source
    (classified : nat -> bool) (spans : list nat) : list nat * list nat :=
  match spans with
  | [] => ([], [])
  | span :: rest =>
      let '(known, unknown) := account_source classified rest in
      if classified span
      then (span :: known, unknown)
      else (known, span :: unknown)
  end.

Theorem source_accounting_is_total : forall classified spans span,
  In span spans ->
  In span (fst (account_source classified spans)) \/
  In span (snd (account_source classified spans)).
Proof.
  intros classified spans.
  induction spans as [|head tail hypothesis]; intros span present.
  - inversion present.
  - simpl in present.
    destruct present as [equal | in_tail].
    + subst span. simpl. destruct (account_source classified tail) as [known unknown].
      destruct (classified head); simpl; auto.
    + specialize (hypothesis span in_tail).
      simpl. destruct (account_source classified tail) as [known unknown].
      destruct (classified head); simpl in *; tauto.
Qed.

Theorem unclassified_source_is_preserved : forall classified spans span,
  In span spans -> classified span = false ->
  In span (snd (account_source classified spans)).
Proof.
  intros classified spans.
  induction spans as [|head tail hypothesis]; intros span present rejected.
  - inversion present.
  - simpl in present.
    destruct present as [equal | in_tail].
    + subst span. simpl. destruct (account_source classified tail).
      rewrite rejected. simpl; auto.
    + simpl. destruct (account_source classified tail) as [known unknown].
      destruct (classified head); simpl; auto.
Qed.

Record history_machine_state : Type := history_machine_state_value {
  pending_history_nodes : nat;
  history_native_frames : nat
}.

Definition history_machine_step
    (state : history_machine_state) : history_machine_state :=
  history_machine_state_value
    (Nat.pred (pending_history_nodes state))
    (history_native_frames state).

Fixpoint run_history_machine
    (steps : nat) (state : history_machine_state) : history_machine_state :=
  match steps with
  | 0 => state
  | S rest => run_history_machine rest (history_machine_step state)
  end.

Theorem history_validation_uses_constant_native_stack : forall steps state,
  history_native_frames state = 1 ->
  history_native_frames (run_history_machine steps state) = 1.
Proof.
  induction steps as [|rest hypothesis]; intros state bounded; simpl.
  - exact bounded.
  - apply hypothesis. destruct state as [pending native]. exact bounded.
Qed.

(** * Assurance authority, freshness, controls, and attestations *)

Inductive evidence_authority : Type :=
| TheoremProof
| BoundedModelCheck
| StatisticalInference
| EmpiricalTest
| Assumption
| Unsupported
| OutOfScope.

Inductive obligation_kind : Type :=
| TheoremObligation
| BoundedSafetyObligation
| StatisticalObligation
| EmpiricalObligation.

Definition authority_discharges
    (authority : evidence_authority) (obligation : obligation_kind) : bool :=
  match authority, obligation with
  | TheoremProof, TheoremObligation => true
  | BoundedModelCheck, BoundedSafetyObligation => true
  | StatisticalInference, StatisticalObligation => true
  | EmpiricalTest, EmpiricalObligation => true
  | _, _ => false
  end.

Theorem statistics_do_not_discharge_theorem_obligations :
  authority_discharges StatisticalInference TheoremObligation = false.
Proof. reflexivity. Qed.

Theorem bounded_models_do_not_discharge_unbounded_theorems :
  authority_discharges BoundedModelCheck TheoremObligation = false.
Proof. reflexivity. Qed.

Record evidence_context : Type := evidence_context_value {
  evidence_subject : nat;
  evidence_configuration : nat;
  evidence_tool : nat;
  evidence_environment : nat;
  evidence_schema : nat;
  evidence_assumptions : list nat
}.

Definition evidence_fresh (bound observed : evidence_context) : Prop := bound = observed.

Theorem changed_subject_invalidates_evidence : forall bound observed,
  evidence_subject bound <> evidence_subject observed ->
  ~ evidence_fresh bound observed.
Proof.
  intros bound observed changed equal_context.
  apply changed. now rewrite equal_context.
Qed.

Record reviewer_attestation : Type := reviewer_attestation_value {
  attested_revision : nat;
  attested_claims : list nat;
  attestation_signature : nat
}.

Definition attestation_applies
    (attestation : reviewer_attestation) (revision claim : nat) : Prop :=
  attested_revision attestation = revision /\
  In claim (attested_claims attestation) /\
  attestation_signature attestation <> 0.

Theorem attestation_is_revision_bound : forall attestation revision claim,
  attestation_applies attestation revision claim ->
  attested_revision attestation = revision.
Proof. intros attestation revision claim [bound _]; exact bound. Qed.

Definition assurance_verified
    (authority : evidence_authority)
    (obligation : obligation_kind)
    (applicability : applicability_axis)
    (fresh negative_control attested : bool) : bool :=
  authority_discharges authority obligation &&
  match applicability with Applicable => true | _ => false end &&
  fresh && negative_control && attested.

Theorem verified_assurance_requires_negative_control :
  forall authority obligation applicability fresh attested,
  assurance_verified authority obligation applicability fresh false attested = false.
Proof.
  intros authority obligation applicability fresh attested.
  unfold assurance_verified.
  repeat rewrite andb_false_r.
  reflexivity.
Qed.

Theorem inapplicable_evidence_cannot_verify : forall authority obligation fresh negative attested,
  assurance_verified authority obligation NotApplicable fresh negative attested = false.
Proof.
  intros authority obligation fresh negative attested.
  unfold assurance_verified; simpl.
  destruct (authority_discharges authority obligation); reflexivity.
Qed.

(** * Documentation evidence and claim-language validation *)

Record generated_asset_manifest : Type := generated_asset_manifest_value {
  manifest_source_digest : nat;
  manifest_generator_digest : nat;
  manifest_environment_digest : nat;
  manifest_output_digest : nat;
  manifest_deterministic : bool
}.

Definition generated_asset_current
    (declared observed : generated_asset_manifest) : bool :=
  Nat.eqb (manifest_source_digest declared) (manifest_source_digest observed) &&
  Nat.eqb (manifest_generator_digest declared) (manifest_generator_digest observed) &&
  Nat.eqb (manifest_environment_digest declared) (manifest_environment_digest observed) &&
  Nat.eqb (manifest_output_digest declared) (manifest_output_digest observed) &&
  manifest_deterministic declared && manifest_deterministic observed.

Theorem changed_source_marks_generated_asset_stale : forall declared observed,
  manifest_source_digest declared <> manifest_source_digest observed ->
  generated_asset_current declared observed = false.
Proof.
  intros declared observed changed.
  unfold generated_asset_current.
  destruct (Nat.eqb (manifest_source_digest declared) (manifest_source_digest observed)) eqn:matches.
  - apply Nat.eqb_eq in matches. contradiction.
  - reflexivity.
Qed.

Theorem changed_generator_marks_generated_asset_stale : forall declared observed,
  manifest_generator_digest declared <> manifest_generator_digest observed ->
  generated_asset_current declared observed = false.
Proof.
  intros declared observed changed.
  unfold generated_asset_current.
  destruct (Nat.eqb (manifest_source_digest declared) (manifest_source_digest observed)); simpl.
  - destruct (Nat.eqb (manifest_generator_digest declared) (manifest_generator_digest observed)) eqn:matches.
    + apply Nat.eqb_eq in matches. contradiction.
    + reflexivity.
  - reflexivity.
Qed.

Inductive lint_mode : Type := CheckOnly | ApplyFixes.
Inductive claim_kind : Type := TheoremClaim | BoundedClaim | StatisticalClaim
  | EmpiricalClaim | AssumptionClaim | ScopeExclusion.

Definition claim_authorized (claim : claim_kind) (authority : evidence_authority) : bool :=
  match claim, authority with
  | TheoremClaim, TheoremProof
  | BoundedClaim, BoundedModelCheck
  | StatisticalClaim, StatisticalInference
  | EmpiricalClaim, EmpiricalTest
  | AssumptionClaim, Assumption
  | ScopeExclusion, OutOfScope => true
  | _, _ => false
  end.

Theorem statistical_wording_cannot_claim_a_theorem :
  claim_authorized TheoremClaim StatisticalInference = false.
Proof. reflexivity. Qed.

Definition lint_document
    (fixer : list nat -> list nat)
    (mode : lint_mode) (document : list nat) : list nat :=
  match mode with
  | CheckOnly => document
  | ApplyFixes => fixer document
  end.

Theorem check_only_lint_is_non_mutating : forall fixer document,
  lint_document fixer CheckOnly document = document.
Proof. reflexivity. Qed.

Record lint_machine_state : Type := lint_machine_state_value {
  pending_document_nodes : nat;
  lint_native_frames : nat
}.

Definition lint_machine_step (state : lint_machine_state) : lint_machine_state :=
  lint_machine_state_value
    (Nat.pred (pending_document_nodes state))
    (lint_native_frames state).

Fixpoint run_lint_machine
    (steps : nat) (state : lint_machine_state) : lint_machine_state :=
  match steps with
  | 0 => state
  | S rest => run_lint_machine rest (lint_machine_step state)
  end.

Theorem documentation_traversal_uses_constant_native_stack : forall steps state,
  lint_native_frames state = 1 ->
  lint_native_frames (run_lint_machine steps state) = 1.
Proof.
  induction steps as [|rest hypothesis]; intros state bounded; simpl.
  - exact bounded.
  - apply hypothesis. destruct state as [pending native]. exact bounded.
Qed.
