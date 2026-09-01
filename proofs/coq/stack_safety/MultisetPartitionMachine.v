(** * Stack-safe fair multiset-partition generation

    A multiset partition chooses a quantity [q_i] for each distinct item,
    bounded by both that item's multiplicity and the still-required total.
    The historical implementation expressed this as native recursion and
    combined quantity branches with a left fold of fair [interleave].

    This theory verifies the replacement's two machines.  [ChoiceStep]
    advances an item cursor while preserving the selected/remaining sum and
    per-item conservation.  [Pull] interprets a heap-resident expression of
    empty, yielded, and interleaved streams.  A successful pull removes exactly
    the recursive stream's first value and rotates the two interleave operands;
    an exhausted left operand delegates to the right operand.  Thus bounded
    consumers observe the same prefix order as the recursive left-fold
    interleave, while native call depth is constant.

    No axiom, admission, assumption, or proof escape is used.
*)

From Stdlib Require Import Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
From Stdlib Require Import Wf_nat.
Import ListNotations.

Record ChoiceState : Type := {
  item_index : nat;
  remaining_count : nat;
  selected_count : nat
}.

Definition choice_invariant
    (item_count requested : nat)
    (state : ChoiceState) : Prop :=
  item_index state <= item_count /\
  remaining_count state + selected_count state = requested.

Inductive ChoiceStep (counts : list nat) : ChoiceState -> ChoiceState -> Prop :=
| TakeQuantity :
    forall state multiplicity quantity,
      nth_error counts (item_index state) = Some multiplicity ->
      quantity <= multiplicity ->
      quantity <= remaining_count state ->
      ChoiceStep counts state
        {| item_index := S (item_index state);
           remaining_count := remaining_count state - quantity;
           selected_count := selected_count state + quantity |}.

Theorem choice_step_preserves_invariant :
  forall counts requested state next,
    choice_invariant (length counts) requested state ->
    ChoiceStep counts state next ->
    choice_invariant (length counts) requested next.
Proof.
  intros counts requested state next [Hindex Hsum] Hstep.
  destruct Hstep as [state multiplicity quantity Hnth Hmult Hremaining].
  split; simpl.
  - apply nth_error_Some. rewrite Hnth. discriminate.
  - lia.
Qed.

Theorem choice_step_advances_cursor :
  forall counts state next,
    ChoiceStep counts state next ->
    item_index next = S (item_index state).
Proof.
  intros counts state next Hstep.
  now destruct Hstep.
Qed.

Definition cursor_rank (counts : list nat) (state : ChoiceState) : nat :=
  length counts - item_index state.

Theorem choice_step_decreases_cursor_rank :
  forall counts requested state next,
    choice_invariant (length counts) requested state ->
    ChoiceStep counts state next ->
    cursor_rank counts next < cursor_rank counts state.
Proof.
  intros counts requested state next [Hindex Hsum] Hstep.
  destruct Hstep as [state multiplicity quantity Hnth Hmult Hremaining].
  assert (Hstrict : item_index state < length counts).
  { apply nth_error_Some. rewrite Hnth. discriminate. }
  unfold cursor_rank. simpl. lia.
Qed.

Definition choice_transition
    (counts : list nat)
    (next state : ChoiceState) : Prop :=
  ChoiceStep counts state next.

Theorem choice_transition_well_founded :
  forall counts requested,
    well_founded
      (fun next state =>
        choice_invariant (length counts) requested state /\
        choice_transition counts next state).
Proof.
  intros counts requested.
  apply (well_founded_lt_compat ChoiceState (cursor_rank counts)).
  intros next state [Hinvariant Hstep].
  now apply (choice_step_decreases_cursor_rank counts requested state next).
Qed.

Theorem terminal_choice_selects_requested_total :
  forall item_count requested state,
    choice_invariant item_count requested state ->
    remaining_count state = 0 ->
    selected_count state = requested.
Proof.
  intros item_count requested state [Hindex Hsum] Hremaining.
  lia.
Qed.

Theorem per_item_conservation :
  forall multiplicity quantity,
    quantity <= multiplicity ->
    quantity + (multiplicity - quantity) = multiplicity.
Proof.
  intros multiplicity quantity Hquantity. lia.
Qed.

Definition suffix_available (counts : list nat) (start : nat) : nat :=
  fold_right Nat.add 0 (skipn start counts).

Definition feasible
    (counts : list nat)
    (state : ChoiceState) : bool :=
  remaining_count state <=? suffix_available counts (item_index state).

Theorem infeasible_suffix_rejects :
  forall counts state,
    feasible counts state = false ->
    suffix_available counts (item_index state) < remaining_count state.
Proof.
  intros counts state Hfeasible.
  unfold feasible in Hfeasible.
  now apply Nat.leb_gt.
Qed.

