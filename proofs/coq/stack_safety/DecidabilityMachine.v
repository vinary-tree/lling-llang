(** * Stack-safe decidability classification

    This development models the exact control flow of
    [classify_decidability_inner].  [Down] carries the inherited boundedness
    bit; heap-resident frames encode unfinished unary and binary contexts.
    An unbounded infinite quantifier transitions directly to tier 4 without
    inspecting its body, while [EBounded] forces bounded mode for every
    descendant.  Thus the machine preserves both the result and the original
    short-circuit behavior.

    Tiers are represented by their ordered ranks 0 through 3.  The executable
    Rust enum supplies the corresponding names.  The verified transition
    measure gives at most three machine transitions per source node and makes
    native call-stack depth independent of expression depth.
*)

From Stdlib Require Import Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
From Stdlib Require Import Wf_nat.
Import ListNotations.

Inductive Expr : Type :=
| ELeaf : Expr
| ERelation : Expr
| ENot : Expr -> Expr
| EBinary : Expr -> Expr -> Expr
| EFinite : Expr -> Expr
| EInfinite : Expr -> Expr
| EBounded : Expr -> Expr.

Fixpoint classify (expression : Expr) (bounded : bool) : nat :=
  match expression with
  | ELeaf => 0
  | ERelation => 1
  | ENot child => classify child bounded
  | EBinary lhs rhs => Nat.max (classify lhs bounded) (classify rhs bounded)
  | EFinite body => classify body bounded
  | EInfinite body =>
      if bounded then Nat.max (classify body true) 2 else 3
  | EBounded body => classify body true
  end.

Fixpoint node_count (expression : Expr) : nat :=
  match expression with
  | ELeaf | ERelation => 1
  | ENot child | EFinite child | EInfinite child | EBounded child =>
      1 + node_count child
  | EBinary lhs rhs => 1 + node_count lhs + node_count rhs
  end.

Inductive Frame : Type :=
| KPass : Frame
| KFloor : nat -> Frame
| KLeft : Expr -> bool -> Frame
| KRight : nat -> Frame.

Inductive State : Type :=
| Down : Expr -> bool -> list Frame -> State
| Up : nat -> list Frame -> State.

Fixpoint continue (frames : list Frame) (tier : nat) : nat :=
  match frames with
  | [] => tier
  | KPass :: rest => continue rest tier
  | KFloor floor :: rest => continue rest (Nat.max tier floor)
  | KLeft rhs bounded :: rest =>
      continue rest (Nat.max tier (classify rhs bounded))
  | KRight left_tier :: rest => continue rest (Nat.max left_tier tier)
  end.

Definition meaning (state : State) : nat :=
  match state with
  | Down expression bounded frames =>
      continue frames (classify expression bounded)
  | Up tier frames => continue frames tier
  end.

Definition step (state : State) : option State :=
  match state with
  | Down expression bounded frames =>
      match expression with
      | ELeaf => Some (Up 0 frames)
      | ERelation => Some (Up 1 frames)
      | ENot child => Some (Down child bounded (KPass :: frames))
      | EBinary lhs rhs =>
          Some (Down lhs bounded (KLeft rhs bounded :: frames))
      | EFinite body => Some (Down body bounded (KPass :: frames))
      | EInfinite body =>
          if bounded
          then Some (Down body true (KFloor 2 :: frames))
          else Some (Up 3 frames)
      | EBounded body => Some (Down body true (KPass :: frames))
      end
  | Up _ [] => None
  | Up tier (KPass :: rest) => Some (Up tier rest)
  | Up tier (KFloor floor :: rest) =>
      Some (Up (Nat.max tier floor) rest)
  | Up tier (KLeft rhs bounded :: rest) =>
      Some (Down rhs bounded (KRight tier :: rest))
  | Up tier (KRight left_tier :: rest) =>
      Some (Up (Nat.max left_tier tier) rest)
  end.

Definition terminal_result (state : State) : option nat :=
  match state with
  | Up tier [] => Some tier
  | _ => None
  end.

Theorem step_preserves_meaning :
  forall state next,
    step state = Some next -> meaning state = meaning next.
