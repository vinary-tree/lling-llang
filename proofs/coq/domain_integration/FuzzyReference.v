(** * FuzzyReference — exact fuzzy-query reference semantics

    A dictionary feed is an indexed family (a fiber for each query index), not
    a fibration merely because it is indexed.  The index fixes the immutable
    dictionary snapshot, query, edit configuration, and budget.  Candidate
    generation may over-approximate that fiber.  Only an independent confirmer
    can establish membership in the exact reference denotation.

    The central theorem states the complete contract used by the future
    libdictenstein/liblevenshtein adapter: a complete generator and an exact
    confirmer produce precisely the reference set when both operate at the
    same index.  No candidate-membership flag can replace those premises.
*)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.

Require Import LlingLlang.optimizer.RewriteSemantics.

Section IndexedFuzzyReference.

Variables Snapshot Query Configuration Term : Type.

(** One fiber index.  Equality of all four fields is required before evidence
    produced at one boundary may be consumed at another. *)
Record fuzzy_index : Type := {
  index_snapshot : Snapshot;
  index_query : Query;
  index_configuration : Configuration;
  index_budget : nat
}.

(** Dictionary membership and the independently defined edit cost. *)
Variable present_at : Snapshot -> Term -> Prop.
Variable edit_cost : Configuration -> Query -> Term -> nat.

(** Exact reference membership for one immutable query index. *)
Definition reference_member (index : fuzzy_index) (term : Term) : Prop :=
  present_at (index_snapshot index) term /\
  edit_cost (index_configuration index) (index_query index) term <=
    index_budget index.

(** The candidate feed may contain false positives. *)
Variable candidate_member : fuzzy_index -> Term -> Prop.

(** The confirmer is deliberately independent of candidate membership. *)
Variable confirmed_member : fuzzy_index -> Term -> Prop.

Definition generator_complete (index : fuzzy_index) : Prop :=
  forall term, reference_member index term -> candidate_member index term.

Definition confirmer_sound : Prop :=
  forall index term,
    confirmed_member index term -> reference_member index term.

Definition confirmer_complete : Prop :=
  forall index term,
    reference_member index term -> confirmed_member index term.

(** Equality of a fiber index is structural rather than nominal. *)
Definition same_index (left right : fuzzy_index) : Prop :=
  index_snapshot left = index_snapshot right /\
  index_query left = index_query right /\
  index_configuration left = index_configuration right /\
  index_budget left = index_budget right.

(** A result can cross the generator/confirmer boundary only through the two
    separately indexed predicates. *)
Definition accepted_member
    (generated_at confirmed_at : fuzzy_index)
    (term : Term) : Prop :=
  candidate_member generated_at term /\ confirmed_member confirmed_at term.

Lemma same_index_preserves_reference :
  forall left right,
    same_index left right ->
    forall term, reference_member left term <-> reference_member right term.
Proof.
  intros [left_snapshot left_query left_configuration left_budget]
         [right_snapshot right_query right_configuration right_budget].
  unfold same_index; simpl.
  intros [snapshot_equal [query_equal [configuration_equal budget_equal]]] term.
  subst; reflexivity.
Qed.

(** Independent confirmation alone makes every accepted result sound, even if
    candidate generation over-approximates. *)
Theorem accepted_members_are_sound :
  confirmer_sound ->
  forall generated_at confirmed_at term,
    same_index generated_at confirmed_at ->
    accepted_member generated_at confirmed_at term ->
    reference_member generated_at term.
Proof.
  intros confirmation_is_sound generated_at confirmed_at term same
         [_ confirmed].
  apply (proj2 (same_index_preserves_reference generated_at confirmed_at same term)).
  exact (confirmation_is_sound confirmed_at term confirmed).
Qed.

(** Complete generation plus independent exact confirmation is extensionally
    equal to exhaustive reference enumeration. *)
Theorem complete_confirmed_feed_equals_reference :
  confirmer_sound ->
  confirmer_complete ->
  forall generated_at confirmed_at,
    same_index generated_at confirmed_at ->
    generator_complete generated_at ->
    forall term,
      accepted_member generated_at confirmed_at term <->
      reference_member generated_at term.
Proof.
  intros confirmation_is_sound confirmation_is_complete
         generated_at confirmed_at same generation_is_complete term.
  split.
  - apply accepted_members_are_sound; assumption.
  - intro reference.
    split.
    + exact (generation_is_complete term reference).
    + apply confirmation_is_complete.
      apply (proj1 (same_index_preserves_reference
        generated_at confirmed_at same term)).
      exact reference.
Qed.

(** A complete exact publication certificate packages every premise needed by
    the denotational theorem.  Precision/completeness flags are present, but
    they cannot construct any of the semantic premises. *)
Record exact_fuzzy_certificate
    (generated_at confirmed_at : fuzzy_index) : Type := {
  fuzzy_claim : analysis_claim;
  fuzzy_precision_is_exact : claim_precision fuzzy_claim = Exact;
  fuzzy_completeness_is_complete : claim_completeness fuzzy_claim = Complete;
  fuzzy_indices_match : same_index generated_at confirmed_at;
  fuzzy_generator_is_complete : generator_complete generated_at;
  fuzzy_confirmer_is_sound : confirmer_sound;
  fuzzy_confirmer_is_complete : confirmer_complete
}.

