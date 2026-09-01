(** * Stack-safe behavioral-formula iteration

    Behavioral formulas add two control forms to the unary/binary expression
    machine proved in [ContinuationMachine]: a nested bounded domain and a
    body that is evaluated repeatedly by a quantifier or a finite fixpoint
    iteration.  This development verifies the control invariants required by
    the specialized Rust pushdown automata.

    [peel_domain] descends through a bounded-domain spine while pushing limits
    on a heap stack.  Reaching the base applies those limits inner-to-outer.
    [peel_domain_correct] proves equality with the recursive specification;
    [apply_limits_false_sticky] proves that truncation can never be hidden by
    an enclosing bound.

    [step] models formula traversal after semantic operators and environments
    have been defunctionalized into typed frames.  [FRepeat count body] covers
    both quantifier enumeration and the bounded Kleene iteration used for
    least/greatest fixpoints.  A repeat frame is reused for every body
    evaluation, so native call-stack depth is constant.  The strict potential
    proof accounts for every scheduled body evaluation and yields a linear
    transition bound in the actual amount of work.

    Semantic preservation for leaf, unary, and ordered binary operators is the
    theorem [ContinuationMachine.initial_terminal_correct].  This file supplies
    the additional domain-order, exactness, repetition, termination, and work
    invariants.  It uses no axioms, admissions, parameters, or proof escapes.
*)

From Stdlib Require Import Arith.
From Stdlib Require Import Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
From Stdlib Require Import Wf_nat.
Import ListNotations.

Section BoundedDomain.

  Context {A : Type}.

  Inductive Domain : Type :=
  | DValues : list A -> Domain
  | DBounded : Domain -> nat -> Domain.

  Fixpoint domain_semantics (domain : Domain) : list A * bool :=
    match domain with
    | DValues values => (values, true)
    | DBounded inner limit =>
        let '(values, exact) := domain_semantics inner in
        (firstn limit values, andb exact (Nat.leb (length values) limit))
    end.

  Fixpoint apply_limits
      (limits : list nat) (values : list A) (exact : bool) : list A * bool :=
    match limits with
    | [] => (values, exact)
    | limit :: rest =>
        apply_limits rest
          (firstn limit values)
          (andb exact (Nat.leb (length values) limit))
    end.

  Fixpoint peel_domain
      (domain : Domain) (limits : list nat) : list A * bool :=
    match domain with
    | DValues values => apply_limits limits values true
    | DBounded inner limit => peel_domain inner (limit :: limits)
    end.

  Definition run_domain (domain : Domain) : list A * bool :=
    peel_domain domain [].

  Lemma apply_limits_append :
    forall first second values exact,
      apply_limits (first ++ second) values exact =
      let '(middle_values, middle_exact) :=
        apply_limits first values exact in
      apply_limits second middle_values middle_exact.
  Proof.
    induction first as [| limit rest IH]; intros second values exact; simpl.
    - reflexivity.
    - apply IH.
  Qed.

  Theorem peel_domain_correct :
    forall domain pending,
      peel_domain domain pending =
      let '(values, exact) := domain_semantics domain in
      apply_limits pending values exact.
  Proof.
    induction domain as [values | inner IH limit]; intros pending; simpl.
    - reflexivity.
    - rewrite IH.
      destruct (domain_semantics inner) as [values exact].
      reflexivity.
  Qed.

  Corollary run_domain_correct :
    forall domain, run_domain domain = domain_semantics domain.
  Proof.
    intros domain. unfold run_domain.
    rewrite peel_domain_correct.
    destruct (domain_semantics domain). reflexivity.
  Qed.

  Lemma apply_limits_false_sticky :
    forall limits values,
      snd (apply_limits limits values false) = false.
  Proof.
    induction limits as [| limit rest IH]; intros values; simpl.
    - reflexivity.
    - apply IH.
  Qed.

  Lemma bounded_truncation_is_inexact :
    forall values exact limit,
      limit < length values ->
      snd
        (apply_limits [limit] values exact) = false.
  Proof.
    intros values exact limit Htruncated. simpl.
    apply Nat.leb_gt in Htruncated.
    rewrite Htruncated, Bool.andb_false_r. reflexivity.
  Qed.

  Lemma nontruncating_bound_preserves_values :
    forall values exact limit,
      length values <= limit ->
      apply_limits [limit] values exact = (values, exact).
  Proof.
    intros values exact limit Hfits. simpl.
    rewrite firstn_all2 by exact Hfits.
    apply Nat.leb_le in Hfits. rewrite Hfits, Bool.andb_true_r.
    reflexivity.
  Qed.

End BoundedDomain.

