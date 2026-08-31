(** * PlanDag — finite, stack-safe optimizer dependency plans

    Every dependency edge carries an implicit certificate through a plan-wide
    rank function: the dependency rank is strictly lower than the dependent
    rank.  Natural-number descent excludes cycles.  An implementation can then
    schedule a ready queue iteratively, without recursive graph traversal.
*)

From Stdlib Require Import Lists.List.
From Stdlib Require Import Arith.Arith.
From Stdlib Require Import micromega.Lia.
Import ListNotations.

Record plan : Type := {
  plan_nodes : list nat;
  plan_edges : list (nat * nat);
  plan_rank : nat -> nat
}.

Definition well_formed_plan (candidate : plan) : Prop :=
  NoDup (plan_nodes candidate) /\
  forall dependency dependent,
    In (dependency, dependent) (plan_edges candidate) ->
    In dependency (plan_nodes candidate) /\
    In dependent (plan_nodes candidate) /\
    plan_rank candidate dependency < plan_rank candidate dependent.

Inductive depends_on (candidate : plan) : nat -> nat -> Prop :=
| depends_direct : forall dependency dependent,
    In (dependency, dependent) (plan_edges candidate) ->
    depends_on candidate dependency dependent
| depends_transitive : forall first middle last,
    In (first, middle) (plan_edges candidate) ->
    depends_on candidate middle last ->
    depends_on candidate first last.

Theorem dependency_path_increases_rank :
  forall candidate first last,
    well_formed_plan candidate ->
    depends_on candidate first last ->
    plan_rank candidate first < plan_rank candidate last.
Proof.
  intros candidate first last Hplan Hpath.
  induction Hpath.
  - destruct Hplan as [_ Hedges].
    destruct (Hedges dependency dependent H) as [_ [_ Hrank]].
    exact Hrank.
  - destruct Hplan as [_ Hedges].
    destruct (Hedges first middle H) as [_ [_ Hedge]].
    assert (Htail : plan_rank candidate middle < plan_rank candidate last).
    { exact IHHpath. }
    lia.
Qed.

Theorem well_formed_plan_is_acyclic :
  forall candidate node,
    well_formed_plan candidate ->
    ~ depends_on candidate node node.
Proof.
  intros candidate node Hplan Hcycle.
  pose proof (dependency_path_increases_rank candidate node node Hplan Hcycle).
  lia.
Qed.

Theorem well_formed_plan_has_no_self_edge :
  forall candidate node,
    well_formed_plan candidate ->
    ~ In (node, node) (plan_edges candidate).
Proof.
  intros candidate node Hplan Hedge.
  apply (well_formed_plan_is_acyclic candidate node Hplan).
  apply depends_direct; exact Hedge.
Qed.

Section OrderedProvenance.

Variable Event : Type.
Definition provenance : Type := list (nat * Event).

Definition commit
    (expected sequence : nat)
    (events : provenance)
    (event : Event) : option (nat * provenance) :=
  if Nat.eqb sequence expected
  then Some (S expected, events ++ [(sequence, event)])
  else None.

Theorem commit_expected_sequence :
  forall expected events event,
    commit expected expected events event =
      Some (S expected, events ++ [(expected, event)]).
Proof. intros; unfold commit; rewrite Nat.eqb_refl; reflexivity. Qed.

Theorem commit_rejects_out_of_order :
  forall expected sequence events event,
    sequence <> expected ->
    commit expected sequence events event = None.
Proof.
  intros; unfold commit.
  apply Nat.eqb_neq in H; rewrite H; reflexivity.
Qed.

Theorem commit_preserves_existing_prefix :
  forall expected sequence events event next committed,
    commit expected sequence events event = Some (next, committed) ->
    exists suffix, committed = events ++ suffix.
Proof.
  intros expected sequence events event next committed Hcommit.
  unfold commit in Hcommit.
  destruct (Nat.eqb sequence expected) eqn:Heq; try discriminate.
  injection Hcommit as _ Hcommitted.
  exists [(sequence, event)]. symmetry; exact Hcommitted.
Qed.

Theorem successful_commit_advances_once :
  forall expected sequence events event next committed,
    commit expected sequence events event = Some (next, committed) ->
    sequence = expected /\ next = S expected.
Proof.
  intros expected sequence events event next committed Hcommit.
  unfold commit in Hcommit.
  destruct (Nat.eqb sequence expected) eqn:Heq; try discriminate.
  apply Nat.eqb_eq in Heq.
  injection Hcommit as Hnext _.
  auto.
Qed.

End OrderedProvenance.
