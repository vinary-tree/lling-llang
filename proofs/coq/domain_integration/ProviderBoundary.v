(** * ProviderBoundary — independent assurance and one-way public APIs

    The model is deliberately provider-neutral.  A downstream consumer may
    depend on an upstream provider's public API; the provider never depends on
    the consumer and neither party crosses the other's private boundary.
    Exact publication additionally requires a trusted guarantee from a
    different control domain, bound to the same canonical evidence identity.
*)

From Stdlib Require Import Bool.Bool Arith.PeanoNat.
Require Import LlingLlang.domain_integration.ProviderResult.
Require Import LlingLlang.domain_integration.CanonicalArtifact.

Record actor_identity : Type := actor_identity_value {
  actor_name : nat;
  actor_control_domain : nat
}.

Definition policy_independent
    (producer verifier : actor_identity) : bool :=
  negb (Nat.eqb
    (actor_control_domain producer)
    (actor_control_domain verifier)).

Theorem policy_independence_is_irreflexive : forall actor,
  policy_independent actor actor = false.
Proof.
  intros [name domain]; unfold policy_independent; simpl.
  now rewrite Nat.eqb_refl.
Qed.

Theorem shared_control_domain_is_dependent : forall producer verifier,
  actor_control_domain producer = actor_control_domain verifier ->
  policy_independent producer verifier = false.
Proof.
  intros [producer_name producer_domain]
         [verifier_name verifier_domain] same_domain.
  unfold policy_independent; simpl in *; subst.
  now rewrite Nat.eqb_refl.
Qed.

Theorem distinct_names_do_not_imply_independence :
  actor_name (actor_identity_value 0 7) <>
    actor_name (actor_identity_value 1 7) /\
  policy_independent
    (actor_identity_value 0 7)
    (actor_identity_value 1 7) = false.
Proof. simpl; split; [discriminate | reflexivity]. Qed.

Record candidate_report : Type := candidate_report_value {
  candidate_binding : evidence_binding;
  candidate_status : result_status;
  candidate_producer : actor_identity
}.

Record independent_guarantee : Type := independent_guarantee_value {
  guarantee_binding : evidence_binding;
  guarantee_verifier : actor_identity;
  guarantee_trusted : bool
}.

Record exact_publication_certificate
    (requested : evidence_binding)
    (candidate : candidate_report)
    (guarantee : independent_guarantee) : Prop := {
  certificate_candidate_exact :
    candidate_status candidate = CompleteExact;
  certificate_candidate_fresh :
    same_evidence_binding (candidate_binding candidate) requested;
  certificate_guarantee_fresh :
    same_evidence_binding (guarantee_binding guarantee) requested;
  certificate_guarantee_trusted :
    guarantee_trusted guarantee = true;
  certificate_policy_independent :
    policy_independent
      (candidate_producer candidate)
      (guarantee_verifier guarantee) = true
}.

Theorem exact_publication_has_exact_candidate : forall requested candidate guarantee,
  exact_publication_certificate requested candidate guarantee ->
  candidate_status candidate = CompleteExact.
Proof.
  intros requested candidate guarantee certificate.
  exact (certificate_candidate_exact requested candidate guarantee certificate).
Qed.

Theorem approximate_candidate_cannot_self_promote : forall requested candidate guarantee,
  candidate_status candidate = CompleteApproximate ->
  ~ exact_publication_certificate requested candidate guarantee.
Proof.
  intros requested candidate guarantee approximate certificate.
  pose proof (certificate_candidate_exact
    requested candidate guarantee certificate) as exact.
  rewrite approximate in exact; discriminate.
Qed.

Theorem incomplete_candidate_cannot_self_promote : forall requested candidate guarantee,
  candidate_status candidate = Incomplete ->
  ~ exact_publication_certificate requested candidate guarantee.
Proof.
  intros requested candidate guarantee incomplete certificate.
  pose proof (certificate_candidate_exact
    requested candidate guarantee certificate) as exact.
  rewrite incomplete in exact; discriminate.
Qed.

Theorem stale_candidate_rejects_exact : forall requested candidate guarantee,
  ~ same_evidence_binding (candidate_binding candidate) requested ->
  ~ exact_publication_certificate requested candidate guarantee.
Proof.
  intros requested candidate guarantee stale certificate.
  exact (stale (certificate_candidate_fresh
    requested candidate guarantee certificate)).
Qed.

Theorem stale_guarantee_rejects_exact : forall requested candidate guarantee,
  ~ same_evidence_binding (guarantee_binding guarantee) requested ->
  ~ exact_publication_certificate requested candidate guarantee.
Proof.
  intros requested candidate guarantee stale certificate.
  exact (stale (certificate_guarantee_fresh
    requested candidate guarantee certificate)).
Qed.

Theorem untrusted_guarantee_rejects_exact : forall requested candidate guarantee,
  guarantee_trusted guarantee = false ->
  ~ exact_publication_certificate requested candidate guarantee.
Proof.
  intros requested candidate guarantee untrusted certificate.
  pose proof (certificate_guarantee_trusted
    requested candidate guarantee certificate) as trusted.
  rewrite untrusted in trusted; discriminate.
