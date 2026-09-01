(** * Stack-safe ordered syntax-tree machines

    [SyntaxNode] is an ordered, unbounded-arity tree.  Its observable API uses
    three related traversals: ordered preorder scans, a text projection that
    prunes the children of text-bearing nodes, and structural equality over two
    trees.  Native recursion is not part of their semantics.  This development
    gives each traversal an explicit heap worklist and proves refinement against
    the recursive equations before the Rust implementation is changed.

    The postorder reconstruction used by clone and borrowed-to-owned conversion
    is the [OrderedForestMachine] proved in the neighboring development.  The
    preorder machine below covers error collection, kind search, node counting,
    debugging event order, and ownership teardown.  The text machine preserves
    the exact pruning and left-to-right concatenation order of [get_text].  The
    pair machine preserves structural equality, including child arity and order.

    Every nonterminal transition strictly decreases the number of source nodes
    still represented by the worklist.  Consequently all three machines
    terminate, use input-depth-independent native stack, perform linear work,
    and require heap space proportional to the explicit frontier.
*)

From Stdlib Require Import Arith.
From Stdlib Require Import Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
From Stdlib Require Import Wf_nat.
Import ListNotations.

Section SyntaxTreeMachine.

  Context {Payload Text : Type}.
  Variable same_payload : Payload -> Payload -> bool.
  Variable same_text : Text -> Text -> bool.

  Inductive Tree : Type :=
  | Node : Payload -> option Text -> Forest -> Tree
  with Forest : Type :=
  | FNil : Forest
  | FCons : Tree -> Forest -> Forest.

  Scheme Tree_ind' := Induction for Tree Sort Prop
  with Forest_ind' := Induction for Forest Sort Prop.

  Fixpoint preorder (tree : Tree) : list Payload :=
    match tree with
    | Node payload _ children => payload :: preorder_forest children
    end
  with preorder_forest (forest : Forest) : list Payload :=
    match forest with
    | FNil => []
    | FCons child rest => preorder child ++ preorder_forest rest
    end.

  Fixpoint visible_text (tree : Tree) : list Text :=
    match tree with
    | Node _ (Some text) _ => [text]
    | Node _ None children => visible_text_forest children
    end
  with visible_text_forest (forest : Forest) : list Text :=
    match forest with
    | FNil => []
    | FCons child rest => visible_text child ++ visible_text_forest rest
    end.

  Fixpoint node_count (tree : Tree) : nat :=
    match tree with
    | Node _ _ children => 1 + forest_count children
    end
  with forest_count (forest : Forest) : nat :=
    match forest with
    | FNil => 0
    | FCons child rest => node_count child + forest_count rest
    end.

  Fixpoint forest_nodes (forest : Forest) : list Tree :=
    match forest with
    | FNil => []
    | FCons child rest => child :: forest_nodes rest
    end.

  Fixpoint pending_preorder (pending : list Tree) : list Payload :=
    match pending with
    | [] => []
    | tree :: rest => preorder tree ++ pending_preorder rest
    end.

  Fixpoint pending_text (pending : list Tree) : list Text :=
    match pending with
    | [] => []
    | tree :: rest => visible_text tree ++ pending_text rest
    end.

  Fixpoint pending_count (pending : list Tree) : nat :=
    match pending with
    | [] => 0
    | tree :: rest => node_count tree + pending_count rest
    end.

  Lemma pending_preorder_app :
    forall left right,
      pending_preorder (left ++ right) =
      pending_preorder left ++ pending_preorder right.
  Proof.
    induction left as [| tree rest IH]; intros right; simpl.
    - reflexivity.
    - rewrite IH, app_assoc. reflexivity.
  Qed.

  Lemma pending_text_app :
    forall left right,
      pending_text (left ++ right) =
      pending_text left ++ pending_text right.
  Proof.
    induction left as [| tree rest IH]; intros right; simpl.
    - reflexivity.
    - rewrite IH, app_assoc. reflexivity.
  Qed.

  Lemma pending_count_app :
    forall left right,
      pending_count (left ++ right) =
      pending_count left + pending_count right.
  Proof.
    induction left as [| tree rest IH]; intros right; simpl.
    - reflexivity.
    - rewrite IH. lia.
  Qed.

  Lemma forest_nodes_preorder :
    forall forest,
      pending_preorder (forest_nodes forest) = preorder_forest forest.
  Proof.
    induction forest as [| child rest IH]; simpl.
    - reflexivity.
    - now rewrite IH.
  Qed.

  Lemma forest_nodes_text :
    forall forest,
      pending_text (forest_nodes forest) = visible_text_forest forest.
  Proof.
    induction forest as [| child rest IH]; simpl.
    - reflexivity.
    - now rewrite IH.
  Qed.

  Lemma forest_nodes_count :
    forall forest,
      pending_count (forest_nodes forest) = forest_count forest.
  Proof.
    induction forest as [| child rest IH]; simpl; lia.
  Qed.

  Record WalkState : Type := {
    walk_pending : list Tree;
    walk_emitted_rev : list Payload
  }.

  Definition walk_meaning (state : WalkState) : list Payload :=
    rev (walk_emitted_rev state) ++ pending_preorder (walk_pending state).

  Definition walk_step (state : WalkState) : option WalkState :=
    match walk_pending state with
    | [] => None
    | Node payload _ children :: rest =>
        Some
          {| walk_pending := forest_nodes children ++ rest;
             walk_emitted_rev := payload :: walk_emitted_rev state |}
    end.

  Definition walk_terminal (state : WalkState) : option (list Payload) :=
    match walk_pending state with
    | [] => Some (rev (walk_emitted_rev state))
    | _ => None
    end.

  Theorem walk_step_preserves_recursive_preorder :
    forall state next,
      walk_step state = Some next -> walk_meaning next = walk_meaning state.
  Proof.
    intros [pending emitted] next Hstep.
    destruct pending as [| [payload text children] rest]; simpl in Hstep.
    - discriminate.
    - inversion Hstep; subst; clear Hstep. unfold walk_meaning; simpl.
      rewrite pending_preorder_app, forest_nodes_preorder.
      repeat rewrite <- app_assoc. reflexivity.
  Qed.

  Definition walk_potential (state : WalkState) : nat :=
    pending_count (walk_pending state).

  Theorem walk_step_consumes_one_node :
    forall state next,
      walk_step state = Some next ->
      walk_potential next + 1 = walk_potential state.
  Proof.
    intros [pending emitted] next Hstep.
    destruct pending as [| [payload text children] rest]; simpl in Hstep.
    - discriminate.
    - inversion Hstep; subst; clear Hstep. unfold walk_potential; simpl.
      rewrite pending_count_app, forest_nodes_count. lia.
  Qed.

  Definition walk_transition (next state : WalkState) : Prop :=
    walk_step state = Some next.

  Theorem walk_transition_well_founded : well_founded walk_transition.
  Proof.
    apply (well_founded_lt_compat WalkState walk_potential).
    intros next state Htransition.
    unfold walk_transition in Htransition.
    pose proof (walk_step_consumes_one_node state next Htransition). lia.
  Qed.

  Inductive WalksN : nat -> WalkState -> WalkState -> Prop :=
  | WalksZero : forall state, WalksN 0 state state
  | WalksStep : forall count state next terminal,
      walk_step state = Some next ->
      WalksN count next terminal ->
      WalksN (S count) state terminal.

  Lemma walks_preserve_meaning :
    forall count state terminal,
      WalksN count state terminal ->
      walk_meaning terminal = walk_meaning state.
  Proof.
    intros count state terminal Hwalks. induction Hwalks.
    - reflexivity.
    - rewrite IHHwalks.
      now apply walk_step_preserves_recursive_preorder in H.
  Qed.

  Lemma walks_consume_exact_work :
    forall count state terminal,
      WalksN count state terminal ->
      count + walk_potential terminal = walk_potential state.
  Proof.
    intros count state terminal Hwalks. induction Hwalks.
    - simpl. lia.
    - pose proof (walk_step_consumes_one_node state next H) as Hconsume.
      simpl. lia.
  Qed.

  Lemma walk_nonterminal_progress :
    forall state,
      walk_terminal state = None -> exists next, walk_step state = Some next.
  Proof.
    intros [pending emitted] Hterminal.
    destruct pending as [| [payload text children] rest].
    - discriminate.
    - eexists. reflexivity.
  Qed.

  Theorem walk_reaches_terminal :
    forall state,
      exists count terminal trace,
        WalksN count state terminal /\
        walk_terminal terminal = Some trace.
  Proof.
    refine
      (well_founded_induction_type
         walk_transition_well_founded
         (fun state =>
            exists count terminal trace,
              WalksN count state terminal /\
              walk_terminal terminal = Some trace)
         _).
    intros state IH.
    destruct (walk_terminal state) as [trace |] eqn:Hterminal.
    - exists 0, state, trace. split; [constructor | exact Hterminal].
    - destruct (walk_nonterminal_progress state Hterminal) as [next Hstep].
      destruct (IH next Hstep) as [count [terminal [trace [Hwalk Htrace]]]].
      exists (S count), terminal, trace. split.
      + econstructor; eauto.
      + exact Htrace.
  Qed.

  Definition initial_walk (tree : Tree) : WalkState :=
    {| walk_pending := [tree]; walk_emitted_rev := [] |}.

  Theorem terminal_walk_equals_recursive_preorder :
    forall tree count terminal trace,
      WalksN count (initial_walk tree) terminal ->
      walk_terminal terminal = Some trace ->
      trace = preorder tree /\ count = node_count tree.
  Proof.
    intros tree count terminal trace Hwalk Hterminal.
    pose proof (walks_preserve_meaning count (initial_walk tree) terminal Hwalk)
      as Hmeaning.
    pose proof
      (walks_consume_exact_work count (initial_walk tree) terminal Hwalk)
      as Hcount.
    destruct terminal as [pending emitted].
    unfold walk_terminal in Hterminal; simpl in Hterminal.
    destruct pending; try discriminate.
    inversion Hterminal; subst; clear Hterminal.
    unfold walk_meaning, initial_walk, walk_potential in *; simpl in *.
    repeat rewrite app_nil_r in Hmeaning. split; [exact Hmeaning | lia].
  Qed.

  Record TextState : Type := {
    text_pending : list Tree;
    text_emitted_rev : list Text
  }.

  Definition text_meaning (state : TextState) : list Text :=
    rev (text_emitted_rev state) ++ pending_text (text_pending state).

  Definition text_step (state : TextState) : option TextState :=
    match text_pending state with
    | [] => None
    | Node _ (Some text) _ :: rest =>
        Some
          {| text_pending := rest;
             text_emitted_rev := text :: text_emitted_rev state |}
    | Node _ None children :: rest =>
        Some
          {| text_pending := forest_nodes children ++ rest;
             text_emitted_rev := text_emitted_rev state |}
    end.

  Definition text_terminal (state : TextState) : option (list Text) :=
    match text_pending state with
    | [] => Some (rev (text_emitted_rev state))
    | _ => None
    end.

  Theorem text_step_preserves_recursive_projection :
    forall state next,
      text_step state = Some next -> text_meaning next = text_meaning state.
  Proof.
    intros [pending emitted] next Hstep.
    destruct pending as [| [payload text children] rest]; simpl in Hstep.
    - discriminate.
    - destruct text as [value |].
      + inversion Hstep; subst; clear Hstep. unfold text_meaning; simpl.
        repeat rewrite <- app_assoc. reflexivity.
      + inversion Hstep; subst; clear Hstep. unfold text_meaning; simpl.
        rewrite pending_text_app, forest_nodes_text. reflexivity.
  Qed.

  Definition text_potential (state : TextState) : nat :=
    pending_count (text_pending state).

  Theorem text_step_strictly_decreases :
    forall state next,
      text_step state = Some next -> text_potential next < text_potential state.
  Proof.
    intros [pending emitted] next Hstep.
    destruct pending as [| [payload text children] rest]; simpl in Hstep.
    - discriminate.
    - destruct text as [value |].
      + inversion Hstep; subst; clear Hstep. unfold text_potential; simpl. lia.
      + inversion Hstep; subst; clear Hstep. unfold text_potential; simpl.
        rewrite pending_count_app, forest_nodes_count. lia.
  Qed.

  Definition text_transition (next state : TextState) : Prop :=
    text_step state = Some next.

  Theorem text_transition_well_founded : well_founded text_transition.
  Proof.
    apply (well_founded_lt_compat TextState text_potential).
    intros next state Htransition.
    unfold text_transition in Htransition.
    now apply text_step_strictly_decreases in Htransition.
  Qed.

  Inductive TextSteps : TextState -> TextState -> Prop :=
  | TextStepsZero : forall state, TextSteps state state
  | TextStepsMore : forall state next terminal,
      text_step state = Some next ->
      TextSteps next terminal ->
      TextSteps state terminal.

  Lemma text_steps_preserve_meaning :
    forall state terminal,
      TextSteps state terminal -> text_meaning terminal = text_meaning state.
  Proof.
    intros state terminal Hsteps. induction Hsteps.
    - reflexivity.
    - rewrite IHHsteps.
      now apply text_step_preserves_recursive_projection in H.
  Qed.

  Definition initial_text (tree : Tree) : TextState :=
    {| text_pending := [tree]; text_emitted_rev := [] |}.

  Theorem terminal_text_equals_recursive_projection :
    forall tree terminal trace,
      TextSteps (initial_text tree) terminal ->
      text_terminal terminal = Some trace ->
      trace = visible_text tree.
  Proof.
    intros tree terminal trace Hsteps Hterminal.
    pose proof (text_steps_preserve_meaning (initial_text tree) terminal Hsteps)
      as Hmeaning.
    destruct terminal as [pending emitted].
    unfold text_terminal in Hterminal; simpl in Hterminal.
    destruct pending; try discriminate.
    inversion Hterminal; subst; clear Hterminal.
    unfold text_meaning, initial_text in Hmeaning; simpl in Hmeaning.
    repeat rewrite app_nil_r in Hmeaning. exact Hmeaning.
  Qed.

  Definition option_equal (left right : option Text) : bool :=
    match left, right with
    | None, None => true
    | Some left_text, Some right_text => same_text left_text right_text
    | _, _ => false
    end.

  Fixpoint tree_equal (left right : Tree) : bool :=
    match left, right with
    | Node left_payload left_text left_children,
      Node right_payload right_text right_children =>
        same_payload left_payload right_payload &&
        option_equal left_text right_text &&
        forest_equal left_children right_children
    end
  with forest_equal (left right : Forest) : bool :=
    match left, right with
    | FNil, FNil => true
    | FCons left_child left_rest, FCons right_child right_rest =>
        tree_equal left_child right_child &&
        forest_equal left_rest right_rest
    | FNil, FCons _ _ => false
    | FCons _ _, FNil => false
    end.

  Fixpoint zip_forest (left right : Forest) : option (list (Tree * Tree)) :=
    match left, right with
    | FNil, FNil => Some []
    | FCons left_child left_rest, FCons right_child right_rest =>
        match zip_forest left_rest right_rest with
        | Some pairs => Some ((left_child, right_child) :: pairs)
        | None => None
        end
    | _, _ => None
    end.

  Fixpoint all_pairs_equal (pairs : list (Tree * Tree)) : bool :=
    match pairs with
    | [] => true
    | entry :: rest =>
        tree_equal (fst entry) (snd entry) && all_pairs_equal rest
    end.

  Lemma all_pairs_equal_app :
    forall left right,
      all_pairs_equal (left ++ right) =
      (all_pairs_equal left && all_pairs_equal right)%bool.
  Proof.
    induction left as [| [left_tree right_tree] rest IH]; intros right; simpl.
    - reflexivity.
    - rewrite IH, andb_assoc. reflexivity.
  Qed.

  Lemma zip_forest_refines_recursive_equality :
    forall left right,
      match zip_forest left right with
      | Some pairs => all_pairs_equal pairs = forest_equal left right
      | None => forest_equal left right = false
      end.
  Proof.
    induction left as [| left_child left_rest IH]; intros right;
      destruct right as [| right_child right_rest]; simpl.
    - reflexivity.
    - reflexivity.
    - reflexivity.
    - specialize (IH right_rest).
      destruct (zip_forest left_rest right_rest) as [pairs |] eqn:Hzip; simpl in *.
      + now rewrite IH.
      + rewrite IH. apply andb_false_r.
  Qed.

  Fixpoint pair_count (pairs : list (Tree * Tree)) : nat :=
    match pairs with
    | [] => 0
    | entry :: rest =>
        node_count (fst entry) + node_count (snd entry) + pair_count rest
    end.

  Lemma pair_count_app :
    forall left right,
      pair_count (left ++ right) = pair_count left + pair_count right.
  Proof.
    induction left as [| [left_tree right_tree] rest IH]; intros right; simpl.
    - reflexivity.
    - rewrite IH. lia.
  Qed.

  Lemma zip_forest_count :
    forall left right pairs,
      zip_forest left right = Some pairs ->
      pair_count pairs = forest_count left + forest_count right.
  Proof.
    induction left as [| left_child left_rest IH]; intros right pairs Hzip;
      destruct right as [| right_child right_rest]; simpl in Hzip; try discriminate.
    - inversion Hzip. reflexivity.
    - destruct (zip_forest left_rest right_rest) as [rest_pairs |] eqn:Hrest;
        try discriminate.
      inversion Hzip; subst; clear Hzip. simpl.
      specialize (IH right_rest rest_pairs Hrest). lia.
  Qed.

  Inductive EqualState : Type :=
  | EqualWork : list (Tree * Tree) -> EqualState
  | EqualDone : bool -> EqualState.

  Definition equal_meaning (state : EqualState) : bool :=
    match state with
    | EqualWork pairs => all_pairs_equal pairs
    | EqualDone result => result
    end.

  Definition equal_step (state : EqualState) : option EqualState :=
    match state with
    | EqualDone _ => None
    | EqualWork [] => None
    | EqualWork
        ((Node left_payload left_text left_children,
          Node right_payload right_text right_children) :: rest) =>
        if same_payload left_payload right_payload &&
           option_equal left_text right_text
        then
          match zip_forest left_children right_children with
          | Some child_pairs => Some (EqualWork (child_pairs ++ rest))
          | None => Some (EqualDone false)
          end
        else Some (EqualDone false)
    end.

  Definition equal_terminal (state : EqualState) : option bool :=
    match state with
    | EqualDone result => Some result
    | EqualWork [] => Some true
    | EqualWork _ => None
    end.

  Theorem equal_step_preserves_recursive_equality :
    forall state next,
      equal_step state = Some next -> equal_meaning next = equal_meaning state.
  Proof.
    intros state next Hstep.
    destruct state as [pairs | result].
    - destruct pairs as [| [[left_payload left_text left_children]
                             [right_payload right_text right_children]] rest];
        simpl in Hstep; try discriminate.
      destruct (same_payload left_payload right_payload &&
                option_equal left_text right_text)%bool eqn:Hroot.
      + destruct (zip_forest left_children right_children) as [children |] eqn:Hzip.
        * inversion Hstep; subst; clear Hstep. unfold equal_meaning; simpl.
          rewrite all_pairs_equal_app.
          pose proof (zip_forest_refines_recursive_equality
                        left_children right_children) as Hrefine.
          rewrite Hzip in Hrefine. rewrite Hrefine, Hroot. reflexivity.
        * inversion Hstep; subst; clear Hstep. unfold equal_meaning; simpl.
          pose proof (zip_forest_refines_recursive_equality
                        left_children right_children) as Hrefine.
          rewrite Hzip in Hrefine. rewrite Hroot, Hrefine. reflexivity.
      + inversion Hstep; subst; clear Hstep. unfold equal_meaning; simpl.
        now rewrite Hroot.
    - discriminate.
  Qed.

  Definition equal_potential (state : EqualState) : nat :=
    match state with
    | EqualWork pairs => pair_count pairs
    | EqualDone _ => 0
    end.

  Theorem equal_step_strictly_decreases :
    forall state next,
      equal_step state = Some next ->
      equal_potential next < equal_potential state.
  Proof.
    intros state next Hstep.
    destruct state as [pairs | result].
    - destruct pairs as [| [[left_payload left_text left_children]
                             [right_payload right_text right_children]] rest];
        simpl in Hstep; try discriminate.
      destruct (same_payload left_payload right_payload &&
                option_equal left_text right_text)%bool eqn:Hroot.
      + destruct (zip_forest left_children right_children) as [children |] eqn:Hzip.
        * inversion Hstep; subst; clear Hstep. unfold equal_potential; simpl.
          rewrite pair_count_app.
          pose proof (zip_forest_count left_children right_children children Hzip).
          lia.
        * inversion Hstep; subst; clear Hstep. unfold equal_potential; simpl. lia.
      + inversion Hstep; subst; clear Hstep. unfold equal_potential; simpl. lia.
    - discriminate.
  Qed.

  Definition equal_transition (next state : EqualState) : Prop :=
    equal_step state = Some next.

  Theorem equal_transition_well_founded : well_founded equal_transition.
  Proof.
    apply (well_founded_lt_compat EqualState equal_potential).
    intros next state Htransition.
    unfold equal_transition in Htransition.
    now apply equal_step_strictly_decreases in Htransition.
  Qed.

  Inductive EqualSteps : EqualState -> EqualState -> Prop :=
  | EqualStepsZero : forall state, EqualSteps state state
  | EqualStepsMore : forall state next terminal,
      equal_step state = Some next ->
      EqualSteps next terminal ->
      EqualSteps state terminal.

  Lemma equal_steps_preserve_meaning :
    forall state terminal,
      EqualSteps state terminal -> equal_meaning terminal = equal_meaning state.
  Proof.
    intros state terminal Hsteps. induction Hsteps.
    - reflexivity.
    - rewrite IHHsteps.
      now apply equal_step_preserves_recursive_equality in H.
  Qed.

  Definition initial_equal (left right : Tree) : EqualState :=
    EqualWork [(left, right)].

  Theorem terminal_equal_matches_recursive_reference :
    forall left right terminal result,
      EqualSteps (initial_equal left right) terminal ->
      equal_terminal terminal = Some result ->
      result = tree_equal left right.
  Proof.
    intros left right terminal result Hsteps Hterminal.
    pose proof (equal_steps_preserve_meaning (initial_equal left right) terminal Hsteps)
      as Hmeaning.
    destruct terminal as [pairs | terminal_result].
    - destruct pairs as [| pair rest]; simpl in Hterminal; try discriminate.
      inversion Hterminal; subst; clear Hterminal.
      unfold equal_meaning, initial_equal in Hmeaning; simpl in Hmeaning.
      rewrite andb_true_r in Hmeaning. exact Hmeaning.
    - inversion Hterminal; subst; clear Hterminal.
      unfold equal_meaning, initial_equal in Hmeaning; simpl in Hmeaning.
      rewrite andb_true_r in Hmeaning. exact Hmeaning.
  Qed.

End SyntaxTreeMachine.
