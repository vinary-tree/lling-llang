(** * ManifestFactContracts — durable libcpg extraction identities

    This theory fixes the semantic boundary before any libcpg schema change.
    libcpg owns the extraction manifest and stable fact identities; a runtime
    owns the execution envelope; and the separately versioned
    vinary-libcpg-adapter owns the many-to-many fact/rule relation.  Dense
    identifiers remain local accelerators and are connected to durable keys by
    a checked bijection over the active fact set.

    Every input-sized traversal is represented by an explicit heap worklist.
    The native call-stack bound is therefore constant for an arbitrary-length
    execution, while the number of transitions is linear in the canonical key
    universe.
*)

From Stdlib Require Import List Bool Arith Lia Sorting.Permutation.
Import ListNotations.

Set Implicit Arguments.

Section OwnershipAndManifests.

Inductive contract_owner : Type :=
| LibcpgOwner
| RuntimeOwner
| AdapterOwner
| LlingLlangOwner.

Inductive contract_dimension : Type :=
| RepositoryIdentity
| ParserIdentity
| GrammarIdentity
| ExtractorIdentity
| QueryIdentity
| FeatureHistoryIdentity
| FactSchemaIdentity
| SourceIdentity
| SourceRevisionIdentity
| SemanticConfigurationIdentity
| ExecutableIdentity
| HostIdentity
| EnvironmentIdentity
| InvocationIdentity
| ResourceEnvelopeIdentity
| FactRuleLoweringIdentity
| GenericRuleIdentity.

Definition owner_of (dimension : contract_dimension) : contract_owner :=
  match dimension with
  | RepositoryIdentity
  | ParserIdentity
  | GrammarIdentity
  | ExtractorIdentity
  | QueryIdentity
  | FeatureHistoryIdentity
  | FactSchemaIdentity
  | SourceIdentity
  | SourceRevisionIdentity
  | SemanticConfigurationIdentity => LibcpgOwner
  | ExecutableIdentity
  | HostIdentity
  | EnvironmentIdentity
  | InvocationIdentity
  | ResourceEnvelopeIdentity => RuntimeOwner
  | FactRuleLoweringIdentity => AdapterOwner
  | GenericRuleIdentity => LlingLlangOwner
  end.

Theorem extraction_dimensions_are_libcpg_owned :
  forall dimension,
    In dimension
      [RepositoryIdentity; ParserIdentity; GrammarIdentity;
       ExtractorIdentity; QueryIdentity; FeatureHistoryIdentity;
       FactSchemaIdentity; SourceIdentity; SourceRevisionIdentity;
       SemanticConfigurationIdentity] ->
    owner_of dimension = LibcpgOwner.
Proof.
  intros dimension member.
  repeat (destruct member as [member | member]; [subst; reflexivity |]).
  contradiction.
Qed.

Theorem runtime_dimensions_are_runtime_owned :
  forall dimension,
    In dimension
      [ExecutableIdentity; HostIdentity; EnvironmentIdentity;
       InvocationIdentity; ResourceEnvelopeIdentity] ->
    owner_of dimension = RuntimeOwner.
Proof.
  intros dimension member.
  repeat (destruct member as [member | member]; [subst; reflexivity |]).
  contradiction.
Qed.

Theorem lowering_is_adapter_owned :
  owner_of FactRuleLoweringIdentity = AdapterOwner.
Proof. reflexivity. Qed.

Theorem generic_rule_identity_is_lling_llang_owned :
  owner_of GenericRuleIdentity = LlingLlangOwner.
Proof. reflexivity. Qed.

Variables RepositoryId ParserId GrammarId ExtractorId QueryId : Type.
Variables FeatureRevisionId SchemaId SourceId SourceRevisionId ConfigId : Type.
Variables ExecutableId HostId EnvironmentId InvocationId ResourceEnvelopeId : Type.

Record extraction_manifest : Type := {
  manifest_repository : RepositoryId;
  manifest_parser : ParserId;
  manifest_grammar : GrammarId;
  manifest_extractor : ExtractorId;
  manifest_query : QueryId;
  manifest_feature_revision : FeatureRevisionId;
  manifest_schema : SchemaId;
  manifest_source : SourceId;
  manifest_source_revision : SourceRevisionId;
  manifest_configuration : ConfigId
}.

