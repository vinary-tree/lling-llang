(** * DictionarySurface — split dictionary and fuzzy adapter contracts

    This theory refines [FuzzyReference] with the concrete identity and
    ownership boundaries required by the Vinary dictionary campaign.  The
    proofs deliberately precede both standalone adapter implementations.

    The [libdictenstein-llattice] adapter owns only lawful dictionary-value
    merge strategies.  The [vinary-dictionary-pipeline] adapter owns the
    indexed dictionary/edit-distance pipeline and depends outward on the four
    domain crates.  Neither adapter may become a dependency of its inputs.
*)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Lia.

Import ListNotations.

Require Import LlingLlang.domain_integration.FuzzyReference.

(** ** Exact query identity *)

Section QueryIdentity.

Variables Snapshot Query Normalization EditProfile : Type.

(** Every field can change the exact denotation and is therefore part of the
    cache, candidate, confirmation, certificate, and publication identity. *)
Record dictionary_query_identity : Type := {
  identity_snapshot : Snapshot;
  identity_query : Query;
  identity_normalization : Normalization;
  identity_edit_profile : EditProfile;
  identity_bound : nat
}.

Definition identity_configuration
    (identity : dictionary_query_identity) : Normalization * EditProfile :=
  (identity_normalization identity, identity_edit_profile identity).

Definition identity_as_fuzzy_index
    (identity : dictionary_query_identity) :
    fuzzy_index Snapshot Query (Normalization * EditProfile) :=
  {| index_snapshot := identity_snapshot identity;
     index_query := identity_query identity;
     index_configuration := identity_configuration identity;
     index_budget := identity_bound identity |}.

Definition same_dictionary_identity
    (left right : dictionary_query_identity) : Prop :=
  identity_snapshot left = identity_snapshot right /\
  identity_query left = identity_query right /\
  identity_normalization left = identity_normalization right /\
  identity_edit_profile left = identity_edit_profile right /\
  identity_bound left = identity_bound right.

Lemma dictionary_identity_is_reflexive :
  forall identity, same_dictionary_identity identity identity.
Proof.
  intros identity; repeat split; reflexivity.
Qed.

Theorem dictionary_identity_exactly_refines_fuzzy_index :
  forall left right,
    same_dictionary_identity left right <->
    same_index Snapshot Query (Normalization * EditProfile)
      (identity_as_fuzzy_index left)
      (identity_as_fuzzy_index right).
Proof.
  intros [left_snapshot left_query left_normalization left_edit left_bound]
         [right_snapshot right_query right_normalization right_edit right_bound].
  unfold same_dictionary_identity, same_index, identity_as_fuzzy_index,
    identity_configuration; simpl.
  split.
  - intros [snapshot_equal
      [query_equal [normalization_equal [edit_equal bound_equal]]]].
    repeat split; congruence.
  - intros [snapshot_equal
      [query_equal [configuration_equal bound_equal]]].
    inversion configuration_equal.
    repeat split; assumption.
Qed.

End QueryIdentity.

(** Constructive controls show that each semantic field is independently
    load-bearing. *)
Definition identity_baseline :
    dictionary_query_identity bool unit bool bool :=
  {| identity_snapshot := false;
     identity_query := tt;
     identity_normalization := false;
     identity_edit_profile := false;
     identity_bound := 1 |}.

Definition identity_stale_snapshot :
    dictionary_query_identity bool unit bool bool :=
  {| identity_snapshot := true;
     identity_query := tt;
     identity_normalization := false;
     identity_edit_profile := false;
     identity_bound := 1 |}.

Definition identity_changed_normalization :
    dictionary_query_identity bool unit bool bool :=
  {| identity_snapshot := false;
     identity_query := tt;
     identity_normalization := true;
     identity_edit_profile := false;
     identity_bound := 1 |}.

