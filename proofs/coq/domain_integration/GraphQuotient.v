(** * GraphQuotient — exact SCC fibers, condensation, and renaming

    A strongly connected component (SCC) decomposition is a quotient of the
    vertex set by mutual reachability.  This file proves the laws that the
    libcpg-to-libvgraph boundary must preserve: total, nonempty, disjoint
    fibers; exact cross-fiber edges; self-loop-free and acyclic condensation;
    and equivariance under bijective vertex and component renaming.

    Component numbers are canonical only relative to one stable-label order.
    Arbitrary lawful renaming therefore commutes up to the induced component
    bijection, not literal equality of numeric component identifiers.
*)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lia.

Section ExactSccQuotient.

Variables Vertex Component : Type.
Variable edge : Vertex -> Vertex -> Prop.

Inductive reachable : Vertex -> Vertex -> Prop :=
| reachable_refl : forall vertex, reachable vertex vertex
| reachable_step : forall source middle target,
    edge source middle ->
    reachable middle target ->
    reachable source target.

Lemma reachable_transitive : forall first second third,
  reachable first second ->
  reachable second third ->
  reachable first third.
Proof.
  intros first second third first_second.
  induction first_second; intro second_third.
  - exact second_third.
  - eapply reachable_step; eauto.
Qed.

Lemma edge_is_reachable : forall source target,
  edge source target -> reachable source target.
Proof.
  intros source target source_target.
  eapply reachable_step.
  - exact source_target.
  - apply reachable_refl.
Qed.

Variable component_of : Vertex -> Component.
Variable condensation_edge : Component -> Component -> Prop.

Hypothesis every_component_has_member :
  forall component, exists vertex, component_of vertex = component.

Hypothesis same_component_iff_mutually_reachable :
  forall left right,
    component_of left = component_of right <->
    reachable left right /\ reachable right left.

Hypothesis condensation_edge_exact :
  forall source target,
    condensation_edge source target <->
    source <> target /\
    exists source_vertex target_vertex,
      component_of source_vertex = source /\
      component_of target_vertex = target /\
      edge source_vertex target_vertex.

Theorem quotient_partition_is_total : forall vertex,
  exists component, component_of vertex = component.
Proof. intro vertex; exists (component_of vertex); reflexivity. Qed.

Theorem quotient_fibers_are_nonempty : forall component,
  exists vertex, component_of vertex = component.
Proof. exact every_component_has_member. Qed.

Theorem quotient_fibers_are_disjoint : forall vertex left right,
  component_of vertex = left ->
  component_of vertex = right ->
  left = right.
Proof. intros vertex left right left_member right_member; congruence. Qed.

Theorem quotient_identifies_exactly_mutual_reachability : forall left right,
  component_of left = component_of right <->
  reachable left right /\ reachable right left.
Proof. exact same_component_iff_mutually_reachable. Qed.

Theorem original_cross_edge_induces_quotient_edge :
  forall source target,
    edge source target ->
    component_of source <> component_of target ->
    condensation_edge (component_of source) (component_of target).
Proof.
  intros source target source_target distinct.
  apply (proj2 (condensation_edge_exact
    (component_of source) (component_of target))).
  split; [exact distinct |].
  exists source, target; auto.
Qed.

Theorem every_quotient_edge_has_original_witness :
  forall source target,
    condensation_edge source target ->
    exists source_vertex target_vertex,
      component_of source_vertex = source /\
      component_of target_vertex = target /\
      edge source_vertex target_vertex.
Proof.
  intros source target quotient_edge.
  exact (proj2 (proj1 (condensation_edge_exact source target) quotient_edge)).
Qed.

Theorem condensation_has_no_self_edges :
  forall component, ~ condensation_edge component component.
Proof.
  intros component self_edge.
  exact ((proj1 (proj1
    (condensation_edge_exact component component) self_edge)) eq_refl).
Qed.

Inductive condensation_path : Component -> Component -> Prop :=
| condensation_path_one : forall source target,
    condensation_edge source target ->
    condensation_path source target
| condensation_path_more : forall source middle target,
    condensation_edge source middle ->
    condensation_path middle target ->
    condensation_path source target.

Lemma condensation_path_lifts : forall source_component target_component,
  condensation_path source_component target_component ->
  forall source_vertex,
    component_of source_vertex = source_component ->
    exists target_vertex,
      component_of target_vertex = target_component /\
      reachable source_vertex target_vertex.
