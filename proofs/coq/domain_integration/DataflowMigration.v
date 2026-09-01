(** * DataflowMigration — lawful libcpg-to-llattice refinement

    The two legacy libcpg lattice interfaces expose the same join-semilattice
    with opposite order observations: intraprocedural dataflow asks [leq a b],
    while IFDS asks whether [a] subsumes [b], namely [leq b a].  This model
    proves the exact adapter equations, the change-flag contract, deterministic
    accumulation, and the resource-cap monotonicity required before migration.

    Termination is deliberately conditional.  Semilattice laws make merge
    order irrelevant; they do not manufacture finite height, monotone transfer,
    widening, or a convergence bound.  A completed run therefore carries a
    witnessed stable iterate.  All runtime worklist state must be heap-owned;
    no theorem here licenses input-dependent native recursion.
*)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

Section LawfulJoin.

Variable A : Type.
Variable join : A -> A -> A.

Hypothesis join_idempotent : forall value, join value value = value.
Hypothesis join_commutative : forall left right, join left right = join right left.
Hypothesis join_associative : forall first second third,
  join (join first second) third = join first (join second third).

Definition join_leq (left right : A) : Prop := join left right = right.

Lemma join_leq_reflexive : forall value, join_leq value value.
Proof. intro value; unfold join_leq; apply join_idempotent. Qed.

Lemma join_leq_transitive : forall first second third,
  join_leq first second ->
  join_leq second third ->
  join_leq first third.
Proof.
  intros first second third first_second second_third.
  unfold join_leq in *.
  rewrite <- second_third.
  rewrite <- join_associative.
  rewrite first_second.
  reflexivity.
Qed.

Lemma join_leq_antisymmetric : forall left right,
  join_leq left right ->
  join_leq right left ->
  left = right.
Proof.
  intros left right left_right right_left.
  unfold join_leq in *.
  rewrite join_commutative in right_left.
  rewrite left_right in right_left.
  symmetry; exact right_left.
Qed.

Lemma join_is_left_upper_bound : forall left right,
  join_leq left (join left right).
Proof.
  intros left right; unfold join_leq.
  rewrite <- join_associative.
  rewrite join_idempotent.
  reflexivity.
Qed.

Lemma join_is_right_upper_bound : forall left right,
  join_leq right (join left right).
Proof.
  intros left right.
  rewrite join_commutative.
  apply join_is_left_upper_bound.
Qed.

Lemma join_is_least_upper_bound : forall left right upper,
  join_leq left upper ->
  join_leq right upper ->
  join_leq (join left right) upper.
Proof.
  intros left right upper left_upper right_upper.
  unfold join_leq in *.
  rewrite join_associative.
  rewrite right_upper.
  exact left_upper.
Qed.

