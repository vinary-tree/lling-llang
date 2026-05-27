# Failed Proof Strategies

This document records proof strategies that were attempted but did not work.
The purpose is to prevent re-attempting approaches that have already been found
to be unproductive.

## Format

Each entry should include:
- **Theorem/Lemma**: What was being proven
- **Strategy**: What approach was attempted
- **Why It Failed**: Technical reason for failure
- **Date**: When the attempt was made
- **Time Spent**: Approximate effort invested

---

## Entries

### [Template Entry]

**Theorem/Lemma**: `example_theorem`

**Strategy**: Describe the approach taken

**Why It Failed**: Explain why it didn't work (missing lemmas, complexity explosion,
wrong abstraction level, etc.)

**Possible Alternatives**: If any ideas for future attempts remain

**Date**: YYYY-MM-DD

**Time Spent**: ~X hours

---

### Occurrence-enumerator exactness — `injection` on a primitive record

**Theorem/Lemma**: `product_occurrence_step_paths_to_NoDup` (and any reasoning
over `mkTransitionOccurrence _ _ _ = mkTransitionOccurrence _ _ _`)

**Strategy**: Decompose a record equality with a second, standalone
`injection Hocc as Hidx _.` after first peeling a `cons` equality.

**Why It Failed**: `TransitionOccurrence` is a **primitive record** (the field
projections η-reduce), so a standalone `injection` on `mk _ _ _ = mk _ _ _`
reports `Error: Nothing to inject.`

**What Works Instead**: A single `injection` on the *outer* `cons` equality
already descends through the primitive record and yields the field equalities
directly (e.g. `next_index = S (next_index + k2)`). The robust idiom that does
not depend on the exact number/order of generated equations is
`injection Heq. intros. lia.` (or extract one field with
`apply (f_equal occ_index) in H; cbn [occ_index] in H`).

**Date**: 2026-05-26

**Time Spent**: ~15 min

---

### Occurrence-enumerator exactness — assorted tactic gotchas

**Context**: Membership soundness/completeness lemmas in `MatrixSemantics.v`.

- **Unresolved `W` evar in a lemma statement.** `forall fst s t, wfst_well_formed
  fst -> In t (get_outgoing fst s) -> tr_from t = s` fails to elaborate
  (`Cannot infer the implicit parameter W of tr_from`) because the only
  constraints come from *polymorphic* accessors (`get_outgoing`,
  `wfst_well_formed`), which leave `W` an evar. **Fix**: annotate the binders
  with the section variable — `forall (fst : Wfst W) (s : StateId) (t : Transition
  W), ...`.
- **`apply L` / `apply L with ...` failing with `Unable to find an instance for
  the variable X`.** Happens when `L`'s conclusion does not mention a variable
  that appears only in its premises (e.g. `get_outgoing_tr_from`'s `fst`, or
  `product_transition_matches_target_lt_dim`'s `source`). **Fix**: supply the
  missing variable explicitly, e.g. `apply L with (fst := fst) (source := source)`.
- **`destruct (consume_label ...) eqn:H` rewrites the goal's existential
  witnesses.** After the `destruct`, a goal conjunct stated as `consume_label ...
  = Some ni` has had its left side replaced by `Some ni`, so it is now
  `Some ni = Some ni`; discharge it with `reflexivity`, not `exact H`.

**Date**: 2026-05-26

**Time Spent**: ~20 min total

---

### De-self-referencing the product semantics — tactic gotchas

**Context**: Reverse walk⇒path and `consume_path_labels`⇒`path_matches` lemmas
(`product_occurrence_walk_connects_path`, `consume_path_labels_some_prefix`).

- **`repeat split` over-solves reflexive goals.** On a goal like
  `[] = [] /\ [] = [] /\ ip <= ip /\ op <= op`, `repeat split` discharges each
  conjunct (because `split` = `constructor`, and `eq_refl`/`le_n` close the
  reflexive sub-goals), leaving zero goals — so a trailing `[t1|t2|t3|t4]` fails
  with "Incorrect number of goals". **Fix**: use explicit nested
  `split; [tac |]. split; [tac |]. split; tac.` instead.
- **`f_equal` on `a :: X = a :: Y` can leave a spurious `a = a` goal**, so
  `f_equal. exact H.` mis-targets. **Fix**: `rewrite H; reflexivity` (which also
  absorbs `path_input rest` vs `map tr_input rest` by conversion).
- **`destruct (tr_input t)` does not substitute when the scrutinee is hidden.**
  In a goal `… = remove_epsilons (path_input (t :: rest))`, `tr_input t` is
  buried inside the unreduced `map tr_input (t :: rest)`, so `destruct (tr_input
  t)` leaves a stuck `match tr_input t with …`. **Fix**: `cbn [path_input map
  remove_epsilons]` *first* to expose the `match tr_input t with …` scrutinee,
  then `destruct (tr_input t)` reduces it.
- **`destruct (f x) eqn:Hq` reverts/re-introduces dependent hypotheses**, so it
  already substitutes `f x` in them; a follow-up `rewrite Hq in Hdep` then fails
  with "Found no subterm". **Fix**: use the already-substituted hypotheses
  directly.
- **Layering**: `accepting_path_weight` / `path_final_weight_or_zero` live in
  `Language.v`, which `Require`s `MatrixSemantics.v`. Weight-grounding lemmas
  that mention them must live in `Language.v`, not `MatrixSemantics.v` (where
  only `Paths`-level `accepting_path`/`path_weight` are in scope).

**Date**: 2026-05-26

**Time Spent**: ~40 min total

---

## Guidelines

1. **Be Specific**: Include the exact tactic sequence or proof structure attempted
2. **Document Error Messages**: Copy relevant Coq error messages
3. **Record Dependencies**: Note if failure was due to missing library support
4. **Suggest Alternatives**: If you have ideas for what might work instead, note them
5. **Time Estimates**: Help future developers budget their time appropriately
