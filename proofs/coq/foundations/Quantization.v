(** * Quantization and Approximate Equality

    Exact real-valued model for the quantization grid used by the Rust
    quantized weight helpers.  This file deliberately models the mathematical
    grid and error contract; it does not model IEEE-754 rounding.
*)

Require Import Coq.Arith.Arith.
Require Import Coq.micromega.Lia.
Require Import Coq.micromega.Lra.
Require Import Coq.micromega.Psatz.
Require Import Coq.Reals.Reals.

Open Scope R_scope.

(** ** Quantization Parameters *)

Record QuantizationParams := mkQuantizationParams {
  q_min : R;
  q_max : R;
  q_max_raw : nat;
  q_max_raw_pos : (q_max_raw > 0)%nat;
  q_range_pos : q_min < q_max;
}.

Definition q_step (params : QuantizationParams) : R :=
  (q_max params - q_min params) / INR (q_max_raw params).

Definition raw_valid (params : QuantizationParams) (raw : nat) : Prop :=
  (raw <= q_max_raw params)%nat.

Definition dequantize (params : QuantizationParams) (raw : nat) : R :=
  q_min params + INR raw * q_step params.

Lemma q_max_raw_INR_pos : forall params,
  0 < INR (q_max_raw params).
Proof.
  intro params.
  apply lt_0_INR.
  apply q_max_raw_pos.
Qed.

Lemma q_step_pos : forall params,
  0 < q_step params.
Proof.
  intro params.
  unfold q_step.
  pose proof q_max_raw_INR_pos params.
  pose proof q_range_pos params.
  apply Rdiv_lt_0_compat; lra.
Qed.

Lemma q_step_nonneg : forall params,
  0 <= q_step params.
Proof.
  intro params.
  pose proof q_step_pos params.
  lra.
Qed.

Lemma dequantize_min : forall params,
  dequantize params 0 = q_min params.
Proof.
  intro params.
  unfold dequantize.
  simpl.
  lra.
Qed.

Lemma dequantize_max : forall params,
  dequantize params (q_max_raw params) = q_max params.
Proof.
  intro params.
  unfold dequantize, q_step.
  pose proof q_max_raw_INR_pos params.
  field_simplify; lra.
Qed.

Lemma dequantize_in_range : forall params raw,
  raw_valid params raw ->
  q_min params <= dequantize params raw <= q_max params.
Proof.
  intros params raw Hraw.
  unfold raw_valid in Hraw.
  split.
  - unfold dequantize.
    pose proof q_step_nonneg params as Hstep.
    pose proof pos_INR raw as Hraw_nonneg.
    nra.
  - rewrite <- (dequantize_max params).
    unfold dequantize.
    apply Rplus_le_compat_l.
    apply Rmult_le_compat_r.
    + apply q_step_nonneg.
    + apply le_INR. exact Hraw.
Qed.

Lemma dequantize_monotone : forall params raw1 raw2,
  (raw1 <= raw2)%nat ->
  dequantize params raw1 <= dequantize params raw2.
Proof.
  intros params raw1 raw2 Hle.
  unfold dequantize.
  pose proof q_step_nonneg params as Hstep.
  pose proof le_INR raw1 raw2 Hle as Hraw_le.
  nra.
Qed.

Lemma dequantize_adjacent_step : forall params raw,
  dequantize params (S raw) - dequantize params raw = q_step params.
Proof.
  intros params raw.
  unfold dequantize.
  rewrite S_INR.
  ring.
Qed.

(** ** Epsilon Approximate Equality *)

Definition eps_approx (epsilon x y : R) : Prop :=
  Rabs (x - y) <= epsilon.

Lemma eps_approx_refl : forall epsilon x,
  0 <= epsilon ->
  eps_approx epsilon x x.
Proof.
  intros epsilon x Heps.
  unfold eps_approx.
  replace (x - x) with 0 by ring.
  rewrite Rabs_R0.
  exact Heps.
Qed.

Lemma eps_approx_sym : forall epsilon x y,
  eps_approx epsilon x y ->
  eps_approx epsilon y x.
Proof.
  intros epsilon x y Happrox.
  unfold eps_approx in *.
  replace (y - x) with (-(x - y)) by ring.
  rewrite Rabs_Ropp.
  exact Happrox.
Qed.

Lemma eps_approx_weaken : forall epsilon1 epsilon2 x y,
  epsilon1 <= epsilon2 ->
  eps_approx epsilon1 x y ->
  eps_approx epsilon2 x y.
