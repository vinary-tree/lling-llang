(** * TypedAbiV2 — additive, typed, domain-neutral ABI metadata

    ABI v2 is additive to the opaque ABI v1 surface.  Its pointer-free metadata
    uses fixed-width fields; opaque handles carry ownership separately.  The
    contract keeps tape, algebra, snapshot, and context identities distinct,
    and keeps precision, completeness, applicability, termination, and
    evidence authority as orthogonal result axes.
*)

From Stdlib Require Import Arith.Arith Bool.Bool Lists.List Lia.
Import ListNotations.

(** ** Typed signatures and opaque ABI v1 migration *)

Definition semantic_id : Type := nat.

Record signature : Type := {
  input_tape : semantic_id;
  output_tape : semantic_id;
  algebra : semantic_id
}.

Inductive signature_view : Type :=
  | OpaqueV1
  | Typed (value : signature).

Definition signatures_compose (left right : signature) : Prop :=
  output_tape left = input_tape right /\ algebra left = algebra right.

Definition compose_signature (left right : signature) : option signature :=
  if Nat.eq_dec (output_tape left) (input_tape right) then
    if Nat.eq_dec (algebra left) (algebra right) then
      Some {| input_tape := input_tape left;
              output_tape := output_tape right;
              algebra := algebra left |}
    else None
  else None.

Theorem compose_signature_sound :
  forall left right result,
    compose_signature left right = Some result ->
    signatures_compose left right
    /\ input_tape result = input_tape left
    /\ output_tape result = output_tape right
    /\ algebra result = algebra left.
Proof.
  intros left right result.
  unfold compose_signature, signatures_compose.
  destruct (Nat.eq_dec (output_tape left) (input_tape right)) as [Htape | Htape].
  - destruct (Nat.eq_dec (algebra left) (algebra right)) as [Halg | Halg].
    + intro Hresult. inversion Hresult. subst. repeat split; assumption || reflexivity.
    + discriminate.
  - discriminate.
Qed.

Definition typed_evidence_allowed (view : signature_view) : bool :=
  match view with
  | OpaqueV1 => false
  | Typed _ => true
  end.

Theorem opaque_v1_cannot_authorize_typed_evidence :
  typed_evidence_allowed OpaqueV1 = false.
Proof. reflexivity. Qed.

(** ** Snapshot and context binding *)

Record identity_binding : Type := {
  binding_signature : signature;
  snapshot_id : semantic_id;
  context_id : semantic_id
}.

Definition same_signature (expected observed : signature) : bool :=
  Nat.eqb (input_tape expected) (input_tape observed)
  && (Nat.eqb (output_tape expected) (output_tape observed)
      && Nat.eqb (algebra expected) (algebra observed)).

Definition same_binding (expected observed : identity_binding) : bool :=
  same_signature (binding_signature expected) (binding_signature observed)
  && (Nat.eqb (snapshot_id expected) (snapshot_id observed)
      && Nat.eqb (context_id expected) (context_id observed)).

Theorem replay_requires_exact_signature :
  forall expected observed,
    same_binding expected observed = true ->
    input_tape (binding_signature expected) =
      input_tape (binding_signature observed)
    /\ output_tape (binding_signature expected) =
      output_tape (binding_signature observed)
    /\ algebra (binding_signature expected) =
      algebra (binding_signature observed).
Proof.
  intros expected observed Hsame.
  unfold same_binding in Hsame.
  apply andb_true_iff in Hsame as [Hsignature _].
  unfold same_signature in Hsignature.
  apply andb_true_iff in Hsignature as [Hinput Hrest].
  apply andb_true_iff in Hrest as [Houtput Halgebra].
  repeat rewrite Nat.eqb_eq in *.
  now repeat split.
Qed.

Theorem replay_requires_snapshot_and_context :
  forall expected observed,
    same_binding expected observed = true ->
    snapshot_id expected = snapshot_id observed
    /\ context_id expected = context_id observed.
Proof.
  intros expected observed Hsame.
  unfold same_binding in Hsame.
  apply andb_true_iff in Hsame as [_ Hidentity].
  apply andb_true_iff in Hidentity as [Hsnapshot Hcontext].
  apply Nat.eqb_eq in Hsnapshot.
  apply Nat.eqb_eq in Hcontext.
  now split.
