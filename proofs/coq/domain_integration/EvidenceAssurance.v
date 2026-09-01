(** * EvidenceAssurance — freshness-bound independent exact publication

    Raw libcpg analysis output is candidate evidence.  Exact publication
    requires a separately trusted guarantee bound to the same subject,
    immutable snapshot, configuration, tool revision, environment, and result
    digest.  Independence is a trust-policy relation, not actor-name inequality.
    Precision and completeness flags are necessary metadata but cannot create
    validation authority.
*)

Section EvidenceBoundary.

Variables Subject Snapshot Configuration Tool Environment Digest Actor Finding : Type.

Record evidence_index : Type := {
  evidence_subject : Subject;
  evidence_snapshot : Snapshot;
  evidence_configuration : Configuration;
  evidence_tool : Tool;
  evidence_environment : Environment
}.

Definition same_evidence_index (left right : evidence_index) : Prop :=
  evidence_subject left = evidence_subject right /\
  evidence_snapshot left = evidence_snapshot right /\
  evidence_configuration left = evidence_configuration right /\
  evidence_tool left = evidence_tool right /\
  evidence_environment left = evidence_environment right.

Inductive assurance_precision : Type :=
| ExactPrecision
| SoundApproximation.

Inductive assurance_completeness : Type :=
| CompleteCoverage
| IncompleteCoverage.

Record candidate_report : Type := {
  report_index : evidence_index;
  report_digest : Digest;
  report_producer : Actor;
  report_precision : assurance_precision;
  report_completeness : assurance_completeness
}.

Record exact_guarantee : Type := {
  guarantee_index : evidence_index;
  guarantee_digest : Digest;
  guarantee_verifier : Actor
}.

Variable reference_member : evidence_index -> Finding -> Prop.
Variable candidate_member : evidence_index -> Finding -> Prop.
Variable confirmed_member : evidence_index -> Finding -> Prop.
Variable trusted_guarantee : exact_guarantee -> Prop.
Variable independent : Actor -> Actor -> Prop.

Hypothesis independence_is_irreflexive :
  forall actor, ~ independent actor actor.

Definition generator_complete (index : evidence_index) : Prop :=
  forall finding,
    reference_member index finding -> candidate_member index finding.

Definition confirmer_sound : Prop :=
  forall index finding,
    confirmed_member index finding -> reference_member index finding.

Definition confirmer_complete : Prop :=
  forall index finding,
    reference_member index finding -> confirmed_member index finding.

Definition accepted_member
    (report : candidate_report)
    (guarantee : exact_guarantee)
    (finding : Finding) : Prop :=
  candidate_member (report_index report) finding /\
  confirmed_member (guarantee_index guarantee) finding.

Record exact_publication_certificate
    (requested : evidence_index)
    (report : candidate_report)
    (guarantee : exact_guarantee) : Prop := {
  certificate_precision :
    report_precision report = ExactPrecision;
  certificate_completeness :
    report_completeness report = CompleteCoverage;
  certificate_report_fresh :
    same_evidence_index (report_index report) requested;
  certificate_guarantee_fresh :
    same_evidence_index (guarantee_index guarantee) requested;
  certificate_digest_binding :
    report_digest report = guarantee_digest guarantee;
  certificate_trust :
    trusted_guarantee guarantee;
  certificate_independence :
    independent (report_producer report) (guarantee_verifier guarantee);
  certificate_generator_complete :
    generator_complete (report_index report);
  certificate_confirmer_sound :
    confirmer_sound;
  certificate_confirmer_complete :
    confirmer_complete
}.

Lemma same_evidence_index_reflexive : forall index,
  same_evidence_index index index.
Proof.
  intros [subject snapshot configuration tool environment].
  unfold same_evidence_index; simpl; auto.
Qed.

Lemma same_evidence_index_symmetric : forall left right,
  same_evidence_index left right ->
  same_evidence_index right left.
Proof.
  intros [ls lsn lc lt le] [rs rsn rc rt re].
  unfold same_evidence_index; simpl.
  intros [subject [snapshot [configuration [tool environment]]]].
  subst; auto.
Qed.

Lemma same_evidence_index_transitive : forall first second third,
  same_evidence_index first second ->
  same_evidence_index second third ->
  same_evidence_index first third.
