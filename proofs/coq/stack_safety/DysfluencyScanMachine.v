(** * Stack-safe, cycle-safe dysfluency-product scanning

    The dysfluency detector explores a product of a phone lattice and a
    pattern automaton.  A matched transition consumes one frame; a
    pattern-only epsilon transition consumes neither a frame nor a phone.
    Consequently, frame and phone caps alone cannot make the recursive
    reference implementation total in the presence of an epsilon cycle.

    This theory verifies the production refinement in three layers.

    - [ScanStep] gives every matched transition a fresh epsilon epoch and
      rejects a pattern-only epsilon edge that repeats a pattern state in the
      current epoch.  Its rank strictly decreases, including across epsilon
      edges.
    - [scheduled_successors] places all ordered matching-pair successors
      before the ordered pattern-only epsilon successors.  The latter list is
      appended exactly once, independently of lattice out-degree.
    - [machine_step] is the defunctionalized depth-first machine.  It emits a
      node before visiting its ordered children, preserves the recursive
      preorder trace exactly, makes progress at every nonterminal state, and
      consumes one unit of finite tree potential per transition.

    The Rust implementation uses the same invariants with heap-resident work
    frames and an epoch-keyed on-path set.  No axiom, admission, parameter,
    or proof escape is used.
*)

From Stdlib Require Import Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
From Stdlib Require Import Wf_nat.
Import ListNotations.

Record ScanConfig : Type := {
  frames_left : nat;
  epsilon_path : list nat;
  phone_count : nat
}.

Definition config_rank (pattern_states : nat) (config : ScanConfig) : nat :=
  frames_left config * S pattern_states
  + (pattern_states - length (epsilon_path config)).

Definition well_formed
    (pattern_states phone_limit : nat)
    (config : ScanConfig) : Prop :=
  length (epsilon_path config) <= pattern_states /\
  NoDup (epsilon_path config) /\
  Forall (fun state => state < pattern_states) (epsilon_path config) /\
  phone_count config <= phone_limit.

Inductive ScanStep (pattern_states phone_limit : nat)
  : ScanConfig -> ScanConfig -> Prop :=
| EpsilonStep :
    forall config target,
      target < pattern_states ->
      ~ In target (epsilon_path config) ->
      length (epsilon_path config) < pattern_states ->
      ScanStep pattern_states phone_limit config
        {| frames_left := frames_left config;
           epsilon_path := target :: epsilon_path config;
           phone_count := phone_count config |}
| MatchedStep :
    forall config remaining target (emits_phone : bool) next_phone_count,
      frames_left config = S remaining ->
      target < pattern_states ->
      next_phone_count =
        (if emits_phone then S (phone_count config) else phone_count config) ->
      next_phone_count <= phone_limit ->
      ScanStep pattern_states phone_limit config
        {| frames_left := remaining;
           epsilon_path := [target];
           phone_count := next_phone_count |}.

Theorem epsilon_step_decreases_rank :
  forall pattern_states phone_limit config next,
    ScanStep pattern_states phone_limit config next ->
    config_rank pattern_states next < config_rank pattern_states config.
Proof.
  intros pattern_states phone_limit config next Hstep.
  destruct Hstep as
      [config target Htarget Hfresh Hlength
      |config remaining target emits next_phones Hframes Htarget Hphones Hlimit].
  - unfold config_rank. simpl. lia.
  - unfold config_rank. simpl. rewrite Hframes. lia.
Qed.

Theorem scan_step_preserves_well_formed :
  forall pattern_states phone_limit config next,
    well_formed pattern_states phone_limit config ->
    ScanStep pattern_states phone_limit config next ->
    well_formed pattern_states phone_limit next.
Proof.
  intros pattern_states phone_limit config next Hwell Hstep.
  destruct Hwell as [Hlength [Hnodup [Hstates Hphones]]].
  destruct Hstep as
      [config target Htarget Hfresh Hstrict
      |config remaining target emits next_phones Hframes Htarget Hnext Hlimit].
  - repeat split; simpl.
    + lia.
    + now constructor.
    + constructor; assumption.
    + exact Hphones.
  - repeat split; simpl.
    + lia.
    + constructor; [simpl; tauto | constructor].
    + constructor; [exact Htarget | constructor].
    + exact Hlimit.
