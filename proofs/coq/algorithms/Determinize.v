(** * Determinization Support Lemmas

    These checked lemmas cover the weighted-subset operations used by
    determinization. Full determinization correctness requires a formal
    executable construction relation; this file does not assert correctness for
    an unspecified output WFST.
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

  Definition determinize_step (fst0 : Wfst W) (ss : SubsetState)
      (label : option Label) : option (SubsetState * W) :=
    let targets := fold_left (fun acc (sw : StateId * W) =>
      let (s, w) := sw in
      let trans := filter (fun t => match tr_input t, label with
                                    | Some l1, Some l2 => Nat.eqb l1 l2
                                    | None, None => true
                                    | _, _ => false
                                    end)
                          (get_outgoing fst0 s) in
      fold_left (fun acc' t =>
        (tr_to t, sr_times w (tr_weight t)) :: acc'
      ) trans acc
    ) ss [] in
    match targets with
    | [] => None
    | _ =>
        let total_weight := fold_left (fun acc (sw : StateId * W) =>
          sr_plus acc (snd sw)) targets sr_zero in
        Some (targets, total_weight)
    end.

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

  Lemma left_divide_sound : forall a divisor quotient,
    ~(divisor ≡ 𝟘) ->
    sr_left_divide a divisor = Some quotient ->
    quotient ⊗ divisor ≡ a.
  Proof.
    intros a divisor quotient Hnz Hdiv.
    eapply sr_left_divide_spec; eauto.
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

  Lemma deterministic_functional_is_sequential : forall fst,
    wfst_deterministic fst ->
    wfst_functional fst ->
    wfst_sequential fst.
  Proof.
    intros fst Hdet Hfun. split; assumption.
  Qed.

End FunctionalDeterminization.
