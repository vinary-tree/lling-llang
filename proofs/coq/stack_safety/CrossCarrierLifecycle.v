(** * Typed cross-carrier lifecycle traversal

    lling-llang owns recursive values whose edges cross representation
    families.  An [AnyDomain] may contain another domain directly or through a
    symbolic term; an [AnyAlgebra] contains child algebras through product,
    sum, list, bag, tree, and map wrappers.  Standalone syntax trees use only
    same-sort edges.  Derived Clone, Debug, comparison, hashing, and Drop follow
    those edges on the native stack.

    This theory models the common typed traversal kernel.  [step] removes one
    node from an explicit heap worklist, emits its observable tag, and schedules
    its ordered children.  The proofs establish:

    - only permitted cross-carrier edges are traversed;
    - the emitted order is exactly recursive preorder;
    - every transition consumes exactly one source node;
    - every nonterminal machine progresses;
    - a terminal run emits the complete recursive trace in exactly [size root]
      transitions.

    Postorder reconstruction uses the continuation discipline proved in
    [OrderedForestMachine.v].  Together the two theories cover the read-only and
    rebuilding lifecycle surfaces implemented by the specialized Rust
    machines.  No axiom, admission, or proof escape is used.
*)

From Stdlib Require Import Lia.
From Stdlib Require Import List.
From Stdlib Require Import Wf_nat.
Import ListNotations.

Inductive Sort : Type :=
| Carrier
| Term
| Algebra
| Syntax.

Inductive Node : Type :=
| Build : Sort -> nat -> list Node -> Node.

Definition node_sort (node : Node) : Sort :=
  match node with
  | Build sort _ _ => sort
  end.

Definition edge_allowed (parent child : Sort) : bool :=
  match parent, child with
  | Carrier, Carrier
  | Carrier, Term
  | Term, Carrier
  | Term, Term
  | Algebra, Algebra
  | Syntax, Syntax => true
  | _, _ => false
  end.

Inductive WellTyped : Node -> Prop :=
| WellTypedBuild :
    forall sort tag children,
      Forall
        (fun child =>
          edge_allowed sort (node_sort child) = true /\ WellTyped child)
        children ->
      WellTyped (Build sort tag children).

Lemma well_typed_children :
  forall sort tag children,
    WellTyped (Build sort tag children) ->
    Forall WellTyped children.
Proof.
  intros sort tag children Htyped.
  inversion Htyped as [actual_sort actual_tag actual_children Hall]; subst.
  eapply Forall_impl; [| exact Hall].
  intros child Hpair. exact (proj2 Hpair).
Qed.

Definition Label : Type := (Sort * nat)%type.

Fixpoint preorder (node : Node) : list Label :=
  match node with
  | Build sort tag children =>
      (sort, tag) :: concat (map preorder children)
  end.

Definition pending_trace (pending : list Node) : list Label :=
  concat (map preorder pending).

Lemma pending_trace_app :
  forall left right,
    pending_trace (left ++ right) =
    pending_trace left ++ pending_trace right.
Proof.
  intros left right.
  unfold pending_trace.
  rewrite map_app, concat_app. reflexivity.
Qed.

Fixpoint node_size (node : Node) : nat :=
  match node with
  | Build _ _ children =>
      1 + fold_right Nat.add 0 (map node_size children)
  end.

Fixpoint pending_size (pending : list Node) : nat :=
  match pending with
  | [] => 0
  | node :: rest => node_size node + pending_size rest
  end.

Lemma pending_size_app :
  forall left right,
    pending_size (left ++ right) =
    pending_size left + pending_size right.
Proof.
  induction left as [| node rest IH]; intros right; simpl.
  - reflexivity.
  - rewrite IH. lia.
Qed.

Lemma pending_size_as_fold :
  forall nodes,
    pending_size nodes =
    fold_right Nat.add 0 (map node_size nodes).
Proof.
  induction nodes as [| node rest IH]; simpl.
  - reflexivity.
  - now rewrite IH.
Qed.

Record Machine : Type := {
  pending : list Node;
  emitted_rev : list Label
}.

Definition step (machine : Machine) : option Machine :=
  match pending machine with
  | [] => None
  | Build sort tag children :: rest =>
      Some
        {| pending := children ++ rest;
           emitted_rev := (sort, tag) :: emitted_rev machine |}
  end.

Definition machine_trace (machine : Machine) : list Label :=
  rev (emitted_rev machine) ++ pending_trace (pending machine).

Theorem step_preserves_recursive_trace :
  forall machine next,
    step machine = Some next ->
    machine_trace next = machine_trace machine.
