(** * Weighted Languages

    Definitions for the weighted language recognized by a WFST.

    The weighted language L(A) of a WFST A assigns a weight to each
    input/output string pair, computed as the semiring sum of all
    accepting path weights.
*)

Require Import Coq.Lists.List.
Require Import LlingLlang.foundations.Semiring.
Require Import LlingLlang.wfst.Definitions.
Require Import LlingLlang.wfst.Paths.

Import ListNotations.

(** ** String Type *)

Section Language.
  Context {W : Type} `{Semiring W}.

  (** A string is a list of labels (None represents epsilon) *)
  Definition LabelString := list (option Label).

  (** Remove epsilon labels from a string *)
  Fixpoint remove_epsilons (s : LabelString) : list Label :=
    match s with
    | [] => []
    | None :: rest => remove_epsilons rest
    | Some l :: rest => l :: remove_epsilons rest
    end.

  (** ** Language Definition *)

  (** The weight of a string pair (input, output) in a WFST
      is the sum of weights of all accepting paths with those labels.

      Note: This is a specification-level definition. Computing it
      requires enumerating all paths, which may be infinite for cyclic WFSTs. *)

  (** Paths that match a given input/output string pair *)
  Definition path_matches (p : @Path W) (input output : LabelString) : Prop :=
    remove_epsilons (@path_input W p) = remove_epsilons input /\
    remove_epsilons (@path_output W p) = remove_epsilons output.

  (** Final-state contribution for an accepting path. *)
  Definition path_final_weight (fst : Wfst W) (p : @Path W) : option W :=
    match p with
    | [] => final_weight fst (wfst_start fst)
    | t :: _ =>
        match last p t with
        | t_last => final_weight fst (tr_to t_last)
        end
    end.

  (** Full accepting-path weight, including the final weight. *)
  Definition accepting_path_weight (fst : Wfst W) (p : @Path W) : W :=
    match path_final_weight fst p with
    | Some fw => path_weight p ⊗ fw
    | None => 𝟘
    end.

  (** ** Language Equivalence *)

  (** One-way language simulation over accepting paths. *)
  Definition language_simulates (fst1 fst2 : Wfst W) : Prop :=
    forall input output : LabelString,
      forall p1 : @Path W,
        accepting_path fst1 p1 -> path_matches p1 input output ->
        exists p2 : @Path W,
          accepting_path fst2 p2 /\
          path_matches p2 input output /\
          accepting_path_weight fst1 p1 ≡ accepting_path_weight fst2 p2.

  (** Two WFSTs are language-equivalent when they simulate each other. *)
  Definition language_equiv (fst1 fst2 : Wfst W) : Prop :=
    language_simulates fst1 fst2 /\ language_simulates fst2 fst1.

  (** Language equivalence is reflexive *)
  Lemma language_equiv_refl : forall fst : Wfst W,
    language_equiv fst fst.
  Proof.
    unfold language_equiv, language_simulates.
    intro fst. split.
    - intros input output p Hacc Hmatch.
      exists p. split.
      + exact Hacc.
      + split; [exact Hmatch | apply sr_eq_refl].
    - intros input output p Hacc Hmatch.
      exists p. split.
      + exact Hacc.
      + split; [exact Hmatch | apply sr_eq_refl].
  Qed.

  (** Language equivalence is symmetric *)
  Lemma language_equiv_sym : forall fst1 fst2 : Wfst W,
    language_equiv fst1 fst2 -> language_equiv fst2 fst1.
  Proof.
    unfold language_equiv.
    intros fst1 fst2 [H12 H21]. split; assumption.
  Qed.

  (** Language equivalence is transitive *)
  Lemma language_equiv_trans : forall fst1 fst2 fst3 : Wfst W,
    language_equiv fst1 fst2 -> language_equiv fst2 fst3 -> language_equiv fst1 fst3.
  Proof.
    unfold language_equiv, language_simulates.
    intros fst1 fst2 fst3 [H12 H21] [H23 H32].
    split.
    - intros input output p1 Hacc1 Hmatch1.
      destruct (H12 input output p1 Hacc1 Hmatch1) as
        [p2 [Hacc2 [Hmatch2 Heq12]]].
      destruct (H23 input output p2 Hacc2 Hmatch2) as
        [p3 [Hacc3 [Hmatch3 Heq23]]].
      exists p3. split.
      + exact Hacc3.
      + split.
        * exact Hmatch3.
        * eapply sr_eq_trans; eauto.
    - intros input output p3 Hacc3 Hmatch3.
      destruct (H32 input output p3 Hacc3 Hmatch3) as
        [p2 [Hacc2 [Hmatch2 Heq32]]].
      destruct (H21 input output p2 Hacc2 Hmatch2) as
        [p1 [Hacc1 [Hmatch1 Heq21]]].
      exists p1. split.
      + exact Hacc1.
      + split.
        * exact Hmatch1.
        * eapply sr_eq_trans; eauto.
  Qed.

  (** ** Acceptor Language *)

  (** For acceptors (input = output), language simplifies to weighted strings *)
  Definition acceptor_language (fst : Wfst W) (s : LabelString) : Prop :=
    exists p : @Path W,
      accepting_path fst p /\
      path_matches p s s.

End Language.

(** ** Language Operations *)

Section LanguageOperations.
  Context {W : Type} `{Semiring W}.

  (** Union of languages: L(A ∪ B)(w) = L(A)(w) ⊕ L(B)(w) *)
  (* Specification: if a string is in either language, its weight is the sum *)

  (** Concatenation of languages: L(A · B)(uv) = L(A)(u) ⊗ L(B)(v) *)
  (* Specification: weights compose for concatenated strings *)

  (** Kleene closure: the weight is the sum over n >= 0 of L(A)^n(w). *)
  (* Specification: sum over all decompositions into A-strings *)

End LanguageOperations.