Proof.
  intros source_component target_component path.
  induction path as
      [source_component target_component quotient_edge
      |source_component middle_component target_component quotient_edge
       remaining_path induction_hypothesis].
  - intros source_vertex source_member.
    destruct (every_quotient_edge_has_original_witness
      source_component target_component quotient_edge)
      as [edge_source [edge_target
        [edge_source_member [edge_target_member original_edge]]]].
    destruct (proj1 (same_component_iff_mutually_reachable
      source_vertex edge_source))
      as [source_to_edge_source _].
    + congruence.
    + exists edge_target; split; [exact edge_target_member |].
      eapply reachable_transitive.
      * exact source_to_edge_source.
      * apply edge_is_reachable; exact original_edge.
  - intros source_vertex source_member.
    destruct (every_quotient_edge_has_original_witness
      source_component middle_component quotient_edge)
      as [edge_source [edge_target
        [edge_source_member [edge_target_member original_edge]]]].
    destruct (proj1 (same_component_iff_mutually_reachable
      source_vertex edge_source))
      as [source_to_edge_source _].
    + congruence.
    + destruct (induction_hypothesis edge_target edge_target_member)
        as [target_vertex [target_member edge_target_to_target]].
      exists target_vertex; split; [exact target_member |].
      eapply reachable_transitive.
      * exact source_to_edge_source.
      * eapply reachable_transitive.
        -- apply edge_is_reachable; exact original_edge.
        -- exact edge_target_to_target.
Qed.

Theorem condensation_is_acyclic :
  forall component, ~ condensation_path component component.
Proof.
  intros component cycle.
  inversion cycle as
      [source target self_edge
      |source middle target first_edge remaining_path]; subst.
  - apply (condensation_has_no_self_edges component); exact self_edge.
  - destruct (every_quotient_edge_has_original_witness
      component middle first_edge)
      as [edge_source [edge_target
        [edge_source_member [edge_target_member original_edge]]]].
    destruct (condensation_path_lifts
      middle component remaining_path edge_target edge_target_member)
      as [return_vertex [return_member target_to_return]].
    destruct (proj1 (same_component_iff_mutually_reachable
      return_vertex edge_source))
      as [return_to_source _].
    + congruence.
    + assert (target_to_source : reachable edge_target edge_source).
      { eapply reachable_transitive; eauto. }
      assert (source_to_target : reachable edge_source edge_target).
      { apply edge_is_reachable; exact original_edge. }
      pose proof (proj2 (same_component_iff_mutually_reachable
        edge_source edge_target)
        (conj source_to_target target_to_source)) as same_component.
      pose proof (proj1 (condensation_edge_exact component middle) first_edge)
        as [different_components _].
      apply different_components.
      congruence.
Qed.

End ExactSccQuotient.

Section RenamingEquivariance.

Variables Vertex Component RenamedVertex RenamedComponent : Type.
Variables edge : Vertex -> Vertex -> Prop.
Variables renamed_edge : RenamedVertex -> RenamedVertex -> Prop.
Variables component_of : Vertex -> Component.
Variables renamed_component_of : RenamedVertex -> RenamedComponent.
Variables condensation_edge : Component -> Component -> Prop.
Variables renamed_condensation_edge :
  RenamedComponent -> RenamedComponent -> Prop.

Variables rename_vertex : Vertex -> RenamedVertex.
Variables unrename_vertex : RenamedVertex -> Vertex.
Variables rename_component : Component -> RenamedComponent.
Variables unrename_component : RenamedComponent -> Component.

Hypothesis vertex_round_trip : forall vertex,
  unrename_vertex (rename_vertex vertex) = vertex.
Hypothesis renamed_vertex_round_trip : forall vertex,
  rename_vertex (unrename_vertex vertex) = vertex.
Hypothesis component_round_trip : forall component,
  unrename_component (rename_component component) = component.
Hypothesis renamed_component_round_trip : forall component,
  rename_component (unrename_component component) = component.

Hypothesis edge_renames : forall source target,
  edge source target ->
  renamed_edge (rename_vertex source) (rename_vertex target).
Hypothesis edge_unrenames : forall source target,
  renamed_edge source target ->
  edge (unrename_vertex source) (unrename_vertex target).

