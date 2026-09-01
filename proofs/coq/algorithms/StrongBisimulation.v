(** * StrongBisimulation — certified strong bisimulation contracts

    This theory fixes the semantic and resource contracts for the replacement
    of [src/symbolic/bisimulation.rs].  The implementation target is Valmari's
    refinable state/transition partition algorithm over a validated dense
    labelled transition system.  This file deliberately proves the contracts
    independently of the future Rust representation.

    A successful relation certificate is a replay of splitter refinements from
    the initial coloring followed by an independently checked stable
    partition.  Replayed refinements never separate bisimilar states; stability
    proves the converse inclusion.  Hence an accepted certificate denotes
    exactly the coarsest strong bisimulation refining the initial coloring.

    Every current block also has a shared Hennessy--Milner characteristic
    formula.  A final block formula therefore distinguishes either orientation
    of every non-bisimilar pair.  The production representation is a
    hash-consed directed acyclic graph; recursive syntax is used only in this
    mathematical semantics.

    The final sections prove dense endpoint validation, finite refinement
    termination, the smaller-half logarithmic charge, exact aggregate work and
    heap inequalities, absence of whole-partition rescans, and a constant
    native-call-stack contract.
*)

From Stdlib Require Import
  List
  Bool
  Arith
  Lia
  PeanoNat
  Relations
  RelationClasses.

Import ListNotations.
Set Implicit Arguments.

Section StrongSemantics.

Context {State Action Color : Type}.
Variable transition : State -> Action -> State -> Prop.
Variable color : State -> Color.

Definition same_color (left right : State) : Prop :=
  color left = color right.

Definition transfer (relation : State -> State -> Prop)
    (left right : State) : Prop :=
  same_color left right /\
  (forall action left_target,
      transition left action left_target ->
      exists right_target,
        transition right action right_target /\
        relation left_target right_target) /\
  (forall action right_target,
      transition right action right_target ->
      exists left_target,
        transition left action left_target /\
        relation left_target right_target).

Definition strong_bisimulation
    (relation : State -> State -> Prop) : Prop :=
  forall left right, relation left right -> transfer relation left right.

Definition bisimilar (left right : State) : Prop :=
  exists relation,
    strong_bisimulation relation /\ relation left right.

Lemma identity_is_strong_bisimulation :
  strong_bisimulation (fun left right => left = right).
Proof.
  intros left right equal; subst right.
  repeat split; try reflexivity.
  - intros action target step.
    exists target; auto.
  - intros action target step.
    exists target; auto.
Qed.

Theorem bisimilar_reflexive : Reflexive bisimilar.
Proof.
  intros state.
  exists (fun left right => left = right).
  split; [exact identity_is_strong_bisimulation | reflexivity].
Qed.

Lemma converse_is_strong_bisimulation : forall relation,
  strong_bisimulation relation ->
  strong_bisimulation (fun left right => relation right left).
Proof.
  intros relation bisimulation left right related.
  specialize (bisimulation right left related).
  destruct bisimulation as [colors [forward backward]].
  repeat split.
  - symmetry; exact colors.
  - exact backward.
  - exact forward.
Qed.

Theorem bisimilar_symmetric : Symmetric bisimilar.
Proof.
  intros left right [relation [bisimulation related]].
  exists (fun first second => relation second first).
  split.
  - now apply converse_is_strong_bisimulation.
  - exact related.
Qed.

Definition compose_relation
    (first second : State -> State -> Prop)
    (left right : State) : Prop :=
  exists middle, first left middle /\ second middle right.

Lemma composition_is_strong_bisimulation : forall first second,
  strong_bisimulation first ->
  strong_bisimulation second ->
  strong_bisimulation (compose_relation first second).
Proof.
  intros first second first_bisim second_bisim left right
    [middle [left_middle middle_right]].
  specialize (first_bisim left middle left_middle).
  specialize (second_bisim middle right middle_right).
  destruct first_bisim as [left_color [left_forward left_backward]].
  destruct second_bisim as [right_color [right_forward right_backward]].
  repeat split.
  - unfold same_color in *; congruence.
  - intros action left_target left_step.
    destruct (left_forward action left_target left_step)
      as [middle_target [middle_step first_related]].
    destruct (right_forward action middle_target middle_step)
      as [right_target [right_step second_related]].
    exists right_target; split; [exact right_step |].
    exists middle_target; auto.
  - intros action right_target right_step.
    destruct (right_backward action right_target right_step)
      as [middle_target [middle_step second_related]].
    destruct (left_backward action middle_target middle_step)
      as [left_target [left_step first_related]].
    exists left_target; split; [exact left_step |].
    exists middle_target; auto.
Qed.

