(** * CanonicalArtifact — stable artifact and evidence-binding identity

    Cryptographic digests are represented by natural-number identities in the
    logic: unequal values are unequal identities.  Collision resistance remains
    a cryptographic implementation assumption and is not claimed here.  The
    executable finite-universe manifest abstraction proves that input order and
    duplicate delivery cannot change the canonical membership vector.
*)

From Stdlib Require Import Bool.Bool Lists.List Arith.PeanoNat Sorting.Permutation.
Import ListNotations.

Definition manifest_bit (artifact : nat) (inputs : list nat) : bool :=
  existsb (Nat.eqb artifact) inputs.

Definition canonical_manifest
    (universe inputs : list nat) : list bool :=
  map (fun artifact => manifest_bit artifact inputs) universe.

Lemma manifest_bit_true_iff : forall artifact inputs,
  manifest_bit artifact inputs = true <-> In artifact inputs.
Proof.
  intros artifact inputs; unfold manifest_bit; rewrite existsb_exists.
  split.
  - intros [candidate [member equal]].
    apply Nat.eqb_eq in equal; now subst.
  - intro member; exists artifact; split; [exact member | apply Nat.eqb_refl].
Qed.

Lemma manifest_bit_permutation : forall artifact left right,
  Permutation left right ->
  manifest_bit artifact left = manifest_bit artifact right.
Proof.
  intros artifact left right permutation.
  destruct (manifest_bit artifact left) eqn:left_bit,
           (manifest_bit artifact right) eqn:right_bit; try reflexivity.
  - apply manifest_bit_true_iff in left_bit.
    apply (Permutation_in artifact permutation) in left_bit.
    apply <- manifest_bit_true_iff in left_bit.
    rewrite right_bit in left_bit; discriminate.
  - apply manifest_bit_true_iff in right_bit.
    apply (Permutation_in artifact (Permutation_sym permutation)) in right_bit.
    apply <- manifest_bit_true_iff in right_bit.
    rewrite left_bit in right_bit; discriminate.
Qed.

Theorem canonical_manifest_permutation_invariant : forall universe left right,
  Permutation left right ->
  canonical_manifest universe left = canonical_manifest universe right.
Proof.
  intros universe left right permutation; induction universe as [|head tail IH].
  - reflexivity.
  - simpl; rewrite (manifest_bit_permutation head left right permutation), IH.
    reflexivity.
Qed.

Lemma manifest_bit_duplicate_invariant : forall queried artifact inputs,
  manifest_bit queried (artifact :: artifact :: inputs) =
  manifest_bit queried (artifact :: inputs).
Proof.
  intros queried artifact inputs; unfold manifest_bit; simpl.
  now destruct (Nat.eqb queried artifact).
Qed.

Theorem canonical_manifest_duplicate_invariant : forall universe artifact inputs,
  canonical_manifest universe (artifact :: artifact :: inputs) =
  canonical_manifest universe (artifact :: inputs).
Proof.
  intros universe artifact inputs; induction universe as [|head tail IH].
  - reflexivity.
  - change
      (manifest_bit head (artifact :: artifact :: inputs) ::
         canonical_manifest tail (artifact :: artifact :: inputs) =
       manifest_bit head (artifact :: inputs) ::
         canonical_manifest tail (artifact :: inputs)).
    rewrite manifest_bit_duplicate_invariant, IH; reflexivity.
Qed.

Record artifact_identity : Type := artifact_identity_value {
  artifact_schema : nat;
  artifact_digest : nat;
  artifact_size : nat;
  artifact_uri_digest : nat;
  artifact_observed_digest : nat;
  artifact_observed_size : nat
}.

Definition artifact_identity_valid (artifact : artifact_identity) : bool :=
  Nat.eqb (artifact_digest artifact) (artifact_uri_digest artifact) &&
  Nat.eqb (artifact_digest artifact) (artifact_observed_digest artifact) &&
  Nat.eqb (artifact_size artifact) (artifact_observed_size artifact).

Theorem valid_artifact_binds_uri : forall artifact,
  artifact_identity_valid artifact = true ->
  artifact_digest artifact = artifact_uri_digest artifact.
Proof.
  intros artifact valid; unfold artifact_identity_valid in valid.
  apply andb_true_iff in valid as [prefix _].
  apply andb_true_iff in prefix as [uri _].
  now apply Nat.eqb_eq.
Qed.

Theorem valid_artifact_binds_observed_digest : forall artifact,
  artifact_identity_valid artifact = true ->
  artifact_digest artifact = artifact_observed_digest artifact.
Proof.
  intros artifact valid; unfold artifact_identity_valid in valid.
  apply andb_true_iff in valid as [prefix _].
  apply andb_true_iff in prefix as [_ observed].
  now apply Nat.eqb_eq.
Qed.

Theorem valid_artifact_binds_observed_size : forall artifact,
  artifact_identity_valid artifact = true ->
  artifact_size artifact = artifact_observed_size artifact.
Proof.
  intros artifact valid; unfold artifact_identity_valid in valid.
  apply andb_true_iff in valid as [_ size].
  now apply Nat.eqb_eq.
Qed.

Theorem digest_tamper_rejects_artifact : forall artifact,
  artifact_digest artifact <> artifact_observed_digest artifact ->
  artifact_identity_valid artifact = false.
Proof.
  intros artifact mismatch; unfold artifact_identity_valid.
  destruct (Nat.eqb (artifact_digest artifact)
                    (artifact_observed_digest artifact)) eqn:equal.
  - apply Nat.eqb_eq in equal; contradiction.
  - now destruct (Nat.eqb (artifact_digest artifact)
                         (artifact_uri_digest artifact)).