Proof.
  intros [fs fsn fc ft fe] [ss ssn sc st se] [ts tsn tc tt te].
  unfold same_evidence_index; simpl.
  intros [subject_one [snapshot_one [configuration_one [tool_one environment_one]]]]
         [subject_two [snapshot_two [configuration_two [tool_two environment_two]]]].
  subst; auto.
Qed.

Lemma same_index_preserves_reference : forall left right,
  same_evidence_index left right ->
  forall finding,
    reference_member left finding <-> reference_member right finding.
Proof.
  intros [ls lsn lc lt le] [rs rsn rc rt re].
  unfold same_evidence_index; simpl.
  intros [subject [snapshot [configuration [tool environment]]]] finding.
  subst; split; intro reference; exact reference.
Qed.

Theorem certified_publication_is_sound :
  forall requested report guarantee
         (certificate : exact_publication_certificate requested report guarantee)
         finding,
    accepted_member report guarantee finding ->
    reference_member requested finding.
Proof.
  intros requested report guarantee certificate finding [_ confirmed].
  apply (proj1 (same_index_preserves_reference
    (guarantee_index guarantee) requested
    (certificate_guarantee_fresh _ _ _ certificate) finding)).
  exact (certificate_confirmer_sound _ _ _ certificate
    (guarantee_index guarantee) finding confirmed).
Qed.

Theorem certified_publication_equals_reference :
  forall requested report guarantee
         (certificate : exact_publication_certificate requested report guarantee)
         finding,
    accepted_member report guarantee finding <->
    reference_member requested finding.
Proof.
  intros requested report guarantee certificate finding.
  split.
  - intro accepted.
    exact (certified_publication_is_sound
      requested report guarantee certificate finding accepted).
  - intro reference.
    split.
    + apply (certificate_generator_complete _ _ _ certificate).
      apply (proj2 (same_index_preserves_reference
        (report_index report) requested
        (certificate_report_fresh _ _ _ certificate) finding)).
      exact reference.
    + apply (certificate_confirmer_complete _ _ _ certificate).
      apply (proj2 (same_index_preserves_reference
        (guarantee_index guarantee) requested
        (certificate_guarantee_fresh _ _ _ certificate) finding)).
      exact reference.
Qed.

Theorem subject_mismatch_is_stale : forall left right,
  evidence_subject left <> evidence_subject right ->
  ~ same_evidence_index left right.
Proof. intros left right mismatch [equal _]; exact (mismatch equal). Qed.

Theorem snapshot_mismatch_is_stale : forall left right,
  evidence_snapshot left <> evidence_snapshot right ->
  ~ same_evidence_index left right.
Proof. intros left right mismatch [_ [equal _]]; exact (mismatch equal). Qed.

Theorem configuration_mismatch_is_stale : forall left right,
  evidence_configuration left <> evidence_configuration right ->
  ~ same_evidence_index left right.
Proof. intros left right mismatch [_ [_ [equal _]]]; exact (mismatch equal). Qed.

Theorem tool_mismatch_is_stale : forall left right,
  evidence_tool left <> evidence_tool right ->
  ~ same_evidence_index left right.
Proof. intros left right mismatch [_ [_ [_ [equal _]]]]; exact (mismatch equal). Qed.

Theorem environment_mismatch_is_stale : forall left right,
  evidence_environment left <> evidence_environment right ->
  ~ same_evidence_index left right.
Proof. intros left right mismatch [_ [_ [_ [_ equal]]]]; exact (mismatch equal). Qed.

Theorem approximate_report_cannot_have_exact_certificate :
  forall requested report guarantee,
    report_precision report = SoundApproximation ->
    ~ exact_publication_certificate requested report guarantee.
Proof.
  intros requested report guarantee approximate certificate.
  pose proof (certificate_precision _ _ _ certificate) as exact.
  rewrite approximate in exact.
  discriminate.
Qed.

Theorem incomplete_report_cannot_have_exact_certificate :
  forall requested report guarantee,
    report_completeness report = IncompleteCoverage ->
    ~ exact_publication_certificate requested report guarantee.
Proof.
  intros requested report guarantee incomplete certificate.
  pose proof (certificate_completeness _ _ _ certificate) as complete.
  rewrite incomplete in complete.
  discriminate.
