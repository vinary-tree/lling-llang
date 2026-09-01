(** * Stack-safe typed continuation machine

    This file verifies the common binary-expression machine used to replace
    input-shaped recursive Rust evaluators and transformers.  Operators and
    values are abstract: an instantiation may evaluate a predicate, rebuild a
    normal form, compile an automaton, construct an SMT term, or unify two
    recursive types.

    The recursive [eval] function is the denotational reference.  [step] is the
    defunctionalized operational semantics implemented by a constant-native-
    stack loop.  Its heap-resident [Frame] values are a specialized pushdown
    alphabet.  [KLeft] records an unvisited right operand; [KRight] records the
    completed left value.  Consequently evaluation order and short-circuiting
    are explicit rather than dependent on host-language recursion.

    Main results:

    - [step_preserves_meaning]: one machine transition refines [eval];
    - [step_decreases_potential]: every nonterminal transition strictly
      decreases a natural-valued work measure;
    - [reaches_terminal]: every state reaches a terminal state;
    - [initial_terminal_correct]: a terminal result from an initial state is
      exactly the recursive reference result;
    - [initial_transition_bound]: at most three transitions per source node;
    - [initial_frame_bound]: the explicit continuation stack never exceeds the
      source-tree height.

    The height carried by [Up] and [KRight] is ghost state.  It is erased in the
    Rust implementation and exists solely to state the exact auxiliary-space
    invariant.  This development uses no axioms, admissions, parameters, or
    proof escapes.
*)

From Stdlib Require Import Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
From Stdlib Require Import Wf_nat.
Import ListNotations.