Qed.

Theorem size_tamper_rejects_artifact : forall artifact,
  artifact_size artifact <> artifact_observed_size artifact ->
  artifact_identity_valid artifact = false.
Proof.
  intros artifact mismatch; unfold artifact_identity_valid.
  destruct (Nat.eqb (artifact_size artifact)
                    (artifact_observed_size artifact)) eqn:equal.
  - apply Nat.eqb_eq in equal; contradiction.
  - now rewrite andb_false_r.
Qed.

Record provider_descriptor_material : Type := provider_descriptor_value {
  descriptor_identifier : nat;
  descriptor_version : nat;
  descriptor_protocol : nat;
  descriptor_build_digest : nat;
  descriptor_capabilities : nat;
  descriptor_guarantees : nat;
  descriptor_determinism : nat;
  descriptor_side_effects : nat;
  descriptor_locks : nat
}.

Definition same_provider_descriptor
    (left right : provider_descriptor_material) : Prop := left = right.

Theorem descriptor_build_change_changes_identity : forall left right,
  descriptor_build_digest left <> descriptor_build_digest right ->
  ~ same_provider_descriptor left right.
Proof.
  intros left right mismatch equal; unfold same_provider_descriptor in equal.
  subst; apply mismatch; reflexivity.
Qed.

Theorem descriptor_capability_change_changes_identity : forall left right,
  descriptor_capabilities left <> descriptor_capabilities right ->
  ~ same_provider_descriptor left right.
Proof.
  intros left right mismatch equal; unfold same_provider_descriptor in equal.
  subst; apply mismatch; reflexivity.
Qed.

Record evidence_binding : Type := evidence_binding_value {
  binding_snapshot_digest : nat;
  binding_configuration_digest : nat;
  binding_provider_identity : nat;
  binding_environment_digest : nat;
  binding_input_manifest : list bool;
  binding_result_digest : nat
}.

Definition same_evidence_binding
    (left right : evidence_binding) : Prop :=
  binding_snapshot_digest left = binding_snapshot_digest right /\
  binding_configuration_digest left = binding_configuration_digest right /\
  binding_provider_identity left = binding_provider_identity right /\
  binding_environment_digest left = binding_environment_digest right /\
  binding_input_manifest left = binding_input_manifest right /\
  binding_result_digest left = binding_result_digest right.

Lemma same_evidence_binding_reflexive : forall binding,
  same_evidence_binding binding binding.
Proof. intros []; unfold same_evidence_binding; simpl; repeat split; reflexivity. Qed.

Lemma same_evidence_binding_symmetric : forall left right,
  same_evidence_binding left right -> same_evidence_binding right left.
Proof.
  intros [ls lc lp le li lr] [rs rc rp re ri rr].
  unfold same_evidence_binding; simpl.
  intros [snapshot [configuration [provider [environment [inputs result]]]]].
  subst; repeat split; reflexivity.
Qed.

Lemma same_evidence_binding_transitive : forall first second third,
  same_evidence_binding first second ->
  same_evidence_binding second third ->
  same_evidence_binding first third.
Proof.
  intros [fs fc fp fe fi fr] [ss sc sp se si sr] [ts tc tp te ti tr].
  unfold same_evidence_binding; simpl.
  intros [snapshot_one [configuration_one [provider_one
    [environment_one [inputs_one result_one]]]]]
    [snapshot_two [configuration_two [provider_two
      [environment_two [inputs_two result_two]]]]].
  subst; repeat split; reflexivity.
Qed.

Theorem snapshot_mismatch_is_stale_binding : forall left right,
  binding_snapshot_digest left <> binding_snapshot_digest right ->
  ~ same_evidence_binding left right.
Proof. intros left right mismatch [equal _]; exact (mismatch equal). Qed.

Theorem configuration_mismatch_is_stale_binding : forall left right,
  binding_configuration_digest left <> binding_configuration_digest right ->
  ~ same_evidence_binding left right.
Proof. intros left right mismatch [_ [equal _]]; exact (mismatch equal). Qed.

Theorem provider_mismatch_is_stale_binding : forall left right,
  binding_provider_identity left <> binding_provider_identity right ->
  ~ same_evidence_binding left right.
Proof. intros left right mismatch [_ [_ [equal _]]]; exact (mismatch equal). Qed.

Theorem environment_mismatch_is_stale_binding : forall left right,
  binding_environment_digest left <> binding_environment_digest right ->
  ~ same_evidence_binding left right.
Proof. intros left right mismatch [_ [_ [_ [equal _]]]]; exact (mismatch equal). Qed.

Theorem input_mismatch_is_stale_binding : forall left right,
  binding_input_manifest left <> binding_input_manifest right ->
  ~ same_evidence_binding left right.
Proof. intros left right mismatch [_ [_ [_ [_ [equal _]]]]]; exact (mismatch equal). Qed.

Theorem result_mismatch_is_stale_binding : forall left right,
  binding_result_digest left <> binding_result_digest right ->
  ~ same_evidence_binding left right.
Proof. intros left right mismatch [_ [_ [_ [_ [_ equal]]]]]; exact (mismatch equal). Qed.

Print Assumptions canonical_manifest_permutation_invariant.
Print Assumptions canonical_manifest_duplicate_invariant.
Print Assumptions digest_tamper_rejects_artifact.
Print Assumptions same_evidence_binding_transitive.
