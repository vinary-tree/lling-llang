(** * Tree-transducer mutual-SCC descent certificate

    The production tree-transducer SCC has three logical phases:

    - [Transduce] selects an applicable rule for an input tree;
    - rule application enters [Instantiate] at the rule's output pattern;
    - instantiation either descends into a strict output subpattern or invokes
      [Transduce] on a strict input child referenced by a variable.

    A rule may reset pattern depth, so neither input size nor pattern size alone
    is a termination measure.  This file proves the lexicographic measure used
    by the specialized pushdown machine.  [MaxPattern] is the maximum output-
    pattern size in the fixed transducer.  Its finite value is computed once
    when the machine is built.

    [call_rank] assigns the rule-selection phase one slot above every legal
    pattern rank.  Strict input descent receives an entire fresh pattern band.
    Every SCC edge therefore decreases.  A heap continuation stack can hold
    the suspended callers, while the native Rust stack remains constant.

    The ordered traversal and error semantics of each pattern's child forest
    are proved independently in [OrderedForestMachine].  This file supplies
    the cross-phase termination invariant that that generic forest proof cannot
    express.
*)

From Stdlib Require Import Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
From Stdlib Require Import Wf_nat.
Import ListNotations.

Section TreeTransducerScc.

  Variable MaxPattern : nat.

  Inductive Call : Type :=
  | Transduce : nat -> Call
  | Instantiate : nat -> nat -> Call.

  Definition band_width : nat := MaxPattern + 2.

  Definition call_rank (call : Call) : nat :=
    match call with
    | Transduce input_size =>
        input_size * band_width + MaxPattern + 1
    | Instantiate input_size pattern_size =>
        input_size * band_width + pattern_size
    end.

  Inductive CallEdge : Call -> Call -> Prop :=
  | SelectRule : forall input_size pattern_size,
      pattern_size <= MaxPattern ->
      CallEdge
        (Instantiate input_size pattern_size)
        (Transduce input_size)
  | EnterSubpattern : forall input_size parent_size child_size,
      child_size < parent_size ->
      CallEdge
        (Instantiate input_size child_size)
        (Instantiate input_size parent_size)
  | InvokeVariable : forall input_size child_input_size pattern_size,
      child_input_size < input_size ->
      pattern_size <= MaxPattern ->
      CallEdge
        (Transduce child_input_size)
        (Instantiate input_size pattern_size).

  Lemma select_rule_decreases :
    forall input_size pattern_size,
      pattern_size <= MaxPattern ->
      call_rank (Instantiate input_size pattern_size) <
      call_rank (Transduce input_size).
  Proof. intros. unfold call_rank, band_width. simpl. lia. Qed.

  Lemma subpattern_decreases :
    forall input_size parent_size child_size,
      child_size < parent_size ->
      call_rank (Instantiate input_size child_size) <
      call_rank (Instantiate input_size parent_size).
  Proof. intros. unfold call_rank. simpl. lia. Qed.

  Lemma variable_child_decreases :
    forall input_size child_input_size pattern_size,
      child_input_size < input_size ->
      pattern_size <= MaxPattern ->
      call_rank (Transduce child_input_size) <
      call_rank (Instantiate input_size pattern_size).
  Proof.
    intros input_size child_input_size pattern_size Hchild Hpattern.
    unfold call_rank, band_width. simpl.
    nia.
  Qed.

  Theorem every_scc_edge_decreases :
    forall next current,
      CallEdge next current -> call_rank next < call_rank current.
  Proof.
    intros next current Hedge. destruct Hedge.
    - now apply select_rule_decreases.
    - now apply subpattern_decreases.
    - now apply variable_child_decreases.
  Qed.

  Theorem call_edge_well_founded : well_founded CallEdge.
  Proof.
    apply (well_founded_lt_compat Call call_rank).
    intros next current Hedge.
    now apply every_scc_edge_decreases in Hedge.
  Qed.

  Inductive CallPathN : nat -> Call -> Call -> Prop :=
  | CallPathZero : forall call, CallPathN 0 call call
  | CallPathStep : forall count current next terminal,
      CallEdge next current ->
      CallPathN count next terminal ->
      CallPathN (S count) current terminal.

  Theorem path_consumes_rank :
    forall count initial terminal,
      CallPathN count initial terminal ->
      count + call_rank terminal <= call_rank initial.
  Proof.
    intros count initial terminal Hpath. induction Hpath.
    - simpl. lia.
    - pose proof (every_scc_edge_decreases next current H) as Hdecrease.
      simpl. lia.
  Qed.

  Corollary explicit_call_stack_bound :
    forall count initial terminal,
      CallPathN count initial terminal -> count <= call_rank initial.
  Proof.
    intros count initial terminal Hpath.
    pose proof (path_consumes_rank count initial terminal Hpath).
    lia.
  Qed.

  Corollary no_recursive_call_cycle :
    forall call positive_count,
      0 < positive_count -> ~ CallPathN positive_count call call.
  Proof.
    intros call positive_count Hpositive Hcycle.
    pose proof (path_consumes_rank positive_count call call Hcycle).
    lia.
  Qed.

End TreeTransducerScc.

Section OrderedCartesianProduct.

  Context {A : Type}.

  Definition extend_prefixes (prefixes : list (list A)) (items : list A)
      : list (list A) :=
    flat_map
      (fun prefix => map (fun item => prefix ++ [item]) items)
      prefixes.

  Lemma extend_prefixes_empty_items :
    forall prefixes, extend_prefixes prefixes [] = [].
  Proof.
    induction prefixes as [| prefix rest IH]; simpl; [reflexivity | exact IH].
  Qed.

  Lemma extend_prefixes_empty_prefixes :
    forall items, extend_prefixes [] items = [].
  Proof. reflexivity. Qed.

  Lemma extend_prefixes_length :
    forall prefixes items,
      length (extend_prefixes prefixes items) =
      length prefixes * length items.
  Proof.
    intros prefixes items. induction prefixes as [| prefix rest IH]; simpl.
    - reflexivity.
    - rewrite app_length, map_length, IH. lia.
  Qed.

  Lemma extend_prefixes_head_block :
    forall prefix rest items,
      extend_prefixes (prefix :: rest) items =
      map (fun item => prefix ++ [item]) items ++
      extend_prefixes rest items.
  Proof. reflexivity. Qed.

  (** [extend_prefixes_head_block] is the exact ordering contract of the Rust
      nested loops: all extensions of the first prefix, in item order, precede
      every extension of the second prefix.  Repeated application therefore
      yields stable left-to-right lexicographic cartesian-product order. *)

End OrderedCartesianProduct.
