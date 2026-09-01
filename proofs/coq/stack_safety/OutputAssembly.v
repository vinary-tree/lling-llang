(** * Ownership-aware deterministic output assembly

    A bottom-up symbolic tree transducer receives owned child results.  When
    exactly one rule is applicable and every required child state has exactly
    one output, cloning those outputs is observationally unnecessary.  The
    implementation may move each uniquely selected child into the parent and
    retain the clone-based Cartesian construction only for ambiguous or repeated
    selections.

    [copy_select] models the reference value semantics.  [move_select] is a
    separately defined ownership-erased model of the move path.  The theorems
    below establish exact value/order refinement, totality under checked indices,
    a no-duplication storage bound, and the linear work law that replaces repeated
    whole-subtree cloning on deterministic unary paths.  No axiom, admission,
    parameter, or proof escape is used.
*)

From Stdlib Require Import Arith.
From Stdlib Require Import Lia.
From Stdlib Require Import List.
Import ListNotations.

Section OutputAssembly.

  Context {Value : Type}.

  Fixpoint copy_select
      (children : list Value)
      (indices : list nat) : option (list Value) :=
    match indices with
    | [] => Some []
    | index :: rest =>
        match nth_error children index, copy_select children rest with
        | Some child, Some selected => Some (child :: selected)
        | _, _ => None
        end
    end.

  Fixpoint move_select
      (children : list Value)
      (indices : list nat) : option (list Value) :=
    match indices with
    | [] => Some []
    | index :: rest =>
        match nth_error children index with
        | Some child =>
            match move_select children rest with
            | Some selected => Some (child :: selected)
            | None => None
            end
        | None => None
        end
    end.

  Theorem move_selection_refines_copy_selection :
    forall children indices,
      move_select children indices = copy_select children indices.
  Proof.
    intros children indices.
    induction indices as [| index rest IH].
    - reflexivity.
    - simpl. destruct (nth_error children index).
      + now rewrite IH.
      + reflexivity.
  Qed.

  Theorem checked_move_selection_is_total :
    forall children indices,
      Forall (fun index => index < length children) indices ->
      exists selected, move_select children indices = Some selected.
  Proof.
    intros children indices Hvalid.
    induction Hvalid as [| index rest Hindex Hrest IH].
    - exists []. reflexivity.
    - destruct IH as [selected Hselected].
      destruct (nth_error children index) eqn:Hchild.
      + exists (v :: selected). simpl. now rewrite Hchild, Hselected.
      + apply nth_error_None in Hchild. lia.
  Qed.

  Theorem unique_selection_count_is_bounded_by_child_count :
    forall (children : list Value) (indices : list nat),
      Forall (fun index => index < length children) indices ->
      NoDup indices ->
      length indices <= length children.
  Proof.
    intros children indices Hvalid Hunique.
    replace (length children) with (length (seq 0 (length children))) by
      now rewrite length_seq.
    apply NoDup_incl_length.
    - exact Hunique.
    - intros index Hin.
      apply in_seq.
      split; [lia|].
      rewrite Forall_forall in Hvalid.
      now apply Hvalid.
  Qed.

  Theorem deterministic_unary_move_is_exact :
    forall child,
      move_select [child] [0] = Some [child] /\
      copy_select [child] [0] = Some [child].
  Proof.
    intros child. split; reflexivity.
  Qed.

End OutputAssembly.

(** Work accounting for a unary path of [depth] transducer nodes.  Moving the
    already-built child performs one assembly unit per node.  Repeatedly cloning
    the complete child performs the triangular number of units. *)
Fixpoint repeated_clone_work (depth : nat) : nat :=
  match depth with
  | 0 => 0
  | S smaller => S smaller + repeated_clone_work smaller
  end.

Definition move_assembly_work (depth : nat) : nat := depth.

Theorem move_assembly_work_is_linear :
  forall depth, move_assembly_work depth = depth.
Proof.
  reflexivity.
Qed.

Theorem repeated_clone_work_is_triangular :
  forall depth,
    2 * repeated_clone_work depth = depth * (depth + 1).
Proof.
  induction depth as [| depth IH].
  - reflexivity.
  - simpl repeated_clone_work. unfold move_assembly_work in *. lia.
Qed.

Theorem moving_is_never_more_work_on_nonempty_paths :
  forall depth,
    0 < depth ->
    move_assembly_work depth <= repeated_clone_work depth.
Proof.
  intros depth Hpositive.
  induction depth as [| depth IH].
  - lia.
  - destruct depth.
    + reflexivity.
    + simpl in *. unfold move_assembly_work in *. specialize (IH ltac:(lia)). lia.
Qed.