Proof.
  intros epsilon1 epsilon2 x y Hle Happrox.
  unfold eps_approx in *.
  lra.
Qed.

Lemma eps_approx_triangle : forall epsilon1 epsilon2 x y z,
  eps_approx epsilon1 x y ->
  eps_approx epsilon2 y z ->
  eps_approx (epsilon1 + epsilon2) x z.
Proof.
  intros epsilon1 epsilon2 x y z Hxy Hyz.
  unfold eps_approx in *.
  replace (x - z) with ((x - y) + (y - z)) by ring.
  eapply Rle_trans.
  - apply Rabs_triang.
  - lra.
Qed.

(** ** Quantization Error Contract *)

Definition quantizes_to
    (params : QuantizationParams) (value : R) (raw : nat) : Prop :=
  raw_valid params raw /\
  eps_approx (q_step params / 2) value (dequantize params raw).

(** A raw bucket covers the real values within a half-step of its grid point.
    This is the proof-side contract for nearest-grid rounding after clamping to
    the finite quantization range. *)
Definition raw_bucket_covers
    (params : QuantizationParams) (value : R) (raw : nat) : Prop :=
  raw_valid params raw /\
  dequantize params raw - q_step params / 2 <= value <=
    dequantize params raw + q_step params / 2.

Lemma quantizes_to_valid : forall params value raw,
  quantizes_to params value raw ->
  raw_valid params raw.
Proof.
  intros params value raw [Hvalid _].
  exact Hvalid.
Qed.

Lemma quantizes_to_error_bound : forall params value raw,
  quantizes_to params value raw ->
  eps_approx (q_step params / 2) value (dequantize params raw).
Proof.
  intros params value raw [_ Herror].
  exact Herror.
Qed.

Lemma raw_bucket_covers_quantizes_to : forall params value raw,
  raw_bucket_covers params value raw ->
  quantizes_to params value raw.
Proof.
  intros params value raw [Hvalid Hcovers].
  split.
  - exact Hvalid.
  - unfold eps_approx.
    apply Rabs_le.
    lra.
Qed.

Lemma raw_bucket_covers_error_bound : forall params value raw,
  raw_bucket_covers params value raw ->
  eps_approx (q_step params / 2) value (dequantize params raw).
Proof.
  intros params value raw Hcovers.
  apply quantizes_to_error_bound.
  apply raw_bucket_covers_quantizes_to.
  exact Hcovers.
Qed.

Lemma grid_value_bucket_covers : forall params raw,
  raw_valid params raw ->
  raw_bucket_covers params (dequantize params raw) raw.
Proof.
  intros params raw Hvalid.
  split.
  - exact Hvalid.
  - pose proof q_step_pos params.
    lra.
Qed.

Lemma grid_value_quantizes_exactly : forall params raw,
  raw_valid params raw ->
  quantizes_to params (dequantize params raw) raw.
Proof.
  intros params raw Hvalid.
  split.
  - exact Hvalid.
  - apply eps_approx_refl.
    pose proof q_step_pos params.
    lra.
Qed.

Lemma quantized_value_in_range : forall params value raw,
  quantizes_to params value raw ->
  q_min params <= dequantize params raw <= q_max params.
Proof.
  intros params value raw Hquant.
  apply dequantize_in_range.
  apply quantizes_to_valid with (value := value).
  exact Hquant.
Qed.

Lemma same_raw_values_close : forall params x y raw,
  quantizes_to params x raw ->
  quantizes_to params y raw ->
  eps_approx (q_step params) x y.
Proof.
  intros params x y raw Hx Hy.
  apply eps_approx_weaken with
    (epsilon1 := q_step params / 2 + q_step params / 2).
  - lra.
  - eapply eps_approx_triangle.
    + apply quantizes_to_error_bound. exact Hx.
    + apply eps_approx_sym.
      apply quantizes_to_error_bound. exact Hy.
Qed.

Lemma adjacent_grid_values_close : forall params raw,
  eps_approx (q_step params)
    (dequantize params raw)
    (dequantize params (S raw)).
Proof.
  intros params raw.
  unfold eps_approx.
  replace (dequantize params raw - dequantize params (S raw))
    with (-(q_step params)).
  - rewrite Rabs_Ropp.
    rewrite Rabs_right.
    + lra.
    + pose proof q_step_nonneg params. lra.
  - pose proof dequantize_adjacent_step params raw.
    lra.
Qed.
