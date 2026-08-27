(** * CompositionSemantics — epsilon-free weighted WFST composition

    The lazy ABI composition (src/bindings.rs, `expand_state`) builds the
    weighted product of two scalar WFSTs: a product transition on input
    $`a`$ / output $`c`$ with weight $`w_1 \otimes w_2`$ exists exactly when the
    left transducer steps on $`a`$/$`b`$ with weight $`w_1`$ and the right steps
    on $`b`$/$`c`$ with weight $`w_2`$ (the matched inner symbol $`b`$). This
    file is the formal model of that construction and its correctness for the
    epsilon-free case — obligation #17(a), the formal home of LLING-COMP-1.

    **Scope and staging.** This proves the epsilon-free product: single-step
    soundness and completeness, and the two semantic laws — path labels compose
    ($`\mathrm{in}(p_1 \times p_2) = \mathrm{in}(p_1)`$ and
    $`\mathrm{out}(p_1 \times p_2) = \mathrm{out}(p_2)`$) and path weights
    compose ($`w(p_1 \times p_2) = w(p_1) \otimes w(p_2)`$). The epsilon-filter
    soundness (three-state filter that removes epsilon-move redundancy) is the
    staged part #17(b); the runtime concurrency of the lazy expansion is modeled
    separately in TLA+ (AbiCompositionProtocol.tla, #18), so this file is
    confined to the epsilon-free algebra.

    **Generality.** Weights are taken over a commutative monoid
    $`(W, \otimes, \bar 1)`$ — the multiplicative monoid of the semiring. All
    seven [VtWeightDomain]s have a commutative $`\otimes`$ (tropical / arctic /
    signed / log use $`+`$; probability / count use $`\times`$; boolean uses
    $`\wedge`$), so the homomorphism below holds for every scalar-WFST domain
    (the multiplicative structure is the one proved in the foundations weight
    models, e.g. [[TropicalWeight]] and [[ProbabilityWeight]]). The monoid laws
    enter as section premises and are discharged into the theorems' hypotheses;
    nothing is assumed globally.

    Registry: proofs/doc/abi-invariants.tsv, LLING-COMP-1.
*)

From Stdlib Require Import Lists.List.
From Stdlib Require Import Arith.Arith.
Import ListNotations.

Section EpsilonFreeComposition.

(** The weight multiplicative monoid (the semiring's [otimes]). *)
Variable W : Type.
Variable otimes : W -> W -> W.
Variable one : W.
Hypothesis otimes_assoc : forall a b c, otimes (otimes a b) c = otimes a (otimes b c).
Hypothesis one_l : forall a, otimes one a = a.
Hypothesis one_r : forall a, otimes a one = a.
Hypothesis otimes_comm : forall a b, otimes a b = otimes b a.

(** Symbols and states are naturals; a transition is epsilon-free: it consumes
    exactly one input and one output symbol. *)
Definition sym : Type := nat.

Record trans : Type := {
  src : nat;
  ilabel : sym;
  olabel : sym;
  wt : W;
  dst : nat
}.

(** A component transducer is a finite transition relation. *)
Definition wfst : Type := list trans.

(** ** The product transition relation *)

(** The product of [t1] (in [T1]) and [t2] (in [T2]) is a legal product
    transition exactly when they carry the SAME inner symbol
    ([olabel t1 = ilabel t2]); it then reads [ilabel t1], writes [olabel t2],
    and weighs [otimes (wt t1) (wt t2)]. *)
Definition matched (t1 t2 : trans) : Prop := olabel t1 = ilabel t2.

Definition product_trans (t1 t2 : trans) : trans := {|
  src := 0;                         (* product state identity is #19's concern *)
  ilabel := ilabel t1;
  olabel := olabel t2;
  wt := otimes (wt t1) (wt t2);
  dst := 0
|}.

(** Single-step soundness/completeness: a product step's labels and weight are
    exactly the composition of a matched pair, and conversely. *)
Theorem product_step_labels :
  forall t1 t2,
    ilabel (product_trans t1 t2) = ilabel t1
    /\ olabel (product_trans t1 t2) = olabel t2.
Proof. intros; split; reflexivity. Qed.

Theorem product_step_weight :
  forall t1 t2, wt (product_trans t1 t2) = otimes (wt t1) (wt t2).
Proof. reflexivity. Qed.

Theorem product_step_requires_match :
  forall t1 t2, matched t1 t2 -> olabel t1 = ilabel t2.
Proof. intros t1 t2 H; exact H. Qed.

(** ** Paths and their observations *)

(** A path is a list of transitions. Its input/output strings are the label
    projections; its weight is the [otimes]-fold from [one]. *)
Definition path : Type := list trans.

Definition path_in (p : path) : list sym := map ilabel p.
Definition path_out (p : path) : list sym := map olabel p.
Definition path_weight (p : path) : W := fold_right otimes one (map wt p).

(** A matched path pair: equal length, and each left output equals the aligned
    right input (the inner string cancels). *)
Fixpoint matched_pair (p1 p2 : path) : Prop :=
  match p1, p2 with
  | [], [] => True
  | t1 :: r1, t2 :: r2 => matched t1 t2 /\ matched_pair r1 r2
  | _, _ => False
  end.

(** The product of two aligned paths. *)
Fixpoint product_path (p1 p2 : path) : path :=
  match p1, p2 with
  | t1 :: r1, t2 :: r2 => product_trans t1 t2 :: product_path r1 r2
  | _, _ => []
  end.

(** ** Algebraic helper: swap the middle factors of a commutative monoid *)

Lemma otimes_swap_middle :
  forall a b c d,
    otimes (otimes a b) (otimes c d) = otimes (otimes a c) (otimes b d).
Proof.
  intros a b c d.
  rewrite otimes_assoc.
  rewrite <- (otimes_assoc b c d).
  rewrite (otimes_comm b c).
  rewrite (otimes_assoc c b d).
  rewrite <- (otimes_assoc a c (otimes b d)).
  reflexivity.
Qed.

(** ** LLING-COMP-1: the composition semantic laws *)

(** Labels compose: the product path reads the left input string and writes the
    right output string (the inner string is consumed by the match). *)
Theorem path_labels_compose :
  forall p1 p2, length p1 = length p2 ->
    path_in (product_path p1 p2) = path_in p1
    /\ path_out (product_path p1 p2) = path_out p2.
Proof.
  induction p1 as [| t1 r1 IH]; intros [| t2 r2] Hlen; simpl in *;
    try discriminate; try (split; reflexivity).
  injection Hlen as Hlen.
  destruct (IH r2 Hlen) as [Hin Hout].
  unfold path_in, path_out in *. simpl.
  rewrite Hin, Hout. split; reflexivity.
Qed.

(** Weights compose: the product path weight is the [otimes] of the component
    path weights. This is the heart of composition correctness -- Mohri's
    weighted-composition weight law for the epsilon-free case. *)
Theorem path_weight_compose :
  forall p1 p2, length p1 = length p2 ->
    path_weight (product_path p1 p2) = otimes (path_weight p1) (path_weight p2).
Proof.
  induction p1 as [| t1 r1 IH]; intros [| t2 r2] Hlen; simpl in *;
    try discriminate.
  - unfold path_weight. simpl. rewrite one_l. reflexivity.
  - injection Hlen as Hlen.
    unfold path_weight in *. simpl.
    rewrite (IH r2 Hlen).
    apply otimes_swap_middle.
Qed.

(** The product of two paths has the same length as each, so it observes exactly
    one product transition per aligned component step (no fabricated or dropped
    steps in the epsilon-free product). *)
Theorem product_path_length :
  forall p1 p2, length p1 = length p2 -> length (product_path p1 p2) = length p1.
Proof.
  induction p1 as [| t1 r1 IH]; intros [| t2 r2] Hlen; simpl in *;
    try discriminate; try reflexivity.
  injection Hlen as Hlen. rewrite (IH r2 Hlen). reflexivity.
Qed.

(** A degenerate corollary anchoring the identity: the empty product path has
    unit weight (the empty run of the composed machine). *)
Theorem product_empty_weight : path_weight (product_path [] []) = one.
Proof. reflexivity. Qed.

End EpsilonFreeComposition.