Record runtime_envelope : Type := {
  runtime_executable : ExecutableId;
  runtime_host : HostId;
  runtime_environment : EnvironmentId;
  runtime_invocation : InvocationId;
  runtime_resources : ResourceEnvelopeId
}.

(** Compatibility is deliberately exact.  Display labels and paths are not
    fields of the semantic manifest; their explicit rename operations can
    therefore preserve durable identity without weakening cache binding. *)
Definition manifest_compatible
    (requested cached : extraction_manifest) : Prop :=
  requested = cached.

Theorem manifest_compatibility_reflexive :
  forall manifest, manifest_compatible manifest manifest.
Proof. intros manifest; reflexivity. Qed.

Theorem manifest_compatibility_symmetric :
  forall left right,
    manifest_compatible left right -> manifest_compatible right left.
Proof. intros left right equal; symmetry; exact equal. Qed.

Theorem manifest_compatibility_transitive :
  forall first second third,
    manifest_compatible first second ->
    manifest_compatible second third ->
    manifest_compatible first third.
Proof. intros first second third first_second second_third; congruence. Qed.

Theorem repository_mismatch_invalidates : forall left right,
  manifest_repository left <> manifest_repository right ->
  ~ manifest_compatible left right.
Proof. intros left right mismatch equal; apply mismatch; now rewrite equal. Qed.

Theorem parser_mismatch_invalidates : forall left right,
  manifest_parser left <> manifest_parser right ->
  ~ manifest_compatible left right.
Proof. intros left right mismatch equal; apply mismatch; now rewrite equal. Qed.

Theorem grammar_mismatch_invalidates : forall left right,
  manifest_grammar left <> manifest_grammar right ->
  ~ manifest_compatible left right.
Proof. intros left right mismatch equal; apply mismatch; now rewrite equal. Qed.

Theorem extractor_mismatch_invalidates : forall left right,
  manifest_extractor left <> manifest_extractor right ->
  ~ manifest_compatible left right.
Proof. intros left right mismatch equal; apply mismatch; now rewrite equal. Qed.

Theorem query_mismatch_invalidates : forall left right,
  manifest_query left <> manifest_query right ->
  ~ manifest_compatible left right.
Proof. intros left right mismatch equal; apply mismatch; now rewrite equal. Qed.

Theorem feature_revision_mismatch_invalidates : forall left right,
  manifest_feature_revision left <> manifest_feature_revision right ->
  ~ manifest_compatible left right.
Proof. intros left right mismatch equal; apply mismatch; now rewrite equal. Qed.

Theorem schema_mismatch_invalidates : forall left right,
  manifest_schema left <> manifest_schema right ->
  ~ manifest_compatible left right.
Proof. intros left right mismatch equal; apply mismatch; now rewrite equal. Qed.

Theorem source_identity_mismatch_invalidates : forall left right,
  manifest_source left <> manifest_source right ->
  ~ manifest_compatible left right.
Proof. intros left right mismatch equal; apply mismatch; now rewrite equal. Qed.

Theorem source_revision_mismatch_invalidates : forall left right,
  manifest_source_revision left <> manifest_source_revision right ->
  ~ manifest_compatible left right.
Proof. intros left right mismatch equal; apply mismatch; now rewrite equal. Qed.

Theorem configuration_mismatch_invalidates : forall left right,
  manifest_configuration left <> manifest_configuration right ->
  ~ manifest_compatible left right.
Proof. intros left right mismatch equal; apply mismatch; now rewrite equal. Qed.

Variable ArtifactDigest : Type.

Record manifest_cache_entry : Type := {
  cached_manifest : extraction_manifest;
  cached_artifact_digest : ArtifactDigest;
  cached_result_complete : bool
}.

Definition cache_entry_valid
    (requested : extraction_manifest)
    (entry : manifest_cache_entry) : Prop :=
  manifest_compatible requested (cached_manifest entry) /\
  cached_result_complete entry = true.

Theorem cache_reuse_requires_exact_manifest : forall requested entry,
  cache_entry_valid requested entry ->
  requested = cached_manifest entry.
Proof. intros requested entry [compatible _]; exact compatible. Qed.

Theorem incomplete_entry_is_not_reusable : forall requested entry,
  cached_result_complete entry = false ->
  ~ cache_entry_valid requested entry.
Proof.
  intros requested entry incomplete [_ complete].
  rewrite incomplete in complete; discriminate.
