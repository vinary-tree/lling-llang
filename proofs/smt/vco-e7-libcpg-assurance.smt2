; Decidable boundary checks for the E7 libcpg integration and assurance
; contract. Each push/pop block has exactly one result in the companion
; transcript. Rocq carries the unbounded proofs; these finite checks exercise
; the adapter decisions at their executable Boolean/integer boundary.

(set-logic ALL)

; A two-bit finite-set join is bitwise union. The induced order is the
; join-semilattice order used by llattice v2.
(define-fun join2 ((left (_ BitVec 2)) (right (_ BitVec 2))) (_ BitVec 2)
  (bvor left right))
(define-fun leq2 ((left (_ BitVec 2)) (right (_ BitVec 2))) Bool
  (= (join2 left right) right))
(define-fun subsumes2 ((container (_ BitVec 2)) (contained (_ BitVec 2))) Bool
  (leq2 contained container))

; E7-SMT-DATAFLOW-JOIN — union is a least upper bound throughout the finite
; two-bit carrier.
(push)
(declare-const left (_ BitVec 2))
(declare-const right (_ BitVec 2))
(declare-const upper (_ BitVec 2))
(assert (leq2 left upper))
(assert (leq2 right upper))
(assert (not (leq2 (join2 left right) upper)))
(check-sat)
(pop)

; E7-SMT-IFDS-ORDER — libcpg IFDS subsumption is exactly the reversal of the
; llattice v2 join order.
(push)
(declare-const container (_ BitVec 2))
(declare-const contained (_ BitVec 2))
(assert (distinct (subsumes2 container contained)
                  (leq2 contained container)))
(check-sat)
(pop)

; E7-SMT-JOIN-ASSIGN — the mutation flag is true exactly when the joined value
; differs from the old left operand.
(push)
(declare-const old (_ BitVec 2))
(declare-const incoming (_ BitVec 2))
(declare-const changed Bool)
(assert (= changed (distinct (join2 old incoming) old)))
(assert (or (and changed (= (join2 old incoming) old))
            (and (not changed) (distinct (join2 old incoming) old))))
(check-sat)
(pop)

; E7-SMT-MERGE-CANONICAL — reordering and duplicating joins cannot alter the
; accumulated finite-set value.
(push)
(declare-const first (_ BitVec 2))
(declare-const second (_ BitVec 2))
(declare-const third (_ BitVec 2))
(assert (or
  (distinct (join2 (join2 first second) third)
            (join2 first (join2 third second)))
  (distinct (join2 first (join2 second second))
            (join2 first second))))
(check-sat)
(pop)

; A finite quotient edge exists exactly for an original cross-fiber edge.
(define-fun quotient-edge
    ((source-component Int) (target-component Int) (original-edge Bool)) Bool
  (and (distinct source-component target-component) original-edge))

; E7-SMT-QUOTIENT-SELF — condensation graphs cannot contain self edges.
(push)
(declare-const component Int)
(assert (quotient-edge component component true))
(check-sat)
(pop)

; E7-SMT-QUOTIENT-EXACT — every quotient edge has a cross-component original
; witness in the finite boundary abstraction.
(push)
(declare-const source-component Int)
(declare-const target-component Int)
(declare-const original-edge Bool)
(assert (quotient-edge source-component target-component original-edge))
(assert (or (= source-component target-component) (not original-edge)))
(check-sat)
(pop)

; E7-SMT-CSR-WORK — the validated import charge is exactly two vertex passes
; plus two edge passes, hence 2(V+E), for arbitrary nonnegative sizes.
(push)
(declare-const vertices Int)
(declare-const edges Int)
(assert (>= vertices 0))
(assert (>= edges 0))
(assert (distinct (+ (* 2 vertices) (* 2 edges))
                  (* 2 (+ vertices edges))))
(check-sat)
(pop)

; Exact publication requires all semantic bindings and assurance predicates.
(define-fun publish-exact
    ((precision-exact Bool)
     (coverage-complete Bool)
     (subject-fresh Bool)
     (snapshot-fresh Bool)
     (configuration-fresh Bool)
     (tool-fresh Bool)
     (environment-fresh Bool)
     (digest-bound Bool)
     (trusted Bool)
     (independent Bool)
     (verifier-distinct Bool)) Bool
  (and precision-exact coverage-complete subject-fresh snapshot-fresh
       configuration-fresh tool-fresh environment-fresh digest-bound trusted
       independent verifier-distinct))

; E7-SMT-EVIDENCE-STALE — changing any evidence-index coordinate blocks exact
; publication. The disjunction asks Z3 for a forbidden stale publication.
(push)
(assert (or
  (publish-exact true true false true true true true true true true true)
  (publish-exact true true true false true true true true true true true)
  (publish-exact true true true true false true true true true true true)
  (publish-exact true true true true true false true true true true true)
  (publish-exact true true true true true true false true true true true)))
(check-sat)
(pop)

; E7-SMT-EVIDENCE-BINDING — a result-digest mismatch or untrusted guarantee
; blocks exact publication.
(push)
(assert (or
  (publish-exact true true true true true true true false true true true)
  (publish-exact true true true true true true true true false true true)))
(check-sat)
(pop)

; E7-SMT-EVIDENCE-INDEPENDENCE — distinct actor names are insufficient when
; the trust policy says that the guarantee depends on the producer.
(push)
(assert (publish-exact true true true true true true true true true false true))
(check-sat)
(pop)

; E7-SMT-EVIDENCE-SELF — self-confirmation cannot establish exactness.
(push)
(assert (publish-exact true true true true true true true true true true false))
(check-sat)
(pop)

; E7-SMT-NO-PROMOTION — approximate precision or incomplete coverage cannot be
; promoted by otherwise valid evidence.
(push)
(assert (or
  (publish-exact false true true true true true true true true true true)
  (publish-exact true false true true true true true true true true true)))
(check-sat)
(pop)

; E7-SMT-VALID-WITNESS — the boundary is satisfiable when every required
; premise is present; this guards against a vacuous always-reject policy.
(push)
(assert (publish-exact true true true true true true true true true true true))
(check-sat)
(pop)