Qed.

Theorem snapshot_mismatch_rejects_replay :
  forall expected observed,
    snapshot_id expected <> snapshot_id observed ->
    same_binding expected observed = false.
Proof.
  intros expected observed Hneq.
  unfold same_binding.
  apply Nat.eqb_neq in Hneq.
  now rewrite Hneq, !andb_false_r.
Qed.

Theorem context_mismatch_rejects_replay :
  forall expected observed,
    context_id expected <> context_id observed ->
    same_binding expected observed = false.
Proof.
  intros expected observed Hneq.
  unfold same_binding.
  apply Nat.eqb_neq in Hneq.
  now rewrite Hneq, !andb_false_r.
Qed.

Theorem input_signature_mismatch_rejects_replay :
  forall expected observed,
    input_tape (binding_signature expected) <>
      input_tape (binding_signature observed) ->
    same_binding expected observed = false.
Proof.
  intros expected observed Hneq.
  unfold same_binding, same_signature.
  apply Nat.eqb_neq in Hneq.
  now rewrite Hneq.
Qed.

Theorem output_signature_mismatch_rejects_replay :
  forall expected observed,
    output_tape (binding_signature expected) <>
      output_tape (binding_signature observed) ->
    same_binding expected observed = false.
Proof.
  intros expected observed Hneq.
  unfold same_binding, same_signature.
  apply Nat.eqb_neq in Hneq.
  now rewrite Hneq, !andb_false_r.
Qed.

Theorem algebra_signature_mismatch_rejects_replay :
  forall expected observed,
    algebra (binding_signature expected) <>
      algebra (binding_signature observed) ->
    same_binding expected observed = false.
Proof.
  intros expected observed Hneq.
  unfold same_binding, same_signature.
  apply Nat.eqb_neq in Hneq.
  now rewrite Hneq, !andb_false_r.
Qed.

(** ** Additive structure headers *)

Record abi_header : Type := {
  struct_size : nat;
  abi_version : nat;
  enabled_flags : list nat;
  reserved_word : nat
}.

Definition header_valid
    (required_size known_flag_count : nat) (header : abi_header) : Prop :=
  required_size <= struct_size header
  /\ abi_version header = 2
  /\ reserved_word header = 0
  /\ NoDup (enabled_flags header)
  /\ Forall (fun flag => flag < known_flag_count) (enabled_flags header).

Theorem header_extension_preserves_valid :
  forall required known header extra,
    header_valid required known header ->
    header_valid required known
      {| struct_size := struct_size header + extra;
         abi_version := abi_version header;
         enabled_flags := enabled_flags header;
         reserved_word := reserved_word header |}.
Proof.
  intros required known header extra Hvalid.
  unfold header_valid in *.
  destruct Hvalid as [Hsize [Hversion [Hreserved [Hnodup Hknown]]]].
  repeat split; simpl; auto; lia.
Qed.

Theorem header_rejects_nonzero_reserved :
  forall required known header,
    reserved_word header <> 0 -> ~ header_valid required known header.
Proof.
  intros required known header Hnonzero Hvalid.
  unfold header_valid in Hvalid.
  destruct Hvalid as [_ [_ [Hzero _]]].
  contradiction.
Qed.

Theorem header_rejects_unknown_flag :
  forall required known header flag,
    In flag (enabled_flags header) ->
    known <= flag ->
    ~ header_valid required known header.
Proof.
  intros required known header flag Hin Hunknown Hvalid.
  unfold header_valid in Hvalid.
  destruct Hvalid as [_ [_ [_ [_ Hall]]]].
  apply Forall_forall with (x := flag) in Hall; auto; lia.
Qed.

(** ** Raw discriminants are decoded before becoming Rust enums *)

Inductive precision : Type :=
  | PrecisionExact | PrecisionApproximate | PrecisionUnknown.

Inductive completeness : Type :=
  | CompletionComplete | CompletionIncomplete.

