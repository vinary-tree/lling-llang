(** * ProductRegistry — the lazy-composition product-state interner

    Lazy composition assigns each reachable product state
    $`(q_L, q_R, \mathrm{filter})`$ a dense `u64` id the first time it is seen,
    caching the assignment so the same product state always maps to the same id.
    In `src/bindings.rs` this is [ProductRegistry], a pair of a `Vec` (id ->
    state) and a `HashMap` (state -> id) kept mutually inverse. This file is the
    formal model of that interner and its correctness laws — obligation #19,
    the formal home of the product-registry invariants
    (registry: proofs/doc/abi-invariants.tsv, LLING-COMP-2..5).

    The essential invariant is that the id vector has no duplicate states, which
    makes id <-> state a bijection on the interned set. Registration is
    idempotent (an already-interned state returns its existing id and does not
    grow the registry) and append-only (a fresh state takes the next id and
    never disturbs an existing one). The `HashMap` side of the Rust structure is
    a cache of the inverse of the `Vec`; the model derives it from the vector,
    so the two-map coherence the Rust code maintains by construction is here a
    theorem about a single source of truth.

    Registry: proofs/doc/abi-invariants.tsv, LLING-COMP-2..5.
*)

From Stdlib Require Import Lists.List.
From Stdlib Require Import Arith.Arith.
Import ListNotations.

(** An ABI product state: (left, right, filter), modeled over [nat]. The Rust
    [AbiProductState] carries two provider state ids and an epsilon-filter tag;
    only decidable equality matters to the interner. *)
Definition state : Type := (nat * nat * nat)%type.

Definition state_eq_dec : forall x y : state, {x = y} + {x <> y}.
Proof. repeat decide equality. Defined.

(** The registry is the id vector; the id of a state is its index. *)
Definition registry : Type := list state.

(** Well-formed: no duplicate states, so index <-> state is a bijection. Every
    registry this model builds preserves it (proved below). *)
Definition wf (r : registry) : Prop := NoDup r.

(** id -> state. *)
Definition get (r : registry) (id : nat) : option state := nth_error r id.

(** state -> first id, if interned (the model of the HashMap side). *)
Fixpoint find (r : registry) (s : state) : option nat :=
  match r with
  | [] => None
  | x :: xs =>
      if state_eq_dec x s
      then Some 0
      else match find xs s with
           | Some i => Some (S i)
           | None => None
           end
  end.

(** register: return the existing id if interned, else append and assign the
    next id (mirrors [ProductRegistry::register]; the fallible `u64` conversion
    is a representation limit the algebra abstracts over — ids are unbounded
    [nat] here). *)
Definition register (r : registry) (s : state) : (nat * registry) :=
  match find r s with
  | Some id => (id, r)
  | None => (length r, r ++ [s])
  end.

(** A fresh registry seeded with the start product state (id 0). *)
Definition initial (start : state) : registry := [start].

(** ** Helper lemmas about [find] and [nth_error] *)

Lemma find_some_nth :
  forall r s i, find r s = Some i -> nth_error r i = Some s.
Proof.
  induction r as [| x xs IH]; intros s i H; simpl in H.
  - discriminate.
  - destruct (state_eq_dec x s) as [->|Hneq].
    + injection H as <-. reflexivity.
    + destruct (find xs s) as [j|] eqn:Hf; [| discriminate].
      injection H as <-. simpl. apply IH. exact Hf.
Qed.

Lemma find_none_not_in :
  forall r s, find r s = None -> ~ In s r.
Proof.
  induction r as [| x xs IH]; intros s H; simpl in *.
  - intros [].
  - destruct (state_eq_dec x s) as [->|Hneq].
    + discriminate.
    + destruct (find xs s) as [j|] eqn:Hf; [discriminate|].
      intros [Heq | Hin].
      * apply Hneq. exact Heq.
      * apply (IH s Hf). exact Hin.
Qed.

Lemma in_find_some :
  forall r s, In s r -> exists i, find r s = Some i.
Proof.
  intros r s Hin.
  destruct (find r s) as [i|] eqn:Hf.
  - exists i. reflexivity.
  - exfalso. apply (find_none_not_in r s Hf). exact Hin.
Qed.

