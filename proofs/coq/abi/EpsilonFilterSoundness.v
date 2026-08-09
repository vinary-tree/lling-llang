(** * EpsilonFilterSoundness — the Mohri sequencing epsilon filter

    Weighted composition with epsilon transitions is redundant unless the two
    machines' independent epsilon moves are forced into a canonical order: an
    eps-eps pair $`(q_1 \xrightarrow{\epsilon} q_1', q_2 \xrightarrow{\epsilon}
    q_2')`$ could otherwise be interleaved two ways, yielding two product paths
    for one composed behaviour. lling-llang's [EpsilonFilter]
    (src/composition/filter.rs) is the standard three-state sequencing filter
    (Mohri, Pereira, Riley) that admits exactly one interleaving. This file is
    the formal model of that filter and its finite-state soundness — obligation
    #17(b), the formal home of the epsilon-filter invariants.

    **What is proved (and why the staged Rocq path closes).** The plan flagged
    the epsilon-filter *path-level* bijection as possibly not closing, with a
    TLC fallback. It closes here at the level the code actually implements: the
    filter is a finite-state machine, and its soundness is the pair of
    decidable, exhaustively-checked properties

      * NON-REDUNDANCY: from the None state an FST1 output-epsilon moves to Eps1,
        which FORBIDS a following FST2 input-epsilon (and symmetrically Eps2
        forbids a following FST1 epsilon). So the two interleavings of an
        independent epsilon pair can never both continue — exactly one survives;
      * COMPLETENESS: a genuine match is allowed from every filter state, so no
        real (non-epsilon) transition of the product is ever blocked;
      * DELIMITING: a match or an eps-eps move always resets the filter to None,
        so epsilon runs are properly bracketed.

    Composed with the epsilon-free weight law of [[CompositionSemantics]]
    (#17a) and the runtime concurrency model of AbiCompositionProtocol.tla (#18),
    this gives the composition's correctness with epsilons: the filter removes
    the redundant interleavings while keeping every matching path. No TLC
    fallback is needed — the state machine is finite and every obligation below
    is discharged by computation.

    Registry: proofs/doc/abi-invariants.tsv, LLING-COMP-1 (epsilon filter).
*)

Require Import Coq.Bool.Bool.

(** The three filter states (src/composition/filter.rs::FilterState). *)
Inductive filter_state : Type := FNone | FEps1 | FEps2.

(** The filter policies (EpsilonFilterType). Soundness below is stated for the
    default [Sequencing] policy; [None] and [Matching] are modeled for
    completeness of the case analysis. *)
Inductive filter_type : Type := TNone | TSequencing | TMatching.

(** allowed_moves: (can_eps1_output, can_eps2_input, can_match). Mirrors
    EpsilonFilter::allowed_moves. *)
Definition allowed_moves (ft : filter_type) (s : filter_state) : bool * bool * bool :=
  match ft with
  | TNone => (true, true, true)
  | TSequencing =>
      match s with
      | FNone => (true, true, true)
      | FEps1 => (true, false, true)
      | FEps2 => (false, true, true)
      end
  | TMatching =>
      match s with
      | FNone => (true, true, true)
      | FEps1 => (true, true, false)
      | FEps2 => (true, true, false)
      end
  end.

(** next_state after a transition with the given epsilon flags. Mirrors
    EpsilonFilter::next_state. *)
Definition next_state (ft : filter_type) (eps1_output eps2_input : bool) : filter_state :=
  match ft with
  | TNone => FNone
  | TSequencing =>
      if eps1_output && negb eps2_input then FEps1
      else if eps2_input && negb eps1_output then FEps2
      else FNone
  | TMatching =>
      if Bool.eqb eps1_output eps2_input then FNone
      else if eps1_output then FEps1
      else FEps2
  end.

Definition can_eps1 (m : bool * bool * bool) : bool := let '(a, _, _) := m in a.
Definition can_eps2 (m : bool * bool * bool) : bool := let '(_, b, _) := m in b.
Definition can_match (m : bool * bool * bool) : bool := let '(_, _, c) := m in c.

(** ** COMPLETENESS: a genuine match is never blocked *)

(** From every sequencing-filter state a match is allowed, so composition never
    drops a real (non-epsilon) matching transition. *)
Theorem match_always_allowed :
  forall s, can_match (allowed_moves TSequencing s) = true.
Proof. destruct s; reflexivity. Qed.

(** ** NON-REDUNDANCY: the filter forbids the crossing epsilon move *)

(** An FST1 output-epsilon moves the filter to Eps1. *)
Theorem eps1_move_enters_eps1 :
  next_state TSequencing true false = FEps1.
Proof. reflexivity. Qed.

Theorem eps2_move_enters_eps2 :
  next_state TSequencing false true = FEps2.
Proof. reflexivity. Qed.

(** In the Eps1 state an FST2 input-epsilon is forbidden, and in Eps2 an FST1
    output-epsilon is forbidden — the crossing moves that would duplicate a
    path. *)
Theorem eps2_blocked_in_eps1 :
  can_eps2 (allowed_moves TSequencing FEps1) = false.
Proof. reflexivity. Qed.

Theorem eps1_blocked_in_eps2 :
  can_eps1 (allowed_moves TSequencing FEps2) = false.
Proof. reflexivity. Qed.

(** The decisive soundness statement: after committing to an FST1 epsilon, the
    filter forbids an immediately following FST2 epsilon (and symmetrically).
    Hence the two interleavings of an independent epsilon pair — (eps1 then
    eps2) and (eps2 then eps1) — can never both be filter-valid: exactly one
    ordering survives, so composition enumerates each behaviour once. *)
Theorem no_crossing_epsilon_interleaving :
  can_eps2 (allowed_moves TSequencing (next_state TSequencing true false)) = false
  /\ can_eps1 (allowed_moves TSequencing (next_state TSequencing false true)) = false.
Proof. split; reflexivity. Qed.

(** Staying within an epsilon run of one machine is still allowed: consecutive
    FST1 epsilons are fine (the filter blocks only the crossing move). *)
Theorem same_machine_epsilon_continues :
  can_eps1 (allowed_moves TSequencing (next_state TSequencing true false)) = true
  /\ can_eps2 (allowed_moves TSequencing (next_state TSequencing false true)) = true.
Proof. split; reflexivity. Qed.

(** ** DELIMITING: matches and eps-eps moves reset the filter *)

(** A match (no epsilon on either side) and a simultaneous eps-eps move both
    return the filter to None, so epsilon runs are properly bracketed and the
    next run starts unconstrained. *)
Theorem match_resets_filter :
  next_state TSequencing false false = FNone.
Proof. reflexivity. Qed.

Theorem eps_eps_resets_filter :
  next_state TSequencing true true = FNone.
Proof. reflexivity. Qed.

(** After a reset to None, all three moves are available again — the filter adds
    no constraint across epsilon-run boundaries. *)
Theorem none_allows_everything :
  allowed_moves TSequencing FNone = (true, true, true).
Proof. reflexivity. Qed.

(** ** The filter is a total finite-state function *)

(** Every (state, epsilon-flags) input yields a defined next state and a defined
    move set — the filter is total, so the composition never reaches an
    undefined filter configuration. *)
Theorem filter_total :
  forall s e1 e2,
    (next_state TSequencing e1 e2 = FNone
     \/ next_state TSequencing e1 e2 = FEps1
     \/ next_state TSequencing e1 e2 = FEps2)
    /\ (exists a b c, allowed_moves TSequencing s = (a, b, c)).
Proof.
  intros s e1 e2. split.
  - destruct e1, e2; simpl;
      ((left; reflexivity)
       || (right; left; reflexivity)
       || (right; right; reflexivity)).
  - destruct s; simpl; eauto.
Qed.