Theorem bisimilar_transitive : Transitive bisimilar.
Proof.
  intros left middle right
    [first [first_bisim left_middle]]
    [second [second_bisim middle_right]].
  exists (compose_relation first second).
  split.
  - now apply composition_is_strong_bisimulation.
  - exists middle; auto.
Qed.

Global Instance bisimilar_equivalence : Equivalence bisimilar.
Proof.
  split.
  - exact bisimilar_reflexive.
  - exact bisimilar_symmetric.
  - exact bisimilar_transitive.
Qed.

Definition preimage (action : Action) (target_set : State -> Prop)
    (source : State) : Prop :=
  exists target,
    transition source action target /\ target_set target.

Definition saturated (set : State -> Prop) : Prop :=
  forall left right,
    bisimilar left right -> (set left <-> set right).

Lemma color_block_is_saturated : forall representative,
  saturated (fun state => same_color representative state).
Proof.
  intros representative left right
    [relation [relation_bisimulation related]].
  pose proof (proj1 (relation_bisimulation left right related)) as colors.
  unfold same_color in *.
  split; intro member; congruence.
Qed.

Lemma preimage_preserves_saturation : forall action target_set,
  saturated target_set ->
  saturated (preimage action target_set).
Proof.
  intros action target_set target_saturated left right
    [relation [relation_bisimulation related]].
  pose proof (relation_bisimulation left right related) as matching.
  destruct matching as [_ [forward backward]].
  split.
  - intros [left_target [left_step in_target]].
    destruct (forward action left_target left_step)
      as [right_target [right_step target_related]].
    exists right_target; split; [exact right_step |].
    apply (proj1 (target_saturated left_target right_target
      (ex_intro _ relation (conj relation_bisimulation target_related)))).
    exact in_target.
  - intros [right_target [right_step in_target]].
    destruct (backward action right_target right_step)
      as [left_target [left_step target_related]].
    exists left_target; split; [exact left_step |].
    apply (proj2 (target_saturated left_target right_target
      (ex_intro _ relation (conj relation_bisimulation target_related)))).
    exact in_target.
Qed.

Definition relation_equivalence
    (relation : State -> State -> Prop) : Prop :=
  Reflexive relation /\ Symmetric relation /\ Transitive relation.

Definition contains_bisimilarity
    (relation : State -> State -> Prop) : Prop :=
  forall left right, bisimilar left right -> relation left right.

Definition split_relation
    (relation : State -> State -> Prop)
    (predicate : State -> Prop)
    (left right : State) : Prop :=
  relation left right /\ (predicate left <-> predicate right).

Lemma same_color_equivalence : relation_equivalence same_color.
Proof.
  unfold relation_equivalence.
  split.
  - intros state; unfold same_color; reflexivity.
  - split.
    + intros left right equal; unfold same_color in *.
      symmetry; exact equal.
    + intros left middle right first second; unfold same_color in *.
      congruence.
Qed.

Lemma bisimilar_states_have_same_color : forall left right,
  bisimilar left right -> same_color left right.
Proof.
  intros left right [relation [bisimulation related]].
  exact (proj1 (bisimulation left right related)).
Qed.

Lemma same_color_contains_bisimilarity :
  contains_bisimilarity same_color.
Proof.
  exact bisimilar_states_have_same_color.
Qed.

Lemma split_relation_equivalence : forall relation predicate,
  relation_equivalence relation ->
  relation_equivalence (split_relation relation predicate).
Proof.
  intros relation predicate [reflexive [symmetric transitive]].
  unfold relation_equivalence.
  split.
  - intros state; split; [apply reflexive | tauto].
  - split.
    + intros left right [related equivalent].
      split; [now apply symmetric | tauto].
    + intros left middle right [first first_equiv] [second second_equiv].
      split.
      * eapply transitive; eauto.
      * tauto.
Qed.

Lemma relation_block_is_saturated : forall relation representative,
  relation_equivalence relation ->
  contains_bisimilarity relation ->
  saturated (fun state => relation representative state).
Proof.
  intros relation representative [_ [_ transitive]] contains
    left right bisim.
  split; intro member.
  - eapply transitive; [exact member | now apply contains].
  - eapply transitive; [exact member |].
    apply contains; now apply bisimilar_symmetric.
Qed.

Lemma safe_split_contains_bisimilarity :
  forall relation action representative,
    relation_equivalence relation ->
    contains_bisimilarity relation ->
    contains_bisimilarity
      (split_relation relation
        (preimage action (fun state => relation representative state))).
Proof.
  intros relation action representative equivalent contains left right bisim.
  split.
  - now apply contains.
  - apply preimage_preserves_saturation.
    + now apply relation_block_is_saturated.
    + exact bisim.
Qed.

Lemma saturated_split_contains_bisimilarity :
  forall relation predicate,
    contains_bisimilarity relation ->
    saturated predicate ->
    contains_bisimilarity (split_relation relation predicate).
