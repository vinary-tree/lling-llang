(** * RewriteSemantics — proof-carrying optimizer preservation

    Exact denotation, precision, and completeness are independent axes.  A
    rewrite may preserve all observed behavior yet still carry an approximate
    analysis certificate.  Consequently, plan composition uses conservative
    meets and never promotes either analysis axis without a fresh witness.
*)

From Stdlib Require Import Lists.List.
Import ListNotations.

Section RewriteSemantics.

Variable Trace : Type.
Definition denotation : Type := Trace -> Prop.

Definition exact_rewrite (source target : denotation) : Prop :=
  forall trace, source trace <-> target trace.

Theorem exact_rewrite_reflexive :
  forall semantics, exact_rewrite semantics semantics.
Proof. firstorder. Qed.

Theorem exact_rewrite_symmetric :
  forall source target,
    exact_rewrite source target -> exact_rewrite target source.
Proof. firstorder. Qed.

Theorem exact_rewrite_transitive :
  forall source middle target,
    exact_rewrite source middle ->
    exact_rewrite middle target ->
    exact_rewrite source target.
Proof. firstorder. Qed.

Record exact_witness (source target : denotation) : Type := {
  witness_preserves_denotation : exact_rewrite source target
}.

Definition compose_exact_witness
    {source middle target}
    (first : exact_witness source middle)
    (second : exact_witness middle target) : exact_witness source target :=
  {| witness_preserves_denotation :=
       exact_rewrite_transitive source middle target
         (witness_preserves_denotation _ _ first)
         (witness_preserves_denotation _ _ second) |}.

Inductive precision : Type :=
| Exact
| SoundApproximation.

Inductive completeness : Type :=
| Complete
| Incomplete.

Definition meet_precision (left right : precision) : precision :=
  match left, right with
  | Exact, Exact => Exact
  | _, _ => SoundApproximation
  end.

Definition meet_completeness
    (left right : completeness) : completeness :=
  match left, right with
  | Complete, Complete => Complete
  | _, _ => Incomplete
  end.

Theorem precision_never_self_promotes :
  forall left right,
    meet_precision left right = Exact -> left = Exact /\ right = Exact.
Proof. destruct left, right; simpl; intros; try discriminate; auto. Qed.

Theorem completeness_never_self_promotes :
  forall left right,
    meet_completeness left right = Complete ->
    left = Complete /\ right = Complete.
Proof. destruct left, right; simpl; intros; try discriminate; auto. Qed.

Theorem approximate_precision_is_absorbing_left :
  forall right,
    meet_precision SoundApproximation right = SoundApproximation.
Proof. destruct right; reflexivity. Qed.

Theorem incomplete_is_absorbing_right :
  forall left,
    meet_completeness left Incomplete = Incomplete.
Proof. destruct left; reflexivity. Qed.

Record analysis_claim : Type := {
  claim_precision : precision;
  claim_completeness : completeness
}.

Definition combine_claims (left right : analysis_claim) : analysis_claim :=
  {| claim_precision :=
       meet_precision (claim_precision left) (claim_precision right);
     claim_completeness :=
       meet_completeness
         (claim_completeness left) (claim_completeness right) |}.

Theorem exact_complete_combination_requires_exact_complete_inputs :
  forall left right,
    claim_precision (combine_claims left right) = Exact ->
    claim_completeness (combine_claims left right) = Complete ->
    claim_precision left = Exact /\
    claim_precision right = Exact /\
    claim_completeness left = Complete /\
    claim_completeness right = Complete.
Proof.
  intros left right Hprecision Hcomplete.
  destruct (precision_never_self_promotes _ _ Hprecision) as [Hpl Hpr].
  destruct (completeness_never_self_promotes _ _ Hcomplete) as [Hcl Hcr].
  auto.
Qed.

(** Publishing an exact result requires a denotational witness independently
    of the precision flag.  There is intentionally no constructor that turns
    [Exact] into this certificate. *)
Record publishable_exact (source target : denotation) : Type := {
  published_claim : analysis_claim;
  published_is_exact : claim_precision published_claim = Exact;
  published_witness : exact_witness source target
}.

Theorem publishable_exact_preserves_denotation :
  forall source target (certificate : publishable_exact source target),
    exact_rewrite source target.
Proof.
  intros source target certificate.
  exact (witness_preserves_denotation _ _ (published_witness _ _ certificate)).
Qed.

End RewriteSemantics.
