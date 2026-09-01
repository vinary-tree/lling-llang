(** * Polarity-directed normalization

    The Rust Presburger normalizer currently alternates between
    [push_negation_inward] and [negate_pred].  This file proves the semantic
    basis for replacing that mutual SCC with one explicit machine whose control
    state contains a polarity bit.

    Positive polarity preserves a formula; negative polarity denotes its
    Boolean complement.  Negation flips the bit.  Conjunction and disjunction
    swap under negative polarity.  Atomic negation uses the theory-specific
    [negate_atom] operation.  Negated existential formulae remain as the
    residual [Not (Exists ...)] form consumed by the automata compiler, matching
    the production representation exactly.
*)

From Stdlib Require Import Bool.
From Stdlib Require Import List.
Import ListNotations.

Section PolarityNormalization.

  Context {Atom : Type}.
  Variable atom_sem : Atom -> bool.
  Variable negate_atom : Atom -> Atom.
  Hypothesis negate_atom_sound :
    forall atom, atom_sem (negate_atom atom) = negb (atom_sem atom).
  Variable exists_sem : nat -> bool -> bool.

  Inductive Formula : Type :=
  | FTrue : Formula
  | FFalse : Formula
  | FAtom : Atom -> Formula
  | FAnd : Formula -> Formula -> Formula
  | FOr : Formula -> Formula -> Formula
  | FNot : Formula -> Formula
  | FExists : nat -> Formula -> Formula.

  Fixpoint eval (formula : Formula) : bool :=
    match formula with
    | FTrue => true
    | FFalse => false
    | FAtom atom => atom_sem atom
    | FAnd lhs rhs => eval lhs && eval rhs
    | FOr lhs rhs => eval lhs || eval rhs
    | FNot inner => negb (eval inner)
    | FExists variable body => exists_sem variable (eval body)
    end.

  Fixpoint normalize (negative : bool) (formula : Formula) : Formula :=
    match formula with
    | FTrue => if negative then FFalse else FTrue
    | FFalse => if negative then FTrue else FFalse
    | FAtom atom =>
        if negative then FAtom (negate_atom atom) else FAtom atom
    | FAnd lhs rhs =>
        if negative
        then FOr (normalize true lhs) (normalize true rhs)
        else FAnd (normalize false lhs) (normalize false rhs)
    | FOr lhs rhs =>
        if negative
        then FAnd (normalize true lhs) (normalize true rhs)
        else FOr (normalize false lhs) (normalize false rhs)
    | FNot inner => normalize (negb negative) inner
    | FExists variable body =>
        if negative
        then FNot (FExists variable (normalize false body))
        else FExists variable (normalize false body)
    end.

  Definition push_negation_inward (formula : Formula) : Formula :=
    normalize false formula.

  Definition negate_formula (formula : Formula) : Formula :=
    normalize true formula.

  Lemma and_negated :
    forall lhs rhs,
      negb (lhs && rhs) = negb lhs || negb rhs.
  Proof. destruct lhs, rhs; reflexivity. Qed.

  Lemma or_negated :
    forall lhs rhs,
      negb (lhs || rhs) = negb lhs && negb rhs.
  Proof. destruct lhs, rhs; reflexivity. Qed.

  Theorem normalize_semantics :
    forall formula negative,
      eval (normalize negative formula) =
      if negative then negb (eval formula) else eval formula.
  Proof.
    induction formula as
      [| | atom | lhs IHlhs rhs IHrhs | lhs IHlhs rhs IHrhs
       | inner IHinner | variable body IHbody];
      intros negative; destruct negative; simpl.
    - reflexivity.
    - reflexivity.
    - reflexivity.
    - reflexivity.
    - rewrite negate_atom_sound. reflexivity.
    - reflexivity.
    - rewrite IHlhs, IHrhs. symmetry. apply and_negated.
    - rewrite IHlhs, IHrhs. reflexivity.
    - rewrite IHlhs, IHrhs. symmetry. apply or_negated.
    - rewrite IHlhs, IHrhs. reflexivity.
    - rewrite IHinner. destruct (eval inner); reflexivity.
    - rewrite IHinner. reflexivity.
    - rewrite IHbody. reflexivity.
    - rewrite IHbody. reflexivity.
  Qed.

  Corollary push_negation_inward_sound :
    forall formula,
      eval (push_negation_inward formula) = eval formula.
  Proof.
    intro formula. unfold push_negation_inward.
    apply normalize_semantics.
  Qed.

  Corollary negate_formula_sound :
    forall formula,
      eval (negate_formula formula) = negb (eval formula).
  Proof.
    intro formula. unfold negate_formula.
    apply normalize_semantics.
  Qed.

  Inductive ResidualNnf : Formula -> Prop :=
  | NnfTrue : ResidualNnf FTrue
  | NnfFalse : ResidualNnf FFalse
  | NnfAtom : forall atom, ResidualNnf (FAtom atom)
  | NnfAnd : forall lhs rhs,
      ResidualNnf lhs -> ResidualNnf rhs -> ResidualNnf (FAnd lhs rhs)
  | NnfOr : forall lhs rhs,
      ResidualNnf lhs -> ResidualNnf rhs -> ResidualNnf (FOr lhs rhs)
  | NnfExists : forall variable body,
      ResidualNnf body -> ResidualNnf (FExists variable body)
  | NnfNotExists : forall variable body,
      ResidualNnf body -> ResidualNnf (FNot (FExists variable body)).

  Theorem normalize_is_residual_nnf :
    forall formula negative,
      ResidualNnf (normalize negative formula).
  Proof.
    induction formula as
      [| | atom | lhs IHlhs rhs IHrhs | lhs IHlhs rhs IHrhs
       | inner IHinner | variable body IHbody];
      intros negative; destruct negative; simpl.
    - constructor.
    - constructor.
    - constructor.
    - constructor.
    - constructor.
    - constructor.
    - constructor; [apply IHlhs | apply IHrhs].
    - constructor; [apply IHlhs | apply IHrhs].
    - constructor; [apply IHlhs | apply IHrhs].
    - constructor; [apply IHlhs | apply IHrhs].
    - apply IHinner.
    - apply IHinner.
    - constructor. apply IHbody.
    - constructor. apply IHbody.
  Qed.

  Corollary push_negation_inward_shape :
    forall formula, ResidualNnf (push_negation_inward formula).
  Proof.
    intro formula. unfold push_negation_inward.
    apply normalize_is_residual_nnf.
  Qed.

  Corollary negate_formula_shape :
    forall formula, ResidualNnf (negate_formula formula).
  Proof.
    intro formula. unfold negate_formula.
    apply normalize_is_residual_nnf.
  Qed.

End PolarityNormalization.