Proof.
  intros relation predicate contains predicate_saturated left right bisim.
  split.
  - now apply contains.
  - now apply predicate_saturated.
Qed.

Inductive replayed_refinement :
    (State -> State -> Prop) -> Prop :=
| replay_initial :
    replayed_refinement same_color
| replay_split : forall relation action representative,
    replayed_refinement relation ->
    (forall state,
      preimage action (fun target => relation representative target) state \/
      ~ preimage action (fun target => relation representative target) state) ->
    replayed_refinement
      (split_relation relation
        (preimage action (fun state => relation representative state))).

Theorem replayed_refinement_equivalence : forall relation,
  replayed_refinement relation -> relation_equivalence relation.
Proof.
  intros relation replay.
  induction replay.
  - apply same_color_equivalence.
  - now apply split_relation_equivalence.
Qed.

Theorem replayed_refinement_contains_bisimilarity : forall relation,
  replayed_refinement relation -> contains_bisimilarity relation.
Proof.
  intros relation replay.
  induction replay.
  - apply same_color_contains_bisimilarity.
  - apply safe_split_contains_bisimilarity.
    + now apply replayed_refinement_equivalence.
    + exact IHreplay.
Qed.

Definition relation_certificate_accepts
    (relation : State -> State -> Prop) : Prop :=
  replayed_refinement relation /\
  strong_bisimulation relation /\
  (forall left right, relation left right \/ ~ relation left right).

Theorem accepted_relation_certificate_sound : forall relation left right,
  relation_certificate_accepts relation ->
  relation left right ->
  bisimilar left right.
Proof.
  intros relation left right [_ [stable _]] related.
  exists relation; auto.
Qed.

Theorem accepted_relation_certificate_complete : forall relation left right,
  relation_certificate_accepts relation ->
  bisimilar left right ->
  relation left right.
Proof.
  intros relation left right [replay [_ _]] bisim.
  now apply (replayed_refinement_contains_bisimilarity replay).
Qed.

Theorem accepted_relation_certificate_exact : forall relation left right,
  relation_certificate_accepts relation ->
  (relation left right <-> bisimilar left right).
Proof.
  intros relation left right accepted.
  split.
  - intro related.
    eapply accepted_relation_certificate_sound; eauto.
  - intro bisim.
    eapply accepted_relation_certificate_complete; eauto.
Qed.

End StrongSemantics.

Section CharacteristicWitnesses.

Context {State Action Color : Type}.
Variable transition : State -> Action -> State -> Prop.
Variable color : State -> Color.

Inductive modal_formula : Type :=
| FormulaColor : Color -> modal_formula
| FormulaAnd : modal_formula -> modal_formula -> modal_formula
| FormulaNot : modal_formula -> modal_formula
| FormulaDiamond : Action -> modal_formula -> modal_formula.

Fixpoint satisfies (state : State) (formula : modal_formula) : Prop :=
  match formula with
  | FormulaColor expected => color state = expected
  | FormulaAnd left_formula right_formula =>
      satisfies state left_formula /\ satisfies state right_formula
  | FormulaNot inner => ~ satisfies state inner
  | FormulaDiamond action inner =>
      exists target,
        transition state action target /\ satisfies target inner
  end.

Lemma modal_formula_preserved_by_strong_bisimulation :
  forall relation,
    @strong_bisimulation State Action Color transition color relation ->
    forall formula left right,
      relation left right ->
      (satisfies left formula <-> satisfies right formula).
Proof.
  intros relation bisimulation formula.
  induction formula as
      [expected
      | left_formula left_induction right_formula right_induction
      | inner inner_induction
      | action inner inner_induction];
    intros left right related.
  - simpl.
    pose proof (proj1 (bisimulation left right related)) as colors.
    unfold same_color in colors.
    now rewrite colors.
  - simpl.
    rewrite (left_induction left right related).
    rewrite (right_induction left right related).
    tauto.
  - simpl.
    rewrite (inner_induction left right related).
    tauto.
  - simpl.
    pose proof (bisimulation left right related) as matching.
    destruct matching as [_ [forward backward]].
    split.
    + intros [left_target [left_step left_holds]].
      destruct (forward action left_target left_step)
        as [right_target [right_step targets_related]].
      exists right_target; split; [exact right_step |].
      apply (proj1 (inner_induction left_target right_target targets_related)).
      exact left_holds.
    + intros [right_target [right_step right_holds]].
      destruct (backward action right_target right_step)
        as [left_target [left_step targets_related]].
      exists left_target; split; [exact left_step |].
      apply (proj2 (inner_induction left_target right_target targets_related)).
      exact right_holds.
Qed.

Theorem modal_formula_is_saturated : forall formula,
  @saturated State Action Color transition color
    (fun state => satisfies state formula).
