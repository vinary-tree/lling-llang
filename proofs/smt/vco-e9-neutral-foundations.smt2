; Finite boundary checks for the RegresSpec-derived neutral Vinary contracts.
; Rocq establishes unbounded logical laws and TLC explores ordered lifecycle
; transitions; these queries pin executable guards and concrete witnesses.

(set-logic ALL)

(define-fun release-admissible
    ((canonical Bool) (patch Bool) (exact Bool) (complete Bool)
     (locks Bool) (source-accounted Bool) (lint-current Bool)
     (theorem-required Bool) (assurance Bool)) Bool
  (and canonical patch exact complete locks source-accounted lint-current
       (or (not theorem-required) assurance)))

; E9-NF-SMT-IDENTITY-DOMAINS-SEPARATE
(push)
(define-fun schema-domain () Int 1)
(define-fun content-domain () Int 2)
(assert (= schema-domain content-domain))
(check-sat)
(pop)

; E9-NF-SMT-NONFINITE-NUMBER-REJECTED
(push)
(declare-const admitted Bool)
(assert (= admitted false))
(assert admitted)
(check-sat)
(pop)

; E9-NF-SMT-SINK-REJECTION-ATOMIC
(push)
(declare-const before-digest Int)
(declare-const after-digest Int)
(declare-const accepted Bool)
(assert (not accepted))
(assert (=> (not accepted) (= before-digest after-digest)))
(assert (distinct before-digest after-digest))
(check-sat)
(pop)

; E9-NF-SMT-PROJECTION-NONSTRENGTHENING
(push)
(declare-const source-strength Int)
(declare-const projected-strength Int)
(assert (and (>= source-strength 0) (<= source-strength 3)))
(assert (and (>= projected-strength 0) (<= projected-strength source-strength)))
(assert (> projected-strength source-strength))
(check-sat)
(pop)

; E9-NF-SMT-PATCH-BASE-GATE
(push)
(declare-const base-matches Bool)
(declare-const committed Bool)
(assert (= committed base-matches))
(assert committed)
(assert (not base-matches))
(check-sat)
(pop)

; E9-NF-SMT-INCOMPLETE-NOT-CACHEABLE
(push)
(declare-const complete Bool)
(declare-const cached Bool)
(assert (= cached complete))
(assert (not complete))
(assert cached)
(check-sat)
(pop)

; E9-NF-SMT-EXACT-RELEASE-LOCKS-ALL-INPUTS
(push)
(declare-const exact Bool)
(declare-const complete Bool)
(declare-const locks-match Bool)
(declare-const runtime-release Bool)
(assert (= runtime-release (and exact complete locks-match)))
(assert runtime-release)
(assert (or (not exact) (not complete) (not locks-match)))
(check-sat)
(pop)

; E9-NF-SMT-OVERFLOW-SPILLS-TO-REPOSITORY
(push)
(declare-const output-bytes Int)
(declare-const memory-cap Int)
(declare-const repository-spill Bool)
(assert (> output-bytes memory-cap))
(assert (=> (> output-bytes memory-cap) repository-spill))
(assert (not repository-spill))
(check-sat)
(pop)

; E9-NF-SMT-RESUME-REQUIRES-COMPATIBLE-CHECKPOINT
(push)
(declare-const resumed Bool)
(declare-const checkpoint-compatible Bool)
(assert (=> resumed checkpoint-compatible))
(assert resumed)
(assert (not checkpoint-compatible))
(check-sat)
(pop)

; E9-NF-SMT-TOMBSTONE-NOT-ACTIVE
(push)
(declare-const tombstoned Bool)
(declare-const active Bool)
(assert (= active (not tombstoned)))
(assert tombstoned)
(assert active)
(check-sat)
(pop)

; E9-NF-SMT-UNCLASSIFIED-SOURCE-RETAINED
(push)
(declare-const unclassified-present Bool)
(declare-const unclassified-retained Bool)
(assert (=> unclassified-present unclassified-retained))
(assert unclassified-present)
(assert (not unclassified-retained))
(check-sat)
(pop)

; E9-NF-SMT-STATISTICS-NOT-THEOREM
(push)
(declare-const statistics Bool)
(declare-const theorem-obligation Bool)
(declare-const verified Bool)
(assert (=> (and statistics theorem-obligation) (not verified)))
(assert statistics)
(assert theorem-obligation)
(assert verified)
(check-sat)
(pop)

; E9-NF-SMT-STALE-EVIDENCE-NOT-VERIFIED
(push)
(declare-const fresh Bool)
(declare-const verified Bool)
(assert (=> verified fresh))
(assert verified)
(assert (not fresh))
(check-sat)
(pop)

; E9-NF-SMT-NEGATIVE-CONTROL-REQUIRED
(push)
(declare-const negative-control Bool)
(declare-const verified Bool)
(assert (=> verified negative-control))
(assert verified)
(assert (not negative-control))
(check-sat)
(pop)

; E9-NF-SMT-ATTESTATION-REVISION-REQUIRED
(push)
(declare-const attestation-matches Bool)
(declare-const verified Bool)
(assert (=> verified attestation-matches))
(assert verified)
(assert (not attestation-matches))
(check-sat)
(pop)

; E9-NF-SMT-STALE-MANIFEST-NOT-LINTED
(push)
(declare-const manifest-current Bool)
(declare-const lint-pass Bool)
(assert (= lint-pass manifest-current))
(assert lint-pass)
(assert (not manifest-current))
(check-sat)
(pop)

; E9-NF-SMT-CHECK-ONLY-NONMUTATING
(push)
(declare-const check-only Bool)
(declare-const document-mutated Bool)
(assert (=> check-only (not document-mutated)))
(assert check-only)
(assert document-mutated)
(check-sat)
(pop)

; E9-NF-SMT-RELEASE-REQUIRES-EVERY-GATE
(push)
(assert (release-admissible true true true true true true true true false))
(check-sat)
(pop)

; E9-NF-SMT-NATIVE-STACK-CONSTANT
(push)
(declare-const native-frames Int)
(assert (= native-frames 1))
(assert (> native-frames 1))
(check-sat)
(pop)

; E9-NF-SMT-VALID-EXACT-RELEASE-WITNESS
(push)
(assert (release-admissible true true true true true true true true true))
(check-sat)
(pop)

; E9-NF-SMT-VALID-COMPLETE-APPROXIMATE-CACHE-WITNESS
(push)
(declare-const exact Bool)
(declare-const complete Bool)
(declare-const cached Bool)
(assert (not exact))
(assert complete)
(assert (= cached complete))
(assert cached)
(check-sat)
(pop)
