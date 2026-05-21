(** * Paths in WFSTs

    Definitions for paths through WFSTs and their weights.

    A path is a sequence of transitions from a start state to an end state.
    The weight of a path is the semiring product of transition weights.
*)

Require Import Coq.Lists.List.
Require Import Coq.Classes.Morphisms.
Require Import LlingLlang.foundations.Semiring.
Require Import LlingLlang.wfst.Definitions.

Import ListNotations.

(** ** Path Definition *)

Section Paths.
  Context {W : Type} `{Semiring W}.

  #[local]
  Instance path_sr_times_Proper :
    Proper (sr_eq ==> sr_eq ==> sr_eq) sr_times := sr_times_proper.

  (** A path is a list of transitions *)
  Definition Path := list (Transition W).

  (** The empty path *)
  Definition empty_path : Path := [].

  (** ** Path Validity *)

  (** A path is valid if each transition's target is the next transition's source *)
  Fixpoint path_valid (p : Path) : Prop :=
    match p with
    | [] => True
    | [_] => True
    | t1 :: (t2 :: _) as rest =>
        tr_to t1 = tr_from t2 /\ path_valid rest
    end.

  (** A path is valid in a WFST if it starts at start, ends at final, and is connected *)
  Definition path_valid_in_wfst (fst : Wfst W) (p : Path) : Prop :=
    path_valid p /\
    match p with
    | [] => is_final fst (wfst_start fst) = true
    | t :: _ =>
        tr_from t = wfst_start fst /\
        match last p t with
        | t_last => is_final fst (tr_to t_last) = true
        end
    end.

  (** ** Path Weight *)

  (** The weight of a path is the product of transition weights *)
  Fixpoint path_weight (p : Path) : W :=
    match p with
    | [] => sr_one  (* Empty path has weight 1 *)
    | t :: rest => sr_times (tr_weight t) (path_weight rest)
    end.

  (** ** Path Labels *)

  (** Input labels of a path *)
  Definition path_input (p : Path) : list (option Label) :=
    map tr_input p.

  (** Output labels of a path *)
  Definition path_output (p : Path) : list (option Label) :=
    map tr_output p.

  (** ** Path Properties *)

  (** Weight of empty path is one *)
  Lemma path_weight_empty : path_weight empty_path ≡ 𝟙.
  Proof.
    simpl. apply sr_eq_refl.
  Qed.

  (** Weight of singleton path is the transition weight *)
  Lemma path_weight_singleton : forall t : Transition W,
    path_weight [t] ≡ tr_weight t.
  Proof.
    intro t. simpl.
    apply sr_times_one_r.
  Qed.

  (** Weight of concatenated paths is the product of weights *)
  Lemma path_weight_concat : forall p1 p2 : Path,
    path_weight (p1 ++ p2) ≡ sr_times (path_weight p1) (path_weight p2).
  Proof.
    intros p1 p2.
    induction p1 as [| t p1' IH].
    - simpl. apply sr_eq_sym. apply sr_times_one_l.
    - simpl.
      eapply sr_eq_trans with (b := tr_weight t ⊗ (path_weight p1' ⊗ path_weight p2)).
      + apply sr_times_proper; [apply sr_eq_refl | exact IH].
      + apply sr_eq_sym. apply sr_times_assoc.
  Qed.

  (** ** Accepting Paths *)

  (** An accepting path is one that:
      1. Starts at the start state
      2. Ends at a final state
      3. Is valid (connected) *)
  Definition accepting_path (fst : Wfst W) (p : Path) : Prop :=
    path_valid_in_wfst fst p.

  (** ** Path Enumeration *)

  (** All paths of length n from state s *)
  (* Note: This is a simplified version; actual implementation would
     need to handle cycles carefully *)

  (** ** Path Extension *)

  (** Extend a path by one transition *)
  Definition extend_path (p : Path) (t : Transition W) : Path :=
    p ++ [t].

  (** Extension preserves validity when transitions connect *)
  Lemma extend_path_valid : forall p t,
    path_valid p ->
    (p = [] \/ exists t_last, last p t_last = t_last /\ tr_to t_last = tr_from t) ->
    path_valid (extend_path p t).
  Proof.
    intros p t Hp Hconnect.
    unfold extend_path.
    induction p as [| t1 p' IH].
    - simpl. auto.
    - simpl in *.
      destruct p' as [| t2 p''].
      + simpl.
        destruct Hconnect as [Habs | [t_last [Hlast Hconn]]].
        * discriminate.
        * simpl in Hlast. rewrite Hlast. split; auto.
      + destruct Hp as [Hconn' Hp'].
        split; auto.
        apply IH; auto.
        destruct Hconnect as [Habs | Hex]; [discriminate | right; exact Hex].
  Qed.

End Paths.

(** ** Path Transformations *)

Section PathTransformations.
  Context {W : Type} `{Semiring W}.

  (** Reverse a path (reverses direction of all transitions) *)
  Definition reverse_transition (t : Transition W) : Transition W :=
    mkTransition (tr_to t) (tr_output t) (tr_input t) (tr_from t) (tr_weight t).

  Definition reverse_path (p : Path) : Path :=
    rev (map reverse_transition p).

  (** Project to input (for composition) *)
  Definition project_input (p : Path) : list (option Label) :=
    map (fun t : Transition W => tr_input t) p.

  (** Project to output (for composition) *)
  Definition project_output (p : Path) : list (option Label) :=
    map (fun t : Transition W => tr_output t) p.

End PathTransformations.