(** Pure denotation of Rust's in-place [join_assign]. *)
Variable join_assign : A -> A -> A * bool.
Hypothesis join_assign_value : forall left right,
  fst (join_assign left right) = join left right.
Hypothesis join_assign_flag : forall left right,
  snd (join_assign left right) = true <-> fst (join_assign left right) <> left.

Theorem dataflow_adapter_preserves_join : forall left right,
  fst (join_assign left right) = join left right.
Proof. exact join_assign_value. Qed.

Theorem dataflow_adapter_change_flag_is_exact : forall left right,
  snd (join_assign left right) = true <-> join left right <> left.
Proof.
  intros left right.
  rewrite join_assign_flag.
  rewrite join_assign_value.
  reflexivity.
Qed.

Theorem dataflow_adapter_preserves_order : forall left right,
  join_leq left right <-> join left right = right.
Proof. intros; reflexivity. Qed.

Definition ifds_subsumes (container contained : A) : Prop :=
  join_leq contained container.

Theorem ifds_subsumes_reverses_join_order : forall container contained,
  ifds_subsumes container contained <-> join_leq contained container.
Proof. intros; reflexivity. Qed.

Theorem ifds_subsumes_is_reflexive : forall value,
  ifds_subsumes value value.
Proof. intro value; apply join_leq_reflexive. Qed.

Theorem ifds_subsumes_is_transitive : forall first second third,
  ifds_subsumes first second ->
  ifds_subsumes second third ->
  ifds_subsumes first third.
Proof.
  intros first second third first_second second_third.
  unfold ifds_subsumes in *.
  eapply join_leq_transitive; eauto.
Qed.

(** [Default] is a lawful IFDS bottom only with this explicit equation. *)
Variable bottom : A.
Hypothesis bottom_identity : forall value, join bottom value = value.

Lemma bottom_is_least : forall value, join_leq bottom value.
Proof. intro value; unfold join_leq; apply bottom_identity. Qed.

Theorem ifds_default_bottom_bridge_is_sound : forall value,
  ifds_subsumes value bottom.
Proof. intro value; unfold ifds_subsumes; apply bottom_is_least. Qed.

Fixpoint join_all (values : list A) : A :=
  match values with
  | [] => bottom
  | value :: rest => join value (join_all rest)
  end.

Lemma join_all_permutation_invariant : forall left right,
  Permutation left right -> join_all left = join_all right.
Proof.
  intros left right permutation.
  induction permutation; simpl; congruence.
Qed.

Lemma join_all_duplicate_invariant : forall value rest,
  join_all (value :: value :: rest) = join_all (value :: rest).
Proof.
  intros value rest; simpl.
  rewrite <- join_associative.
  rewrite join_idempotent.
  reflexivity.
Qed.

(** A deterministic transfer endomap.  The production solver separately owes
    monotonicity, finite height or widening, and a heap-worklist refinement. *)
Variable transfer : A -> A.
Hypothesis transfer_monotone : forall left right,
  join_leq left right -> join_leq (transfer left) (transfer right).
Hypothesis transfer_inflationary : forall value,
  join_leq value (transfer value).

Fixpoint iterate (steps : nat) (state : A) : A :=
  match steps with
  | 0 => state
  | S previous => transfer (iterate previous state)
  end.

Definition stable (state : A) : Prop := transfer state = state.
Definition run_state (steps : nat) : A := iterate steps bottom.

Lemma iterate_is_ascending : forall steps,
  join_leq (run_state steps) (run_state (S steps)).
Proof. intro steps; apply transfer_inflationary. Qed.

Lemma stable_remains_stable : forall state,
  stable state -> stable (transfer state).
Proof.
  intros state stable_state.
  unfold stable in *.
  rewrite stable_state.
  exact stable_state.
Qed.

Lemma iterate_from_stable_is_constant : forall state,
  stable state -> forall extra, iterate extra state = state.
Proof.
  intros state stable_state extra.
  induction extra; simpl; auto.
  rewrite IHextra.
  exact stable_state.
Qed.

Lemma iterate_additive : forall first second state,
  iterate (first + second) state = iterate second (iterate first state).
Proof.
  intros first second state.
  induction second; simpl.
  - rewrite Nat.add_0_r; reflexivity.
  - rewrite Nat.add_succ_r; simpl; rewrite IHsecond; reflexivity.
Qed.

Definition completes_within (budget : nat) (output : A) : Prop :=
  exists steps,
    steps <= budget /\ stable (run_state steps) /\ output = run_state steps.

Theorem completion_is_monotone_in_budget : forall smaller larger output,
  smaller <= larger ->
  completes_within smaller output ->
  completes_within larger output.
Proof.
  intros smaller larger output within [steps [bounded [fixed result]]].
  exists steps; repeat split; try assumption; lia.
Qed.

Lemma stable_run_is_constant_after : forall fixed later,
  fixed <= later ->
  stable (run_state fixed) ->
  run_state later = run_state fixed.
Proof.
  intros fixed later ordered fixed_stable.
  replace later with (fixed + (later - fixed)) by lia.
  unfold run_state.
  rewrite iterate_additive.
  apply iterate_from_stable_is_constant.
  exact fixed_stable.
Qed.

Theorem completed_outputs_are_unique : forall first_budget second_budget
    first_output second_output,
  completes_within first_budget first_output ->
  completes_within second_budget second_output ->
  first_output = second_output.
Proof.
  intros first_budget second_budget first_output second_output
         [first_steps [_ [first_stable first_result]]]
         [second_steps [_ [second_stable second_result]]].
  subst first_output; subst second_output.
  destruct (Nat.le_ge_cases first_steps second_steps) as [ordered | ordered].
  - symmetry; apply stable_run_is_constant_after; assumption.
  - apply stable_run_is_constant_after; assumption.
Qed.

Theorem larger_budget_preserves_completed_output : forall smaller larger output,
  smaller <= larger ->
  completes_within smaller output ->
  forall larger_output,
    completes_within larger larger_output ->
    larger_output = output.
Proof.
  intros smaller larger output ordered smaller_complete larger_output larger_complete.
  symmetry.
  eapply completed_outputs_are_unique; eauto.
Qed.

Definition incomplete_at (budget : nat) : Prop :=
  forall output, ~ completes_within budget output.

Theorem higher_budget_incomplete_implies_lower_budget_incomplete :
  forall smaller larger,
    smaller <= larger ->
    incomplete_at larger ->
    incomplete_at smaller.
Proof.
  intros smaller larger ordered larger_incomplete output smaller_complete.
  apply (larger_incomplete output).
  eapply completion_is_monotone_in_budget; eauto.
Qed.

(** Explicit heap machine shape for the future Rust refinement.  [pending]
    and [states] are unbounded mathematical sequences and therefore must become
    heap allocations; [phase] is the finite native control state. *)
Inductive worklist_phase : Type :=
| PopPending
| ApplyTransfer
| PropagateSuccessors
| WorklistHalted.

Record heap_worklist_state : Type := {
  worklist_phase_value : worklist_phase;
  pending : list nat;
  states : list A
}.

Theorem worklist_control_is_finite :
  forall phase,
    phase = PopPending \/ phase = ApplyTransfer \/
    phase = PropagateSuccessors \/ phase = WorklistHalted.
Proof. destruct phase; auto. Qed.

End LawfulJoin.