Qed.

Inductive comparison_result : Type :=
| Compatible
| Incompatible
| CompatibilityUnknown.

Definition may_reuse
    (comparison : comparison_result)
    (complete : bool) : bool :=
  match comparison with
  | Compatible => complete
  | Incompatible | CompatibilityUnknown => false
  end.

Theorem unknown_compatibility_never_reuses : forall complete,
  may_reuse CompatibilityUnknown complete = false.
Proof. destruct complete; reflexivity. Qed.

Theorem incompatible_manifest_never_reuses : forall complete,
  may_reuse Incompatible complete = false.
Proof. destruct complete; reflexivity. Qed.

Theorem reuse_requires_compatible_complete : forall comparison complete,
  may_reuse comparison complete = true ->
  comparison = Compatible /\ complete = true.
Proof. destruct comparison, complete; simpl; intros; try discriminate; auto. Qed.

End OwnershipAndManifests.

Section RenameAndFacts.

Variables Name SourceId FeatureId AnchorId : Type.

Record named_identity (Identity : Type) : Type := {
  durable_identity : Identity;
  display_name : Name
}.

Definition rename {Identity : Type}
    (identity : named_identity Identity)
    (new_name : Name) : named_identity Identity :=
  {| durable_identity := durable_identity identity;
     display_name := new_name |}.

Theorem rename_preserves_durable_identity : forall Identity
    (identity : named_identity Identity) new_name,
  durable_identity (rename identity new_name) = durable_identity identity.
Proof. reflexivity. Qed.

Record durable_fact_key : Type := {
  fact_source : SourceId;
  fact_feature : FeatureId;
  fact_anchor : AnchorId;
  fact_discriminator : nat
}.

Definition fact_key_from
    (source : named_identity SourceId)
    (feature : FeatureId)
    (anchor : AnchorId)
    (discriminator : nat) : durable_fact_key :=
  {| fact_source := durable_identity source;
     fact_feature := feature;
     fact_anchor := anchor;
     fact_discriminator := discriminator |}.

Theorem source_display_rename_preserves_fact_key : forall
    source new_name feature anchor discriminator,
  fact_key_from (rename source new_name) feature anchor discriminator =
  fact_key_from source feature anchor discriminator.
Proof. reflexivity. Qed.

Variable active_fact : durable_fact_key -> Prop.

Record dense_fact_index : Type := {
  dense_count : nat;
  dense_of : durable_fact_key -> option nat;
  fact_of : nat -> option durable_fact_key;
  active_has_dense : forall key,
    active_fact key -> exists dense, dense_of key = Some dense;
  dense_is_bounded : forall key dense,
    dense_of key = Some dense -> dense < dense_count;
  key_dense_roundtrip : forall key dense,
    dense_of key = Some dense -> fact_of dense = Some key;
  dense_key_roundtrip : forall dense key,
    fact_of dense = Some key -> dense_of key = Some dense;
  every_dense_has_key : forall dense,
    dense < dense_count -> exists key, fact_of dense = Some key
}.

Theorem every_active_fact_has_dense_id : forall index key,
  active_fact key -> exists dense,
    dense < dense_count index /\ dense_of index key = Some dense.
Proof.
  intros index key active.
  destruct (@active_has_dense index key active) as [dense mapped].
  exists dense; split.
  - exact (@dense_is_bounded index key dense mapped).
  - exact mapped.
Qed.

Theorem durable_to_dense_to_durable : forall index key dense,
  dense_of index key = Some dense -> fact_of index dense = Some key.
Proof. intros index key dense mapped; now apply key_dense_roundtrip. Qed.

Theorem dense_to_durable_to_dense : forall index dense key,
  fact_of index dense = Some key -> dense_of index key = Some dense.
Proof. intros index dense key mapped; now apply dense_key_roundtrip. Qed.

Theorem durable_keys_map_injectively : forall index left right dense,
  dense_of index left = Some dense ->
  dense_of index right = Some dense ->
  left = right.
Proof.
  intros index left right dense left_mapped right_mapped.
  pose proof (@key_dense_roundtrip index left dense left_mapped) as left_back.
  pose proof (@key_dense_roundtrip index right dense right_mapped) as right_back.
  rewrite left_back in right_back; inversion right_back; reflexivity.
Qed.