Proof.
  intros formula left right [relation [bisimulation related]].
  eapply modal_formula_preserved_by_strong_bisimulation.
  - exact bisimulation.
  - exact related.
Qed.

Definition certifies (formula : modal_formula) (set : State -> Prop) : Prop :=
  forall state, satisfies state formula <-> set state.

Definition distinguishes
    (formula : modal_formula) (left right : State) : Prop :=
  satisfies left formula /\ ~ satisfies right formula.

Lemma color_formula_certifies_color_block : forall representative,
  certifies (FormulaColor (color representative))
    (fun state => @same_color State Color color representative state).
Proof.
  intros representative state; unfold same_color; simpl.
  split; intro equal; symmetry; exact equal.
Qed.

Lemma diamond_formula_certifies_preimage : forall action formula set,
  certifies formula set ->
  certifies (FormulaDiamond action formula)
    (@preimage State Action transition action set).
Proof.
  intros action formula set certificate state.
  simpl; unfold preimage.
  split.
  - intros [target [step holds]].
    exists target; split; [exact step | now apply (proj1 (certificate target))].
  - intros [target [step member]].
    exists target; split; [exact step | now apply (proj2 (certificate target))].
Qed.

Lemma and_formula_certifies_intersection : forall left_formula right_formula
    left_set right_set,
  certifies left_formula left_set ->
  certifies right_formula right_set ->
  certifies (FormulaAnd left_formula right_formula)
    (fun state => left_set state /\ right_set state).
Proof.
  intros left_formula right_formula left_set right_set
    left_certificate right_certificate state.
  destruct (left_certificate state) as [left_sound left_complete].
  destruct (right_certificate state) as [right_sound right_complete].
  simpl; tauto.
Qed.

Lemma not_formula_certifies_complement : forall formula set,
  certifies formula set ->
  certifies (FormulaNot formula) (fun state => ~ set state).
Proof.
  intros formula set certificate state.
  destruct (certificate state) as [sound complete].
  simpl; tauto.
Qed.

Definition classes_have_certificates
    (relation : State -> State -> Prop) : Prop :=
  forall representative,
    exists formula,
      certifies formula (fun state => relation representative state).

Lemma initial_colors_have_certificates :
  classes_have_certificates (@same_color State Color color).
Proof.
  intros representative.
  exists (FormulaColor (color representative)).
  apply color_formula_certifies_color_block.
Qed.

Lemma safe_split_preserves_class_certificates :
  forall relation action target_representative,
    classes_have_certificates relation ->
    (forall state,
      @preimage State Action transition action
        (fun target => relation target_representative target) state \/
      ~ @preimage State Action transition action
        (fun target => relation target_representative target) state) ->
    classes_have_certificates
      (@split_relation State relation
        (@preimage State Action transition action
          (fun state => relation target_representative state))).
Proof.
  intros relation action target_representative certificates
    preimage_decidable representative.
  destruct (certificates representative)
    as [source_formula source_certificate].
  destruct (certificates target_representative)
    as [target_formula target_certificate].
  assert (diamond_certificate :
    certifies (FormulaDiamond action target_formula)
      (@preimage State Action transition action
        (fun state => relation target_representative state))).
  {
    apply diamond_formula_certifies_preimage.
    exact target_certificate.
  }
  destruct (preimage_decidable representative)
    as [representative_reaches | representative_misses].
  - exists (FormulaAnd source_formula
      (FormulaDiamond action target_formula)).
    intros state.
    unfold split_relation.
    simpl.
    rewrite (source_certificate state), (diamond_certificate state).
    tauto.
  - exists (FormulaAnd source_formula
      (FormulaNot (FormulaDiamond action target_formula))).
    intros state.
    unfold split_relation.
    simpl.
    rewrite (source_certificate state), (diamond_certificate state).
    tauto.
Qed.

Theorem replayed_classes_have_characteristic_formulas : forall relation,
  @replayed_refinement State Action Color transition color relation ->
  classes_have_certificates relation.
Proof.
  intros relation replay.
  induction replay.
  - exact initial_colors_have_certificates.
  - apply safe_split_preserves_class_certificates.
    + exact IHreplay.
    + exact H.
Qed.

Theorem separated_states_have_distinguishing_formula :
  forall relation left right,
    @replayed_refinement State Action Color transition color relation ->
    ~ relation left right ->
    exists formula, distinguishes formula left right.
Proof.
  intros relation left right replay separated.
  destruct (replayed_classes_have_characteristic_formulas replay left)
    as [formula certificate].
  exists formula.
  split.
  - apply (proj2 (certificate left)).
    apply (proj1 (replayed_refinement_equivalence replay)).
  - intro right_holds.
    apply separated.
    apply (proj1 (certificate right)); exact right_holds.
Qed.