Proof.
  intros state next Hstep.
  destruct state as [expression bounded frames | tier frames].
  - destruct expression; simpl in Hstep;
      try (inversion Hstep; subst; reflexivity).
    destruct bounded; inversion Hstep; subst; reflexivity.
  - destruct frames as [| frame rest]; [discriminate |].
    destruct frame; simpl in Hstep; inversion Hstep; subst; reflexivity.
Qed.

Definition frame_potential (frame : Frame) : nat :=
  match frame with
  | KPass | KFloor _ | KRight _ => 1
  | KLeft rhs _ => 3 * node_count rhs + 2
  end.

Definition continuation_potential (frames : list Frame) : nat :=
  fold_right (fun frame rest => frame_potential frame + rest) 0 frames.

Definition potential (state : State) : nat :=
  match state with
  | Down expression _ frames =>
      3 * node_count expression + continuation_potential frames
  | Up _ frames => continuation_potential frames
  end.

Theorem step_decreases_potential :
  forall state next,
    step state = Some next -> potential next < potential state.
Proof.
  intros state next Hstep.
  destruct state as [expression bounded frames | tier frames].
  - destruct expression; simpl in Hstep;
      try (inversion Hstep; subst; simpl; lia).
    destruct bounded; inversion Hstep; subst; simpl; lia.
  - destruct frames as [| frame rest]; [discriminate |].
    destruct frame; simpl in Hstep; inversion Hstep; subst; simpl; lia.
Qed.

Definition transition (next state : State) : Prop := step state = Some next.

Theorem transition_well_founded : well_founded transition.
Proof.
  apply (well_founded_lt_compat State potential).
  intros next state Htransition.
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
  intros count state terminal Hreach. induction Hreach.
  - reflexivity.
  - rewrite (step_preserves_meaning state next H). exact IHHreach.
Qed.

Lemma reaches_consumes_potential :
  forall count state terminal,
    ReachesN count state terminal ->
    count + potential terminal <= potential state.
Proof.
  intros count state terminal Hreach. induction Hreach.
  - simpl. lia.
  - apply step_decreases_potential in H. simpl. lia.
Qed.

Lemma nonterminal_steps :
  forall state,
    terminal_result state = None -> exists next, step state = Some next.
Proof.
  intros state Hterminal.
  destruct state as [expression bounded frames | tier frames].
  - destruct expression; try (eexists; reflexivity).
    destruct bounded; eexists; reflexivity.
  - destruct frames as [| frame rest].
    + discriminate.
    + destruct frame; eexists; reflexivity.
Qed.

Theorem reaches_terminal :
  forall state,
    exists count terminal result,
      ReachesN count state terminal /\ terminal_result terminal = Some result.
Proof.
  refine
    (well_founded_induction_type
      transition_well_founded
      (fun state => exists count terminal result,
        ReachesN count state terminal /\ terminal_result terminal = Some result)
      _).
  intros state IH.
  destruct (terminal_result state) as [result |] eqn:Hterminal.
  - exists 0, state, result. split; [constructor | exact Hterminal].
  - destruct (nonterminal_steps state Hterminal) as [next Hstep].
    destruct (IH next Hstep) as [count [terminal [result [Hreach Hresult]]]].
    exists (S count), terminal, result. split.
    + now apply ReachesStep with next.
    + exact Hresult.
Qed.

Corollary initial_terminal_correct :
  forall expression count terminal result,
    ReachesN count (Down expression false []) terminal ->
    terminal_result terminal = Some result ->
    result = classify expression false.
Proof.
  intros expression count terminal result Hreach Hresult.
  pose proof (reaches_preserves_meaning count _ _ Hreach) as Hmeaning.
  destruct terminal as [terminal_expression terminal_bounded terminal_frames
                       | terminal_tier terminal_frames];
    simpl in Hresult; try discriminate.
  destruct terminal_frames; inversion Hresult; subst; simpl in Hmeaning.
  symmetry. exact Hmeaning.
Qed.

Corollary initial_transition_bound :
  forall expression count terminal,
    ReachesN count (Down expression false []) terminal ->
    count <= 3 * node_count expression.
Proof.
  intros expression count terminal Hreach.
  pose proof (reaches_consumes_potential count _ _ Hreach).
  simpl in H. lia.
Qed.