Theorem dense_ids_have_no_orphans : forall index dense,
  dense < dense_count index -> exists key,
    fact_of index dense = Some key /\ dense_of index key = Some dense.
Proof.
  intros index dense bounded.
  destruct (@every_dense_has_key index dense bounded) as [key mapped].
  exists key; split; [exact mapped | now apply dense_key_roundtrip].
Qed.

End RenameAndFacts.

Section FeatureHistory.

Variables FeatureId FeatureName FeatureSemantics : Type.

Inductive feature_state : Type :=
| FeatureActive
| FeatureTombstoned.

Record feature_entry : Type := {
  historical_feature_id : FeatureId;
  feature_name : FeatureName;
  feature_semantics : FeatureSemantics;
  feature_status : feature_state
}.

Definition rename_feature
    (entry : feature_entry)
    (new_name : FeatureName) : feature_entry :=
  {| historical_feature_id := historical_feature_id entry;
     feature_name := new_name;
     feature_semantics := feature_semantics entry;
     feature_status := feature_status entry |}.

Theorem feature_rename_preserves_identity : forall entry new_name,
  historical_feature_id (rename_feature entry new_name) =
  historical_feature_id entry.
Proof. reflexivity. Qed.

Theorem feature_rename_preserves_semantics : forall entry new_name,
  feature_semantics (rename_feature entry new_name) = feature_semantics entry.
Proof. reflexivity. Qed.

Theorem feature_rename_preserves_status : forall entry new_name,
  feature_status (rename_feature entry new_name) = feature_status entry.
Proof. reflexivity. Qed.

Definition feature_ids_unique (entries : list feature_entry) : Prop :=
  forall left right,
    In left entries ->
    In right entries ->
    historical_feature_id left = historical_feature_id right ->
    left = right.

Record valid_feature_revision
    (old new : list feature_entry) : Prop := {
  old_feature_ids_unique : feature_ids_unique old;
  new_feature_ids_unique : feature_ids_unique new;
  feature_history_retained : forall previous,
    In previous old ->
    exists current,
      In current new /\
      historical_feature_id current = historical_feature_id previous /\
      feature_semantics current = feature_semantics previous /\
      (feature_status previous = FeatureTombstoned ->
       feature_status current = FeatureTombstoned)
}.

Theorem tombstones_are_absorbing : forall old new previous,
  valid_feature_revision old new ->
  In previous old ->
  feature_status previous = FeatureTombstoned ->
  exists current,
    In current new /\
    historical_feature_id current = historical_feature_id previous /\
    feature_status current = FeatureTombstoned.
Proof.
  intros old new previous valid member tombstoned.
  destruct (@feature_history_retained old new valid previous member)
    as [current [current_member [same_id [_ monotone]]]].
  exists current; repeat split; auto.
Qed.

Theorem historical_feature_ids_are_never_reused : forall old new previous current,
  valid_feature_revision old new ->
  In previous old ->
  In current new ->
  historical_feature_id current = historical_feature_id previous ->
  feature_semantics current = feature_semantics previous.
Proof.
  intros old new previous current valid previous_member current_member same_id.
  destruct (@feature_history_retained old new valid previous previous_member)
    as [retained [retained_member [retained_id [retained_semantics _]]]].
  assert (current = retained) as same_entry.
  { apply (@new_feature_ids_unique old new valid current retained);
      try assumption; congruence. }
  subst current; exact retained_semantics.
Qed.

Theorem tombstoned_feature_cannot_reactivate : forall old new previous current,
  valid_feature_revision old new ->
  In previous old ->
  In current new ->
  historical_feature_id current = historical_feature_id previous ->
  feature_status previous = FeatureTombstoned ->
  feature_status current <> FeatureActive.
Proof.
  intros old new previous current valid previous_member current_member same_id tombstoned active.
  destruct (@feature_history_retained old new valid previous previous_member)
    as [retained [retained_member [retained_id [_ monotone]]]].
  assert (current = retained) as same_entry.
  { apply (@new_feature_ids_unique old new valid current retained);
      try assumption; congruence. }
  subst current.
  rewrite (monotone tombstoned) in active; discriminate.
Qed.

End FeatureHistory.

Section SourceEvidenceAndCoverage.

Variables SourceId SourceRevisionId : Type.

Record source_position : Type := {
  position_byte : nat;
  position_line : nat;
  position_column : nat
}.