Theorem accepted_certificate_decides_with_witness :
  forall relation left right,
    @relation_certificate_accepts State Action Color transition color relation ->
    relation left right \/
    exists formula, distinguishes formula left right.
Proof.
  intros relation left right accepted.
  destruct accepted as [replay [_ decidable]].
  destruct (decidable left right) as [related | separated].
  - now left.
  - right.
    exact (@separated_states_have_distinguishing_formula
      relation left right replay separated).
Qed.

(** Valmari's labelled nondeterministic driver refines a state block by two
    modal predicates for a transition cluster: states whose matching label is
    confined to the target block, and states with some transition into the
    target block.  This replay relation records those predicates directly.
    The preceding invariance theorem is the semantic bridge that makes these
    implementation-shaped refinements as safe as a plain predecessor split. *)
Inductive modal_replayed_refinement :
    (State -> State -> Prop) -> Prop :=
| modal_replay_initial :
    modal_replayed_refinement (@same_color State Color color)
| modal_replay_split : forall relation predicate,
    modal_replayed_refinement relation ->
    (forall state,
      satisfies state predicate \/ ~ satisfies state predicate) ->
    modal_replayed_refinement
      (@split_relation State relation
        (fun state => satisfies state predicate)).

Theorem modal_replayed_refinement_equivalence : forall relation,
  modal_replayed_refinement relation -> relation_equivalence relation.
Proof.
  intros relation replay.
  induction replay as
      [|relation predicate replay induction predicate_decidable].
  - apply same_color_equivalence.
  - destruct induction as [reflexive [symmetric transitive]].
    unfold relation_equivalence.
    split.
    + intros state; split; [now apply reflexive | tauto].
    + split.
      * intros left right [related same_predicate].
        split; [now apply symmetric | tauto].
      * intros left middle right
          [left_middle left_predicate]
          [middle_right right_predicate].
        split.
        -- eapply transitive; eauto.
        -- tauto.
Qed.

Theorem modal_replayed_refinement_contains_bisimilarity : forall relation,
  modal_replayed_refinement relation ->
  @contains_bisimilarity State Action Color transition color relation.
Proof.
  intros relation replay.
  induction replay as
      [|relation predicate replay induction predicate_decidable].
  - apply same_color_contains_bisimilarity.
  - intros left right bisim.
    split.
    + now apply induction.
    + apply modal_formula_is_saturated.
      exact bisim.
Qed.

Lemma modal_split_preserves_class_certificates :
  forall relation predicate,
    classes_have_certificates relation ->
    (forall state,
      satisfies state predicate \/ ~ satisfies state predicate) ->
    classes_have_certificates
      (@split_relation State relation
        (fun state => satisfies state predicate)).
Proof.
  intros relation predicate certificates predicate_decidable representative.
  destruct (certificates representative)
    as [source_formula source_certificate].
  destruct (predicate_decidable representative)
    as [representative_holds | representative_misses].
  - exists (FormulaAnd source_formula predicate).
    intros state.
    unfold split_relation.
    simpl.
    rewrite (source_certificate state).
    tauto.
  - exists (FormulaAnd source_formula (FormulaNot predicate)).
    intros state.
    unfold split_relation.
    simpl.
    rewrite (source_certificate state).
    tauto.
Qed.

Theorem modal_replayed_classes_have_characteristic_formulas :
  forall relation,
    modal_replayed_refinement relation ->
    classes_have_certificates relation.
Proof.
  intros relation replay.
  induction replay as
      [|relation predicate replay induction predicate_decidable].
  - exact initial_colors_have_certificates.
  - now apply modal_split_preserves_class_certificates.
Qed.

Theorem modal_separated_states_have_distinguishing_formula :
  forall relation left right,
    modal_replayed_refinement relation ->
    ~ relation left right ->
    exists formula, distinguishes formula left right.
Proof.
  intros relation left right replay separated.
  destruct (modal_replayed_classes_have_characteristic_formulas replay left)
    as [formula certificate].
  exists formula.
  split.
  - apply (proj2 (certificate left)).
    apply (proj1 (modal_replayed_refinement_equivalence replay)).
  - intro right_holds.
    apply separated.
    apply (proj1 (certificate right)); exact right_holds.
Qed.

Definition modal_relation_certificate_accepts
    (relation : State -> State -> Prop) : Prop :=
  modal_replayed_refinement relation /\
  @strong_bisimulation State Action Color transition color relation /\
  (forall left right, relation left right \/ ~ relation left right).

Theorem accepted_modal_relation_certificate_exact :
  forall relation left right,
    modal_relation_certificate_accepts relation ->
    (relation left right <->
      @bisimilar State Action Color transition color left right).
Proof.
  intros relation left right [replay [stable decidable]].
  split.
  - intro related.
    exists relation; auto.
  - intro bisim.
    now apply (modal_replayed_refinement_contains_bisimilarity replay).
Qed.

