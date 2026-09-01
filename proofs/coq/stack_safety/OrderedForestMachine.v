(** * Ordered n-ary forest machine

    Rust recursive syntax frequently stores children in [Vec] rather than in a
    binary pair.  This file verifies the specialized continuation machine for
    that representation.  The source is modeled as a mutually inductive tree
    and forest, so every recursive call is structurally visible to Rocq.

    A [Children] frame stores values already produced in reverse order and the
    unvisited child suffix.  Prepending a completed value is constant time;
    reversing once at the parent boundary reconstructs the original left-to-
    right order.  The machine therefore avoids both native recursion and the
    quadratic repeated-append trap.

    Theorems establish semantic and error-order equivalence to the recursive
    fold, strict progress, termination, an exact successful execution length
    of at most twice the node count, and an explicit-stack bound linear in the
    source size.  The Rust conformance layer strengthens the last bound to
    source height because each frame denotes one unfinished ancestor.
*)

From Stdlib Require Import Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
From Stdlib Require Import Wf_nat.
Import ListNotations.

Section OrderedForestMachine.

  Context {Label Value Error : Type}.
  Variable node_sem : Label -> list Value -> (Value + Error)%type.

  Inductive Tree : Type :=
  | Node : Label -> Forest -> Tree
  with Forest : Type :=
  | FNil : Forest
  | FCons : Tree -> Forest -> Forest.

  Scheme Tree_ind' := Induction for Tree Sort Prop
  with Forest_ind' := Induction for Forest Sort Prop.

  Definition bind {A B : Type}
      (result : (A + Error)%type)
      (next : A -> (B + Error)%type) : (B + Error)%type :=
    match result with
    | inl value => next value
    | inr error => inr error
    end.

  Fixpoint eval_tree (tree : Tree) : (Value + Error)%type :=
    match tree with
    | Node label FNil => node_sem label []
    | Node label (FCons child rest) =>
        bind (eval_tree child)
          (fun value =>
             bind (eval_forest rest)
               (fun values => node_sem label (value :: values)))
    end
  with eval_forest (forest : Forest) : (list Value + Error)%type :=
    match forest with
    | FNil => inl []
    | FCons child rest =>
        bind (eval_tree child)
          (fun value =>
             bind (eval_forest rest)
               (fun values => inl (value :: values)))
    end.

  Fixpoint node_count (tree : Tree) : nat :=
    match tree with
    | Node _ children => 1 + forest_count children
    end
  with forest_count (forest : Forest) : nat :=
    match forest with
    | FNil => 0
    | FCons child rest => node_count child + forest_count rest
    end.

  Fixpoint tree_steps (tree : Tree) : nat :=
    match tree with
    | Node _ FNil => 1
    | Node _ (FCons child rest) =>
        1 + tree_steps child + after_child_steps rest
    end
  with after_child_steps (forest : Forest) : nat :=
    match forest with
    | FNil => 1
    | FCons child rest =>
        1 + tree_steps child + after_child_steps rest
    end.

  Inductive Frame : Type :=
  | Children : Label -> list Value -> Forest -> Frame.

  Inductive State : Type :=
  | Down : Tree -> list Frame -> State
  | Up : (Value + Error)%type -> list Frame -> State.

  Definition resume (frame : Frame) (result : (Value + Error)%type)
      : (Value + Error)%type :=
    match frame with
    | Children label done_rev remaining =>
        bind result
          (fun value =>
             bind (eval_forest remaining)
               (fun remaining_values =>
                  node_sem label
                    (rev done_rev ++ value :: remaining_values)))
    end.

  Fixpoint continue (frames : list Frame) (result : (Value + Error)%type)
      : (Value + Error)%type :=
    match frames with
    | [] => result
    | frame :: rest => continue rest (resume frame result)
    end.

  Definition meaning (state : State) : (Value + Error)%type :=
    match state with
    | Down tree frames => continue frames (eval_tree tree)
    | Up result frames => continue frames result
    end.

  Definition step (state : State) : option State :=
    match state with
    | Down (Node label FNil) frames =>
        Some (Up (node_sem label []) frames)
    | Down (Node label (FCons child remaining)) frames =>
        Some (Down child (Children label [] remaining :: frames))
    | Up (inr _) _ => None
    | Up (inl _) [] => None
    | Up (inl value) (Children label done_rev FNil :: rest) =>
        Some (Up (node_sem label (rev (value :: done_rev))) rest)
    | Up (inl value)
        (Children label done_rev (FCons next remaining) :: rest) =>
        Some
          (Down next
            (Children label (value :: done_rev) remaining :: rest))
    end.

  Definition terminal_result (state : State) : option (Value + Error)%type :=
    match state with
    | Up (inr error) _ => Some (inr error)
    | Up (inl value) [] => Some (inl value)
    | _ => None
    end.

  Lemma continue_error :
    forall frames error,
      continue frames (inr error) = inr error.
  Proof.
    induction frames as [| [label done remaining] rest IH]; intros error; simpl.
    - reflexivity.
    - rewrite IH. reflexivity.
  Qed.

  Lemma continue_initial_child :
    forall frames label child remaining,
      continue frames
        (bind (eval_tree child)
          (fun value =>
             bind (eval_forest remaining)
               (fun values => node_sem label (value :: values)))) =
      continue (Children label [] remaining :: frames) (eval_tree child).
  Proof.
    intros frames label child remaining. simpl.
    destruct (eval_tree child) as [value | error]; simpl.
    - reflexivity.
    - rewrite continue_error. reflexivity.
  Qed.

  Lemma reverse_completed_child :
    forall (done_rev : list Value) value,
      rev (value :: done_rev) = rev done_rev ++ [value].
  Proof. intros. simpl. reflexivity. Qed.

  Lemma continue_next_child :
    forall frames label done_rev value next remaining,
      continue
        (Children label done_rev (FCons next remaining) :: frames)
        (inl value) =
      continue
        (Children label (value :: done_rev) remaining :: frames)
        (eval_tree next).
  Proof.
    intros frames label done_rev value next remaining. simpl.
    destruct (eval_tree next) as [next_value | error]; simpl.
    - destruct (eval_forest remaining) as [remaining_values | remaining_error]; simpl.
      + rewrite <- app_assoc. reflexivity.
      + rewrite continue_error. reflexivity.
    - rewrite continue_error. reflexivity.
  Qed.

  Lemma step_preserves_meaning :
    forall state next,
      step state = Some next -> meaning state = meaning next.
  Proof.
    intros state next Hstep.
    destruct state as [[label children] frames | result frames].
    - destruct children as [| child remaining];
        simpl in Hstep; inversion Hstep; subst; simpl.
      + reflexivity.
      + apply continue_initial_child.
    - destruct result as [value | error].
      + destruct frames as [| [label done_rev remaining] rest].
        * discriminate.
        * destruct remaining as [| next_child next_remaining];
            simpl in Hstep; inversion Hstep; subst; simpl.
          -- reflexivity.
          -- apply continue_next_child.
      + discriminate.
  Qed.

  Definition frame_potential (frame : Frame) : nat :=
    match frame with
    | Children _ _ remaining => after_child_steps remaining
    end.

  Definition continuation_potential (frames : list Frame) : nat :=
    fold_right (fun frame rest => frame_potential frame + rest) 0 frames.

  Definition potential (state : State) : nat :=
    match state with
    | Down tree frames => tree_steps tree + continuation_potential frames
    | Up _ frames => continuation_potential frames
    end.

  Lemma step_decreases_potential :
    forall state next,
      step state = Some next -> potential next < potential state.
  Proof.
    intros state next Hstep.
    destruct state as [[label children] frames | result frames].
    - destruct children; simpl in Hstep; inversion Hstep; simpl; lia.
    - destruct result as [value | error].
      + destruct frames as [| [label done_rev remaining] rest].
        * discriminate.
        * destruct remaining; simpl in Hstep; inversion Hstep; simpl; lia.
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
      terminal_result state = None ->
      exists next, step state = Some next.
  Proof.
    intros state Hterminal.
    destruct state as [[label children] frames | result frames].
    - destruct children; eexists; reflexivity.
    - destruct result as [value | error].
      + destruct frames as [| [label done_rev remaining] rest].
        * discriminate.
        * destruct remaining; eexists; reflexivity.
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
    - exists 0, state, result. split; [constructor | exact Hterminal].
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
    destruct state as [tree frames | outcome frames].
    - discriminate.
    - destruct outcome as [value | error].
      + destruct frames as [| frame rest].
        * inversion Hterminal. reflexivity.
        * discriminate.
      + inversion Hterminal. apply continue_error.
  Qed.

  Definition initial (tree : Tree) : State := Down tree [].

  Theorem initial_terminal_correct :
    forall tree count terminal result,
      ReachesN count (initial tree) terminal ->
      terminal_result terminal = Some result ->
      result = eval_tree tree.
  Proof.
    intros tree count terminal result Hreach Hterminal.
    pose proof (reaches_preserves_meaning count (initial tree) terminal Hreach)
      as Hmeaning.
    pose proof (terminal_result_is_meaning terminal result Hterminal)
      as Hresult.
    rewrite Hresult in Hmeaning.
    unfold initial, meaning, continue in Hmeaning.
    symmetry. exact Hmeaning.
  Qed.

  Lemma tree_steps_node_bound :
    forall tree, tree_steps tree + 1 <= 2 * node_count tree
  with after_child_steps_forest_bound :
    forall forest, after_child_steps forest <= 2 * forest_count forest + 1.
  Proof.
    - intros [label children]. destruct children as [| child rest]; simpl.
      + lia.
      + pose proof (tree_steps_node_bound child).
        pose proof (after_child_steps_forest_bound rest). lia.
    - intros forest. destruct forest as [| child rest]; simpl.
      + lia.
      + pose proof (tree_steps_node_bound child).
        pose proof (after_child_steps_forest_bound rest). lia.
  Qed.

  Theorem initial_transition_bound :
    forall tree count terminal,
      ReachesN count (initial tree) terminal ->
      count <= 2 * node_count tree.
  Proof.
    intros tree count terminal Hreach.
    pose proof (reaches_consumes_potential count (initial tree) terminal Hreach)
      as Hpotential.
    pose proof (tree_steps_node_bound tree) as Hsteps.
    unfold initial, potential, continuation_potential in Hpotential.
    simpl in Hpotential. lia.
  Qed.

  Lemma frame_potential_positive :
    forall frame, 1 <= frame_potential frame.
  Proof.
    intros [label done remaining]. destruct remaining; simpl; lia.
  Qed.

  Lemma frame_count_bounded_by_continuation_potential :
    forall frames, length frames <= continuation_potential frames.
  Proof.
    induction frames as [| frame rest IH]; simpl.
    - lia.
    - pose proof (frame_potential_positive frame). lia.
  Qed.

  Theorem initial_frame_bound :
    forall tree count state,
      ReachesN count (initial tree) state ->
      length
        (match state with
         | Down _ frames => frames
         | Up _ frames => frames
         end) <= 2 * node_count tree.
  Proof.
    intros tree count state Hreach.
    pose proof (reaches_consumes_potential count (initial tree) state Hreach)
      as Hpotential.
    pose proof (tree_steps_node_bound tree) as Hsteps.
    destruct state as [focused frames | result frames]; simpl.
    - pose proof (frame_count_bounded_by_continuation_potential frames).
      unfold initial, potential in Hpotential.
      simpl in Hpotential. lia.
    - pose proof (frame_count_bounded_by_continuation_potential frames).
      unfold initial, potential in Hpotential.
      simpl in Hpotential. lia.
  Qed.

End OrderedForestMachine.