Theorem certified_fuzzy_result_equals_reference :
  forall generated_at confirmed_at
         (certificate : exact_fuzzy_certificate generated_at confirmed_at)
         term,
    accepted_member generated_at confirmed_at term <->
    reference_member generated_at term.
Proof.
  intros generated_at confirmed_at certificate term.
  apply complete_confirmed_feed_equals_reference.
  - exact (fuzzy_confirmer_is_sound _ _ certificate).
  - exact (fuzzy_confirmer_is_complete _ _ certificate).
  - exact (fuzzy_indices_match _ _ certificate).
  - exact (fuzzy_generator_is_complete _ _ certificate).
Qed.

(** Outcome labels keep exactness and approximation honest. *)
Inductive fuzzy_outcome : Type :=
| CompleteExactResult
| CompleteApproximateResult
| IncompleteResult.

Definition classify_fuzzy_outcome
    (result_precision : precision)
    (result_completeness : completeness) : fuzzy_outcome :=
  match result_completeness, result_precision with
  | Complete, Exact => CompleteExactResult
  | Complete, SoundApproximation => CompleteApproximateResult
  | Incomplete, _ => IncompleteResult
  end.

Theorem exact_outcome_requires_both_exact_and_complete :
  forall result_precision result_completeness,
    classify_fuzzy_outcome result_precision result_completeness =
      CompleteExactResult ->
    result_precision = Exact /\ result_completeness = Complete.
Proof.
  destruct result_precision, result_completeness; simpl; intros;
    try discriminate; auto.
Qed.

Theorem incomplete_outcome_is_absorbing :
  forall result_precision,
    classify_fuzzy_outcome result_precision Incomplete = IncompleteResult.
Proof. destruct result_precision; reflexivity. Qed.

End IndexedFuzzyReference.

(** ** Constructive negative controls

    These witnesses prevent three tempting but invalid shortcuts from being
    introduced by the implementation: stale-snapshot reuse, edit-configuration
    reuse, and candidate self-confirmation.
*)

Definition counter_present (snapshot : bool) (_ : unit) : Prop :=
  snapshot = false.

Definition counter_cost
    (configuration : bool) (_ : unit) (_ : unit) : nat :=
  if configuration then 1 else 0.

Definition counter_candidate
    (_ : fuzzy_index bool unit bool) (term : unit) : Prop := term = tt.

Definition counter_confirmed
    (index : fuzzy_index bool unit bool) (term : unit) : Prop :=
  reference_member bool unit bool unit counter_present counter_cost index term.

Definition old_snapshot_index : fuzzy_index bool unit bool :=
  {| index_snapshot := false;
     index_query := tt;
     index_configuration := false;
     index_budget := 0 |}.

Definition new_snapshot_index : fuzzy_index bool unit bool :=
  {| index_snapshot := true;
     index_query := tt;
     index_configuration := false;
     index_budget := 0 |}.

Definition changed_configuration_index : fuzzy_index bool unit bool :=
  {| index_snapshot := false;
     index_query := tt;
     index_configuration := true;
     index_budget := 0 |}.

Theorem stale_snapshot_can_drop_a_reference_member :
  reference_member bool unit bool unit counter_present counter_cost
    old_snapshot_index tt /\
  ~ accepted_member bool unit bool unit counter_candidate counter_confirmed
      old_snapshot_index
      new_snapshot_index tt.
Proof.
  split.
  - unfold reference_member, counter_present, counter_cost,
      old_snapshot_index; simpl; auto.
  - unfold accepted_member, counter_candidate, counter_confirmed,
      reference_member, counter_present, counter_cost,
      old_snapshot_index, new_snapshot_index; simpl.
    intros [_ [impossible _]].
    discriminate.
Qed.

Theorem changed_configuration_can_drop_a_reference_member :
  reference_member bool unit bool unit counter_present counter_cost
    old_snapshot_index tt /\
  ~ accepted_member bool unit bool unit counter_candidate counter_confirmed
      old_snapshot_index
      changed_configuration_index tt.
Proof.
  split.
  - unfold reference_member, counter_present, counter_cost,
      old_snapshot_index; simpl; auto.
  - unfold accepted_member, counter_candidate, counter_confirmed,
      reference_member, counter_present, counter_cost,
      old_snapshot_index, changed_configuration_index; simpl.
    lia.
Qed.

Theorem candidate_membership_cannot_self_confirm :
  counter_candidate new_snapshot_index tt /\
  ~ reference_member bool unit bool unit counter_present counter_cost
      new_snapshot_index tt.
Proof.
  split.
  - reflexivity.
  - unfold reference_member, counter_present, counter_cost,
      new_snapshot_index; simpl.
    intuition discriminate.
Qed.

Theorem incomplete_generation_can_miss_a_reference_member :
  exists candidate : fuzzy_index bool unit bool -> unit -> Prop,
    reference_member bool unit bool unit counter_present counter_cost
      old_snapshot_index tt /\
    ~ candidate old_snapshot_index tt.
Proof.
  exists (fun _ _ => False).
  split.
  - unfold reference_member, counter_present, counter_cost,
      old_snapshot_index; simpl; auto.
  - simpl; auto.
Qed.