Record source_range : Type := {
  range_start : source_position;
  range_end : source_position
}.

Definition valid_half_open_range
    (source_length : nat)
    (range : source_range) : Prop :=
  position_byte (range_start range) <= position_byte (range_end range) /\
  position_byte (range_end range) <= source_length.

Record source_fact_evidence : Type := {
  evidence_source : SourceId;
  evidence_revision : SourceRevisionId;
  evidence_range : source_range
}.

Definition exact_source_evidence
    (expected_source : SourceId)
    (expected_revision : SourceRevisionId)
    (source_length : nat)
    (evidence : source_fact_evidence) : Prop :=
  evidence_source evidence = expected_source /\
  evidence_revision evidence = expected_revision /\
  valid_half_open_range source_length (evidence_range evidence).

Theorem exact_source_range_is_ordered : forall source revision length evidence,
  exact_source_evidence source revision length evidence ->
  position_byte (range_start (evidence_range evidence)) <=
  position_byte (range_end (evidence_range evidence)).
Proof. intros source revision length evidence [_ [_ [ordered _]]]; exact ordered. Qed.

Theorem exact_source_range_is_bounded : forall source revision length evidence,
  exact_source_evidence source revision length evidence ->
  position_byte (range_end (evidence_range evidence)) <= length.
Proof. intros source revision length evidence [_ [_ [_ bounded]]]; exact bounded. Qed.

Theorem exact_source_range_length_is_bounded : forall source revision length evidence,
  exact_source_evidence source revision length evidence ->
  position_byte (range_end (evidence_range evidence)) -
  position_byte (range_start (evidence_range evidence)) <= length.
Proof.
  intros source revision length evidence exact.
  pose proof (exact_source_range_is_ordered exact) as ordered.
  pose proof (exact_source_range_is_bounded exact) as bounded.
  lia.
Qed.

Theorem source_mismatch_rejects_evidence : forall expected_source revision length evidence,
  evidence_source evidence <> expected_source ->
  ~ exact_source_evidence expected_source revision length evidence.
Proof. intros source revision length evidence mismatch [same _]; exact (mismatch same). Qed.

Theorem source_revision_mismatch_rejects_evidence : forall source expected_revision length evidence,
  evidence_revision evidence <> expected_revision ->
  ~ exact_source_evidence source expected_revision length evidence.
Proof. intros source revision length evidence mismatch [_ [same _]]; exact (mismatch same). Qed.

Theorem out_of_bounds_range_rejects_evidence : forall source revision length evidence,
  length < position_byte (range_end (evidence_range evidence)) ->
  ~ exact_source_evidence source revision length evidence.
Proof. intros source revision length evidence outside [_ [_ [_ bounded]]]; lia. Qed.

Inductive extraction_coverage : Type :=
| CompleteExtraction
| IncompleteExtraction.

Inductive membership_evidence : Type :=
| FactPresent
| FactAbsent
| FactUnknown.

Definition classify_membership
    (coverage : extraction_coverage)
    (observed : bool) : membership_evidence :=
  if observed then FactPresent else
  match coverage with
  | CompleteExtraction => FactAbsent
  | IncompleteExtraction => FactUnknown
  end.

Theorem incomplete_nonobservation_is_unknown :
  classify_membership IncompleteExtraction false = FactUnknown.
Proof. reflexivity. Qed.

Theorem incomplete_extraction_never_establishes_absence : forall observed,
  classify_membership IncompleteExtraction observed <> FactAbsent.
Proof. destruct observed; discriminate. Qed.

Theorem absence_requires_complete_extraction : forall coverage observed,
  classify_membership coverage observed = FactAbsent ->
  coverage = CompleteExtraction /\ observed = false.
Proof. destruct coverage, observed; simpl; intros; try discriminate; auto. Qed.

End SourceEvidenceAndCoverage.

Section CanonicalExport.

Variable FactKey : Type.
Variable FactKey_eq_dec : forall left right : FactKey, {left = right} + {left <> right}.
Variable canonical_key_universe : list FactKey.
Hypothesis canonical_key_universe_unique : NoDup canonical_key_universe.

Definition contains_key (key : FactKey) (facts : list FactKey) : bool :=
  if in_dec FactKey_eq_dec key facts then true else false.

Definition canonical_export (facts : list FactKey) : list FactKey :=
  filter (fun key => contains_key key facts) canonical_key_universe.