Inductive applicability : Type :=
  | Applicable | Unsupported | ApplicabilityUnknown.

Inductive termination : Type :=
  | Succeeded | Cancelled | BudgetExhausted | Failed.

Inductive evidence_state : Type :=
  | EvidenceNone | EvidenceCandidate | EvidenceVerified
  | EvidenceStale | EvidenceInvalid.

Definition decode_precision (raw : nat) : option precision :=
  match raw with
  | 1 => Some PrecisionExact
  | 2 => Some PrecisionApproximate
  | 3 => Some PrecisionUnknown
  | _ => None
  end.

Definition decode_completeness (raw : nat) : option completeness :=
  match raw with
  | 1 => Some CompletionComplete
  | 2 => Some CompletionIncomplete
  | _ => None
  end.

Definition decode_applicability (raw : nat) : option applicability :=
  match raw with
  | 1 => Some Applicable
  | 2 => Some Unsupported
  | 3 => Some ApplicabilityUnknown
  | _ => None
  end.

Definition decode_termination (raw : nat) : option termination :=
  match raw with
  | 1 => Some Succeeded
  | 2 => Some Cancelled
  | 3 => Some BudgetExhausted
  | 4 => Some Failed
  | _ => None
  end.

Definition decode_evidence (raw : nat) : option evidence_state :=
  match raw with
  | 0 => Some EvidenceNone
  | 1 => Some EvidenceCandidate
  | 2 => Some EvidenceVerified
  | 3 => Some EvidenceStale
  | 4 => Some EvidenceInvalid
  | _ => None
  end.

Theorem raw_axes_reject_unknown_values :
  decode_precision 99 = None
  /\ decode_completeness 99 = None
  /\ decode_applicability 99 = None
  /\ decode_termination 99 = None
  /\ decode_evidence 99 = None.
Proof. repeat split; reflexivity. Qed.

(** ** Orthogonal outcomes and publication *)

Record operation_outcome : Type := {
  outcome_precision : precision;
  outcome_completeness : completeness;
  outcome_applicability : applicability;
  outcome_termination : termination;
  outcome_evidence : evidence_state;
  resource_present : bool;
  evidence_present : bool
}.

Definition outcome_well_formed (outcome : operation_outcome) : Prop :=
  (In (outcome_termination outcome) [Cancelled; BudgetExhausted; Failed] ->
     resource_present outcome = false /\ evidence_present outcome = false)
  /\ (evidence_present outcome = true ->
       resource_present outcome = true
       /\ In (outcome_evidence outcome) [EvidenceCandidate; EvidenceVerified])
  /\ (resource_present outcome = true ->
       outcome_termination outcome = Succeeded
       /\ outcome_applicability outcome = Applicable)
  /\ (In (outcome_termination outcome) [Cancelled; BudgetExhausted] ->
       outcome_completeness outcome = CompletionIncomplete)
  /\ (outcome_evidence outcome = EvidenceVerified ->
       evidence_present outcome = true).

Definition authoritative_exact (outcome : operation_outcome) : Prop :=
  outcome_precision outcome = PrecisionExact
  /\ outcome_completeness outcome = CompletionComplete
  /\ outcome_applicability outcome = Applicable
  /\ outcome_termination outcome = Succeeded
  /\ outcome_evidence outcome = EvidenceVerified
  /\ evidence_present outcome = true.

Theorem authoritative_exact_requires_verified_evidence :
  forall outcome,
    authoritative_exact outcome ->
    outcome_evidence outcome = EvidenceVerified
    /\ evidence_present outcome = true.
Proof.
  intros outcome Hauthoritative.
  unfold authoritative_exact in Hauthoritative.
  tauto.
Qed.

Theorem cancellation_never_publishes :
  forall outcome,
    outcome_well_formed outcome ->
    outcome_termination outcome = Cancelled ->
    resource_present outcome = false /\ evidence_present outcome = false.
Proof.
  intros outcome Hwell Hcancelled.
  unfold outcome_well_formed in Hwell.
  destruct Hwell as [Hterminal _].
  apply Hterminal. now left.
Qed.

