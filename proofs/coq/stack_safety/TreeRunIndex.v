(** * Exact child-tuple indexing for symbolic tree-automaton runs

    A bottom-up tree-automaton run must not scan every transition at every
    input node.  The production index groups each transition once by
    constructor, arity, and exact child-state tuple.  At a completed node, a
    hybrid evaluator chooses between the stable constructor/arity bucket and
    exact lookups for the Cartesian product of reachable child-state sets.

    This model proves that exact-tuple enumeration has precisely the same
    accepted transition set as the denotational scan, that each transition has
    exactly one stable-bucket and one exact-tuple reference, that the hybrid
    never performs more lookup
    probes than a full structural-bucket scan, and that a deterministic unary
    path requires one exact-tuple probe per node.  Transition indices are sorted
    before guards are evaluated in production, preserving source transition
    order among semantically applicable transitions.  No axiom, admission,
    parameter, or proof escape is used.
*)

From Stdlib Require Import Arith.
From Stdlib Require Import Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
Import ListNotations.

Record TreeTransition : Type := {
  transition_constructor : nat;
  transition_children : list nat;
  transition_target : nat;
  transition_guard_accepts : bool
}.

Fixpoint child_tuples (reachable : list (list nat)) : list (list nat) :=
  match reachable with
  | [] => [[]]
  | states :: rest =>
      flat_map
        (fun state => map (cons state) (child_tuples rest))
        states
  end.

Lemma in_child_tuples_iff :
  forall tuple reachable,
    In tuple (child_tuples reachable) <->
    Forall2 (fun state states => In state states) tuple reachable.
Proof.
  intros tuple reachable. revert tuple.
  induction reachable as [| states rest IH]; intros tuple; simpl.
  - split.
    + intros [Hequal | Himpossible].
      * subst tuple. constructor.
      * contradiction.
    + intro Hmatched. inversion Hmatched. now left.
  - split.
    + intro Hin.
      apply in_flat_map in Hin.
      destruct Hin as [state [Hstate Hmapped]].
      apply in_map_iff in Hmapped.
      destruct Hmapped as [tail [Hequal Htail]].
      subst tuple.
      constructor.
      * exact Hstate.
      * now apply (proj1 (IH tail)).
    + intro Hmatched.
      inversion Hmatched as [| state states' tail rest' Hstate Htail]; subst.
      apply in_flat_map.
      exists state. split.
      * exact Hstate.
      * apply in_map_iff.
        exists tail. split; [reflexivity |].
        now apply (proj2 (IH tail)).
Qed.

Definition same_children (left right : list nat) : bool :=
  if list_eq_dec Nat.eq_dec left right then true else false.

Lemma same_children_true_iff :
  forall left right,
    same_children left right = true <-> left = right.
Proof.
  intros left right.
  unfold same_children.
  destruct (list_eq_dec Nat.eq_dec left right) as [Hequal | Hdifferent].
  - split; [intro; exact Hequal | intro; reflexivity].
  - split.
    + discriminate.
    + intro Hequal. contradiction.
Qed.

Definition exact_bucket
    (constructor : nat)
    (children : list nat)
    (transitions : list TreeTransition) : list TreeTransition :=
  filter
    (fun transition =>
       Nat.eqb constructor (transition_constructor transition) &&
       same_children children (transition_children transition))
    transitions.

Lemma in_exact_bucket_iff :
  forall transition constructor children transitions,
    In transition (exact_bucket constructor children transitions) <->
    In transition transitions /\
    transition_constructor transition = constructor /\
    transition_children transition = children.
Proof.
  intros transition constructor children transitions.
  unfold exact_bucket.
  rewrite filter_In, andb_true_iff, Nat.eqb_eq, same_children_true_iff.
  split.
  - intros [Hin [Hconstructor Hchildren]].
    repeat split; try assumption; symmetry; assumption.
  - intros [Hin [Hconstructor Hchildren]].
    repeat split; try assumption; symmetry; assumption.