Qed.

Theorem untrusted_guarantee_cannot_have_exact_certificate :
  forall requested report guarantee,
    ~ trusted_guarantee guarantee ->
    ~ exact_publication_certificate requested report guarantee.
Proof.
  intros requested report guarantee untrusted certificate.
  exact (untrusted (certificate_trust _ _ _ certificate)).
Qed.

Theorem result_digest_mismatch_rejects_guarantee :
  forall requested report guarantee,
    report_digest report <> guarantee_digest guarantee ->
    ~ exact_publication_certificate requested report guarantee.
Proof.
  intros requested report guarantee mismatch certificate.
  exact (mismatch (certificate_digest_binding _ _ _ certificate)).
Qed.

Theorem stale_report_rejects_exact_publication :
  forall requested report guarantee,
    ~ same_evidence_index (report_index report) requested ->
    ~ exact_publication_certificate requested report guarantee.
Proof.
  intros requested report guarantee stale certificate.
  exact (stale (certificate_report_fresh _ _ _ certificate)).
Qed.

Theorem stale_guarantee_rejects_exact_publication :
  forall requested report guarantee,
    ~ same_evidence_index (guarantee_index guarantee) requested ->
    ~ exact_publication_certificate requested report guarantee.
Proof.
  intros requested report guarantee stale certificate.
  exact (stale (certificate_guarantee_fresh _ _ _ certificate)).
Qed.

Theorem candidate_self_confirmation_is_rejected :
  forall requested report guarantee,
    report_producer report = guarantee_verifier guarantee ->
    ~ exact_publication_certificate requested report guarantee.
Proof.
  intros requested report guarantee same_actor certificate.
  pose proof (certificate_independence _ _ _ certificate) as independent_evidence.
  rewrite same_actor in independent_evidence.
  exact ((independence_is_irreflexive
    (guarantee_verifier guarantee)) independent_evidence).
Qed.

Definition compose_precision
    (left right : assurance_precision) : assurance_precision :=
  match left, right with
  | ExactPrecision, ExactPrecision => ExactPrecision
  | _, _ => SoundApproximation
  end.

Definition compose_completeness
    (left right : assurance_completeness) : assurance_completeness :=
  match left, right with
  | CompleteCoverage, CompleteCoverage => CompleteCoverage
  | _, _ => IncompleteCoverage
  end.

Theorem composed_exactness_has_no_promotion : forall left right,
  compose_precision left right = ExactPrecision ->
  left = ExactPrecision /\ right = ExactPrecision.
Proof. destruct left, right; simpl; intros; try discriminate; auto. Qed.

Theorem composed_completeness_has_no_promotion : forall left right,
  compose_completeness left right = CompleteCoverage ->
  left = CompleteCoverage /\ right = CompleteCoverage.
Proof. destruct left, right; simpl; intros; try discriminate; auto. Qed.

Inductive evidence_validation_phase : Type :=
| CheckPrecision
| CheckCompleteness
| CheckIndexBinding
| CheckDigestBinding
| CheckTrust
| CheckIndependence
| EvidenceAccepted
| EvidenceRejected.

Theorem evidence_validation_control_is_finite : forall phase,
  phase = CheckPrecision \/
  phase = CheckCompleteness \/
  phase = CheckIndexBinding \/
  phase = CheckDigestBinding \/
  phase = CheckTrust \/
  phase = CheckIndependence \/
  phase = EvidenceAccepted \/
  phase = EvidenceRejected.
Proof.
  destruct phase.
  - left; reflexivity.
  - right; left; reflexivity.
  - right; right; left; reflexivity.
  - right; right; right; left; reflexivity.
  - right; right; right; right; left; reflexivity.
  - right; right; right; right; right; left; reflexivity.
  - right; right; right; right; right; right; left; reflexivity.
  - right; right; right; right; right; right; right; reflexivity.
Qed.

End EvidenceBoundary.

Definition never_independent (_ _ : bool) : Prop := False.

Theorem distinct_actor_names_do_not_establish_independence :
  false <> true /\ ~ never_independent false true.
Proof.
  split.
  - discriminate.
  - unfold never_independent; intro impossible; exact impossible.
Qed.
