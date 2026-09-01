(** * Stack-safe token predicates and ordered pattern alternatives

    Token matching contains two independently recursive control structures.
    [PredicateMachine] verifies the ordered, short-circuit evaluator for nested
    [Any], [All], and [Not] token predicates.  [PatternMachine] verifies the
    first-success backtracking discipline used by nested pattern alternatives:
    a failed alternative restores its entry state, while a successful
    alternative commits its state before the enclosing sequence continues.

    Both operational semantics are defunctionalized pushdown automata.  Every
    continuation is a typed heap-resident frame, every transition strictly
    decreases an explicit work potential, and terminal results refine the
    recursive reference equations.  The transition bounds include all
    scheduled children/alternatives and therefore expose the exact linear
    control overhead implemented in Rust.  No axioms, admissions, parameters,
    or proof escapes are used.
*)

From Stdlib Require Import Arith.
From Stdlib Require Import Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
From Stdlib Require Import Wf_nat.
Import ListNotations.

Section PredicateMachine.

  Inductive Pred : Type :=
  | PLeaf : bool -> Pred
  | PNot : Pred -> Pred
  | PAny : Forest -> Pred
  | PAll : Forest -> Pred
  with Forest : Type :=
  | FNil : Forest
  | FCons : Pred -> Forest -> Forest.

  Scheme Pred_ind' := Induction for Pred Sort Prop
  with Forest_ind' := Induction for Forest Sort Prop.

  Fixpoint eval_pred (predicate : Pred) : bool :=
    match predicate with
    | PLeaf value => value
    | PNot inner => negb (eval_pred inner)
    | PAny children => eval_any children
    | PAll children => eval_all children
    end
  with eval_any (children : Forest) : bool :=
    match children with
    | FNil => false
    | FCons child rest => orb (eval_pred child) (eval_any rest)
    end
  with eval_all (children : Forest) : bool :=
    match children with
    | FNil => true
    | FCons child rest => andb (eval_pred child) (eval_all rest)
    end.

  Fixpoint work (predicate : Pred) : nat :=
    match predicate with
    | PLeaf _ => 1
    | PNot inner => 2 + work inner
    | PAny children | PAll children => 1 + forest_work children
    end
  with forest_work (children : Forest) : nat :=
    match children with
    | FNil => 0
    | FCons child rest => work child + 1 + forest_work rest
    end.

  Inductive Frame : Type :=
  | KNot : Frame
  | KAny : Forest -> Frame
  | KAll : Forest -> Frame.

  Inductive State : Type :=
  | Down : Pred -> list Frame -> State
  | Up : bool -> list Frame -> State.

  Fixpoint continue (frames : list Frame) (value : bool) : bool :=
    match frames with
    | [] => value
    | KNot :: rest => continue rest (negb value)
    | KAny remaining :: rest =>
        if value then continue rest true
        else continue rest (eval_any remaining)
    | KAll remaining :: rest =>
        if value then continue rest (eval_all remaining)
        else continue rest false
    end.

  Definition meaning (state : State) : bool :=
    match state with
    | Down predicate frames => continue frames (eval_pred predicate)
    | Up value frames => continue frames value
    end.

  Definition step (state : State) : option State :=
    match state with
    | Down (PLeaf value) frames => Some (Up value frames)
    | Down (PNot inner) frames => Some (Down inner (KNot :: frames))
    | Down (PAny FNil) frames => Some (Up false frames)
    | Down (PAny (FCons child rest)) frames =>
        Some (Down child (KAny rest :: frames))
    | Down (PAll FNil) frames => Some (Up true frames)
    | Down (PAll (FCons child rest)) frames =>
        Some (Down child (KAll rest :: frames))
    | Up _ [] => None
    | Up value (KNot :: rest) => Some (Up (negb value) rest)
    | Up true (KAny _ :: rest) => Some (Up true rest)
    | Up false (KAny FNil :: rest) => Some (Up false rest)
    | Up false (KAny (FCons child tail) :: rest) =>
        Some (Down child (KAny tail :: rest))
    | Up false (KAll _ :: rest) => Some (Up false rest)
    | Up true (KAll FNil :: rest) => Some (Up true rest)
    | Up true (KAll (FCons child tail) :: rest) =>
        Some (Down child (KAll tail :: rest))
    end.

  Theorem step_preserves_meaning :
    forall state next,
      step state = Some next -> meaning state = meaning next.
  Proof.
    intros state next Hstep.
    destruct state as [predicate frames | value frames].
    - destruct predicate as [value | inner | children | children];
        simpl in Hstep.
      + inversion Hstep; subst; reflexivity.
      + inversion Hstep; subst; reflexivity.
      + destruct children as [| child rest].
        * inversion Hstep; subst; reflexivity.
        * inversion Hstep; subst. simpl.
          destruct (eval_pred child); reflexivity.
      + destruct children as [| child rest].
        * inversion Hstep; subst; reflexivity.
        * inversion Hstep; subst. simpl.
          destruct (eval_pred child); reflexivity.
    - destruct frames as [| frame rest].
      + destruct value; simpl in Hstep; discriminate Hstep.
      + destruct frame as [| remaining | remaining]; simpl in Hstep.
        * destruct value; inversion Hstep; subst; reflexivity.
        * destruct value; [inversion Hstep; subst; reflexivity |].
          destruct remaining as [| child tail].
          -- inversion Hstep; subst; reflexivity.
          -- inversion Hstep; subst. simpl.
             destruct (eval_pred child); reflexivity.
        * destruct value; [| inversion Hstep; subst; reflexivity].
          destruct remaining as [| child tail].
          -- inversion Hstep; subst; reflexivity.
          -- inversion Hstep; subst. simpl.
             destruct (eval_pred child); reflexivity.
  Qed.

  Definition frame_potential (frame : Frame) : nat :=
    match frame with
    | KNot => 1
    | KAny remaining | KAll remaining => 1 + forest_work remaining
    end.

  Definition continuation_potential (frames : list Frame) : nat :=
    fold_right (fun frame rest => frame_potential frame + rest) 0 frames.

  Definition potential (state : State) : nat :=
    match state with
    | Down predicate frames => work predicate + continuation_potential frames
    | Up _ frames => continuation_potential frames
    end.

  Theorem step_decreases_potential :
    forall state next,
      step state = Some next -> potential next < potential state.
  Proof.
    intros state next Hstep.
    destruct state as [predicate frames | value frames].
    - destruct predicate as [value | inner | children | children];
        simpl in Hstep.
      + inversion Hstep; subst; simpl; lia.
      + inversion Hstep; subst; simpl; lia.
      + destruct children; inversion Hstep; subst; simpl; lia.
      + destruct children; inversion Hstep; subst; simpl; lia.
    - destruct frames as [| frame rest].
      + destruct value; simpl in Hstep; discriminate Hstep.
      + destruct frame as [| remaining | remaining]; simpl in Hstep.
        * destruct value; inversion Hstep; subst; simpl; lia.
        * destruct value; [inversion Hstep; subst; simpl; lia |].
          destruct remaining; inversion Hstep; subst; simpl; lia.
        * destruct value; [| inversion Hstep; subst; simpl; lia].
          destruct remaining; inversion Hstep; subst; simpl; lia.
  Qed.

  Definition transition (next state : State) : Prop := step state = Some next.

  Theorem transition_well_founded : well_founded transition.
  Proof.
    apply (well_founded_lt_compat State potential).
    intros next state Htransition.
    now apply step_decreases_potential in Htransition.
  Qed.

  Definition terminal_result (state : State) : option bool :=
    match state with
    | Up value [] => Some value
    | _ => None
    end.

  Lemma nonterminal_steps :
    forall state,
      terminal_result state = None -> exists next, step state = Some next.
  Proof.
    intros state Hterminal.
    destruct state as [predicate frames | value frames].
    - destruct predicate as [value | inner | children | children].
      + eexists. reflexivity.
      + eexists. reflexivity.
      + destruct children; eexists; reflexivity.
      + destruct children; eexists; reflexivity.
    - destruct frames as [| frame rest].
      + discriminate.
      + destruct frame as [| remaining | remaining].
        * destruct value; eexists; reflexivity.
        * destruct value; [| destruct remaining]; eexists; reflexivity.
        * destruct value; [destruct remaining |]; eexists; reflexivity.
  Qed.

  Inductive ReachesN : nat -> State -> State -> Prop :=
  | ReachesZero : forall state, ReachesN 0 state state
  | ReachesStep : forall count state next final,
      step state = Some next ->
      ReachesN count next final ->
      ReachesN (S count) state final.

  Lemma reaches_preserves_meaning :
    forall count state final,
      ReachesN count state final -> meaning state = meaning final.
  Proof.
    intros count state final Hreach. induction Hreach.
    - reflexivity.
    - rewrite (step_preserves_meaning state next H). exact IHHreach.
  Qed.

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
      exists count final result,
        ReachesN count state final /\ terminal_result final = Some result.
  Proof.
    refine
      (well_founded_induction_type
        transition_well_founded
        (fun state => exists count final result,
          ReachesN count state final /\ terminal_result final = Some result)
        _).
    intros state IH.
    destruct (terminal_result state) as [result |] eqn:Hterminal.
    - exists 0, state, result. split; [constructor | exact Hterminal].
    - destruct (nonterminal_steps state Hterminal) as [next Hstep].
      destruct (IH next Hstep) as [count [final [result [Hreach Hresult]]]].
      exists (S count), final, result. split.
      + now apply ReachesStep with next.
      + exact Hresult.
  Qed.

  Corollary initial_terminal_correct :
    forall predicate count final result,
      ReachesN count (Down predicate []) final ->
      terminal_result final = Some result ->
      result = eval_pred predicate.
  Proof.
    intros predicate count final result Hreach Hresult.
    pose proof (reaches_preserves_meaning count _ _ Hreach) as Hmeaning.
    destruct final as [final_predicate final_frames | final_value final_frames];
      simpl in Hresult; try discriminate.
    destruct final_frames; inversion Hresult; subst; simpl in Hmeaning.
    symmetry. exact Hmeaning.
  Qed.

  Corollary initial_transition_bound :
    forall predicate count final,
      ReachesN count (Down predicate []) final -> count <= work predicate.
  Proof.
    intros predicate count final Hreach.
    pose proof (reaches_consumes_potential count _ _ Hreach).
    simpl in H. lia.
  Qed.