Definition identity_changed_edit_profile :
    dictionary_query_identity bool unit bool bool :=
  {| identity_snapshot := false;
     identity_query := tt;
     identity_normalization := false;
     identity_edit_profile := true;
     identity_bound := 1 |}.

Definition identity_changed_bound :
    dictionary_query_identity bool unit bool bool :=
  {| identity_snapshot := false;
     identity_query := tt;
     identity_normalization := false;
     identity_edit_profile := false;
     identity_bound := 2 |}.

Theorem stale_snapshot_identity_is_rejected :
  ~ same_dictionary_identity bool unit bool bool
      identity_baseline identity_stale_snapshot.
Proof. unfold same_dictionary_identity; simpl; intuition discriminate. Qed.

Theorem changed_normalization_identity_is_rejected :
  ~ same_dictionary_identity bool unit bool bool
      identity_baseline identity_changed_normalization.
Proof. unfold same_dictionary_identity; simpl; intuition discriminate. Qed.

Theorem changed_edit_profile_identity_is_rejected :
  ~ same_dictionary_identity bool unit bool bool
      identity_baseline identity_changed_edit_profile.
Proof. unfold same_dictionary_identity; simpl; intuition discriminate. Qed.

Theorem changed_bound_identity_is_rejected :
  ~ same_dictionary_identity bool unit bool bool
      identity_baseline identity_changed_bound.
Proof. unfold same_dictionary_identity; simpl; intuition discriminate. Qed.

(** ** External and dense identifier correspondence *)

Section IdentifierCorrespondence.

Variable ExternalKey : Type.

(** Dense identifiers are snapshot-local hot-path indices.  External keys are
    caller-owned durable identities.  The two partial maps must agree in both
    directions wherever either publishes a correspondence. *)
Record dense_external_map : Type := {
  external_for : nat -> option ExternalKey;
  dense_for : ExternalKey -> option nat;
  dense_round_trip : forall external dense,
    dense_for external = Some dense -> external_for dense = Some external;
  external_round_trip : forall dense external,
    external_for dense = Some external -> dense_for external = Some dense
}.

Definition identifiers_correspond
    (mapping : dense_external_map)
    (external : ExternalKey)
    (dense : nat) : Prop :=
  dense_for mapping external = Some dense /\
  external_for mapping dense = Some external.

Theorem external_key_has_at_most_one_dense_identifier :
  forall (mapping : dense_external_map) external left right,
    dense_for mapping external = Some left ->
    dense_for mapping external = Some right ->
    left = right.
Proof. intros mapping external left right left_map right_map; congruence. Qed.

Theorem dense_identifier_has_at_most_one_external_key :
  forall (mapping : dense_external_map) dense left right,
    external_for mapping dense = Some left ->
    external_for mapping dense = Some right ->
    left = right.
Proof. intros mapping dense left right left_map right_map; congruence. Qed.

Theorem either_direction_establishes_exact_correspondence :
  forall (mapping : dense_external_map) external dense,
    dense_for mapping external = Some dense \/
    external_for mapping dense = Some external ->
    identifiers_correspond mapping external dense.
Proof.
  intros mapping external dense [forward | reverse].
  - split; [exact forward | exact (dense_round_trip mapping external dense forward)].
  - split; [exact (external_round_trip mapping dense external reverse) | exact reverse].
Qed.

End IdentifierCorrespondence.

(** ** Lawful algebra adapters *)

Section AlgebraContracts.

Variable Carrier : Type.

Record join_semilattice_laws (join : Carrier -> Carrier -> Carrier) : Prop := {
  join_idempotent : forall value, join value value = value;
  join_commutative : forall left right, join left right = join right left;
  join_associative : forall first second third,
    join (join first second) third = join first (join second third)
}.

Record meet_semilattice_laws (meet : Carrier -> Carrier -> Carrier) : Prop := {
  meet_idempotent : forall value, meet value value = value;
  meet_commutative : forall left right, meet left right = meet right left;
  meet_associative : forall first second third,
    meet (meet first second) third = meet first (meet second third)
}.

