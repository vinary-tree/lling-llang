(** * Shortest-Distance Specification Lemmas *)

Require Import Coq.Lists.List.
Require Import Coq.Arith.Arith.
Require Import Coq.Classes.Morphisms.
Require Import LlingLlang.foundations.Semiring.
Require Import LlingLlang.wfst.Definitions.
Require Import LlingLlang.wfst.Paths.

Import ListNotations.

Section ShortestDistanceSpec.
  Context {W : Type} `{Semiring W}.

  #[local]
  Instance shortest_sr_plus_Proper :
    Proper (sr_eq ==> sr_eq ==> sr_eq) sr_plus := sr_plus_proper.

  #[local]
  Instance shortest_sr_times_Proper :
    Proper (sr_eq ==> sr_eq ==> sr_eq) sr_times := sr_times_proper.

  Definition ShortestDistanceVector := StateId -> W.

  Definition init_distances (fst : Wfst W) : ShortestDistanceVector :=
    fun s => if Nat.eqb s (wfst_start fst) then sr_one else sr_zero.

  Definition relax (d : ShortestDistanceVector) (t : Transition W)
    : ShortestDistanceVector :=
    fun s => if Nat.eqb s (tr_to t)
             then sr_plus (d s) (sr_times (d (tr_from t)) (tr_weight t))
             else d s.

	  Definition relax_all (fst : Wfst W) (d : ShortestDistanceVector)
	    : ShortestDistanceVector :=
    fold_left (fun d' state =>
      fold_left (fun d'' t => relax d'' t)
                (ws_outgoing state)
                d')
	              (wfst_states fst)
	              d.

  Definition distance_relaxation_closed
      (fst : Wfst W) (d : ShortestDistanceVector) : Prop :=
    forall t : Transition W,
      transition_in_wfst fst t ->
      d (tr_to t) ≡ d (tr_to t) ⊕ (d (tr_from t) ⊗ tr_weight t).

  Definition shortest_distance_solution
      (fst : Wfst W) (d : ShortestDistanceVector) : Prop :=
    wfst_well_formed fst /\
    d (wfst_start fst) ≡ 𝟙 /\
    distance_relaxation_closed fst d.

  Lemma init_distance_start : forall fst,
    init_distances fst (wfst_start fst) ≡ (𝟙 : W).
  Proof.
    intro fst. unfold init_distances.
    rewrite Nat.eqb_refl. apply sr_eq_refl.
  Qed.

  Lemma init_distance_nonstart : forall fst s,
    s <> wfst_start fst ->
    init_distances fst s ≡ (𝟘 : W).
  Proof.
    intros fst s Hneq. unfold init_distances.
    destruct (Nat.eqb_spec s (wfst_start fst)).
    - contradiction.
    - apply sr_eq_refl.
  Qed.

  Lemma relax_updates_target : forall d t,
    relax d t (tr_to t) ≡ d (tr_to t) ⊕ (d (tr_from t) ⊗ tr_weight t).
  Proof.
    intros d t. unfold relax.
    rewrite Nat.eqb_refl. apply sr_eq_refl.
  Qed.

  Lemma relax_preserves_other : forall d t s,
    s <> tr_to t ->
    relax d t s ≡ d s.
  Proof.
    intros d t s Hneq. unfold relax.
    destruct (Nat.eqb_spec s (tr_to t)).
    - contradiction.
    - apply sr_eq_refl.
  Qed.

  Lemma relax_all_empty_fst : forall d start n,
    relax_all (mkWfst [] start n : Wfst W) d = d.
  Proof.
    intros d start n. unfold relax_all. simpl. reflexivity.
  Qed.

  Lemma empty_wfst_relaxation_closed : forall start n d,
    distance_relaxation_closed (mkWfst [] start n : Wfst W) d.
  Proof.
    intros start n d t Hin.
    unfold transition_in_wfst in Hin.
    unfold get_outgoing, get_state in Hin.
    destruct (tr_from t);
    simpl in Hin.
    - destruct Hin.
    - destruct Hin.
  Qed.

  Lemma init_distances_empty_solution :
    shortest_distance_solution
      (mkWfst [] NO_STATE 0 : Wfst W)
      (init_distances (mkWfst [] NO_STATE 0 : Wfst W)).
  Proof.
    unfold shortest_distance_solution.
    split.
    - apply empty_wfst_well_formed.
    - split.
      + apply init_distance_start.
      + apply empty_wfst_relaxation_closed.
  Qed.

  Lemma path_weight_is_single_source_product : forall t,
    path_weight [t] ≡ tr_weight t.
  Proof.
    apply path_weight_singleton.
  Qed.

End ShortestDistanceSpec.

(** ** Queue Type Model *)

Inductive QueueType :=
  | FifoQueue
  | TopologicalQueue
  | ShortestFirstQueue
  | AutoQueue.

Lemma queue_type_discrete : forall q : QueueType,
  q = FifoQueue \/ q = TopologicalQueue \/ q = ShortestFirstQueue \/ q = AutoQueue.
Proof.
  intro q. destruct q; auto.
Qed.