End PredicateMachine.

Module OrderedPatternMachine.

Section PatternMachine.

  Context {Primitive MatchState : Type}.
  Variable primitive_sem : Primitive -> MatchState -> option MatchState.

  Inductive Pattern : Type :=
  | Done : Pattern
  | PrimitiveStep : Primitive -> Pattern
  | Sequence : Pattern -> Pattern -> Pattern
  | Alternative : Pattern -> Pattern -> Pattern.

  Definition bind
      (result : option MatchState)
      (next : MatchState -> option MatchState) : option MatchState :=
    match result with
    | Some state => next state
    | None => None
    end.

  Fixpoint eval (pattern : Pattern) (state : MatchState)
      : option MatchState :=
    match pattern with
    | Done => Some state
    | PrimitiveStep primitive => primitive_sem primitive state
    | Sequence lhs rhs => bind (eval lhs state) (eval rhs)
    | Alternative preferred fallback =>
        match eval preferred state with
        | Some preferred_state => Some preferred_state
        | None => eval fallback state
        end
    end.

  Fixpoint work (pattern : Pattern) : nat :=
    match pattern with
    | Done | PrimitiveStep _ => 1
    | Sequence lhs rhs | Alternative lhs rhs =>
        2 + work lhs + work rhs
    end.

  Inductive Frame : Type :=
  | KSequence : Pattern -> Frame
  | KAlternative : Pattern -> MatchState -> Frame.

  Inductive Machine : Type :=
  | Down : Pattern -> MatchState -> list Frame -> Machine
  | Up : option MatchState -> list Frame -> Machine.

  Fixpoint continue
      (frames : list Frame) (result : option MatchState) : option MatchState :=
    match frames with
    | [] => result
    | KSequence rhs :: rest =>
        match result with
        | Some state => continue rest (eval rhs state)
        | None => continue rest None
        end
    | KAlternative fallback original :: rest =>
        match result with
        | Some state => continue rest (Some state)
        | None => continue rest (eval fallback original)
        end
    end.

  Definition meaning (machine : Machine) : option MatchState :=
    match machine with
    | Down pattern state frames => continue frames (eval pattern state)
    | Up result frames => continue frames result
    end.

  Definition step (machine : Machine) : option Machine :=
    match machine with
    | Down Done state frames => Some (Up (Some state) frames)
    | Down (PrimitiveStep primitive) state frames =>
        Some (Up (primitive_sem primitive state) frames)
    | Down (Sequence lhs rhs) state frames =>
        Some (Down lhs state (KSequence rhs :: frames))
    | Down (Alternative preferred fallback) state frames =>
        Some (Down preferred state (KAlternative fallback state :: frames))
    | Up _ [] => None
    | Up None (KSequence _ :: rest) => Some (Up None rest)
    | Up (Some state) (KSequence rhs :: rest) =>
        Some (Down rhs state rest)
    | Up (Some state) (KAlternative _ _ :: rest) =>
        Some (Up (Some state) rest)
    | Up None (KAlternative fallback original :: rest) =>
        Some (Down fallback original rest)
    end.

  Theorem step_preserves_meaning :
    forall machine next,
      step machine = Some next -> meaning machine = meaning next.
  Proof.
    intros machine next Hstep.
    destruct machine as [pattern state frames | result frames].
    - destruct pattern as [| primitive | lhs rhs | preferred fallback]; simpl in Hstep.
      + inversion Hstep; subst; reflexivity.
      + inversion Hstep; subst; reflexivity.
      + inversion Hstep; subst. simpl.
        destruct (eval lhs state); reflexivity.
      + inversion Hstep; subst. simpl.
        destruct (eval preferred state); reflexivity.
    - destruct frames as [| frame rest].
      + destruct result; simpl in Hstep; discriminate Hstep.
      + destruct frame as [rhs | fallback original]; destruct result;
          simpl in Hstep; inversion Hstep; subst; reflexivity.
  Qed.

  Definition frame_potential (frame : Frame) : nat :=
    match frame with
    | KSequence rhs | KAlternative rhs _ => work rhs + 1
    end.

  Definition continuation_potential (frames : list Frame) : nat :=
    fold_right (fun frame rest => frame_potential frame + rest) 0 frames.

  Definition potential (machine : Machine) : nat :=
    match machine with
    | Down pattern _ frames => work pattern + continuation_potential frames
    | Up _ frames => continuation_potential frames
    end.

  Theorem step_decreases_potential :
    forall machine next,
      step machine = Some next -> potential next < potential machine.
  Proof.
    intros machine next Hstep.
    destruct machine as [pattern state frames | result frames].
    - destruct pattern; simpl in Hstep; inversion Hstep; subst; simpl; lia.
    - destruct frames as [| frame rest].
      + destruct result; simpl in Hstep; discriminate Hstep.
      + destruct frame as [rhs | fallback original]; destruct result;
          simpl in Hstep; inversion Hstep; subst; simpl; lia.
  Qed.

  Definition transition (next machine : Machine) : Prop :=
    step machine = Some next.

  Theorem transition_well_founded : well_founded transition.
  Proof.
    apply (well_founded_lt_compat Machine potential).
    intros next machine Htransition.
    now apply step_decreases_potential in Htransition.
  Qed.

  Definition terminal_result (machine : Machine) : option (option MatchState) :=
    match machine with
    | Up result [] => Some result
    | _ => None
    end.

  Lemma nonterminal_steps :
    forall machine,
      terminal_result machine = None -> exists next, step machine = Some next.
  Proof.
    intros machine Hterminal.
    destruct machine as [pattern state frames | result frames].
    - destruct pattern; eexists; reflexivity.
    - destruct frames as [| frame rest].
      + discriminate.
      + destruct frame; destruct result; eexists; reflexivity.
  Qed.

  Inductive ReachesN : nat -> Machine -> Machine -> Prop :=
  | ReachesZero : forall machine, ReachesN 0 machine machine
  | ReachesStep : forall count machine next final,
      step machine = Some next ->
      ReachesN count next final ->
      ReachesN (S count) machine final.

  Lemma reaches_preserves_meaning :
    forall count machine final,
      ReachesN count machine final -> meaning machine = meaning final.
  Proof.
    intros count machine final Hreach. induction Hreach.
    - reflexivity.
    - rewrite (step_preserves_meaning machine next H). exact IHHreach.
  Qed.

  Lemma reaches_consumes_potential :
    forall count machine final,
      ReachesN count machine final ->
      count + potential final <= potential machine.
  Proof.
    intros count machine final Hreach. induction Hreach.
    - simpl. lia.
    - apply step_decreases_potential in H. simpl. lia.
  Qed.

  Theorem reaches_terminal :
    forall machine,
      exists count final result,
        ReachesN count machine final /\ terminal_result final = Some result.
  Proof.
    refine
      (well_founded_induction_type
        transition_well_founded
        (fun machine => exists count final result,
          ReachesN count machine final /\ terminal_result final = Some result)
        _).
    intros machine IH.
    destruct (terminal_result machine) as [result |] eqn:Hterminal.
    - exists 0, machine, result. split; [constructor | exact Hterminal].
    - destruct (nonterminal_steps machine Hterminal) as [next Hstep].
      destruct (IH next Hstep) as [count [final [result [Hreach Hresult]]]].
      exists (S count), final, result. split.
      + now apply ReachesStep with next.
      + exact Hresult.
  Qed.

  Corollary initial_terminal_correct :
    forall pattern state count final result,
      ReachesN count (Down pattern state []) final ->
      terminal_result final = Some result ->
      result = eval pattern state.
  Proof.
    intros pattern state count final result Hreach Hresult.
    pose proof (reaches_preserves_meaning count _ _ Hreach) as Hmeaning.
    destruct final as [final_pattern final_state final_frames
                      | final_result final_frames];
      simpl in Hresult; try discriminate.
    destruct final_frames; inversion Hresult; subst; simpl in Hmeaning.
    symmetry. exact Hmeaning.
  Qed.

  Corollary initial_transition_bound :
    forall pattern state count final,
      ReachesN count (Down pattern state []) final -> count <= work pattern.
  Proof.
    intros pattern state count final Hreach.
    pose proof (reaches_consumes_potential count _ _ Hreach).
    simpl in H. lia.
  Qed.

End PatternMachine.

End OrderedPatternMachine.