Qed.

Theorem dependent_guarantee_rejects_exact : forall requested candidate guarantee,
  policy_independent
    (candidate_producer candidate)
    (guarantee_verifier guarantee) = false ->
  ~ exact_publication_certificate requested candidate guarantee.
Proof.
  intros requested candidate guarantee dependent certificate.
  pose proof (certificate_policy_independent
    requested candidate guarantee certificate) as independent.
  rewrite dependent in independent; discriminate.
Qed.

Theorem self_confirmation_rejects_exact : forall requested candidate guarantee,
  candidate_producer candidate = guarantee_verifier guarantee ->
  ~ exact_publication_certificate requested candidate guarantee.
Proof.
  intros requested candidate guarantee same_actor certificate.
  pose proof (certificate_policy_independent
    requested candidate guarantee certificate) as independent.
  rewrite same_actor in independent.
  rewrite policy_independence_is_irreflexive in independent; discriminate.
Qed.

Inductive boundary_party : Type :=
| UpstreamProvider
| DownstreamConsumer.

Inductive access_surface : Type :=
| PublicApi
| PrivateInternals.

Definition lawful_dependency
    (from to : boundary_party)
    (surface : access_surface) : bool :=
  match from, to, surface with
  | DownstreamConsumer, UpstreamProvider, PublicApi => true
  | _, _, _ => false
  end.

Theorem lawful_dependency_is_one_way_public : forall from to surface,
  lawful_dependency from to surface = true ->
  from = DownstreamConsumer /\
  to = UpstreamProvider /\
  surface = PublicApi.
Proof. destruct from, to, surface; simpl; intros; try discriminate; auto. Qed.

Theorem reverse_dependency_is_forbidden : forall surface,
  lawful_dependency UpstreamProvider DownstreamConsumer surface = false.
Proof. now destruct surface. Qed.

Theorem private_internals_are_forbidden : forall from to,
  lawful_dependency from to PrivateInternals = false.
Proof. now destruct from, to. Qed.

Inductive native_owner : Type :=
| ProviderOwner
| ConsumerOwner.

Record native_handle : Type := native_handle_value {
  handle_owner : native_owner;
  handle_borrows : nat;
  handle_released : bool
}.

Definition borrow_handle (handle : native_handle) : option native_handle :=
  if handle_released handle then None
  else Some (native_handle_value
    (handle_owner handle) (S (handle_borrows handle)) false).

Definition release_borrow (handle : native_handle) : option native_handle :=
  match handle_borrows handle with
  | O => None
  | S remaining => Some (native_handle_value
      (handle_owner handle) remaining (handle_released handle))
  end.

Definition destroy_handle (handle : native_handle) : option native_handle :=
  match handle_owner handle, handle_borrows handle, handle_released handle with
  | ProviderOwner, O, false =>
      Some (native_handle_value ProviderOwner O true)
  | _, _, _ => None
  end.

Theorem borrow_preserves_owner : forall handle borrowed,
  borrow_handle handle = Some borrowed ->
  handle_owner borrowed = handle_owner handle.
Proof.
  intros [owner borrows released] borrowed result; simpl in *.
  destruct released; try discriminate; inversion result; reflexivity.
Qed.

Theorem release_preserves_owner : forall handle released,
  release_borrow handle = Some released ->
  handle_owner released = handle_owner handle.
Proof.
  intros [owner borrows is_released] released result; simpl in *.
  destruct borrows; try discriminate; inversion result; reflexivity.
Qed.

Theorem release_never_underflows : forall owner released,
  release_borrow (native_handle_value owner O released) = None.
Proof. reflexivity. Qed.

Theorem only_provider_destroys_unborrowed_live_handle : forall handle destroyed,
  destroy_handle handle = Some destroyed ->
  handle_owner handle = ProviderOwner /\
  handle_borrows handle = O /\
  handle_released handle = false /\
  handle_released destroyed = true.
Proof.
  intros [owner borrows released] destroyed result; simpl in *.
  destruct owner, borrows, released; try discriminate;
    inversion result; auto.
Qed.

Inductive boundary_control_phase : Type :=
| NegotiatePublicApi
| CaptureIdentity
| InvokeProvider
| ClassifyProviderResult
| ValidateIndependentGuarantee
| PublishOrReject
| ReleaseNativeResources.

Theorem boundary_control_is_finite : forall phase,
  phase = NegotiatePublicApi \/
  phase = CaptureIdentity \/
  phase = InvokeProvider \/
  phase = ClassifyProviderResult \/
  phase = ValidateIndependentGuarantee \/
  phase = PublishOrReject \/
  phase = ReleaseNativeResources.
Proof. destruct phase; auto 7. Qed.

Print Assumptions approximate_candidate_cannot_self_promote.
Print Assumptions incomplete_candidate_cannot_self_promote.
Print Assumptions distinct_names_do_not_imply_independence.
Print Assumptions lawful_dependency_is_one_way_public.
Print Assumptions only_provider_destroys_unborrowed_live_handle.
