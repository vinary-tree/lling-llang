(** * Fail-Closed WFST Minimization Input Validation

    The minimizer must validate the original, immutable input before trimming,
    weight pushing, partition refinement, estimation, or output construction.
    This model scans arcs in state/transition order.  An [Arc] records the state
    whose outgoing slice owns it, the transition's declared source and target,
    and its index within that slice.  [input_identity] is an opaque caller-supplied
    identity for the input snapshot; validation preserves it in diagnostics.

    Theorems below establish acceptance completeness, first-error provenance,
    and fail-closed sequencing.  They do not claim that the existing weighted
    minimization algorithm is globally correct; that separate proof is partial.
*)

From Stdlib Require Import Lists.List Arith.PeanoNat Bool.Bool.
Require Import LlingLlang.wfst.Definitions.

Import ListNotations.

Record Arc := mkArc {
  arc_owner : nat;
  arc_source : nat;
  arc_target : nat;
  arc_ordinal : nat;
}.

Record EndpointError := mkEndpointError {
  error_input_identity : nat;
  error_state_count : nat;
  error_owner : nat;
  error_source : nat;
  error_target : nat;
  error_ordinal : nat;
}.

Definition valid_arcb (state_count : nat) (arc : Arc) : bool :=
  (Nat.eqb (arc_source arc) (arc_owner arc) &&
   Nat.ltb (arc_source arc) state_count) &&
  Nat.ltb (arc_target arc) state_count.

Definition endpoint_error
    (input_identity state_count : nat) (arc : Arc) : EndpointError :=
  mkEndpointError input_identity state_count
    (arc_owner arc) (arc_source arc) (arc_target arc) (arc_ordinal arc).

Fixpoint first_invalid
    (state_count input_identity : nat) (arcs : list Arc)
    : option EndpointError :=
  match arcs with
  | [] => None
  | arc :: rest =>
      if valid_arcb state_count arc
      then first_invalid state_count input_identity rest
      else Some (endpoint_error input_identity state_count arc)
  end.

Lemma valid_arcb_exact : forall state_count arc,
  valid_arcb state_count arc = true <->
  arc_source arc = arc_owner arc /\
  arc_source arc < state_count /\
  arc_target arc < state_count.
Proof.
  intros state_count arc.
  unfold valid_arcb.
  split.
  - intro Hvalid.
    apply andb_true_iff in Hvalid as [Hsource Htarget].
    apply andb_true_iff in Hsource as [Howner Hsource].
    apply Nat.eqb_eq in Howner.
    apply Nat.ltb_lt in Hsource.
    apply Nat.ltb_lt in Htarget.
    repeat split; assumption.
  - intros [Howner [Hsource Htarget]].
    apply andb_true_iff; split.
    + apply andb_true_iff; split.
      * apply Nat.eqb_eq; exact Howner.
      * apply Nat.ltb_lt; exact Hsource.
    + apply Nat.ltb_lt; exact Htarget.
Qed.

Lemma first_invalid_none_iff : forall state_count input_identity arcs,
  first_invalid state_count input_identity arcs = None <->
  Forall (fun arc => valid_arcb state_count arc = true) arcs.
Proof.
  intros state_count input_identity arcs.
  induction arcs as [| arc rest IH].
  - simpl. split; intro H.
    + constructor.
    + reflexivity.
  - simpl.
    destruct (valid_arcb state_count arc) eqn:Harc.
    + split; intro H.
      * constructor; [exact Harc | apply IH; exact H].
      * inversion H; subst. apply IH; assumption.
    + split; intro H.
      * discriminate H.
      * inversion H; subst. congruence.
Qed.

Lemma first_invalid_first_error :
  forall state_count input_identity arcs error,
    first_invalid state_count input_identity arcs = Some error ->
    exists prefix arc suffix,
      arcs = prefix ++ arc :: suffix /\
      Forall (fun candidate => valid_arcb state_count candidate = true) prefix /\
      valid_arcb state_count arc = false /\
      error = endpoint_error input_identity state_count arc.
Proof.
  intros state_count input_identity arcs.
  induction arcs as [| arc rest IH]; intros error Hscan.
  - discriminate Hscan.
  - simpl in Hscan.
    destruct (valid_arcb state_count arc) eqn:Harc.
    + apply IH in Hscan.
      destruct Hscan as [prefix [bad [suffix [Hdecomp [Hprefix [Hbad Herr]]]]]].
      exists (arc :: prefix), bad, suffix.
      split.
      * simpl. rewrite Hdecomp. reflexivity.
      * split.
        -- constructor; assumption.
        -- split; assumption.
    + inversion Hscan; subst.
      exists [], arc, rest.
      repeat split; try assumption; try constructor; reflexivity.
Qed.

Lemma first_invalid_error_identity :
  forall state_count input_identity arcs error,
    first_invalid state_count input_identity arcs = Some error ->
    error_input_identity error = input_identity /\
    error_state_count error = state_count.
Proof.
  intros state_count input_identity arcs error Hscan.
  apply first_invalid_first_error in Hscan.
  destruct Hscan as [prefix [arc [suffix [_ [_ [_ Herr]]]]]].
  rewrite Herr. split; reflexivity.
Qed.

Lemma first_invalid_complete :
  forall state_count input_identity arcs,
    ~ Forall (fun arc => valid_arcb state_count arc = true) arcs ->
    exists error, first_invalid state_count input_identity arcs = Some error.
