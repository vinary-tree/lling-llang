(** * Exact Interval Arithmetic

    A small real-valued interval model for bounding numerical error.  This is
    the proof-side counterpart to implementation-level approximate equality:
    operations are sound when the exact real result remains inside the computed
    interval.
*)

Require Import Coq.micromega.Lra.
Require Import Coq.Reals.Reals.

Open Scope R_scope.

Record Interval := mkInterval {
  i_lower : R;
  i_upper : R;
  i_well_formed : i_lower <= i_upper;
}.

Definition interval_contains (i : Interval) (x : R) : Prop :=
  i_lower i <= x <= i_upper i.

Definition interval_width (i : Interval) : R :=
  i_upper i - i_lower i.

Definition interval_midpoint (i : Interval) : R :=
  (i_lower i + i_upper i) / 2.

Definition point_interval (x : R) : Interval.
Proof.
  refine (mkInterval x x _).
  lra.
Defined.

Definition interval_add (a b : Interval) : Interval.
Proof.
  refine (mkInterval
    (i_lower a + i_lower b)
    (i_upper a + i_upper b)
    _).
  pose proof i_well_formed a.
  pose proof i_well_formed b.
  lra.
Defined.

Definition interval_neg (a : Interval) : Interval.
Proof.
  refine (mkInterval (-(i_upper a)) (-(i_lower a)) _).
  pose proof i_well_formed a.
  lra.
Defined.

Definition interval_sub (a b : Interval) : Interval :=
  interval_add a (interval_neg b).

Definition interval_widen (a : Interval) (epsilon : R)
    (Hepsilon : 0 <= epsilon) : Interval.
Proof.
  refine (mkInterval
    (i_lower a - epsilon)
    (i_upper a + epsilon)
    _).
  pose proof i_well_formed a.
  lra.
Defined.

Lemma interval_width_nonneg : forall i,
  0 <= interval_width i.
Proof.
  intro i.
  unfold interval_width.
  pose proof i_well_formed i.
  lra.
Qed.

Lemma point_interval_contains : forall x,
  interval_contains (point_interval x) x.
Proof.
  intro x.
  unfold interval_contains, point_interval.
  simpl.
  lra.
Qed.

Lemma interval_add_sound : forall a b x y,
  interval_contains a x ->
  interval_contains b y ->
  interval_contains (interval_add a b) (x + y).
Proof.
  intros a b x y Ha Hb.
  unfold interval_contains in *.
  simpl in *.
  lra.
Qed.

Lemma interval_neg_sound : forall a x,
  interval_contains a x ->
  interval_contains (interval_neg a) (-x).
Proof.
  intros a x Ha.
  unfold interval_contains in *.
  simpl in *.
  lra.
Qed.

Lemma interval_sub_sound : forall a b x y,
  interval_contains a x ->
  interval_contains b y ->
  interval_contains (interval_sub a b) (x - y).
Proof.
  intros a b x y Ha Hb.
  unfold interval_sub.
  replace (x - y) with (x + -y) by ring.
  apply interval_add_sound.
  - exact Ha.
  - apply interval_neg_sound. exact Hb.
Qed.

Lemma interval_widen_contains_original : forall a epsilon Hepsilon x,
  interval_contains a x ->
  interval_contains (interval_widen a epsilon Hepsilon) x.
Proof.
  intros a epsilon Hepsilon x Ha.
  unfold interval_contains in *.
  simpl in *.
  lra.
Qed.

Lemma interval_widen_width : forall a epsilon Hepsilon,
  interval_width (interval_widen a epsilon Hepsilon) =
    interval_width a + 2 * epsilon.
Proof.
  intros a epsilon Hepsilon.
  unfold interval_width.
  simpl.
  ring.
Qed.

Lemma interval_contains_midpoint : forall a,
  interval_contains a (interval_midpoint a).
Proof.
  intro a.
  unfold interval_contains, interval_midpoint.
  pose proof i_well_formed a.
  split; lra.
Qed.

Lemma interval_point_width_zero : forall x,
  interval_width (point_interval x) = 0.
Proof.
  intro x.
  unfold interval_width, point_interval.
  simpl.
  ring.
Qed.