Proof.
  intros [pending emitted] next Hstep.
  destruct pending as [| [sort tag children] rest]; simpl in Hstep.
  - discriminate.
  - inversion Hstep; subst. unfold machine_trace, pending_trace. simpl.
    rewrite map_app, concat_app. simpl.
    repeat rewrite <- app_assoc. reflexivity.
Qed.

Theorem step_consumes_exactly_one_node :
  forall machine next,
    step machine = Some next ->
    S (pending_size (pending next)) = pending_size (pending machine).
Proof.
  intros [pending emitted] next Hstep.
  destruct pending as [| [sort tag children] rest]; simpl in Hstep.
  - discriminate.
  - inversion Hstep; subst. simpl. rewrite pending_size_app.
    rewrite pending_size_as_fold. simpl. lia.
Qed.

Definition PendingWellTyped (machine : Machine) : Prop :=
  Forall WellTyped (pending machine).

Theorem step_preserves_edge_typing :
  forall machine next,
    PendingWellTyped machine ->
    step machine = Some next ->
    PendingWellTyped next.
Proof.
  intros [pending emitted] next Htyped Hstep.
  destruct pending as [| [sort tag children] rest]; simpl in Hstep.
  - discriminate.
  - inversion Hstep; subst. inversion Htyped as [| parent tail Hparent Htail]; subst.
    apply Forall_app. split.
    + now apply (well_typed_children sort tag children).
    + exact Htail.
Qed.

Theorem nonterminal_progress :
  forall machine,
    pending machine <> [] ->
    exists next, step machine = Some next.
Proof.
  intros [pending emitted] Hnonempty.
  destruct pending as [| [sort tag children] rest].
  - contradiction.
  - eexists. reflexivity.
Qed.

Definition transition (next machine : Machine) : Prop :=
  step machine = Some next.

Theorem transition_well_founded :
  well_founded transition.
Proof.
  apply (well_founded_lt_compat Machine (fun machine => pending_size (pending machine))).
  intros next machine Hstep.
  unfold transition in Hstep.
  apply step_consumes_exactly_one_node in Hstep. lia.
Qed.

Inductive Steps : nat -> Machine -> Machine -> Prop :=
| StepsZero : forall machine, Steps 0 machine machine
| StepsMore :
    forall count machine middle final,
      step machine = Some middle ->
      Steps count middle final ->
      Steps (S count) machine final.

Theorem steps_preserve_recursive_trace :
  forall count initial final,
    Steps count initial final ->
    machine_trace final = machine_trace initial.
Proof.
  intros count initial final Hsteps.
  induction Hsteps as
      [machine | count machine middle final Hstep Hsteps IH].
  - reflexivity.
  - rewrite IH. now apply step_preserves_recursive_trace in Hstep.
Qed.

Theorem steps_account_for_exact_work :
  forall count initial final,
    Steps count initial final ->
    count + pending_size (pending final) =
    pending_size (pending initial).
Proof.
  intros count initial final Hsteps.
  induction Hsteps as
      [machine | count machine middle final Hstep Hsteps IH].
  - simpl. lia.
  - apply step_consumes_exactly_one_node in Hstep. simpl. lia.
Qed.

Definition initial_machine (root : Node) : Machine :=
  {| pending := [root]; emitted_rev := [] |}.

Theorem terminal_run_equals_recursive_reference :
  forall root count final,
    Steps count (initial_machine root) final ->
    pending final = [] ->
    rev (emitted_rev final) = preorder root /\
    count = node_size root.
Proof.
  intros root count final Hsteps Hterminal.
  split.
  - pose proof (steps_preserve_recursive_trace count (initial_machine root) final Hsteps)
      as Htrace.
    unfold machine_trace, initial_machine in Htrace. simpl in Htrace.
    rewrite Hterminal in Htrace. unfold pending_trace in Htrace.
    simpl in Htrace. now repeat rewrite app_nil_r in Htrace.
  - pose proof (steps_account_for_exact_work count (initial_machine root) final Hsteps)
      as Hwork.
    unfold initial_machine in Hwork. simpl in Hwork.
    rewrite Hterminal in Hwork. simpl in Hwork. lia.
Qed.

Corollary terminal_native_stack_is_input_independent :
  forall root count final,
    Steps count (initial_machine root) final ->
    pending final = [] ->
    count = node_size root.
Proof.
  intros root count final Hsteps Hterminal.
  now apply (terminal_run_equals_recursive_reference root count final).
Qed.