Theorem accepted_modal_certificate_decides_with_witness :
  forall relation left right,
    modal_relation_certificate_accepts relation ->
    relation left right \/
    exists formula, distinguishes formula left right.
Proof.
  intros relation left right [replay [stable decidable]].
  destruct (decidable left right) as [related | separated].
  - now left.
  - right.
    eapply modal_separated_states_have_distinguishing_formula.
    + exact replay.
    + exact separated.
Qed.

End CharacteristicWitnesses.

Section DenseValidation.

Record dense_edge : Type := {
  edge_source : nat;
  edge_label : nat;
  edge_target : nat
}.

Definition edge_validb (state_count : nat) (edge : dense_edge) : bool :=
  (edge_source edge <? state_count) &&
  (edge_target edge <? state_count).

Definition edges_validb (state_count : nat) (edges : list dense_edge) : bool :=
  forallb (edge_validb state_count) edges.

Definition vector_domain_validb (state_count : nat) (values : list nat) : bool :=
  Nat.eqb (length values) state_count.

Definition state_index_validb (state_count state : nat) : bool :=
  state <? state_count.

Lemma edge_validb_spec : forall state_count edge,
  edge_validb state_count edge = true <->
  edge_source edge < state_count /\ edge_target edge < state_count.
Proof.
  intros state_count edge.
  unfold edge_validb.
  rewrite Bool.andb_true_iff.
  repeat rewrite Nat.ltb_lt.
  tauto.
Qed.

Theorem edges_validb_spec : forall state_count edges,
  edges_validb state_count edges = true <->
  Forall (fun edge =>
    edge_source edge < state_count /\ edge_target edge < state_count) edges.
Proof.
  intros state_count edges.
  unfold edges_validb.
  rewrite forallb_forall.
  split.
  - intros all_valid.
    apply Forall_forall.
    intros edge member.
    apply edge_validb_spec.
    now apply all_valid.
  - intros all_valid edge member.
    apply edge_validb_spec.
    rewrite Forall_forall in all_valid.
    exact (all_valid edge member).
Qed.

Theorem malformed_source_is_rejected : forall state_count edges edge,
  In edge edges ->
  state_count <= edge_source edge ->
  edges_validb state_count edges = false.
Proof.
  intros state_count edges edge member malformed.
  destruct (edges_validb state_count edges) eqn:valid; [|reflexivity].
  apply edges_validb_spec in valid.
  rewrite Forall_forall in valid.
  specialize (valid edge member).
  lia.
Qed.

Theorem malformed_target_is_rejected : forall state_count edges edge,
  In edge edges ->
  state_count <= edge_target edge ->
  edges_validb state_count edges = false.
Proof.
  intros state_count edges edge member malformed.
  destruct (edges_validb state_count edges) eqn:valid; [|reflexivity].
  apply edges_validb_spec in valid.
  rewrite Forall_forall in valid.
  specialize (valid edge member).
  lia.
Qed.

Theorem validated_source_is_indexable : forall state_count edges edge,
  edges_validb state_count edges = true ->
  In edge edges ->
  edge_source edge < state_count.
Proof.
  intros state_count edges edge valid member.
  apply edges_validb_spec in valid.
  rewrite Forall_forall in valid.
  exact (proj1 (valid edge member)).
Qed.

Theorem validated_target_is_indexable : forall state_count edges edge,
  edges_validb state_count edges = true ->
  In edge edges ->
  edge_target edge < state_count.
Proof.
  intros state_count edges edge valid member.
  apply edges_validb_spec in valid.
  rewrite Forall_forall in valid.
  exact (proj2 (valid edge member)).
Qed.

Theorem vector_domain_validb_spec : forall state_count values,
  vector_domain_validb state_count values = true <->
  length values = state_count.
Proof.
  intros state_count values.
  unfold vector_domain_validb.
  now rewrite Nat.eqb_eq.
Qed.

Theorem short_vector_is_rejected : forall state_count values,
  length values < state_count ->
  vector_domain_validb state_count values = false.
Proof.
  intros state_count values shorter.
  unfold vector_domain_validb.
  apply Nat.eqb_neq; lia.
Qed.

Theorem long_vector_is_rejected : forall state_count values,
  state_count < length values ->
  vector_domain_validb state_count values = false.
Proof.
  intros state_count values longer.
  unfold vector_domain_validb.
  apply Nat.eqb_neq; lia.
Qed.

Theorem query_index_validb_spec : forall state_count state,
  state_index_validb state_count state = true <-> state < state_count.
Proof.
  intros state_count state.
  unfold state_index_validb.
  apply Nat.ltb_lt.
Qed.

Record validated_dense_lts : Type := {
  validated_state_count : nat;
  validated_edges : list dense_edge;
  validated_endpoints :
    Forall (fun edge =>
      edge_source edge < validated_state_count /\
      edge_target edge < validated_state_count) validated_edges
}.

