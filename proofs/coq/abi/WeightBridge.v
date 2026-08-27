(** * WeightBridge — the f64 ABI weight bridge over the seven VtWeightDomains

    The scalar-WFST ABI carries every arc/final weight as a raw IEEE-754 `f64`
    tagged by a [VtWeightDomain]. Before such a value may enter a typed weight
    it must be validated against the domain it claims: this file is the formal
    model of that ingestion gate — the `repr_ok` predicate and its `decode`
    partial function — for all seven domains
    (`vinary-tree-interop::VtWeightDomain`, discriminants 1..7):

      1 TropicalF64  2 LogF64  3 ProbabilityF64  4 ArcticF64
      5 SignedTropicalF64  6 CountF64  7 BooleanF64

    It closes obligation #16 and is the formal home of invariants
    LLING-BRIDGE-1..4 (registry: proofs/doc/abi-invariants.tsv). It is the proof
    backing for family finding F1 / ledger LLING-B2: the `−∞` value that the
    tropical/log/signed-tropical ingestion sites used to admit (via a NaN-only
    check) is proved *outside* those domains' representable sets here.

    **What is modeled and what is explicitly NOT.** We model a raw f64 as an
    element of the extended reals with a NaN token,
    $`\mathbb{R} \cup \{+\infty, -\infty, \mathrm{NaN}\}`$ ([wire]), and prove:
    representability per domain is decidable and NaN-free (BRIDGE-1); `decode`
    round-trips exactly on the valid set (BRIDGE-2); the domain sentinels
    ($`+\infty`$ for tropical/log, $`-\infty`$ for arctic) survive, and the
    combine operation $`\oplus`$ (min for tropical, max for arctic) is *exact*
    — it returns one operand unchanged, no rounding (BRIDGE-3); and the
    out-of-domain infinities are rejected (BRIDGE-4).

    We do NOT model IEEE-754 rounding of the $`\otimes`$ operation (real
    addition): that operation is genuinely inexact under rounding, so no
    exactness theorem is claimed for it — an explicit non-claim, consistent
    with the family rule that f64 semiring laws are false over IEEE `+`. The
    per-domain algebra itself is proved in the foundations models
    ([[TropicalWeight]], [[ArcticWeight]], [[LogWeight]], [[ProbabilityWeight]],
    [[CountWeight]], [[BooleanWeight]], [[SignedTropicalWeight]]); this file is
    only the representation/decoding boundary between the wire and those
    carriers.

    Registry: proofs/doc/abi-invariants.tsv, LLING-BRIDGE-1..4.
*)

From Stdlib Require Import Reals.Reals.
From Stdlib Require Import micromega.Lra.

Open Scope R_scope.

(** The seven weight domains, in ABI discriminant order. *)
Inductive WeightDomain : Type :=
  | Tropical        (* 1 *)
  | Log             (* 2 *)
  | Probability     (* 3 *)
  | Arctic          (* 4 *)
  | SignedTropical  (* 5 *)
  | Count           (* 6 *)
  | Boolean.        (* 7 *)

(** A raw f64 abstracted as an extended real with a NaN token. This models
    representability and exact transfer; it deliberately does not model the
    rounding of arithmetic (see the header non-claim). *)
Inductive wire : Type :=
  | WFinite (r : R)
  | WPosInf
  | WNegInf
  | WNaN.

(** The exact-transfer valid set per domain: a wire value that crosses the ABI
    unchanged and lands in the domain's carrier. Mirrors each Rust
    `is_valid_raw` where one exists, and defines the canonical set where none
    does (Probability/Count: non-negative; Boolean: {0,1}).

    Count's carrier is the non-negative integers; the f64 wire additionally
    carries an integrality side-check (`fract == 0`) that is decidable on a
    concrete f64 but not expressible over abstract `R` — modeled here by the
    necessary non-negativity condition, with integrality noted as the f64-level
    residual (the same shape of non-claim as IEEE rounding). *)
