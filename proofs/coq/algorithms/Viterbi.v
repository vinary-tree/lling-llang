(** * Viterbi Specification Lemmas

    This module contains checked facts about the finite candidate-list and
    Bellman-update specifications used to state Viterbi-style dynamic programs.
    It intentionally avoids claiming correctness for an executable Rocq
    Viterbi implementation that is not present in this proof tree.
*)

Require Import Coq.Lists.List.
Require Import Coq.Arith.Arith.
Require Import Coq.Classes.Morphisms.
Require Import LlingLlang.foundations.Semiring.
Require Import LlingLlang.foundations.TropicalWeight.
Require Import LlingLlang.wfst.Definitions.
Require Import LlingLlang.wfst.Paths.
Require Import LlingLlang.wfst.Language.

Import ListNotations.

(** ** Finite Candidate Specification *)

Section ViterbiSpec.
  Context {W : Type} `{IdempotentSemiring W}.

  #[local]
  Instance viterbi_sr_plus_Proper :
    Proper (sr_eq ==> sr_eq ==> sr_eq) sr_plus := sr_plus_proper.

  #[local]
  Instance viterbi_sr_times_Proper :
    Proper (sr_eq ==> sr_eq ==> sr_eq) sr_times := sr_times_proper.

  Definition PathSet := list (@Path W).

  Definition all_accepting (fst : Wfst W) (candidates : PathSet) : Prop :=
    Forall (accepting_path fst) candidates.

  Definition viterbi_value_over (candidates : PathSet) : W :=
    fold_right (fun p acc => path_weight p ⊕ acc) 𝟘 candidates.

  Lemma viterbi_value_over_empty :
    viterbi_value_over [] ≡ (𝟘 : W).
  Proof.
    unfold viterbi_value_over. simpl. apply sr_eq_refl.
  Qed.

  Lemma viterbi_value_over_cons : forall p candidates,
    viterbi_value_over (p :: candidates) ≡
    path_weight p ⊕ viterbi_value_over candidates.
  Proof.
    intros p candidates. unfold viterbi_value_over. simpl. apply sr_eq_refl.
  Qed.

  Lemma all_accepting_cons_inv : forall fst p candidates,
    all_accepting fst (p :: candidates) ->
    accepting_path fst p /\ all_accepting fst candidates.
  Proof.
    intros fst p candidates Hacc.
    unfold all_accepting in Hacc.
    inversion Hacc; subst; split; assumption.
  Qed.

  (** ** Bellman Equation *)

  Definition bellman_update (fst : Wfst W) (d : StateId -> W) (s : StateId) : W :=
    let transitions := get_outgoing fst s in
    let final_w := match final_weight fst s with
                   | Some w => w
                   | None => sr_zero
                   end in
    fold_left (fun acc t => sr_plus acc (sr_times (tr_weight t) (d (tr_to t))))
              transitions final_w.

  Lemma bellman_update_no_outgoing : forall fst d s final_w,
    get_outgoing fst s = [] ->
    final_weight fst s = Some final_w ->
    bellman_update fst d s ≡ final_w.
  Proof.
    intros fst d s final_w Hout Hfinal.
    unfold bellman_update. rewrite Hout, Hfinal. simpl. apply sr_eq_refl.
  Qed.

  Lemma bellman_update_nonfinal_no_outgoing : forall fst d s,
    get_outgoing fst s = [] ->
    final_weight fst s = None ->
    bellman_update fst d s ≡ 𝟘.
  Proof.
    intros fst d s Hout Hfinal.
    unfold bellman_update. rewrite Hout, Hfinal. simpl. apply sr_eq_refl.
  Qed.

End ViterbiSpec.

(** ** Tropical Semiring Instantiation *)

Section ViterbiTropical.

  Lemma tropical_viterbi_empty_is_unreachable :
    viterbi_value_over (@nil (@Path tropical)) ≡ (𝟘 : tropical).
  Proof.
    apply viterbi_value_over_empty.
  Qed.

End ViterbiTropical.

(** ** Acyclic WFST Predicate *)

Section ViterbiAcyclic.
  Context {W : Type} `{IdempotentSemiring W}.

  Definition wfst_acyclic (fst : Wfst W) : Prop :=
    forall s : StateId,
      is_valid_state fst s ->
      forall p : @Path W,
        path_valid p ->
        (forall t : Transition W, In t p -> tr_from t = s \/ tr_to t = s) ->
        length p > 0 ->
        match p with
        | [] => False
        | t :: _ => tr_from t <> s \/ match last p t with
                                      | t_last => tr_to t_last <> s
                                      end
        end.

  Lemma wfst_acyclic_excludes_closed_nonempty_path : forall fst s p,
    wfst_acyclic fst ->
    is_valid_state fst s ->
    path_valid p ->
    (forall t : Transition W, In t p -> tr_from t = s \/ tr_to t = s) ->
    length p > 0 ->
    match p with
    | [] => False
    | t :: _ => tr_from t <> s \/ match last p t with
                                  | t_last => tr_to t_last <> s
                                  end
    end.
  Proof.
    intros fst s p Hacyclic Hvalid Hpath Htouch Hlen.
    apply Hacyclic; assumption.
  Qed.

End ViterbiAcyclic.