Theorem validated_lts_cannot_contain_malformed_endpoint :
  forall lts edge,
    In edge (validated_edges lts) ->
    edge_source edge < validated_state_count lts /\
    edge_target edge < validated_state_count lts.
Proof.
  intros lts edge member.
  pose proof (validated_endpoints lts) as endpoints.
  rewrite Forall_forall in endpoints.
  exact (endpoints edge member).
Qed.

End DenseValidation.

Section CanonicalRelation.

Definition relation_at (blocks : list nat) (left right : nat) : bool :=
  match nth_error blocks left, nth_error blocks right with
  | Some left_block, Some right_block => Nat.eqb left_block right_block
  | _, _ => false
  end.

Definition canonical_relation_matrix (blocks : list nat) : list (list bool) :=
  map (fun left_block =>
    map (fun right_block => Nat.eqb left_block right_block) blocks) blocks.

Theorem relation_at_is_exact : forall blocks left right left_block right_block,
  nth_error blocks left = Some left_block ->
  nth_error blocks right = Some right_block ->
  (relation_at blocks left right = true <-> left_block = right_block).
Proof.
  intros blocks left right left_block right_block left_at right_at.
  unfold relation_at; rewrite left_at, right_at.
  apply Nat.eqb_eq.
Qed.

Theorem canonical_relation_matrix_outer_length : forall blocks,
  length (canonical_relation_matrix blocks) = length blocks.
Proof.
  intros blocks; unfold canonical_relation_matrix.
  now rewrite length_map.
Qed.

Theorem canonical_relation_matrix_inner_length : forall blocks row,
  In row (canonical_relation_matrix blocks) ->
  length row = length blocks.
Proof.
  intros blocks row member.
  unfold canonical_relation_matrix in member.
  apply in_map_iff in member.
  destruct member as [block [equal _]].
  subst row; now rewrite length_map.
Qed.

Theorem canonical_relation_matrix_invariant_under_injective_relabeling :
  forall blocks relabel,
    (forall first second, relabel first = relabel second -> first = second) ->
    canonical_relation_matrix (map relabel blocks) =
    canonical_relation_matrix blocks.
Proof.
  intros blocks relabel injective.
  assert (eqb_invariant : forall first second,
    Nat.eqb (relabel first) (relabel second) = Nat.eqb first second).
  {
    intros first second.
    destruct (Nat.eqb first second) eqn:equal.
    - apply Nat.eqb_eq in equal; subst second.
      now rewrite Nat.eqb_refl.
    - apply Nat.eqb_neq in equal.
      apply Nat.eqb_neq.
      intro relabeled_equal.
      apply equal; now apply injective.
  }
  unfold canonical_relation_matrix.
  rewrite map_map.
  apply map_ext_in.
  intros first _.
  rewrite map_map.
  apply map_ext_in.
  intros second _.
  apply eqb_invariant.
Qed.

Theorem relation_at_invariant_under_injective_relabeling :
  forall blocks relabel left right,
    (forall first second, relabel first = relabel second -> first = second) ->
    relation_at (map relabel blocks) left right =
    relation_at blocks left right.
Proof.
  intros blocks relabel left right injective.
  unfold relation_at.
  destruct (nth_error blocks left) as [left_block |] eqn:left_at;
    destruct (nth_error blocks right) as [right_block |] eqn:right_at.
  - rewrite nth_error_map, left_at.
    rewrite nth_error_map, right_at.
    simpl.
    destruct (Nat.eqb left_block right_block) eqn:equal.
    + apply Nat.eqb_eq in equal; subst right_block.
      now rewrite Nat.eqb_refl.
    + apply Nat.eqb_neq in equal.
      apply Nat.eqb_neq.
      intro relabeled_equal.
      apply equal; now apply injective.
  - rewrite nth_error_map, left_at.
    rewrite nth_error_map, right_at.
    reflexivity.
  - rewrite nth_error_map, left_at.
    reflexivity.
  - rewrite nth_error_map, left_at.
    reflexivity.
Qed.

End CanonicalRelation.

Section TerminationAndResources.

Inductive refinement_progress (state_count : nat) :
    nat -> nat -> Prop :=
| refinement_done : forall block_count,
    block_count <= state_count ->
    refinement_progress state_count block_count 0
| refinement_step : forall block_count remaining_steps,
    block_count < state_count ->
    refinement_progress state_count (S block_count) remaining_steps ->
    refinement_progress state_count block_count (S remaining_steps).

Theorem strict_refinement_terminates : forall state_count block_count steps,
  refinement_progress state_count block_count steps ->
  steps <= state_count - block_count.
Proof.
  intros state_count block_count steps progress.
  induction progress.
  - lia.
  - simpl; lia.
Qed.

