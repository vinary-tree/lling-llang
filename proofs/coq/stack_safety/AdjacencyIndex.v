(** * Stable dense/sparse adjacency indexing

    Graph-product traversals must not rescan every transition at every visited
    state.  Production therefore partitions transition references once: a
    dense vector serves declared state identifiers and a sparse map serves
    out-of-range identifiers whose historical lookup behavior must remain
    observable until a separately versioned validation change is made.

    [outgoing] is the denotational reference: stable filtering of the original
    transition sequence.  [lookup_partitioned] models the optimized lookup.
    The main results prove exact list equality for every identifier, preservation
    of transition order, one-sided dense/sparse placement, and linear reference
    storage.  No axiom, admission, parameter, or proof escape is used.
*)

From Stdlib Require Import Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
Import ListNotations.

Section AdjacencyIndex.

  Context {Payload : Type}.

  Record Transition : Type := {
    source : nat;
    target : nat;
    payload : Payload
  }.

  Definition outgoing (state : nat) (transitions : list Transition) : list Transition :=
    filter (fun transition => Nat.eqb state (source transition)) transitions.

  Definition build_dense
      (declared_states : nat)
      (transitions : list Transition) : list (list Transition) :=
    map (fun state => outgoing state transitions) (seq 0 declared_states).

  Definition build_sparse
      (declared_states : nat)
      (transitions : list Transition) : list Transition :=
    filter
      (fun transition => Nat.leb declared_states (source transition))
      transitions.

  Definition lookup_partitioned
      (declared_states state : nat)
      (transitions : list Transition) : option (list Transition) :=
    if Nat.ltb state declared_states then
      nth_error (build_dense declared_states transitions) state
    else
      Some (outgoing state (build_sparse declared_states transitions)).

  Lemma nth_error_mapped_seq :
    forall count base index transitions,
      index < count ->
      nth_error
        (map (fun state => outgoing state transitions) (seq base count))
        index =
      Some (outgoing (base + index) transitions).
  Proof.
    induction count as [| count IH]; intros base index transitions Hindex.
    - lia.
    - destruct index as [| index].
      + simpl. now rewrite Nat.add_0_r.
      + simpl.
        rewrite (IH (S base) index transitions) by lia.
        replace (S base + index) with (base + S index) by lia.
        reflexivity.
  Qed.

  Theorem dense_lookup_preserves_order :
    forall declared_states state transitions,
      state < declared_states ->
      nth_error (build_dense declared_states transitions) state =
      Some (outgoing state transitions).
  Proof.
    intros declared_states state transitions Hstate.
    unfold build_dense.
    rewrite (nth_error_mapped_seq declared_states 0 state transitions Hstate).
    simpl. reflexivity.
  Qed.

  Lemma sparse_outgoing_preserves_order :
    forall declared_states state transitions,
      declared_states <= state ->
      outgoing state (build_sparse declared_states transitions) =
      outgoing state transitions.
  Proof.
    intros declared_states state transitions Hstate.
    induction transitions as [| transition rest IH].
    - reflexivity.
    - unfold build_sparse in *.
      simpl.
      destruct (Nat.leb declared_states (source transition)) eqn:Hsource.
      + simpl. destruct (Nat.eqb state (source transition)); simpl; now rewrite IH.
      + destruct (Nat.eqb state (source transition)) eqn:Hequal.
        * apply Nat.eqb_eq in Hequal. subst.
          apply Nat.leb_gt in Hsource. lia.
        * simpl. now rewrite IH.
  Qed.

  Theorem partitioned_lookup_preserves_exact_outgoing_sequence :
    forall declared_states state transitions,
      lookup_partitioned declared_states state transitions =
      Some (outgoing state transitions).
  Proof.
    intros declared_states state transitions.
    unfold lookup_partitioned.
    destruct (Nat.ltb state declared_states) eqn:Hstate.
    - apply Nat.ltb_lt in Hstate.
      now apply dense_lookup_preserves_order.
    - apply Nat.ltb_ge in Hstate.
      now rewrite sparse_outgoing_preserves_order.
  Qed.

  Definition dense_entries
      (declared_states : nat)
      (transitions : list Transition) : list Transition :=
    filter
      (fun transition => Nat.ltb (source transition) declared_states)
      transitions.

  Theorem dense_sparse_reference_count_is_linear :
    forall declared_states transitions,
      length (dense_entries declared_states transitions) +
      length (build_sparse declared_states transitions) =
      length transitions.
  Proof.
    intros declared_states transitions.
    induction transitions as [| transition rest IH].
    - reflexivity.
    - unfold dense_entries, build_sparse in *.
      simpl.
      destruct (Nat.ltb (source transition) declared_states) eqn:Hlt;
        destruct (Nat.leb declared_states (source transition)) eqn:Hge;
        simpl.
      + apply Nat.ltb_lt in Hlt. apply Nat.leb_le in Hge. lia.
      + lia.
      + lia.
      + apply Nat.ltb_ge in Hlt. apply Nat.leb_gt in Hge. lia.
  Qed.

  Theorem dense_bucket_count_is_declared_state_count :
    forall declared_states transitions,
      length (build_dense declared_states transitions) = declared_states.
  Proof.
    intros declared_states transitions.
    unfold build_dense.
    now rewrite length_map, length_seq.
  Qed.

End AdjacencyIndex.
