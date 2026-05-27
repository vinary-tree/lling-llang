(** * WFST Definitions

    Core definitions for Weighted Finite State Transducers.

    This module defines:
    - States and state identifiers
    - Transitions with input/output labels and weights
    - WFST structure

    These definitions correspond to the types in lling-llang's
    [src/wfst/] module.
*)

Require Import Coq.Lists.List.
Require Import Coq.Arith.Arith.
Require Import LlingLlang.foundations.Semiring.

Import ListNotations.

(** ** Basic Types *)

(** State identifier - corresponds to StateId in Rust *)
Definition StateId := nat.

(** Label type - generic over the label alphabet *)
Definition Label := nat.

(** Special value for "no state".

    Rust uses [u32::MAX] as the sentinel.  We keep [StateId] as [nat] in
    Rocq so list indexing remains simple, but use the same numeric sentinel. *)
Definition NO_STATE : StateId := 4294967295.

(** Epsilon label (no symbol) *)
Definition EPSILON : option Label := None.

(** ** Transitions *)

(** A weighted transition in a WFST *)
Record Transition (W : Type) := mkTransition {
  tr_from : StateId;
  tr_input : option Label;
  tr_output : option Label;
  tr_to : StateId;
  tr_weight : W;
}.

Arguments mkTransition {W}.
Arguments tr_from {W}.
Arguments tr_input {W}.
Arguments tr_output {W}.
Arguments tr_to {W}.
Arguments tr_weight {W}.

(** ** WFST State *)

(** A state in a WFST with outgoing transitions and final weight *)
Record WfstState (W : Type) := mkWfstState {
  ws_id : StateId;
  ws_outgoing : list (Transition W);
  ws_final : option W;  (* None if not final, Some w if final with weight w *)
}.

Arguments mkWfstState {W}.
Arguments ws_id {W}.
Arguments ws_outgoing {W}.
Arguments ws_final {W}.

(** ** WFST Structure *)

(** A Weighted Finite State Transducer *)
Record Wfst (W : Type) := mkWfst {
  wfst_states : list (WfstState W);
  wfst_start : StateId;
  wfst_num_states : nat;
}.

Arguments mkWfst {W}.
Arguments wfst_states {W}.
Arguments wfst_start {W}.
Arguments wfst_num_states {W}.

(** ** WFST Accessors *)

Section WfstAccessors.
  Context {W : Type}.

  (** Get a state by ID *)
  Definition get_state (fst : Wfst W) (s : StateId) : option (WfstState W) :=
    nth_error (wfst_states fst) s.

  (** Check if a state is valid *)
  Definition is_valid_state (fst : Wfst W) (s : StateId) : Prop :=
    s < wfst_num_states fst.

  (** Get outgoing transitions from a state *)
  Definition get_outgoing (fst : Wfst W) (s : StateId) : list (Transition W) :=
    match get_state fst s with
    | Some state => ws_outgoing state
    | None => []
    end.

  (** Check if a state is final *)
  Definition is_final (fst : Wfst W) (s : StateId) : bool :=
    match get_state fst s with
    | Some state => match ws_final state with
                    | Some _ => true
                    | None => false
                    end
    | None => false
    end.

  (** Get final weight (returns None for non-final states) *)
  Definition final_weight (fst : Wfst W) (s : StateId) : option W :=
    match get_state fst s with
    | Some state => ws_final state
    | None => None
    end.

End WfstAccessors.

(** ** Well-formedness *)

Section WellFormed.
  Context {W : Type}.

  (** A transition is well-formed if it refers to valid states. *)
  Definition transition_well_formed (fst : Wfst W) (t : Transition W) : Prop :=
    is_valid_state fst (tr_from t) /\ is_valid_state fst (tr_to t).

  (** A transition stored in a state's outgoing list must agree with the
      state's id.  This mirrors Rust's vector-backed representation where
      transitions are stored under their source state. *)
  Definition transition_well_formed_from
      (fst : Wfst W) (source : StateId) (t : Transition W) : Prop :=
    tr_from t = source /\ transition_well_formed fst t.

  (** A state is well-formed if its id is valid and all outgoing transitions are
      well-formed transitions from that state. *)
  Definition state_well_formed (fst : Wfst W) (state : WfstState W) : Prop :=
    is_valid_state fst (ws_id state) /\
    Forall (transition_well_formed_from fst (ws_id state)) (ws_outgoing state).

  (** Vector-backed WFSTs use state ids as list indices. *)
  Definition states_indexed (fst : Wfst W) : Prop :=
    forall s state, get_state fst s = Some state -> ws_id state = s.

  (** Non-empty WFSTs have a valid start state.  Empty WFSTs use [NO_STATE],
      matching the Rust API. *)
  Definition start_well_formed (fst : Wfst W) : Prop :=
    match wfst_states fst with
    | [] => wfst_start fst = NO_STATE
    | _ => is_valid_state fst (wfst_start fst)
    end.

  (** A WFST is well-formed if:
      - The start state is valid, or the WFST is empty and uses [NO_STATE]
      - All states are well-formed
      - State ids match their list indices
      - The number of states matches the list length *)
  Definition wfst_well_formed (fst : Wfst W) : Prop :=
    start_well_formed fst /\
    Forall (state_well_formed fst) (wfst_states fst) /\
    states_indexed fst /\
    length (wfst_states fst) = wfst_num_states fst.

  Lemma empty_wfst_well_formed :
    wfst_well_formed (mkWfst [] NO_STATE 0 : Wfst W).
  Proof.
    unfold wfst_well_formed, start_well_formed, states_indexed, get_state.
    simpl.
    split; [reflexivity |].
    split; [constructor |].
    split; [| reflexivity].
    - intros s state Hget. destruct s; discriminate.
  Qed.

End WellFormed.

(** ** Determinism *)

Section Determinism.
  Context {W : Type}.

  (** A state is deterministic if it has at most one outgoing transition
      for each input label *)
  Definition state_deterministic (state : WfstState W) : Prop :=
    forall t1 t2 : Transition W,
      In t1 (ws_outgoing state) ->
      In t2 (ws_outgoing state) ->
      tr_input t1 = tr_input t2 ->
      t1 = t2.

  (** A WFST is deterministic if all states are deterministic *)
  Definition wfst_deterministic (fst : Wfst W) : Prop :=
    forall state : WfstState W,
      In state (wfst_states fst) ->
      state_deterministic state.

End Determinism.

(** ** Acceptor (FSA) *)

Section Acceptor.
  Context {W : Type}.

  (** A transition is identity (input = output) *)
  Definition transition_is_identity (t : Transition W) : Prop :=
    tr_input t = tr_output t.

  (** A WFST is an acceptor if all transitions are identity *)
  Definition wfst_is_acceptor (fst : Wfst W) : Prop :=
    forall state : WfstState W,
      In state (wfst_states fst) ->
      forall t : Transition W,
        In t (ws_outgoing state) ->
        transition_is_identity t.

End Acceptor.