Definition repr_ok (d : WeightDomain) (w : wire) : Prop :=
  match d, w with
  | Tropical, WFinite _ => True
  | Tropical, WPosInf => True
  | Log, WFinite _ => True
  | Log, WPosInf => True
  | Arctic, WFinite _ => True
  | Arctic, WNegInf => True
  (* SignedTropical: the semiring-valid set is the finite reals and +infinity;
     -infinity is representable but excluded from the semiring (see below). *)
  | SignedTropical, WFinite _ => True
  | SignedTropical, WPosInf => True
  | Probability, WFinite r => 0 <= r
  | Probability, WPosInf => True
  | Count, WFinite r => 0 <= r
  | Count, WPosInf => True
  | Boolean, WFinite r => r = 0 \/ r = 1
  | _, _ => False
  end.

(** ** BRIDGE-1: ingestion is a total, NaN-free decision *)

(** The ingestion check is decidable for every domain and value: the ABI can
    always answer accept/reject in finite time. *)
Definition repr_ok_dec (d : WeightDomain) (w : wire) :
  {repr_ok d w} + {~ repr_ok d w}.
Proof.
  destruct d, w; simpl;
    try (left; exact I);
    try (right; tauto).
  - apply Rle_dec.                    (* Probability, WFinite: 0 <= r *)
  - apply Rle_dec.                    (* Count, WFinite: 0 <= r *)
  - (* Boolean, WFinite: r = 0 \/ r = 1 *)
    destruct (Req_EM_T r 0) as [->|Hn0]; [left; now left|].
    destruct (Req_EM_T r 1) as [->|Hn1]; [left; now right|].
    right; intros [H|H]; [exact (Hn0 H) | exact (Hn1 H)].
Defined.

(** NaN is never a valid value in any domain — the universal ingestion safety
    law. *)
Theorem repr_ok_never_nan : forall d, ~ repr_ok d WNaN.
Proof. destruct d; simpl; tauto. Qed.

(** ** BRIDGE-4: the out-of-domain infinities are rejected *)

(** The exact family finding F1 / ledger LLING-B2 witness: `−∞` is outside the
    tropical, log, and signed-tropical domains, so a NaN-only ingestion check
    (which admits `−∞`) is unsound. *)
Theorem tropical_rejects_neg_inf : ~ repr_ok Tropical WNegInf.
Proof. simpl; tauto. Qed.

Theorem log_rejects_neg_inf : ~ repr_ok Log WNegInf.
Proof. simpl; tauto. Qed.

Theorem signed_tropical_rejects_neg_inf : ~ repr_ok SignedTropical WNegInf.
Proof. simpl; tauto. Qed.

(** The dual: `+∞` is outside the arctic (max-plus) domain. *)
Theorem arctic_rejects_pos_inf : ~ repr_ok Arctic WPosInf.
Proof. simpl; tauto. Qed.

Theorem f1_witness_is_rejected :
  ~ repr_ok Tropical WNegInf
  /\ ~ repr_ok Log WNegInf
  /\ ~ repr_ok SignedTropical WNegInf.
Proof. repeat split; simpl; tauto. Qed.

(** ** BRIDGE-2: decode round-trips exactly on the valid set *)

(** Decoding is "validate, then transfer unchanged": [Some w] exactly when
    [repr_ok], and the value is never altered. *)
Definition decode (d : WeightDomain) (w : wire) : option wire :=
  if repr_ok_dec d w then Some w else None.

Theorem decode_some_iff_repr_ok :
  forall d w, decode d w = Some w <-> repr_ok d w.
Proof.
  intros d w; unfold decode; destruct (repr_ok_dec d w) as [Hok|Hno]; split; intro H.
  - exact Hok.
  - reflexivity.
  - discriminate H.
  - contradiction.
Qed.

Theorem decode_none_iff_invalid :
  forall d w, decode d w = None <-> ~ repr_ok d w.