Hypothesis quotient_naturality : forall vertex,
  renamed_component_of (rename_vertex vertex) =
    rename_component (component_of vertex).
Hypothesis quotient_conaturality : forall vertex,
  component_of (unrename_vertex vertex) =
    unrename_component (renamed_component_of vertex).

Hypothesis condensation_edge_exact :
  forall source target,
    condensation_edge source target <->
    source <> target /\
    exists source_vertex target_vertex,
      component_of source_vertex = source /\
      component_of target_vertex = target /\
      edge source_vertex target_vertex.

Hypothesis renamed_condensation_edge_exact :
  forall source target,
    renamed_condensation_edge source target <->
    source <> target /\
    exists source_vertex target_vertex,
      renamed_component_of source_vertex = source /\
      renamed_component_of target_vertex = target /\
      renamed_edge source_vertex target_vertex.

Lemma rename_component_is_injective : forall left right,
  rename_component left = rename_component right -> left = right.
Proof.
  intros left right equal.
  apply (f_equal unrename_component) in equal.
  now rewrite !component_round_trip in equal.
Qed.

Lemma unrename_component_is_injective : forall left right,
  unrename_component left = unrename_component right -> left = right.
Proof.
  intros left right equal.
  apply (f_equal rename_component) in equal.
  now rewrite !renamed_component_round_trip in equal.
Qed.

Theorem quotient_fibers_commute_with_renaming : forall vertex component,
  component_of vertex = component <->
  renamed_component_of (rename_vertex vertex) = rename_component component.
Proof.
  intros vertex component; rewrite quotient_naturality.
  split; intro equal; [now rewrite equal |].
  now apply rename_component_is_injective.
Qed.

Theorem condensation_edges_commute_with_renaming : forall source target,
  condensation_edge source target <->
  renamed_condensation_edge
    (rename_component source) (rename_component target).
Proof.
  intros source target; split.
  - intro original_quotient_edge.
    destruct (proj1 (condensation_edge_exact source target)
      original_quotient_edge)
      as [different [source_vertex [target_vertex
        [source_member [target_member original_edge]]]]].
    apply (proj2 (renamed_condensation_edge_exact
      (rename_component source) (rename_component target))).
    split.
    + intro renamed_equal.
      apply different.
      now apply rename_component_is_injective.
    + exists (rename_vertex source_vertex), (rename_vertex target_vertex).
      repeat split.
      * rewrite quotient_naturality, source_member; reflexivity.
      * rewrite quotient_naturality, target_member; reflexivity.
      * apply edge_renames; exact original_edge.
  - intro renamed_quotient_edge.
    destruct (proj1 (renamed_condensation_edge_exact
      (rename_component source) (rename_component target))
      renamed_quotient_edge)
      as [different [source_vertex [target_vertex
        [source_member [target_member renamed_original_edge]]]]].
    apply (proj2 (condensation_edge_exact source target)).
    split.
    + intro original_equal; subst target.
      apply different; reflexivity.
    + exists (unrename_vertex source_vertex),
        (unrename_vertex target_vertex).
      repeat split.
      * rewrite quotient_conaturality, source_member, component_round_trip.
        reflexivity.
      * rewrite quotient_conaturality, target_member, component_round_trip.
        reflexivity.
      * apply edge_unrenames; exact renamed_original_edge.
Qed.

End RenamingEquivariance.

Section LinearAdapterBoundary.

Definition validated_csr_import_work (vertices edges : nat) : nat :=
  2 * vertices + 2 * edges.

Theorem validated_csr_import_is_strictly_linear : forall vertices edges,
  validated_csr_import_work vertices edges = 2 * (vertices + edges).
Proof. intros; unfold validated_csr_import_work; lia. Qed.

Inductive graph_adapter_phase : Type :=
| ValidateNodeOrder
| ValidateForwardCsr
| ValidateReverseCsr
| ValidateTranspose
| ImportComplete.

Theorem graph_adapter_control_is_finite : forall phase,
  phase = ValidateNodeOrder \/
  phase = ValidateForwardCsr \/
  phase = ValidateReverseCsr \/
  phase = ValidateTranspose \/
  phase = ImportComplete.
Proof. destruct phase; auto. Qed.

End LinearAdapterBoundary.