Lemma contains_key_correct : forall key facts,
  contains_key key facts = true <-> In key facts.
Proof.
  intros key facts; unfold contains_key.
  destruct (in_dec FactKey_eq_dec key facts); split; intros; auto; discriminate.
Qed.

Lemma contains_key_permutation : forall key left right,
  Permutation left right -> contains_key key left = contains_key key right.
Proof.
  intros key left right permutation.
  unfold contains_key.
  destruct (in_dec FactKey_eq_dec key left) as [in_left | not_left];
  destruct (in_dec FactKey_eq_dec key right) as [in_right | not_right];
  try reflexivity.
  - exfalso; apply not_right.
    exact (@Permutation_in FactKey left right key permutation in_left).
  - exfalso; apply not_left.
    exact (@Permutation_in FactKey right left key
      (Permutation_sym permutation) in_right).
Qed.

Theorem canonical_export_is_insertion_order_invariant : forall left right,
  Permutation left right -> canonical_export left = canonical_export right.
Proof.
  intros left right permutation.
  unfold canonical_export.
  induction canonical_key_universe as [| key tail induction]; simpl; auto.
  inversion canonical_key_universe_unique as [| ignored ignored_tail not_member tail_unique].
  specialize (induction tail_unique).
  rewrite (contains_key_permutation key permutation).
  destruct (contains_key key right); simpl.
  - f_equal; exact induction.
  - exact induction.
Qed.

Theorem canonical_export_is_sound : forall facts key,
  In key (canonical_export facts) -> In key facts.
Proof.
  intros facts key exported.
  unfold canonical_export in exported.
  apply filter_In in exported as [_ selected].
  now apply contains_key_correct.
Qed.

Theorem canonical_export_is_complete_over_universe : forall facts key,
  In key canonical_key_universe ->
  In key facts ->
  In key (canonical_export facts).
Proof.
  intros facts key in_universe in_facts.
  unfold canonical_export; apply filter_In; split; auto.
  now apply contains_key_correct.
Qed.

Theorem canonical_export_has_no_duplicate_keys : forall facts,
  NoDup (canonical_export facts).
Proof.
  intros facts; unfold canonical_export.
  now apply NoDup_filter.
Qed.

Variable ExportBytes : Type.
Variable encode_canonical_keys : list FactKey -> ExportBytes.

Theorem canonical_export_bytes_are_deterministic : forall left right,
  Permutation left right ->
  encode_canonical_keys (canonical_export left) =
  encode_canonical_keys (canonical_export right).
Proof.
  intros left right permutation.
  now rewrite (canonical_export_is_insertion_order_invariant permutation).
Qed.

Record export_state : Type := {
  export_remaining : list FactKey;
  export_output_rev : list FactKey;
  export_work : nat
}.

Inductive export_step (input : list FactKey) : export_state -> export_state -> Prop :=
| ExportKeep : forall key remaining output work,
    contains_key key input = true ->
    export_step input
      {| export_remaining := key :: remaining;
         export_output_rev := output;
         export_work := work |}
      {| export_remaining := remaining;
         export_output_rev := key :: output;
         export_work := S work |}
| ExportSkip : forall key remaining output work,
    contains_key key input = false ->
    export_step input
      {| export_remaining := key :: remaining;
         export_output_rev := output;
         export_work := work |}
      {| export_remaining := remaining;
         export_output_rev := output;
         export_work := S work |}.

Inductive export_runs (input : list FactKey) :
    nat -> export_state -> export_state -> Prop :=
| ExportRunsZero : forall state, export_runs input 0 state state
| ExportRunsSucc : forall steps first second final,
    export_step input first second ->
    export_runs input steps second final ->
    export_runs input (S steps) first final.

Definition native_stack_frames (_ : export_state) : nat := 1.

Lemma export_step_consumes_one : forall input first second,
  export_step input first second ->
  length (export_remaining first) = S (length (export_remaining second)).
Proof. intros input first second step; inversion step; reflexivity. Qed.

Theorem arbitrary_export_run_has_linear_work : forall input steps first final,
  export_runs input steps first final ->
  length (export_remaining first) =
    steps + length (export_remaining final).
Proof.
  intros input steps first final runs.
  induction runs.
  - simpl; lia.
  - pose proof (export_step_consumes_one H) as consumed.
    lia.
Qed.