Proof.
  intros state_count input_identity arcs Hbad.
  destruct (first_invalid state_count input_identity arcs) as [error|] eqn:Hscan.
  - exists error; reflexivity.
  - apply first_invalid_none_iff in Hscan. contradiction.
Qed.

(** Empty inputs use Rust's [NO_STATE] sentinel.  A nonempty input must start
    at an existing state.  The start check precedes every arc check. *)
Definition valid_startb (state_count start : nat) : bool :=
  if Nat.eqb state_count 0
  then Nat.eqb start NO_STATE
  else Nat.ltb start state_count.

(** [NO_STATE] is reserved, so every representable state identifier must be
    strictly below it.  This rejects a generic [Wfst] implementation reporting
    more states than Rust's [StateId] can address, before any narrowing cast. *)
Definition valid_state_countb (state_count : nat) : bool :=
  Nat.leb state_count NO_STATE.

Inductive ValidationError :=
| InvalidStateCount : nat -> nat -> ValidationError
| InvalidStart : nat -> nat -> nat -> ValidationError
| InvalidEndpoint : EndpointError -> ValidationError.

Definition validate_input
    (state_count start input_identity : nat) (arcs : list Arc)
    : option ValidationError :=
  if valid_state_countb state_count then
    if valid_startb state_count start
    then match first_invalid state_count input_identity arcs with
         | None => None
         | Some error => Some (InvalidEndpoint error)
         end
    else Some (InvalidStart input_identity state_count start)
  else Some (InvalidStateCount input_identity state_count).

Lemma validate_input_none_iff :
  forall state_count start input_identity arcs,
    validate_input state_count start input_identity arcs = None <->
    valid_state_countb state_count = true /\
    valid_startb state_count start = true /\
    Forall (fun arc => valid_arcb state_count arc = true) arcs.
Proof.
  intros state_count start input_identity arcs.
  unfold validate_input.
  destruct (valid_state_countb state_count) eqn:Hcount.
  2: { split; intro H.
       - discriminate H.
       - destruct H as [Hvalid _]. congruence. }
  destruct (valid_startb state_count start) eqn:Hstart.
  - destruct (first_invalid state_count input_identity arcs) as [error|] eqn:Hscan.
    + split; intro H.
      * discriminate H.
        * destruct H as [_ [_ Hvalid]].
        pose proof (proj2 (first_invalid_none_iff state_count input_identity arcs)
                     Hvalid) as Hnone.
        rewrite Hscan in Hnone. discriminate Hnone.
    + split; intro H.
      * repeat split; try reflexivity.
        apply (proj1 (first_invalid_none_iff state_count input_identity arcs)).
        exact Hscan.
      * reflexivity.
  - split; intro H.
    + discriminate H.
    + destruct H as [_ [Hvalid _]]. congruence.
Qed.

Lemma invalid_state_count_rejected_first :
  forall state_count start input_identity arcs,
    valid_state_countb state_count = false ->
    validate_input state_count start input_identity arcs =
      Some (InvalidStateCount input_identity state_count).
Proof.
  intros state_count start input_identity arcs Hcount.
  unfold validate_input. rewrite Hcount. reflexivity.
Qed.

Lemma invalid_start_rejected_before_arcs :
  forall state_count start input_identity arcs,
    valid_state_countb state_count = true ->
    valid_startb state_count start = false ->
    validate_input state_count start input_identity arcs =
      Some (InvalidStart input_identity state_count start).
Proof.
  intros state_count start input_identity arcs Hcount Hstart.
  unfold validate_input. rewrite Hcount, Hstart. reflexivity.
Qed.

Definition checked_then {R : Type}
    (state_count start input_identity : nat) (arcs : list Arc)
    (transform : list Arc -> R) : ValidationError + R :=
  match validate_input state_count start input_identity arcs with
  | Some error => inl error
  | None => inr (transform arcs)
  end.

Lemma checked_then_preserves_valid_input :
  forall (R : Type) state_count start input_identity arcs
         (transform : list Arc -> R),
    valid_startb state_count start = true ->
    valid_state_countb state_count = true ->
    Forall (fun arc => valid_arcb state_count arc = true) arcs ->
    checked_then state_count start input_identity arcs transform =
      inr (transform arcs).
Proof.
  intros R state_count start input_identity arcs transform Hstart Hcount Harcs.
  unfold checked_then.
  rewrite (proj2 (validate_input_none_iff state_count start input_identity arcs)
           (conj Hcount (conj Hstart Harcs))).
  reflexivity.
Qed.

Lemma checked_then_rejects_before_transform :
  forall (R : Type) state_count start input_identity arcs error
         (transform : list Arc -> R),
    validate_input state_count start input_identity arcs = Some error ->
    checked_then state_count start input_identity arcs transform = inl error.
Proof.
  intros R state_count start input_identity arcs error transform Hbad.
  unfold checked_then. rewrite Hbad. reflexivity.
Qed.

Example malformed_target_is_rejected :
  validate_input 2 0 17 [mkArc 0 0 1 0; mkArc 0 0 99 1] =
    Some (InvalidEndpoint (mkEndpointError 17 2 0 0 99 1)).
Proof. reflexivity. Qed.

Example malformed_owner_is_rejected :
  validate_input 2 0 17 [mkArc 0 1 1 0] =
    Some (InvalidEndpoint (mkEndpointError 17 2 0 1 1 0)).
Proof. reflexivity. Qed.

Example malformed_start_is_rejected_first :
  validate_input 2 99 17 [mkArc 0 0 99 0] =
    Some (InvalidStart 17 2 99).
Proof. reflexivity. Qed.