Theorem budget_exhaustion_never_publishes :
  forall outcome,
    outcome_well_formed outcome ->
    outcome_termination outcome = BudgetExhausted ->
    resource_present outcome = false /\ evidence_present outcome = false.
Proof.
  intros outcome Hwell Hbudget.
  unfold outcome_well_formed in Hwell.
  destruct Hwell as [Hterminal _].
  apply Hterminal. right; now left.
Qed.

Theorem cancellation_is_incomplete :
  forall outcome,
    outcome_well_formed outcome ->
    outcome_termination outcome = Cancelled ->
    outcome_completeness outcome = CompletionIncomplete.
Proof.
  intros outcome Hwell Hcancelled.
  unfold outcome_well_formed in Hwell.
  destruct Hwell as [_ [_ [_ [Hincomplete _]]]].
  apply Hincomplete. now left.
Qed.

Theorem exact_and_incomplete_are_independent :
  exists p c, p = PrecisionExact /\ c = CompletionIncomplete.
Proof. exists PrecisionExact, CompletionIncomplete. now split. Qed.

Theorem approximate_and_complete_are_independent :
  exists p c, p = PrecisionApproximate /\ c = CompletionComplete.
Proof. exists PrecisionApproximate, CompletionComplete. now split. Qed.

(** ** Canonical budgets and sticky cancellation *)

Definition canonical_limit (enabled : bool) (value : nat) : Prop :=
  if enabled then 0 < value else value = 0.

Theorem inactive_limit_must_be_zero :
  forall value, canonical_limit false value -> value = 0.
Proof. intros value Hcanonical. exact Hcanonical. Qed.

Theorem active_limit_must_be_positive :
  forall value, canonical_limit true value -> 0 < value.
Proof. intros value Hcanonical. exact Hcanonical. Qed.

Inductive cancellation_reason : Type :=
  | Requested | Deadline | Budget | Source.

Inductive cancellation_state : Type :=
  | Live
  | CancellationRequested (reason : cancellation_reason).

Definition request_cancel
    (state : cancellation_state) (reason : cancellation_reason)
    : cancellation_state :=
  match state with
  | Live => CancellationRequested reason
  | CancellationRequested first => CancellationRequested first
  end.

Theorem cancellation_is_sticky :
  forall first second,
    request_cancel (request_cancel Live first) second =
    CancellationRequested first.
Proof. reflexivity. Qed.

(** ** Opaque-handle ownership *)

Inductive handle_state : Type := HandleLive | HandleReleased.

Definition release_handle (state : handle_state) : option handle_state :=
  match state with
  | HandleLive => Some HandleReleased
  | HandleReleased => None
  end.

Theorem first_release_succeeds :
  release_handle HandleLive = Some HandleReleased.
Proof. reflexivity. Qed.

Theorem double_release_is_rejected :
  release_handle HandleReleased = None.
Proof. reflexivity. Qed.

(** ** Fixed-width pointer-free metadata layouts *)

Inductive pod_field : Type := U32 | U64 | Bytes16 | Bytes32.

Definition field_size (field : pod_field) : nat :=
  match field with U32 => 4 | U64 => 8 | Bytes16 => 16 | Bytes32 => 32 end.

Fixpoint layout_size (layout : list pod_field) : nat :=
  match layout with
  | [] => 0
  | field :: rest => field_size field + layout_size rest
  end.

Definition descriptor_layout : list pod_field :=
  [U32; U32; U64; U64; Bytes16; Bytes16; Bytes16; Bytes16; Bytes32].

Definition budget_layout : list pod_field :=
  [U32; U32; U64; U64; U64; U64; U64; U64; U64; U64].

Definition outcome_layout : list pod_field :=
  [U32; U32; U64; U64; U32; U32; U32; U32; U32; U32;
   U64; U64; U64; U64; U64; U64].

Theorem descriptor_layout_is_120_bytes : layout_size descriptor_layout = 120.
Proof. reflexivity. Qed.

Theorem budget_layout_is_72_bytes : layout_size budget_layout = 72.
Proof. reflexivity. Qed.

Theorem outcome_layout_is_96_bytes : layout_size outcome_layout = 96.
Proof. reflexivity. Qed.