Section RepeatedFormulaMachine.

  Inductive Formula : Type :=
  | FLeaf : Formula
  | FUnary : Formula -> Formula
  | FBinary : Formula -> Formula -> Formula
  | FRepeat : nat -> Formula -> Formula.

  Fixpoint work (formula : Formula) : nat :=
    match formula with
    | FLeaf => 1
    | FUnary child => 2 + work child
    | FBinary lhs rhs => 3 + work lhs + work rhs
    | FRepeat count body => 1 + count * (work body + 1)
    end.

  Inductive Frame : Type :=
  | KUnary : Frame
  | KLeft : Formula -> Frame
  | KRight : Frame
  | KRepeat : Formula -> nat -> Frame.

  Inductive State : Type :=
  | Down : Formula -> list Frame -> State
  | Up : list Frame -> State.

  Definition step (state : State) : option State :=
    match state with
    | Down FLeaf frames => Some (Up frames)
    | Down (FUnary child) frames =>
        Some (Down child (KUnary :: frames))
    | Down (FBinary lhs rhs) frames =>
        Some (Down lhs (KLeft rhs :: frames))
    | Down (FRepeat 0 _) frames => Some (Up frames)
    | Down (FRepeat (S remaining) body) frames =>
        Some (Down body (KRepeat body remaining :: frames))
    | Up [] => None
    | Up (KUnary :: rest) => Some (Up rest)
    | Up (KLeft rhs :: rest) => Some (Down rhs (KRight :: rest))
    | Up (KRight :: rest) => Some (Up rest)
    | Up (KRepeat _ 0 :: rest) => Some (Up rest)
    | Up (KRepeat body (S remaining) :: rest) =>
        Some (Down body (KRepeat body remaining :: rest))
    end.

  Definition frame_potential (frame : Frame) : nat :=
    match frame with
    | KUnary | KRight => 1
    | KLeft rhs => work rhs + 2
    | KRepeat body remaining => 1 + remaining * (work body + 1)
    end.

  Definition continuation_potential (frames : list Frame) : nat :=
    fold_right (fun frame rest => frame_potential frame + rest) 0 frames.

  Definition potential (state : State) : nat :=
    match state with
    | Down formula frames => work formula + continuation_potential frames
    | Up frames => continuation_potential frames
    end.

  Theorem step_decreases_potential :
    forall state next,
      step state = Some next -> potential next < potential state.
  Proof.
    intros state next Hstep.
    destruct state as [formula frames | frames].
    - destruct formula as [| child | lhs rhs | count body]; simpl in Hstep.
      + inversion Hstep; subst; simpl; lia.
      + inversion Hstep; subst; simpl; lia.
      + inversion Hstep; subst; simpl; lia.
      + destruct count; inversion Hstep; subst; simpl; nia.
    - destruct frames as [| frame rest]; [discriminate |].
      destruct frame as [| rhs | | body remaining]; simpl in Hstep.
      + inversion Hstep; subst; simpl; lia.
      + inversion Hstep; subst; simpl; lia.
      + inversion Hstep; subst; simpl; lia.
      + destruct remaining; inversion Hstep; subst; simpl; nia.
  Qed.

  Definition transition (next state : State) : Prop :=
    step state = Some next.

  Theorem transition_well_founded : well_founded transition.
  Proof.
    apply (well_founded_lt_compat State potential).
    intros next state Htransition.
    now apply step_decreases_potential in Htransition.
  Qed.

  Definition terminal (state : State) : bool :=
    match state with
    | Up [] => true
    | _ => false
    end.

  Lemma nonterminal_steps :
    forall state,
      terminal state = false -> exists next, step state = Some next.
  Proof.
    intros state Hterminal.
    destruct state as [formula frames | frames].
    - destruct formula as [| child | lhs rhs | count body].
      + eexists. reflexivity.
      + eexists. reflexivity.
      + eexists. reflexivity.
      + destruct count; eexists; reflexivity.
    - destruct frames as [| frame rest].
      + discriminate.
      + destruct frame as [| rhs | | body remaining].
        * eexists. reflexivity.
        * eexists. reflexivity.
        * eexists. reflexivity.
        * destruct remaining; eexists; reflexivity.
  Qed.

  Inductive ReachesN : nat -> State -> State -> Prop :=
  | ReachesZero : forall state, ReachesN 0 state state
  | ReachesStep : forall count state next final,
      step state = Some next ->
      ReachesN count next final ->
      ReachesN (S count) state final.

  Lemma reaches_consumes_potential :
    forall count state final,
      ReachesN count state final ->
      count + potential final <= potential state.
  Proof.
    intros count state final Hreach. induction Hreach.
    - simpl. lia.
    - apply step_decreases_potential in H. simpl. lia.
  Qed.

  Theorem reaches_terminal :
    forall state,
      exists count final,
        ReachesN count state final /\ terminal final = true.
  Proof.
    refine
      (well_founded_induction_type
        transition_well_founded
        (fun state => exists count final,
          ReachesN count state final /\ terminal final = true)
        _).
    intros state IH.
    destruct (terminal state) eqn:Hterminal.
    - exists 0, state. split; [constructor | exact Hterminal].
    - destruct (nonterminal_steps state Hterminal) as [next Hstep].
      destruct (IH next Hstep) as [count [final [Hreach Hfinal]]].
      exists (S count), final. split.
      + now apply ReachesStep with next.
      + exact Hfinal.
  Qed.

  Corollary initial_transition_bound :
    forall formula count final,
      ReachesN count (Down formula []) final ->
      count <= work formula.
  Proof.
    intros formula count final Hreach.
    pose proof (reaches_consumes_potential count _ _ Hreach).
    simpl in H. lia.
  Qed.

End RepeatedFormulaMachine.
