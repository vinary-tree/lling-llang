(** * Floating Roundoff Contracts

    Abstract real-valued model of a rounded floating operation.  The model does
    not encode IEEE-754 bit patterns; instead it states the error contract that
    an implementation-level floating operation must satisfy and proves how that
    contract composes with interval enclosures.
*)

Require Import Coq.micromega.Lra.
Require Import Coq.Reals.Reals.
Require Import LlingLlang.foundations.Interval.

Open Scope R_scope.

Definition roundoff_error (epsilon exact rounded : R) : Prop :=
  0 <= epsilon /\ Rabs (rounded - exact) <= epsilon.

Definition rounded_add (epsilon x y rounded : R) : Prop :=
  roundoff_error epsilon (x + y) rounded.

Definition rounded_sub (epsilon x y rounded : R) : Prop :=
  roundoff_error epsilon (x - y) rounded.

Lemma roundoff_error_exact : forall exact,
  roundoff_error 0 exact exact.
Proof.
  intro exact.
  unfold roundoff_error.
  split; [lra |].
  replace (exact - exact) with 0 by ring.
  rewrite Rabs_R0.
  lra.
Qed.

Lemma roundoff_error_bound : forall epsilon exact rounded,
  roundoff_error epsilon exact rounded ->
  exact - epsilon <= rounded <= exact + epsilon.
Proof.
  intros epsilon exact rounded [Heps Habs].
  assert (Hupper : rounded - exact <= epsilon).
  { eapply Rle_trans.
    - apply Rle_abs.
    - exact Habs. }
  assert (Hlower : exact - rounded <= epsilon).
  { replace (exact - rounded) with (-(rounded - exact)) by ring.
    eapply Rle_trans.
    - apply Rle_abs.
    - rewrite Rabs_Ropp. exact Habs. }
  lra.
Qed.

Lemma roundoff_error_sym_distance : forall epsilon exact rounded,
  roundoff_error epsilon exact rounded ->
  Rabs (exact - rounded) <= epsilon.
Proof.
  intros epsilon exact rounded [_ Habs].
  replace (exact - rounded) with (-(rounded - exact)) by ring.
  rewrite Rabs_Ropp.
  exact Habs.
Qed.

Lemma rounded_add_interval_sound : forall epsilon Heps a b x y rounded,
  interval_contains a x ->
  interval_contains b y ->
  rounded_add epsilon x y rounded ->
  interval_contains
    (interval_widen (interval_add a b) epsilon Heps)
    rounded.
Proof.
  intros epsilon Heps a b x y rounded Hx Hy Hadd.
  pose proof interval_add_sound a b x y Hx Hy as Hsum.
  pose proof roundoff_error_bound epsilon (x + y) rounded Hadd as Hround.
  unfold interval_contains in *.
  simpl in *.
  lra.
Qed.

Lemma rounded_sub_interval_sound : forall epsilon Heps a b x y rounded,
  interval_contains a x ->
  interval_contains b y ->
  rounded_sub epsilon x y rounded ->
  interval_contains
    (interval_widen (interval_sub a b) epsilon Heps)
    rounded.
Proof.
  intros epsilon Heps a b x y rounded Hx Hy Hsub.
  pose proof interval_sub_sound a b x y Hx Hy as Hdiff.
  pose proof roundoff_error_bound epsilon (x - y) rounded Hsub as Hround.
  unfold interval_contains in *.
  simpl in *.
  lra.
Qed.

Lemma rounded_add_zero_error_exact : forall x y,
  rounded_add 0 x y (x + y).
Proof.
  intros x y.
  unfold rounded_add.
  apply roundoff_error_exact.
Qed.

Lemma rounded_sub_zero_error_exact : forall x y,
  rounded_sub 0 x y (x - y).
Proof.
  intros x y.
  unfold rounded_sub.
  apply roundoff_error_exact.
Qed.