Section FairPullMachine.

  Variable Output : Type.

  Fixpoint interleave_fuel
      (fuel : nat)
      (left right : list Output) : list Output :=
    match left with
    | [] => right
    | value :: rest =>
        match fuel with
        | 0 => left ++ right
        | S remaining_fuel =>
            value :: interleave_fuel remaining_fuel right rest
        end
    end.

  Definition interleave (left right : list Output) : list Output :=
    interleave_fuel (length left + length right) left right.

  Lemma interleave_empty_left :
    forall right, interleave [] right = right.
  Proof.
    intros right. destruct right; reflexivity.
  Qed.

  Lemma interleave_value_left :
    forall value rest right,
      interleave (value :: rest) right =
      value :: interleave right rest.
  Proof.
    intros value rest right.
    unfold interleave. simpl. f_equal.
    rewrite Nat.add_comm. reflexivity.
  Qed.

  Inductive Expression : Type :=
  | Empty : Expression
  | Yield : Output -> Expression
  | Alternate : Expression -> Expression -> Expression.

  Fixpoint denote (expression : Expression) : list Output :=
    match expression with
    | Empty => []
    | Yield value => [value]
    | Alternate lhs rhs => interleave (denote lhs) (denote rhs)
    end.

  Inductive Pull : Expression -> option Output -> Expression -> Prop :=
  | PullEmpty : Pull Empty None Empty
  | PullYield : forall value, Pull (Yield value) (Some value) Empty
  | PullAlternateValue :
      forall left right value left_next,
        Pull left (Some value) left_next ->
        Pull (Alternate left right) (Some value)
          (Alternate right left_next)
  | PullAlternateEmpty :
      forall left right left_next result right_next,
        Pull left None left_next ->
        Pull right result right_next ->
        Pull (Alternate left right) result right_next.

  Theorem pull_refines_recursive_interleave :
    forall expression result next,
      Pull expression result next ->
      match result with
      | Some value => denote expression = value :: denote next
      | None => denote expression = [] /\ denote next = []
      end.
  Proof.
    intros expression result next Hpull.
    induction Hpull as
        [
        |value
        |left right value left_next Hleft IHleft
        |left right left_next result right_next Hleft IHleft Hright IHright].
    - split; reflexivity.
    - reflexivity.
    - simpl in *. rewrite IHleft. apply interleave_value_left.
    - destruct IHleft as [Hleft_empty Hleft_next].
      destruct result as [value |]; simpl in *.
      + rewrite Hleft_empty, interleave_empty_left. exact IHright.
      + destruct IHright as [Hright_empty Hright_next].
        rewrite Hleft_empty, interleave_empty_left. split; assumption.
  Qed.

  Corollary successful_pull_consumes_one_output :
    forall expression value next,
      Pull expression (Some value) next ->
      length (denote next) + 1 = length (denote expression).
  Proof.
    intros expression value next Hpull.
    apply pull_refines_recursive_interleave in Hpull.
    rewrite Hpull. simpl. lia.
  Qed.

  Corollary exhausted_pull_is_exact :
    forall expression next,
      Pull expression None next ->
      denote expression = [] /\ denote next = [].
  Proof.
    intros expression next Hpull.
    now apply pull_refines_recursive_interleave in Hpull.
  Qed.

  Fixpoint left_fold_alternate
      (accumulated : Expression)
      (branches : list Expression) : Expression :=
    match branches with
    | [] => accumulated
    | branch :: rest =>
        left_fold_alternate (Alternate accumulated branch) rest
    end.

  Fixpoint left_fold_interleave
      (accumulated : list Output)
      (branches : list (list Output)) : list Output :=
    match branches with
    | [] => accumulated
    | branch :: rest =>
        left_fold_interleave (interleave accumulated branch) rest
    end.

  (** Once a left fold has exposed its first value, every as-yet-unfolded
      quantity branch wraps the residual from the right.  The Rust schedule
      stores this residual fold as a compact descending quantity interval,
      rather than allocating every branch before the first pull. *)
  Fixpoint residual_fold
      (branches : list (list Output))
      (tail : list Output) : list Output :=
    match branches with
    | [] => tail
    | branch :: rest =>
        residual_fold rest (interleave branch tail)
    end.

  Lemma left_fold_interleave_app :
    forall prefix suffix accumulated,
      left_fold_interleave accumulated (prefix ++ suffix) =
      left_fold_interleave
        (left_fold_interleave accumulated prefix)
        suffix.
  Proof.
    induction prefix as [| branch rest IH]; intros suffix accumulated; simpl.
    - reflexivity.
    - apply IH.
  Qed.

  Lemma all_empty_branches_are_neutral :
    forall branches,
      Forall (fun branch => branch = []) branches ->
      left_fold_interleave [] branches = [].
  Proof.
    intros branches Hall.
    induction Hall as [| branch rest Hbranch Hrest IH]; simpl.
    - reflexivity.
    - subst branch. exact IH.
  Qed.

  Theorem left_fold_head_has_compact_residual :
    forall later value tail,
      left_fold_interleave (value :: tail) later =
      value :: residual_fold later tail.
  Proof.
    induction later as [| branch rest IH]; intros value tail; simpl.
    - reflexivity.
    - rewrite interleave_value_left. apply IH.
  Qed.

  Theorem first_nonempty_branch_has_compact_residual :
    forall empty_prefix value tail later,
      Forall (fun branch => branch = []) empty_prefix ->
      left_fold_interleave []
        (empty_prefix ++ (value :: tail) :: later) =
      value :: residual_fold later tail.
  Proof.
    intros empty_prefix value tail later Hempty.
    rewrite left_fold_interleave_app.
    rewrite (all_empty_branches_are_neutral empty_prefix Hempty).
    simpl. rewrite interleave_empty_left.
    apply left_fold_head_has_compact_residual.
  Qed.

  Theorem left_fold_schedule_is_exact :
    forall branches accumulated,
      denote (left_fold_alternate accumulated branches) =
      left_fold_interleave (denote accumulated) (map denote branches).
  Proof.
    induction branches as [| branch rest IH]; intros accumulated; simpl.
    - reflexivity.
    - apply IH.
  Qed.

  Theorem bounded_prefix_after_pull :
    forall expression value next bound,
      Pull expression (Some value) next ->
      firstn bound (denote expression) =
      firstn bound (value :: denote next).
  Proof.
    intros expression value next bound Hpull.
    apply pull_refines_recursive_interleave in Hpull.
    now rewrite Hpull.
  Qed.

End FairPullMachine.