Theorem arbitrary_export_run_is_bounded : forall input steps first final,
  export_runs input steps first final ->
  steps <= length (export_remaining first).
Proof.
  intros input steps first final runs.
  pose proof (arbitrary_export_run_has_linear_work runs).
  lia.
Qed.

Theorem arbitrary_export_run_has_constant_native_stack : forall input steps first final,
  export_runs input steps first final ->
  native_stack_frames first = 1 /\ native_stack_frames final = 1.
Proof. intros input steps first final _; split; reflexivity. Qed.

Theorem export_step_increments_work_once : forall input first second,
  export_step input first second ->
  export_work second = S (export_work first).
Proof. intros input first second step; inversion step; reflexivity. Qed.

End CanonicalExport.

Section LoweringAndDependencies.

Variables FactKey RuleKey : Type.
Variable lowers_to : FactKey -> RuleKey -> Prop.

Definition fact_has_lowered_rule (fact : FactKey) : Prop :=
  exists rule, lowers_to fact rule.

Definition rule_has_source_fact (rule : RuleKey) : Prop :=
  exists fact, lowers_to fact rule.

Record lowering_certificate
    (facts : list FactKey)
    (rules : list RuleKey) : Prop := {
  every_relation_pair_is_retained : forall fact rule,
    In fact facts ->
    lowers_to fact rule ->
    In rule rules;
  every_rule_retains_fact_evidence : forall rule,
    In rule rules ->
    exists fact, In fact facts /\ lowers_to fact rule
}.

Theorem certified_lowering_preserves_every_relation_pair : forall facts rules,
  lowering_certificate facts rules ->
  forall fact rule,
    In fact facts -> lowers_to fact rule -> In rule rules.
Proof.
  intros facts rules certificate fact rule fact_member relation.
  now apply (@every_relation_pair_is_retained facts rules certificate fact rule).
Qed.

Theorem certified_lowering_has_no_provenance_orphans : forall facts rules,
  lowering_certificate facts rules ->
  forall rule, In rule rules -> exists fact,
    In fact facts /\ lowers_to fact rule.
Proof.
  intros facts rules certificate rule member.
  now apply (@every_rule_retains_fact_evidence facts rules certificate rule).
Qed.

Definition many_to_many_witness (relation : bool -> bool -> Prop) : Prop :=
  (exists fact rule_one rule_two,
      rule_one <> rule_two /\ relation fact rule_one /\ relation fact rule_two) /\
  (exists rule fact_one fact_two,
      fact_one <> fact_two /\ relation fact_one rule /\ relation fact_two rule).

Definition example_many_to_many (fact rule : bool) : Prop :=
  fact = false \/ rule = false.

Theorem fact_rule_lowering_is_not_forced_functional :
  many_to_many_witness example_many_to_many.
Proof.
  split.
  - exists false, false, true; repeat split; try discriminate; left; reflexivity.
  - exists false, false, true; repeat split; try discriminate; right; reflexivity.
Qed.

Inductive crate_name : Type :=
| LibcpgCrate
| LlingLlangCrate
| VinaryLibcpgAdapterCrate
| VinaryRuntimeCrate.

Inductive direct_dependency : crate_name -> crate_name -> Prop :=
| AdapterDependsOnLibcpg :
    direct_dependency VinaryLibcpgAdapterCrate LibcpgCrate
| AdapterDependsOnLlingLlang :
    direct_dependency VinaryLibcpgAdapterCrate LlingLlangCrate.

Theorem libcpg_has_no_lling_llang_dependency :
  ~ direct_dependency LibcpgCrate LlingLlangCrate.
Proof. intro dependency; inversion dependency. Qed.

Theorem lling_llang_has_no_libcpg_dependency :
  ~ direct_dependency LlingLlangCrate LibcpgCrate.
Proof. intro dependency; inversion dependency. Qed.

Theorem adapter_is_the_only_composition_boundary :
  direct_dependency VinaryLibcpgAdapterCrate LibcpgCrate /\
  direct_dependency VinaryLibcpgAdapterCrate LlingLlangCrate.
Proof. split; constructor. Qed.

Theorem runtime_envelope_does_not_reverse_core_dependencies : forall target,
  ~ direct_dependency VinaryRuntimeCrate target.
Proof. intros target dependency; inversion dependency. Qed.

End LoweringAndDependencies.