Qed.

Definition indexed_candidates
    (constructor : nat)
    (reachable : list (list nat))
    (transitions : list TreeTransition) : list TreeTransition :=
  flat_map
    (fun tuple => exact_bucket constructor tuple transitions)
    (child_tuples reachable).

Definition indexed_accepted
    (constructor : nat)
    (reachable : list (list nat))
    (transitions : list TreeTransition) : list TreeTransition :=
  filter transition_guard_accepts
    (indexed_candidates constructor reachable transitions).

Theorem indexed_acceptance_is_exact :
  forall transition constructor reachable transitions,
    In transition (indexed_accepted constructor reachable transitions) <->
    In transition transitions /\
    transition_constructor transition = constructor /\
    Forall2
      (fun state states => In state states)
      (transition_children transition)
      reachable /\
    transition_guard_accepts transition = true.
Proof.
  intros transition constructor reachable transitions.
  unfold indexed_accepted, indexed_candidates.
  rewrite filter_In.
  split.
  - intros [Hcandidate Hguard].
    apply in_flat_map in Hcandidate.
    destruct Hcandidate as [tuple [Htuple Hbucket]].
    apply in_child_tuples_iff in Htuple.
    apply in_exact_bucket_iff in Hbucket.
    destruct Hbucket as [Hin [Hconstructor Hchildren]].
    subst tuple.
    repeat split; assumption.
  - intros [Hin [Hconstructor [Hchildren Hguard]]].
    split; [| exact Hguard].
    apply in_flat_map.
    exists (transition_children transition).
    split.
    + now apply in_child_tuples_iff.
    + apply in_exact_bucket_iff.
      repeat split; assumption.
Qed.

(** The concrete hybrid index stores one stable-bucket reference and one
    exact-tuple reference for each source transition. *)
Definition index_references
    (transitions : list TreeTransition) :
    list (nat * list nat * TreeTransition) :=
  flat_map
    (fun transition =>
       [(transition_constructor transition, [], transition);
        (transition_constructor transition,
         transition_children transition,
         transition)])
    transitions.

Theorem hybrid_index_reference_count_is_linear :
  forall transitions,
    length (index_references transitions) = 2 * length transitions.
Proof.
  intros transitions.
  induction transitions as [| transition rest IH].
  - reflexivity.
  - simpl. lia.
Qed.

Definition scan_probe_work (structural_bucket_size : nat) : nat :=
  structural_bucket_size.

Definition exact_probe_work (reachable : list (list nat)) : nat :=
  length (child_tuples reachable).

Definition hybrid_probe_work
    (structural_bucket_size : nat)
    (reachable : list (list nat)) : nat :=
  Nat.min
    (scan_probe_work structural_bucket_size)
    (exact_probe_work reachable).

Theorem hybrid_never_exceeds_structural_scan :
  forall structural_bucket_size reachable,
    hybrid_probe_work structural_bucket_size reachable <=
    scan_probe_work structural_bucket_size.
Proof.
  intros structural_bucket_size reachable.
  unfold hybrid_probe_work. apply Nat.le_min_l.
Qed.

Lemma singleton_child_tuples :
  forall tuple,
    child_tuples (map (fun state => [state]) tuple) = [tuple].
Proof.
  induction tuple as [| state rest IH].
  - reflexivity.
  - simpl. now rewrite IH.
Qed.

Theorem deterministic_exact_lookup_is_one_probe :
  forall tuple,
    exact_probe_work (map (fun state => [state]) tuple) = 1.
Proof.
  intros tuple.
  unfold exact_probe_work.
  now rewrite singleton_child_tuples.
Qed.

Theorem deterministic_unary_path_lookup_is_linear :
  forall depth,
    depth * exact_probe_work [[0]] = depth.
Proof.
  intros depth. unfold exact_probe_work. simpl. lia.
Qed.