Qed.

Definition scan_transition
    (pattern_states phone_limit : nat)
    (next config : ScanConfig) : Prop :=
  ScanStep pattern_states phone_limit config next.

Theorem scan_transition_well_founded :
  forall pattern_states phone_limit,
    well_founded (scan_transition pattern_states phone_limit).
Proof.
  intros pattern_states phone_limit.
  apply (well_founded_lt_compat ScanConfig (config_rank pattern_states)).
  intros next config Hstep.
  now apply (epsilon_step_decreases_rank pattern_states phone_limit config next).
Qed.

Definition scheduled_successors
    (matching_pairs : list (list nat))
    (pattern_epsilon : list nat) : list nat :=
  concat matching_pairs ++ pattern_epsilon.

Theorem pattern_epsilon_scheduled_once :
  forall matching_pairs pattern_epsilon state,
    count_occ Nat.eq_dec (scheduled_successors matching_pairs pattern_epsilon) state =
    count_occ Nat.eq_dec (concat matching_pairs) state
    + count_occ Nat.eq_dec pattern_epsilon state.
Proof.
  intros matching_pairs pattern_epsilon state.
  unfold scheduled_successors.
  apply count_occ_app.
Qed.

Theorem pattern_epsilon_order_is_suffix :
  forall matching_pairs pattern_epsilon,
    skipn (length (concat matching_pairs))
      (scheduled_successors matching_pairs pattern_epsilon) = pattern_epsilon.
Proof.
  intros matching_pairs pattern_epsilon.
  unfold scheduled_successors.
  rewrite skipn_app.
  rewrite Nat.sub_diag. simpl.
  now rewrite skipn_all.
Qed.