Record lattice_laws
    (join meet : Carrier -> Carrier -> Carrier) : Prop := {
  lattice_join_laws : join_semilattice_laws join;
  lattice_meet_laws : meet_semilattice_laws meet;
  join_absorbs_meet : forall left right, join left (meet left right) = left;
  meet_absorbs_join : forall left right, meet left (join left right) = left
}.

Variable join : Carrier -> Carrier -> Carrier.

(** The dictionary merge adapter is merely the supplied lawful join.  No meet
    operation can be manufactured from this record. *)
Record lawful_join_merge : Type := {
  merge_value : Carrier -> Carrier -> Carrier;
  merge_is_join : forall left right, merge_value left right = join left right;
  supplied_join_laws : join_semilattice_laws join
}.

Theorem lawful_join_merge_is_idempotent :
  forall (adapter : lawful_join_merge) value,
    merge_value adapter value value = value.
Proof.
  intros adapter value.
  rewrite merge_is_join.
  exact (join_idempotent _ (supplied_join_laws adapter) value).
Qed.

Theorem lawful_join_merge_is_commutative :
  forall (adapter : lawful_join_merge) left right,
    merge_value adapter left right = merge_value adapter right left.
Proof.
  intros adapter left right.
  repeat rewrite merge_is_join.
  exact (join_commutative _ (supplied_join_laws adapter) left right).
Qed.

Theorem lawful_join_merge_is_associative :
  forall (adapter : lawful_join_merge) first second third,
    merge_value adapter (merge_value adapter first second) third =
    merge_value adapter first (merge_value adapter second third).
Proof.
  intros adapter first second third.
  repeat rewrite merge_is_join.
  exact (join_associative _ (supplied_join_laws adapter) first second third).
Qed.

End AlgebraContracts.

(** Tropical semiring multiplication is arithmetic addition.  It is not even
    idempotent, so it cannot be installed as a meet semilattice operation. *)
Definition tropical_join_nat (left right : nat) : nat := Nat.min left right.
Definition tropical_times_nat (left right : nat) : nat := left + right.

Theorem tropical_times_is_not_idempotent :
  tropical_times_nat 1 1 <> 1.
Proof. unfold tropical_times_nat; discriminate. Qed.

Theorem tropical_times_cannot_be_meet :
  ~ meet_semilattice_laws nat tropical_times_nat.
Proof.
  intro purported_meet.
  pose proof (meet_idempotent nat tropical_times_nat purported_meet 1) as impossible.
  exact (tropical_times_is_not_idempotent impossible).
Qed.

Theorem tropical_times_breaks_meet_absorption :
  tropical_times_nat 10 (tropical_join_nat 10 5) <> 10.
Proof. unfold tropical_times_nat, tropical_join_nat; simpl; discriminate. Qed.

(** Raw left-biased sequence union preserves encounter order.  On disjoint
    inputs its behavior is list append, which is not commutative as a value.
    Content-set equivalence cannot justify a structural semilattice instance. *)
Definition left_biased_disjoint_merge {A : Type}
    (left right : list A) : list A := left ++ right.

Theorem left_biased_container_merge_is_not_commutative :
  left_biased_disjoint_merge [true] [false] <>
  left_biased_disjoint_merge [false] [true].
Proof. unfold left_biased_disjoint_merge; simpl; discriminate. Qed.

(** IEEE exceptional values require an explicit lawful wrapper or rejection.
    The formal admission predicate makes that boundary total. *)
Inductive numeric_class : Type :=
| FiniteNumber
| PositiveInfinity
| NegativeInfinity
| NotANumber.

Definition admitted_numeric_class (value : numeric_class) : Prop :=
  match value with
  | FiniteNumber => True
  | PositiveInfinity | NegativeInfinity | NotANumber => False
  end.