Inductive smaller_half_trace : nat -> nat -> Prop :=
| smaller_half_stop : forall size,
    0 < size ->
    smaller_half_trace size 0
| smaller_half_step : forall size next_size charges,
    0 < next_size ->
    2 * next_size <= size ->
    smaller_half_trace next_size charges ->
    smaller_half_trace size (S charges).

Lemma smaller_half_power_bound : forall size charges,
  smaller_half_trace size charges ->
  2 ^ charges <= size.
Proof.
  intros size charges trace.
  induction trace.
  - simpl; lia.
  - simpl.
    nia.
Qed.

Theorem smaller_half_charge_is_logarithmic : forall size charges,
  smaller_half_trace size charges ->
  charges <= Nat.log2 size.
Proof.
  intros size charges trace.
  apply (proj1 (Nat.log2_le_pow2 size charges ltac:(destruct trace; lia))).
  now apply smaller_half_power_bound.
Qed.

Fixpoint charge_sum (charges : list nat) : nat :=
  match charges with
  | [] => 0
  | charge :: rest => charge + charge_sum rest
  end.

Lemma charge_sum_bounded : forall charges bound,
  Forall (fun charge => charge <= bound) charges ->
  charge_sum charges <= length charges * bound.
Proof.
  intros charges bound bounded.
  induction bounded; simpl; nia.
Qed.

Record resource_account : Type := {
  state_charge_counts : list nat;
  transition_charge_counts : list nat;
  whole_partition_rescans : nat;
  maximum_native_frames : nat;
  state_partition_cells : nat;
  transition_partition_cells : nat;
  witness_dag_cells : nat
}.

Definition resource_account_valid
    (state_count transition_count : nat)
    (account : resource_account) : Prop :=
  length (state_charge_counts account) = state_count /\
  length (transition_charge_counts account) = transition_count /\
  Forall (fun charge => charge <= Nat.log2 (Nat.max 1 state_count))
    (state_charge_counts account) /\
  Forall (fun charge => charge <= Nat.log2 (Nat.max 1 state_count))
    (transition_charge_counts account) /\
  whole_partition_rescans account = 0 /\
  maximum_native_frames account = 1 /\
  state_partition_cells account <= 8 * state_count + 8 /\
  transition_partition_cells account <= 12 * transition_count + 8 /\
  witness_dag_cells account <=
    (state_count + transition_count) *
      S (Nat.log2 (Nat.max 1 state_count)).

Definition charged_work (account : resource_account) : nat :=
  charge_sum (state_charge_counts account) +
  charge_sum (transition_charge_counts account).

Theorem valid_account_has_no_whole_partition_rescans :
  forall state_count transition_count account,
    resource_account_valid state_count transition_count account ->
    whole_partition_rescans account = 0.
Proof.
  intros state_count transition_count account
    [_ [_ [_ [_ [no_rescans _]]]]].
  exact no_rescans.
Qed.

Theorem valid_account_has_constant_native_stack :
  forall state_count transition_count account,
    resource_account_valid state_count transition_count account ->
    maximum_native_frames account = 1.
Proof.
  intros state_count transition_count account
    [_ [_ [_ [_ [_ [constant_stack _]]]]]].
  exact constant_stack.
Qed.

Theorem valid_account_work_is_quasilinear :
  forall state_count transition_count account,
    resource_account_valid state_count transition_count account ->
    charged_work account <=
      (state_count + transition_count) *
        Nat.log2 (Nat.max 1 state_count).
Proof.
  intros state_count transition_count account
    [state_length [transition_length
      [state_bounded [transition_bounded _]]]].
  unfold charged_work.
  pose proof (@charge_sum_bounded
    (state_charge_counts account) _ state_bounded) as state_work.
  pose proof (@charge_sum_bounded
    (transition_charge_counts account) _ transition_bounded) as transition_work.
  rewrite state_length in state_work.
  rewrite transition_length in transition_work.
  nia.
Qed.

Theorem valid_account_core_heap_is_linear :
  forall state_count transition_count account,
    resource_account_valid state_count transition_count account ->
    state_partition_cells account + transition_partition_cells account <=
      12 * (state_count + transition_count) + 16.
Proof.
  intros state_count transition_count account
    [_ [_ [_ [_ [_ [_ [state_heap [transition_heap _]]]]]]]].
  nia.
Qed.

Theorem valid_account_witness_dag_is_quasilinear :
  forall state_count transition_count account,
    resource_account_valid state_count transition_count account ->
    witness_dag_cells account <=
      (state_count + transition_count) *
        S (Nat.log2 (Nat.max 1 state_count)).
Proof.
  intros state_count transition_count account
    [_ [_ [_ [_ [_ [_ [_ [_ witness_bound]]]]]]]].
  exact witness_bound.
Qed.

End TerminationAndResources.