Lemma nth_error_snoc_end :
  forall (r : registry) (s : state), nth_error (r ++ [s]) (length r) = Some s.
Proof.
  induction r as [| x xs IH]; intro s; simpl.
  - reflexivity.
  - apply IH.
Qed.

Lemma nodup_snoc :
  forall (r : registry) (s : state), NoDup r -> ~ In s r -> NoDup (r ++ [s]).
Proof.
  induction r as [| x xs IH]; intros s Hnd Hnin; simpl.
  - constructor; [intros [] | constructor].
  - inversion Hnd as [| x' xs' Hx Hxs]; subst.
    constructor.
    + rewrite in_app_iff. intros [Hin | Hin].
      * apply Hx. exact Hin.
      * simpl in Hin. destruct Hin as [Heq | []].
        apply Hnin. left. symmetry. exact Heq.
    + apply IH; [exact Hxs |].
      intro Hin. apply Hnin. right. exact Hin.
Qed.

(** ** LLING-COMP-2: registration is idempotent *)

(** An already-interned state returns its existing id and does not grow the
    registry. *)
Theorem register_idempotent :
  forall r s, In s r -> exists id, register r s = (id, r) /\ get r id = Some s.
Proof.
  intros r s Hin.
  destruct (in_find_some r s Hin) as [i Hf].
  exists i. unfold register, get. rewrite Hf. split.
  - reflexivity.
  - apply find_some_nth. exact Hf.
Qed.

(** ** LLING-COMP-3: a fresh state takes the next id, recoverable exactly *)

Theorem register_fresh :
  forall r s, ~ In s r ->
    register r s = (length r, r ++ [s]) /\ get (r ++ [s]) (length r) = Some s.
Proof.
  intros r s Hnin.
  destruct (find r s) as [i|] eqn:Hf.
  - exfalso. apply Hnin. apply find_some_nth in Hf.
    apply nth_error_In in Hf. exact Hf.
  - unfold register, get. rewrite Hf. split.
    + reflexivity.
    + apply nth_error_snoc_end.
Qed.

(** ** LLING-COMP-4: registration is append-only — existing ids are stable *)

Theorem register_stable :
  forall r s id, id < length r -> get (snd (register r s)) id = get r id.
Proof.
  intros r s id Hlt. unfold register, get.
  destruct (find r s) as [i|] eqn:Hf; simpl.
  - reflexivity.
  - apply nth_error_app1. exact Hlt.
Qed.

(** ** LLING-COMP-5: registration preserves well-formedness (the bijection) *)

Theorem register_preserves_wf :
  forall r s, wf r -> wf (snd (register r s)).
Proof.
  intros r s Hwf. unfold wf, register in *.
  destruct (find r s) as [i|] eqn:Hf; simpl.
  - exact Hwf.
  - apply nodup_snoc; [exact Hwf |].
    apply find_none_not_in. exact Hf.
Qed.

(** The round trip: after registering, the returned id decodes back to the
    registered state (whether it was fresh or already present). *)
Theorem register_get_roundtrip :
  forall r s,
    let (id, r') := register r s in get r' id = Some s.
Proof.
  intros r s. unfold register, get.
  destruct (find r s) as [i|] eqn:Hf.
  - apply find_some_nth. exact Hf.
  - apply nth_error_snoc_end.
Qed.

(** On a well-formed registry, id -> state is injective: distinct valid ids
    decode to distinct states (there is no aliasing of product states). *)
Theorem get_injective :
  forall r i j si sj,
    wf r -> get r i = Some si -> get r j = Some sj -> si = sj -> i = j.
Proof.
  intros r i j si sj Hwf Hi Hj Heq. subst sj.
  unfold wf, get in *.
  rewrite NoDup_nth_error in Hwf.
  apply Hwf.
  - assert (Hne : nth_error r i <> None) by (rewrite Hi; discriminate).
    apply nth_error_Some in Hne. exact Hne.
  - rewrite Hi, Hj. reflexivity.
Qed.

(** ** The seed registry *)

Theorem initial_wf : forall start, wf (initial start).
Proof.
  intro start. unfold wf, initial.
  constructor; [intros [] | constructor].
Qed.

Theorem initial_start_is_zero : forall start, get (initial start) 0 = Some start.
Proof. intro start. reflexivity. Qed.