Theorem every_non_finite_numeric_class_is_rejected :
  forall value,
    value <> FiniteNumber -> ~ admitted_numeric_class value.
Proof. destruct value; simpl; intuition. Qed.

Theorem raw_numeric_domain_cannot_be_universally_admitted :
  ~ forall value : numeric_class, admitted_numeric_class value.
Proof. intro admits_all; exact (admits_all NotANumber). Qed.

(** ** Dependency direction and acyclicity *)

Inductive component : Type :=
| Libdictenstein
| Llattice
| Liblevenshtein
| LlingLlang
| LibdictensteinLlattice
| VinaryDictionaryPipeline
| Duallity.

(** [depends_on owner dependency] follows the intended future Cargo graph. *)
Inductive depends_on : component -> component -> Prop :=
| liblevenshtein_uses_libdictenstein :
    depends_on Liblevenshtein Libdictenstein
| dictionary_lattice_uses_libdictenstein :
    depends_on LibdictensteinLlattice Libdictenstein
| dictionary_lattice_uses_llattice :
    depends_on LibdictensteinLlattice Llattice
| dictionary_pipeline_uses_libdictenstein :
    depends_on VinaryDictionaryPipeline Libdictenstein
| dictionary_pipeline_uses_liblevenshtein :
    depends_on VinaryDictionaryPipeline Liblevenshtein
| dictionary_pipeline_uses_llattice :
    depends_on VinaryDictionaryPipeline Llattice
| dictionary_pipeline_uses_lling_llang :
    depends_on VinaryDictionaryPipeline LlingLlang
| duallity_uses_dictionary_lattice :
    depends_on Duallity LibdictensteinLlattice
| duallity_uses_dictionary_pipeline :
    depends_on Duallity VinaryDictionaryPipeline.

Definition component_rank (value : component) : nat :=
  match value with
  | Libdictenstein | Llattice | LlingLlang => 0
  | Liblevenshtein | LibdictensteinLlattice => 1
  | VinaryDictionaryPipeline => 2
  | Duallity => 3
  end.

Theorem every_dependency_points_inward :
  forall owner dependency,
    depends_on owner dependency ->
    component_rank dependency < component_rank owner.
Proof. intros owner dependency edge; destruct edge; simpl; lia. Qed.

Inductive dependency_path : component -> component -> Prop :=
| dependency_path_direct : forall owner dependency,
    depends_on owner dependency -> dependency_path owner dependency
| dependency_path_step : forall owner middle dependency,
    depends_on owner middle ->
    dependency_path middle dependency ->
    dependency_path owner dependency.

Theorem every_dependency_path_strictly_decreases_rank :
  forall owner dependency,
    dependency_path owner dependency ->
    component_rank dependency < component_rank owner.
Proof.
  intros owner dependency path.
  induction path.
  - exact (every_dependency_points_inward owner dependency H).
  - pose proof (every_dependency_points_inward owner middle H) as first_edge.
    lia.
Qed.

Theorem future_adapter_dependency_graph_is_acyclic :
  forall component_value, ~ dependency_path component_value component_value.
Proof.
  intros component_value cycle.
  pose proof (every_dependency_path_strictly_decreases_rank
    component_value component_value cycle).
  lia.
Qed.

Theorem dictionary_lattice_adapter_stays_independent :
  ~ depends_on LibdictensteinLlattice Liblevenshtein /\
  ~ depends_on LibdictensteinLlattice LlingLlang.
Proof. split; intro edge; inversion edge. Qed.

Theorem domain_crates_do_not_depend_on_dictionary_pipeline :
  ~ depends_on Libdictenstein VinaryDictionaryPipeline /\
  ~ depends_on Liblevenshtein VinaryDictionaryPipeline /\
  ~ depends_on Llattice VinaryDictionaryPipeline /\
  ~ depends_on LlingLlang VinaryDictionaryPipeline.
Proof. repeat split; intro edge; inversion edge. Qed.

(** ** Facade equivalence *)

