(** * Determinization Specification and Partial Correctness Lemmas

    These checked lemmas cover weighted-subset operations, normalization
    soundness, functional/sequential preconditions, and the correctness of the
    identity result for WFSTs that are already deterministic.
*)

Require Import Coq.Lists.List.
Require Import Coq.Arith.Arith.
Require Import Coq.Classes.Morphisms.
Require Import LlingLlang.foundations.Semiring.
Require Import LlingLlang.wfst.Definitions.
Require Import LlingLlang.wfst.Paths.
Require Import LlingLlang.wfst.Language.

Import ListNotations.

Section Determinization.
  Context {W : Type} `{Semiring W} `{WeaklyLeftDivisibleSemiring W}.

  #[local]
  Instance determinize_sr_plus_Proper :
    Proper (sr_eq ==> sr_eq ==> sr_eq) sr_plus := sr_plus_proper.

  Definition SubsetState := list (StateId * W).

  Definition subset_weight (ss : SubsetState) (s : StateId) : W :=
    match find (fun p => Nat.eqb (fst p) s) ss with
    | Some (_, w) => w
    | None => sr_zero
    end.

  Definition merge_subsets (ss1 ss2 : SubsetState) : SubsetState :=
    fold_left (fun acc (sw : StateId * W) =>
      let (s, w) := sw in
      match find (fun p => Nat.eqb (fst p) s) acc with
      | Some (_, w') =>
          map (fun p => if Nat.eqb (fst p) s then (s, sr_plus w w') else p) acc
      | None => (s, w) :: acc
      end
    ) ss2 ss1.

  Definition determinize_targets
      (fst0 : Wfst W) (ss : SubsetState) (label : option Label)
      : SubsetState :=
    fold_left (fun acc (sw : StateId * W) =>
      let (s, w) := sw in
      let trans := filter (fun t => match tr_input t, label with
                                    | Some l1, Some l2 => Nat.eqb l1 l2
                                    | None, None => true
                                    | _, _ => false
                                    end)
                          (get_outgoing fst0 s) in
      fold_left (fun acc' t =>
        merge_subsets acc' [(tr_to t, sr_times w (tr_weight t))]
      ) trans acc
    ) ss [].

	  Definition determinize_step (fst0 : Wfst W) (ss : SubsetState)
	      (label : option Label) : option (SubsetState * W) :=
    let targets := determinize_targets fst0 ss label in
    match targets with
    | [] => None
    | _ =>
        let total_weight := fold_left (fun acc (sw : StateId * W) =>
          sr_plus acc (snd sw)) targets sr_zero in
	        Some (targets, total_weight)
	    end.

  Fixpoint normalize_subset (divisor : W) (raw : SubsetState)
      : option SubsetState :=
    match raw with
    | [] => Some []
    | (s, w) :: rest =>
        match sr_left_divide w divisor, normalize_subset divisor rest with
        | Some quotient, Some normalized_rest =>
            Some ((s, quotient) :: normalized_rest)
        | _, _ => None
        end
    end.

  Definition subset_normalized_by
      (divisor : W) (raw normalized : SubsetState) : Prop :=
    Forall2
      (fun sw nw =>
         fst sw = fst nw /\
         sr_left_divide (snd sw) divisor = Some (snd nw))
      raw normalized.

  Definition determinize_step_spec
      (fst0 : Wfst W) (ss : SubsetState) (label : option Label)
      (raw normalized : SubsetState) (total : W) : Prop :=
    determinize_step fst0 ss label = Some (raw, total) /\
    subset_normalized_by total raw normalized.

  Lemma subset_weight_empty : forall s,
    subset_weight [] s ≡ (𝟘 : W).
  Proof.
    intro s. unfold subset_weight. simpl. apply sr_eq_refl.
  Qed.

  Lemma subset_weight_hit_head : forall s w rest,
    subset_weight ((s, w) :: rest) s = w.
  Proof.
    intros s w rest. unfold subset_weight. simpl.
    rewrite Nat.eqb_refl. reflexivity.
  Qed.

  Lemma merge_subsets_empty_empty :
    merge_subsets [] [] = ([] : SubsetState).
  Proof.
    unfold merge_subsets. reflexivity.
  Qed.

  Lemma merge_subsets_nil_r : forall ss,
    merge_subsets ss [] = ss.
  Proof.
    intro ss. unfold merge_subsets. simpl. reflexivity.
  Qed.

	  Lemma determinize_step_empty_subset : forall fst0 label,
	    determinize_step fst0 [] label = None.
  Proof.
    intros fst0 label. unfold determinize_step. simpl. reflexivity.
	  Qed.

  Lemma determinize_step_some_nonempty : forall fst0 ss label raw total,
    determinize_step fst0 ss label = Some (raw, total) ->
    raw <> [].
  Proof.
    intros fst0 ss label raw total Hstep.
    unfold determinize_step in Hstep.
    destruct (determinize_targets fst0 ss label) as [| target rest].
    - simpl in Hstep. discriminate.
    - inversion Hstep; subst. intro Hnil. discriminate Hnil.
  Qed.

  Lemma normalize_subset_sound : forall divisor raw normalized,
    normalize_subset divisor raw = Some normalized ->
    subset_normalized_by divisor raw normalized.
  Proof.
    intros divisor raw.
    induction raw as [| [s w] rest IH]; intros normalized Hnorm.
    - simpl in Hnorm. inversion Hnorm; subst. constructor.
    - simpl in Hnorm.
      destruct (sr_left_divide w divisor) as [quotient |] eqn:Hdiv;
        try discriminate.
      destruct (normalize_subset divisor rest) as [normalized_rest |] eqn:Hrest;
        try discriminate.
      inversion Hnorm; subst.
      constructor.
      + split; [reflexivity | exact Hdiv].
      + apply IH. reflexivity.
  Qed.

  Lemma determinize_step_normalized_sound :
    forall fst0 ss label raw total normalized,
      determinize_step fst0 ss label = Some (raw, total) ->
      normalize_subset total raw = Some normalized ->
      determinize_step_spec fst0 ss label raw normalized total.
  Proof.
    intros fst0 ss label raw total normalized Hstep Hnorm.
    unfold determinize_step_spec.
    split.
    - exact Hstep.
    - apply normalize_subset_sound. exact Hnorm.
  Qed.

	  Lemma left_divide_sound : forall a divisor quotient,
	    ~(divisor ≡ 𝟘) ->
    sr_left_divide a divisor = Some quotient ->
    quotient ⊗ divisor ≡ a.
  Proof.
    intros a divisor quotient Hnz Hdiv.
	    eapply sr_left_divide_spec; eauto.
	  Qed.

  Lemma normalized_subset_sound : forall divisor raw normalized,
    ~(divisor ≡ 𝟘) ->
    subset_normalized_by divisor raw normalized ->
    Forall2
      (fun sw nw => fst sw = fst nw /\ snd nw ⊗ divisor ≡ snd sw)
      raw normalized.
  Proof.
    intros divisor raw normalized Hnz Hnorm.
    induction Hnorm.
    - constructor.
    - destruct x as [sx wx].
      destruct y as [sy wy].
      simpl in *.
      destruct H2 as [Hstate_eq Hdiv].
      constructor.
      + split.
        * exact Hstate_eq.
        * eapply sr_left_divide_spec; eauto.
      + exact IHHnorm.
  Qed.

End Determinization.

(** ** Functional Determinization Preconditions *)

Section FunctionalDeterminization.
  Context {W : Type} `{Semiring W}.

  Definition wfst_functional (fst : Wfst W) : Prop :=
    forall input : LabelString,
      forall p1 p2 : @Path W,
        accepting_path fst p1 -> accepting_path fst p2 ->
        remove_epsilons (@path_input W p1) = remove_epsilons input ->
        remove_epsilons (@path_input W p2) = remove_epsilons input ->
        remove_epsilons (@path_output W p1) = remove_epsilons (@path_output W p2).

	  Definition wfst_sequential (fst : Wfst W) : Prop :=
	    wfst_deterministic fst /\ wfst_functional fst.

  Definition determinize_correct (fst det : Wfst W) : Prop :=
    wfst_well_formed fst /\
    weighted_language_defined fst /\
    wfst_well_formed det /\
    weighted_language_defined det /\
    wfst_deterministic det /\
    language_equiv fst det.

  Lemma already_deterministic_identity_determinize_correct : forall fst,
    wfst_well_formed fst ->
    weighted_language_defined fst ->
    wfst_deterministic fst ->
    determinize_correct fst fst.
  Proof.
    intros fst Hwf Hdefined Hdet.
    unfold determinize_correct.
    split.
    - exact Hwf.
    - split.
      + exact Hdefined.
      + split.
        * exact Hwf.
        * split.
          -- exact Hdefined.
          -- split.
             ++ exact Hdet.
             ++ apply language_equiv_refl. exact Hdefined.
  Qed.

	  Lemma deterministic_functional_is_sequential : forall fst,
	    wfst_deterministic fst ->
    wfst_functional fst ->
    wfst_sequential fst.
  Proof.
    intros fst Hdet Hfun. split; assumption.
  Qed.

End FunctionalDeterminization.