Section DepthFirstMachine.

  Variable Output : Type.

  Inductive SearchTree : Type :=
  | Empty : SearchTree
  | Visit : option Output -> SearchTree -> SearchTree -> SearchTree.

  Fixpoint recursive_trace (tree : SearchTree) : list Output :=
    match tree with
    | Empty => []
    | Visit output first_child remaining_siblings =>
        (match output with Some value => [value] | None => [] end)
        ++ recursive_trace first_child
        ++ recursive_trace remaining_siblings
    end.

  Fixpoint tree_size (tree : SearchTree) : nat :=
    match tree with
    | Empty => 1
    | Visit _ first_child remaining_siblings =>
        1 + tree_size first_child + tree_size remaining_siblings
    end.

  Fixpoint pending_trace (pending : list SearchTree) : list Output :=
    match pending with
    | [] => []
    | tree :: rest => recursive_trace tree ++ pending_trace rest
    end.

  Fixpoint pending_size (pending : list SearchTree) : nat :=
    match pending with
    | [] => 0
    | tree :: rest => tree_size tree + pending_size rest
    end.

  Record Machine : Type := {
    emitted : list Output;
    pending : list SearchTree
  }.

  Definition machine_meaning (machine : Machine) : list Output :=
    emitted machine ++ pending_trace (pending machine).

  Definition machine_potential (machine : Machine) : nat :=
    pending_size (pending machine).

  Definition machine_step (machine : Machine) : option Machine :=
    match pending machine with
    | [] => None
    | Empty :: rest =>
        Some {| emitted := emitted machine; pending := rest |}
    | Visit output first_child remaining_siblings :: rest =>
        Some
          {| emitted := emitted machine
              ++ match output with Some value => [value] | None => [] end;
             pending := first_child :: remaining_siblings :: rest |}
    end.

  Lemma append_singleton_cons :
    forall (prefix : list Output) value suffix,
      prefix ++ value :: suffix = (prefix ++ [value]) ++ suffix.
  Proof.
    intros prefix value suffix.
    induction prefix as [| head tail IH]; simpl; [reflexivity |].
    now rewrite IH.
  Qed.

  Theorem machine_step_preserves_recursive_trace :
    forall machine next,
      machine_step machine = Some next ->
      machine_meaning machine = machine_meaning next.
  Proof.
    intros [emitted pending] next Hstep.
    destruct pending as [| tree rest]; simpl in Hstep; [discriminate |].
    destruct tree as [| output first siblings].
    - inversion Hstep; subst. reflexivity.
    - inversion Hstep; subst. clear Hstep.
      destruct output as [value |]; unfold machine_meaning; simpl.
      + rewrite append_singleton_cons.
        repeat rewrite app_assoc. reflexivity.
      + rewrite app_nil_r.
        repeat rewrite app_assoc. reflexivity.
  Qed.

  Theorem machine_step_decreases_potential :
    forall machine next,
      machine_step machine = Some next ->
      machine_potential next < machine_potential machine.
  Proof.
    intros [emitted pending] next Hstep.
    destruct pending as [| tree rest]; simpl in Hstep; [discriminate |].
    destruct tree as [| output first siblings].
    - inversion Hstep; subst. unfold machine_potential. simpl. lia.
    - inversion Hstep; subst. unfold machine_potential. simpl. lia.
  Qed.

  Definition machine_transition (next machine : Machine) : Prop :=
    machine_step machine = Some next.

  Theorem machine_transition_well_founded : well_founded machine_transition.
  Proof.
    apply (well_founded_lt_compat Machine machine_potential).
    intros next machine Hstep.
    now apply machine_step_decreases_potential in Hstep.
  Qed.

  Definition terminal_result (machine : Machine) : option (list Output) :=
    match pending machine with
    | [] => Some (emitted machine)
    | _ => None
    end.

  Theorem nonterminal_machine_steps :
    forall machine,
      terminal_result machine = None ->
      exists next, machine_step machine = Some next.
  Proof.
    intros [emitted pending] Hterminal.
    destruct pending as [| tree rest]; [discriminate |].
    destruct tree; eexists; reflexivity.
  Qed.

  Inductive ReachesN : nat -> Machine -> Machine -> Prop :=
  | ReachesZero : forall machine, ReachesN 0 machine machine
  | ReachesStep : forall count machine next final,
      machine_step machine = Some next ->
      ReachesN count next final ->
      ReachesN (S count) machine final.

  Theorem reaches_preserves_recursive_trace :
    forall count machine final,
      ReachesN count machine final ->
      machine_meaning machine = machine_meaning final.
  Proof.
    intros count machine final Hreach.
    induction Hreach.
    - reflexivity.
    - rewrite (machine_step_preserves_recursive_trace machine next H).
      exact IHHreach.
  Qed.

  Theorem reaches_consumes_exact_work :
    forall count machine final,
      ReachesN count machine final ->
      count + machine_potential final = machine_potential machine.
  Proof.
    intros count machine final Hreach.
    induction Hreach.
    - simpl. lia.
    - destruct machine as [emitted pending].
      destruct pending as [| tree rest]; simpl in H; [discriminate |].
      destruct tree; inversion H; subst;
        unfold machine_potential in *; simpl in *; lia.
  Qed.

  Theorem reaches_terminal :
    forall machine,
      exists count final result,
        ReachesN count machine final /\
        terminal_result final = Some result.
  Proof.
    refine
      (well_founded_induction_type
        machine_transition_well_founded
        (fun machine => exists count final result,
          ReachesN count machine final /\
          terminal_result final = Some result)
        _).
    intros machine IH.
    destruct (terminal_result machine) as [result |] eqn:Hterminal.
    - exists 0, machine, result. split; [constructor | exact Hterminal].
    - destruct (nonterminal_machine_steps machine Hterminal) as [next Hstep].
      destruct (IH next Hstep) as [count [final [result [Hreach Hresult]]]].
      exists (S count), final, result. split.
      + now apply ReachesStep with next.
      + exact Hresult.
  Qed.

  Theorem terminal_machine_equals_recursive_reference :
    forall tree count final result,
      ReachesN count
        {| emitted := []; pending := [tree] |} final ->
      terminal_result final = Some result ->
      result = recursive_trace tree.
  Proof.
    intros tree count final result Hreach Hterminal.
    apply reaches_preserves_recursive_trace in Hreach.
    destruct final as [emitted pending].
    destruct pending as [| pending_tree rest]; simpl in Hterminal;
      [| discriminate].
    inversion Hterminal; subst.
    unfold machine_meaning in Hreach. simpl in Hreach.
    repeat rewrite app_nil_r in Hreach.
    symmetry. exact Hreach.
  Qed.

End DepthFirstMachine.