Section Facade.

Variables Index Output : Type.
Variable native_adapter facade_adapter : Index -> Output.

Definition facade_delegates_exactly : Prop :=
  forall index, facade_adapter index = native_adapter index.

Theorem exact_delegation_establishes_facade_equivalence :
  facade_delegates_exactly ->
  forall index, facade_adapter index = native_adapter index.
Proof. auto. Qed.

End Facade.

Definition boolean_native_adapter (value : bool) : bool := value.
Definition broken_boolean_facade (value : bool) : bool := negb value.

Theorem facade_that_transforms_results_is_not_equivalent :
  ~ facade_delegates_exactly bool bool
      boolean_native_adapter broken_boolean_facade.
Proof.
  intro delegates.
  specialize (delegates true).
  unfold boolean_native_adapter, broken_boolean_facade in delegates.
  simpl in delegates; discriminate.
Qed.

(** ** Fibers are not fibration evidence *)

Record indexed_family (Index Object : Type) : Type := {
  object_in_fiber : Index -> Object -> Prop
}.

Record fibration_evidence
    (Index Object BaseArrow : Type)
    (family : indexed_family Index Object) : Type := {
  cartesian_lift : BaseArrow -> Object -> option Object;
  cartesian_lift_is_total_on_domain :
    forall arrow object,
      exists lifted, cartesian_lift arrow object = Some lifted;
  cartesian_identity_law : Prop;
  cartesian_composition_law : Prop;
  cartesian_universal_law : Prop
}.

Definition may_claim_fibration
    {Index Object BaseArrow : Type}
    {family : indexed_family Index Object}
    (evidence : option (fibration_evidence Index Object BaseArrow family)) : bool :=
  match evidence with
  | Some _ => true
  | None => false
  end.

Theorem indexed_family_without_lifts_cannot_claim_fibration :
  forall Index Object BaseArrow
         (family : indexed_family Index Object),
    may_claim_fibration
      (Index := Index) (Object := Object) (BaseArrow := BaseArrow)
      (family := family) None = false.
Proof. reflexivity. Qed.

(** ** Termination, caps, cancellation, and stack-safe control *)

Inductive termination_reason : Type :=
| ExhaustedFeed
| ReachedCap
| Cancelled
| ProviderFailed.

Definition termination_is_complete (reason : termination_reason) : bool :=
  match reason with
  | ExhaustedFeed => true
  | ReachedCap | Cancelled | ProviderFailed => false
  end.

Theorem capped_feed_is_incomplete :
  termination_is_complete ReachedCap = false.
Proof. reflexivity. Qed.

Theorem cancelled_feed_is_incomplete :
  termination_is_complete Cancelled = false.
Proof. reflexivity. Qed.

Theorem failed_feed_is_incomplete :
  termination_is_complete ProviderFailed = false.
Proof. reflexivity. Qed.

Section ExplicitWorklist.

Variable Candidate Accepted : Type.
Variable confirm_one : Candidate -> option Accepted.

Record confirmation_machine : Type := {
  confirmation_pending : list Candidate;
  confirmation_accepted : list Accepted
}.

Definition confirmation_step
    (state : confirmation_machine) : option confirmation_machine :=
  match confirmation_pending state with
  | [] => None
  | candidate :: pending =>
      Some
        {| confirmation_pending := pending;
           confirmation_accepted :=
             match confirm_one candidate with
             | Some accepted => accepted :: confirmation_accepted state
             | None => confirmation_accepted state
             end |}
  end.

Theorem confirmation_step_strictly_decreases_pending_work :
  forall before after,
    confirmation_step before = Some after ->
    length (confirmation_pending after) <
    length (confirmation_pending before).
Proof.
  intros [pending accepted] after step.
  destruct pending as [|candidate pending]; simpl in step; try discriminate.
  destruct (confirm_one candidate); inversion step; simpl; lia.
Qed.

End ExplicitWorklist.