Section ContinuationMachine.

  Context {Atom Unary Binary Value Error : Type}.
  Variable atom_sem : Atom -> Value.
  Variable unary_sem : Unary -> Value -> (Value + Error)%type.
  Variable binary_sem : Binary -> Value -> Value -> (Value + Error)%type.

  Inductive Expr : Type :=
  | EAtom : Atom -> Expr
  | EUnary : Unary -> Expr -> Expr
  | EBinary : Binary -> Expr -> Expr -> Expr.

  Definition Outcome := (Value + Error)%type.

  Definition bind (result : Outcome) (next : Value -> Outcome) : Outcome :=
    match result with
    | inl value => next value
    | inr error => inr error
    end.

  Fixpoint eval (expression : Expr) : Outcome :=
    match expression with
    | EAtom atom => inl (atom_sem atom)
    | EUnary operator child =>
        bind (eval child) (unary_sem operator)
    | EBinary operator lhs rhs =>
        bind (eval lhs)
          (fun left_value =>
             bind (eval rhs) (binary_sem operator left_value))
    end.

  Fixpoint node_count (expression : Expr) : nat :=
    match expression with
    | EAtom _ => 1
    | EUnary _ child => 1 + node_count child
    | EBinary _ lhs rhs =>
        1 + node_count lhs + node_count rhs
    end.

  Fixpoint tree_height (expression : Expr) : nat :=
    match expression with
    | EAtom _ => 1
    | EUnary _ child => 1 + tree_height child
    | EBinary _ lhs rhs =>
        1 + Nat.max (tree_height lhs) (tree_height rhs)
    end.

  Inductive Frame : Type :=
  | KUnary : Unary -> Frame
  | KLeft : Binary -> Expr -> Frame
  | KRight : Binary -> Value -> nat -> Frame.

  Inductive State : Type :=
  | Down : Expr -> list Frame -> State
  | Up : Outcome -> nat -> list Frame -> State.

  Fixpoint continue (frames : list Frame) (result : Outcome) : Outcome :=
    match result with
    | inr error => inr error
    | inl value =>
        match frames with
        | [] => inl value
        | KUnary operator :: rest =>
            continue rest (unary_sem operator value)
        | KLeft operator rhs :: rest =>
            continue rest
              (bind (eval rhs) (binary_sem operator value))
        | KRight operator left_value _ :: rest =>
            continue rest (binary_sem operator left_value value)
        end
    end.

  Definition meaning (state : State) : Outcome :=
    match state with
    | Down expression frames => continue frames (eval expression)
    | Up result _ frames => continue frames result
    end.

  Definition step (state : State) : option State :=
    match state with
    | Down expression frames =>
        match expression with
        | EAtom atom => Some (Up (inl (atom_sem atom)) 1 frames)
        | EUnary operator child =>
            Some (Down child (KUnary operator :: frames))
        | EBinary operator lhs rhs =>
            Some (Down lhs (KLeft operator rhs :: frames))
        end
    | Up (inr _) _ _ => None
    | Up (inl _) _ [] => None
    | Up (inl value) height (KUnary operator :: rest) =>
        Some (Up (unary_sem operator value) (1 + height) rest)
    | Up (inl value) height (KLeft operator rhs :: rest) =>
        Some (Down rhs (KRight operator value height :: rest))
    | Up (inl value) height (KRight operator left_value left_height :: rest) =>
        Some
          (Up (binary_sem operator left_value value)
              (1 + Nat.max left_height height)
              rest)
    end.

  Definition terminal_result (state : State) : option Outcome :=
    match state with
    | Up (inr error) _ _ => Some (inr error)
    | Up (inl value) _ [] => Some (inl value)
    | _ => None
    end.

  Lemma continue_error :
    forall frames error,
      continue frames (inr error) = inr error.
  Proof.
    induction frames as [| frame rest IH]; intros error; simpl.
    - reflexivity.
    - destruct frame; reflexivity.
  Qed.

  Lemma continue_nil :
    forall result, continue [] result = result.
  Proof.
    intros result. destruct result; reflexivity.
  Qed.

  Lemma continue_unary_frame :
    forall frames operator result,
      continue frames (bind result (unary_sem operator)) =
      continue (KUnary operator :: frames) result.
  Proof.
    intros frames operator result.
    destruct result as [value | error]; simpl.
    - reflexivity.
    - apply continue_error.
  Qed.

  Lemma continue_left_frame :
    forall frames operator rhs result,
      continue frames
        (bind result
          (fun left_value =>
             bind (eval rhs) (binary_sem operator left_value))) =
      continue (KLeft operator rhs :: frames) result.
  Proof.
    intros frames operator rhs result.
    destruct result as [value | error]; simpl.
    - reflexivity.
    - apply continue_error.
  Qed.

  Lemma continue_right_frame :
    forall frames operator left_value left_height result,
      continue frames (bind result (binary_sem operator left_value)) =
      continue (KRight operator left_value left_height :: frames) result.
  Proof.
    intros frames operator left_value left_height result.
    destruct result as [value | error]; simpl.
    - reflexivity.
    - apply continue_error.
  Qed.

  Lemma step_preserves_meaning :
    forall state next,
      step state = Some next -> meaning state = meaning next.
  Proof.
    intros state next Hstep.
    destruct state as [expression frames | result height frames].
    - destruct expression as [atom | operator child | operator lhs rhs];
        simpl in Hstep; inversion Hstep; subst; simpl.
      + reflexivity.
      + apply continue_unary_frame.
      + apply continue_left_frame.
    - destruct result as [value | error].
      + destruct frames as [| frame rest].
        * discriminate.
        * destruct frame as [operator | operator rhs | operator left_value left_height];
            simpl in Hstep; inversion Hstep; subst; simpl.
          -- reflexivity.
          -- apply (continue_right_frame rest operator value height (eval rhs)).
          -- reflexivity.
      + discriminate.
  Qed.

  Definition frame_potential (frame : Frame) : nat :=
    match frame with
    | KUnary _ => 1
    | KLeft _ rhs => 3 * node_count rhs + 2
    | KRight _ _ _ => 1
    end.

  Definition continuation_potential (frames : list Frame) : nat :=
    fold_right (fun frame rest => frame_potential frame + rest) 0 frames.

  Definition potential (state : State) : nat :=
    match state with
    | Down expression frames =>
        3 * node_count expression + continuation_potential frames
    | Up _ _ frames => continuation_potential frames
    end.

  Lemma step_decreases_potential :
    forall state next,
      step state = Some next -> potential next < potential state.
  Proof.
    intros state next Hstep.
    destruct state as [expression frames | result height frames].
    - destruct expression; simpl in Hstep; inversion Hstep; simpl; lia.
    - destruct result as [value | error].
      + destruct frames as [| frame rest].
        * discriminate.
        * destruct frame; simpl in Hstep; inversion Hstep; simpl; lia.
      + discriminate.
  Qed.

  Definition transition (next state : State) : Prop :=
    step state = Some next.

  Lemma transition_well_founded : well_founded transition.
  Proof.
    apply (well_founded_lt_compat State potential).
    intros next state Htransition.
    unfold transition in Htransition.
    now apply step_decreases_potential in Htransition.
  Qed.

  Inductive ReachesN : nat -> State -> State -> Prop :=
  | ReachesZero : forall state, ReachesN 0 state state
  | ReachesStep : forall count state next terminal,
      step state = Some next ->
      ReachesN count next terminal ->
      ReachesN (S count) state terminal.

  Lemma reaches_preserves_meaning :
    forall count state terminal,
      ReachesN count state terminal -> meaning state = meaning terminal.
  Proof.
    intros count state terminal Hreach.
    induction Hreach.
    - reflexivity.
    - rewrite (step_preserves_meaning state next H).
      exact IHHreach.
  Qed.

  Lemma reaches_consumes_potential :
    forall count state terminal,
      ReachesN count state terminal ->
      count + potential terminal <= potential state.
  Proof.
    intros count state terminal Hreach.
    induction Hreach.
    - simpl. lia.
    - apply step_decreases_potential in H.
      simpl. lia.
  Qed.

  Lemma nonterminal_steps :
    forall state,
      terminal_result state = None ->
      exists next, step state = Some next.
  Proof.
    intros state Hterminal.
    destruct state as [expression frames | result height frames].
    - destruct expression; eexists; reflexivity.
    - destruct result as [value | error].
      + destruct frames as [| frame rest].
        * discriminate.
        * destruct frame; eexists; reflexivity.
      + discriminate.
  Qed.

  Theorem reaches_terminal :
    forall state,
      exists count terminal result,
        ReachesN count state terminal /\
        terminal_result terminal = Some result.
  Proof.
    refine
      (well_founded_induction_type
         transition_well_founded
         (fun state =>
            exists count terminal result,
              ReachesN count state terminal /\
              terminal_result terminal = Some result)
         _).
    intros state IH.
    destruct (terminal_result state) as [result |] eqn:Hterminal.
    - exists 0, state, result. split.
      + constructor.
      + exact Hterminal.
    - destruct (nonterminal_steps state Hterminal) as [next Hstep].
      destruct (IH next Hstep) as [count [terminal [result [Hreach Hresult]]]].
      exists (S count), terminal, result. split.
      + econstructor; eauto.
      + exact Hresult.
  Qed.

  Lemma terminal_result_is_meaning :
    forall state result,
      terminal_result state = Some result -> meaning state = result.
  Proof.
    intros state result Hterminal.
    destruct state as [expression frames | outcome height frames].
    - discriminate.
    - destruct outcome as [value | error].
      + destruct frames as [| frame rest].
        * inversion Hterminal. reflexivity.
        * discriminate.
      + inversion Hterminal. apply continue_error.
  Qed.

  Definition initial (expression : Expr) : State := Down expression [].

  Theorem initial_terminal_correct :
    forall expression count terminal result,
      ReachesN count (initial expression) terminal ->
      terminal_result terminal = Some result ->
      result = eval expression.
  Proof.
    intros expression count terminal result Hreach Hterminal.
    pose proof (reaches_preserves_meaning count (initial expression) terminal Hreach)
      as Hmeaning.
    pose proof (terminal_result_is_meaning terminal result Hterminal)
      as Hresult.
    rewrite Hresult in Hmeaning.
    unfold initial, meaning in Hmeaning.
    rewrite continue_nil in Hmeaning.
    symmetry. exact Hmeaning.
  Qed.

  Theorem initial_transition_bound :
    forall expression count terminal,
      ReachesN count (initial expression) terminal ->
      count <= 3 * node_count expression.
  Proof.
    intros expression count terminal Hreach.
    pose proof (reaches_consumes_potential count (initial expression) terminal Hreach)
      as Hbound.
    unfold initial, potential, continuation_potential in Hbound. simpl in Hbound.
    lia.
  Qed.

  Fixpoint continuation_height (frames : list Frame) (focus_height : nat) : nat :=
    match frames with
    | [] => focus_height
    | KUnary _ :: rest =>
        continuation_height rest (1 + focus_height)
    | KLeft _ rhs :: rest =>
        continuation_height rest
          (1 + Nat.max focus_height (tree_height rhs))
    | KRight _ _ left_height :: rest =>
        continuation_height rest
          (1 + Nat.max left_height focus_height)
    end.

  Definition state_height (state : State) : nat :=
    match state with
    | Down expression frames =>
        continuation_height frames (tree_height expression)
    | Up _ height frames => continuation_height frames height
    end.

  Definition frame_count (state : State) : nat :=
    match state with
    | Down _ frames => length frames
    | Up _ _ frames => length frames
    end.

  Lemma step_preserves_height :
    forall state next,
      step state = Some next -> state_height state = state_height next.
  Proof.
    intros state next Hstep.
    destruct state as [expression frames | result height frames].
    - destruct expression; simpl in Hstep; inversion Hstep; reflexivity.
    - destruct result as [value | error].
      + destruct frames as [| frame rest].
        * discriminate.
        * destruct frame; simpl in Hstep; inversion Hstep; reflexivity.
      + discriminate.
  Qed.

  Lemma reaches_preserves_height :
    forall count state terminal,
      ReachesN count state terminal ->
      state_height state = state_height terminal.
  Proof.
    intros count state terminal Hreach.
    induction Hreach.
    - reflexivity.
    - rewrite (step_preserves_height state next H).
      exact IHHreach.
  Qed.

  Lemma frames_plus_focus_bounded_by_continuation_height :
    forall frames focus_height,
      length frames + focus_height <= continuation_height frames focus_height.
  Proof.
    induction frames as [| frame rest IH]; intros focus_height; simpl.
    - lia.
    - destruct frame; simpl.
      + specialize (IH (1 + focus_height)).
        replace (1 + focus_height) with (S focus_height) in IH by lia.
        rewrite Nat.add_succ_r in IH.
        exact IH.
      + specialize (IH (1 + Nat.max focus_height (tree_height e))).
        eapply Nat.le_trans; [| exact IH].
        pose proof (Nat.le_max_l focus_height (tree_height e)). lia.
      + specialize (IH (1 + Nat.max n focus_height)).
        eapply Nat.le_trans; [| exact IH].
        pose proof (Nat.le_max_r n focus_height). lia.
  Qed.

  Lemma frames_bounded_by_continuation_height :
    forall frames focus_height,
      length frames <= continuation_height frames focus_height.
  Proof.
    intros frames focus_height.
    pose proof
      (frames_plus_focus_bounded_by_continuation_height frames focus_height).
    lia.
  Qed.

  Lemma frame_count_bounded_by_state_height :
    forall state, frame_count state <= state_height state.
  Proof.
    intros state. destruct state; simpl; apply frames_bounded_by_continuation_height.
  Qed.

  Theorem initial_frame_bound :
    forall expression count state,
      ReachesN count (initial expression) state ->
      frame_count state <= tree_height expression.
  Proof.
    intros expression count state Hreach.
    pose proof (frame_count_bounded_by_state_height state) as Hframes.
    pose proof (reaches_preserves_height count (initial expression) state Hreach)
      as Hheight.
    rewrite <- Hheight in Hframes.
    unfold initial, state_height, continuation_height in Hframes.
    simpl in Hframes.
    exact Hframes.
  Qed.

End ContinuationMachine.