Proof.
  intros d w; unfold decode; destruct (repr_ok_dec d w) as [Hok|Hno]; split; intro H.
  - discriminate H.
  - contradiction.
  - exact Hno.
  - reflexivity.
Qed.

(** Exact round trip: a representable value decodes to itself, unchanged. *)
Theorem bridge_round_trips : forall d w, repr_ok d w -> decode d w = Some w.
Proof. intros d w H; apply decode_some_iff_repr_ok; exact H. Qed.

(** NaN never decodes — the safety corollary of BRIDGE-1 through [decode]. *)
Theorem nan_never_decodes : forall d, decode d WNaN = None.
Proof. intro d; apply decode_none_iff_invalid; apply repr_ok_never_nan. Qed.

(** ** BRIDGE-3: domain sentinels survive, and ⊕ is exact *)

(** The additive identity / "unreachable" sentinel of each ordered domain
    survives the bridge unchanged: `+∞` for tropical and log, `−∞` for arctic. *)
Theorem pos_inf_survives_tropical : decode Tropical WPosInf = Some WPosInf.
Proof. apply bridge_round_trips; simpl; exact I. Qed.

Theorem pos_inf_survives_log : decode Log WPosInf = Some WPosInf.
Proof. apply bridge_round_trips; simpl; exact I. Qed.

Theorem neg_inf_survives_arctic : decode Arctic WNegInf = Some WNegInf.
Proof. apply bridge_round_trips; simpl; exact I. Qed.

(** The combine operation ⊕ is EXACT: for tropical it is [Rmin], for arctic it
    is [Rmax], and both return one of their operands unchanged — no rounding is
    ever introduced by ⊕, so the exactness the ABI relies on for path selection
    holds on the nose over the finite part.

    NON-CLAIM: the extend operation ⊗ (real addition) is NOT exact under
    IEEE-754 rounding; no exactness theorem is stated for it. The abstract
    identities `+` satisfies over `R` are the algebra's concern (the foundations
    models), not the bridge's. *)
Theorem tropical_plus_returns_an_operand :
  forall a b : R, Rmin a b = a \/ Rmin a b = b.
Proof.
  intros a b; unfold Rmin; destruct (Rle_dec a b); [left | right]; reflexivity.
Qed.

Theorem arctic_plus_returns_an_operand :
  forall a b : R, Rmax a b = a \/ Rmax a b = b.
Proof.
  intros a b; unfold Rmax; destruct (Rle_dec a b); [right | left]; reflexivity.
Qed.

(** ** The signed-tropical −∞ caveat, made precise *)

(** A wire value is *representable* by a domain if the ABI can hold it at all,
    even where it falls outside the clean semiring. This is broader than
    [repr_ok] in exactly one place: SignedTropical can HOLD `−∞` ("an
    infinitely good reward") even though `−∞` is not part of its semiring
    (see [[SignedTropicalWeight]] — `(+∞) ⊗ (−∞) = NaN` would leave the
    carrier). *)
Definition representable (d : WeightDomain) (w : wire) : Prop :=
  repr_ok d w \/ (d = SignedTropical /\ w = WNegInf).

Theorem repr_ok_implies_representable :
  forall d w, repr_ok d w -> representable d w.
Proof. intros d w H; left; exact H. Qed.

Theorem signed_neg_inf_is_representable_but_not_semiring :
  representable SignedTropical WNegInf /\ ~ repr_ok SignedTropical WNegInf.
Proof.
  split.
  - right; split; reflexivity.
  - simpl; tauto.
Qed.

(** For every domain OTHER than signed-tropical, representability and
    semiring-validity coincide — there is no representable-but-invalid value
    hiding elsewhere. *)
Theorem representable_eq_repr_ok_off_signed :
  forall d w, d <> SignedTropical -> (representable d w <-> repr_ok d w).
Proof.
  intros d w Hd; split.
  - intros [H | [Heq _]]; [exact H | contradiction].
  - apply repr_ok_implies_representable.
Qed.
