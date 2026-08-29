(** * TypedHclg — weighted, typed H/C/L/G composition

    Each component is a morphism between distinct tape types.  Composition is
    available only when the intermediate type matches.  Its denotation joins
    path weights with the multiplicative monoid of one semiring.  The proofs
    below establish denotational associativity and identities without requiring
    the runtime to allocate generic category objects.
*)

From Stdlib Require Import Lists.List.
Import ListNotations.

Require Import LlingLlang.foundations.Semiring.

Section TypedWeightedMorphisms.

Context {Weight : Type} `{WeightSemiring : Semiring Weight}.

(** A weighted transduction relation.  Equivalent weights are observationally
    interchangeable, matching the semiring setoid rather than Coq equality. *)
Record weighted_morphism (Input Output : Type) : Type := {
  realizes : list Input -> list Output -> Weight -> Prop;
  realizes_respects_weight :
    forall input output left_weight right_weight,
      left_weight ≡ right_weight ->
      realizes input output left_weight ->
      realizes input output right_weight
}.

Arguments realizes {Input Output} _ _ _ _.
Arguments realizes_respects_weight {Input Output} _ _ _ _ _ _.

(** First execute [left], then [right]. *)
Definition compose_weighted
    {Input Middle Output}
    (left : weighted_morphism Input Middle)
    (right : weighted_morphism Middle Output) :
    weighted_morphism Input Output.
Proof.
  refine {| realizes := fun input output weight =>
    exists middle left_weight right_weight,
      realizes left input middle left_weight /\
      realizes right middle output right_weight /\
      weight ≡ left_weight ⊗ right_weight |}.
  intros input output left_weight right_weight equivalent
         [middle [first_weight [second_weight
           [left_realizes [right_realizes combined]]]]].
  exists middle, first_weight, second_weight.
  repeat split; try assumption.
  eapply sr_eq_trans.
  - apply sr_eq_sym; exact equivalent.
  - exact combined.
Defined.

Definition identity_weighted (Object : Type) : weighted_morphism Object Object.
Proof.
  refine {| realizes := fun input output weight =>
    input = output /\ weight ≡ 𝟙 |}.
  intros input output left_weight right_weight equivalent
         [same_object is_one].
  split; [exact same_object |].
  eapply sr_eq_trans.
  - apply sr_eq_sym; exact equivalent.
  - exact is_one.
Defined.

Definition morphism_equivalent
    {Input Output}
    (left right : weighted_morphism Input Output) : Prop :=
  forall input output weight,
    realizes left input output weight <-> realizes right input output weight.

Lemma morphism_equivalent_reflexive :
  forall Input Output (morphism : weighted_morphism Input Output),
    morphism_equivalent morphism morphism.
Proof.
  intros Input Output morphism input output weight.
  reflexivity.
Qed.

Lemma morphism_equivalent_transitive :
  forall Input Output
         (first second third : weighted_morphism Input Output),
    morphism_equivalent first second ->
    morphism_equivalent second third ->
    morphism_equivalent first third.
Proof.
  intros Input Output first second third first_second second_third
         input output weight.
  split.
  - intro first_realizes.
    apply (proj1 (second_third input output weight)).
    apply (proj1 (first_second input output weight)).
    exact first_realizes.
  - intro third_realizes.
    apply (proj2 (first_second input output weight)).
    apply (proj2 (second_third input output weight)).
    exact third_realizes.
Qed.

Theorem compose_weighted_associative :
  forall Input FirstMiddle SecondMiddle Output
         (first : weighted_morphism Input FirstMiddle)
         (second : weighted_morphism FirstMiddle SecondMiddle)
         (third : weighted_morphism SecondMiddle Output),
    morphism_equivalent
      (compose_weighted (compose_weighted first second) third)
      (compose_weighted first (compose_weighted second third)).
Proof.
  intros Input FirstMiddle SecondMiddle Output first second third
         input output weight.
  split.
  - intros [second_middle [first_second_weight [third_weight
      [[first_middle [first_weight [second_weight
        [first_realizes [second_realizes first_second_combines]]]]]
       [third_realizes total_combines]]]]].
    exists first_middle, first_weight, (second_weight ⊗ third_weight).
    repeat split; try assumption.
    + exists second_middle, second_weight, third_weight.
      repeat split; try assumption.
      apply sr_eq_refl.
    + eapply sr_eq_trans.
      * exact total_combines.
      * eapply sr_eq_trans.
        -- apply sr_times_proper.
           ++ exact first_second_combines.
           ++ apply sr_eq_refl.
        -- apply sr_times_assoc.
  - intros [first_middle [first_weight [second_third_weight
      [first_realizes
       [[second_middle [second_weight [third_weight
         [second_realizes [third_realizes second_third_combines]]]]]
        total_combines]]]]].
    exists second_middle, (first_weight ⊗ second_weight), third_weight.
    repeat split; try assumption.
    + exists first_middle, first_weight, second_weight.
      repeat split; try assumption.
      apply sr_eq_refl.
    + eapply sr_eq_trans.
      * exact total_combines.
      * eapply sr_eq_trans.
        -- apply sr_times_proper.
           ++ apply sr_eq_refl.
           ++ exact second_third_combines.
        -- apply sr_times_assoc_l.
Qed.

Theorem compose_weighted_left_identity :
  forall Input Output (morphism : weighted_morphism Input Output),
    morphism_equivalent
      (compose_weighted (identity_weighted Input) morphism)
      morphism.
Proof.
  intros Input Output morphism input output weight.
  split.
  - intros [middle [identity_weight [morphism_weight
      [[same_input identity_is_one]
       [morphism_realizes combined]]]]].
    subst middle.
    apply (realizes_respects_weight morphism input output morphism_weight weight).
    + apply sr_eq_sym.
      eapply sr_eq_trans.
      * exact combined.
      * eapply sr_eq_trans.
        -- apply sr_times_proper.
           ++ exact identity_is_one.
           ++ apply sr_eq_refl.
        -- apply sr_times_one_l.
    + exact morphism_realizes.
  - intro morphism_realizes.
    exists input, 𝟙, weight.
    repeat split; try assumption.
    + apply sr_eq_refl.
    + apply sr_eq_sym; apply sr_times_one_l.
Qed.

Theorem compose_weighted_right_identity :
  forall Input Output (morphism : weighted_morphism Input Output),
    morphism_equivalent
      (compose_weighted morphism (identity_weighted Output))
      morphism.
Proof.
  intros Input Output morphism input output weight.
  split.
  - intros [middle [morphism_weight [identity_weight
      [morphism_realizes
       [[same_output identity_is_one] combined]]]]].
    subst middle.
    apply (realizes_respects_weight morphism input output morphism_weight weight).
    + apply sr_eq_sym.
      eapply sr_eq_trans.
      * exact combined.
      * eapply sr_eq_trans.
        -- apply sr_times_proper.
           ++ apply sr_eq_refl.
           ++ exact identity_is_one.
        -- apply sr_times_one_r.
    + exact morphism_realizes.
  - intro morphism_realizes.
    exists output, weight, 𝟙.
    repeat split; try assumption.
    + apply sr_eq_refl.
    + apply sr_eq_sym; apply sr_times_one_r.
Qed.

(** A cross-weight-domain adapter must be an explicit semiring homomorphism.
    Mere conversion of the carrier is insufficient. *)
Section WeightDomainAdapter.

Variables SourceWeight TargetWeight : Type.
Variables source_plus source_times : SourceWeight -> SourceWeight -> SourceWeight.
Variables target_plus target_times : TargetWeight -> TargetWeight -> TargetWeight.
Variables source_zero source_one : SourceWeight.
Variables target_zero target_one : TargetWeight.

Record weight_domain_homomorphism : Type := {
  map_weight : SourceWeight -> TargetWeight;
  map_preserves_zero : map_weight source_zero = target_zero;
  map_preserves_one : map_weight source_one = target_one;
  map_preserves_plus : forall left right,
    map_weight (source_plus left right) =
      target_plus (map_weight left) (map_weight right);
  map_preserves_times : forall left right,
    map_weight (source_times left right) =
      target_times (map_weight left) (map_weight right)
}.

End WeightDomainAdapter.

(** ** The typed H/C/L/G chain *)
Section Hclg.

Variables HmmState ContextDependentPhone Phone Word : Type.

Variable H : weighted_morphism HmmState ContextDependentPhone.
Variable C : weighted_morphism ContextDependentPhone Phone.
Variable L : weighted_morphism Phone Word.
Variable G : weighted_morphism Word Word.

Definition hclg_left_associated : weighted_morphism HmmState Word :=
  compose_weighted (compose_weighted (compose_weighted H C) L) G.

Definition hclg_right_associated : weighted_morphism HmmState Word :=
  compose_weighted H (compose_weighted C (compose_weighted L G)).

Theorem hclg_parenthesizations_are_equivalent :
  morphism_equivalent hclg_left_associated hclg_right_associated.
Proof.
  unfold hclg_left_associated, hclg_right_associated.
  eapply morphism_equivalent_transitive.
  - apply compose_weighted_associative.
  - apply compose_weighted_associative.
Qed.

End Hclg.

End TypedWeightedMorphisms.
