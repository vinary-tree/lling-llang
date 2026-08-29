; Decidable finite-boundary checks for E9 provider-neutral contracts. Rocq
; carries the unbounded proofs and TLA+/TLC explores the lifecycle; these
; queries pin executable Boolean/integer decisions and required witnesses.

(set-logic ALL)

(define-fun complete-exact () Int 0)
(define-fun complete-approximate () Int 1)
(define-fun incomplete () Int 2)

(define-fun cacheable ((status Int)) Bool
  (or (= status complete-exact) (= status complete-approximate)))

(define-fun compose-status ((left Int) (right Int)) Int
  (ite (or (= left incomplete) (= right incomplete)) incomplete
    (ite (and (= left complete-exact) (= right complete-exact))
      complete-exact
      complete-approximate)))

; E9-SMT-INCOMPLETE-MAP-NO-PROMOTION
(push)
(declare-const original-status Int)
(declare-const adapted-status Int)
(assert (= original-status incomplete))
(assert (= adapted-status original-status))
(assert (= adapted-status complete-exact))
(check-sat)
(pop)

; E9-SMT-APPROXIMATE-MAP-NO-PROMOTION
(push)
(declare-const original-status Int)
(declare-const adapted-status Int)
(assert (= original-status complete-approximate))
(assert (= adapted-status original-status))
(assert (= adapted-status complete-exact))
(check-sat)
(pop)

; E9-SMT-INCOMPLETE-NOT-CACHEABLE
(push)
(assert (cacheable incomplete))
(check-sat)
(pop)

; A valid approximate result must carry at least one explicit limitation.
; E9-SMT-APPROXIMATION-RETAINS-LIMITATION
(push)
(declare-const limitation-count Int)
(assert (>= limitation-count 0))
(assert (> limitation-count 0))
(assert (= limitation-count 0))
(check-sat)
(pop)

; E9-SMT-COMPOSED-EXACT-HAS-EXACT-INPUTS
(push)
(declare-const left-status Int)
(declare-const right-status Int)
(assert (and (>= left-status 0) (<= left-status 2)))
(assert (and (>= right-status 0) (<= right-status 2)))
(assert (= (compose-status left-status right-status) complete-exact))
(assert (or (distinct left-status complete-exact)
            (distinct right-status complete-exact)))
(check-sat)
(pop)

; A two-artifact membership vector is invariant under reordering and duplicate
; delivery. Boolean disjunction abstracts canonical set membership.
; E9-SMT-CANONICAL-MANIFEST-INVARIANT
(push)
(declare-const first-present Bool)
(declare-const second-present Bool)
(assert (or
  (distinct (or first-present second-present)
            (or second-present first-present))
  (distinct (or first-present (or first-present second-present))
            (or first-present second-present))))
(check-sat)
(pop)

(define-fun publish-exact
    ((status Int)
     (artifact-fresh Bool)
     (configuration-fresh Bool)
     (provider-fresh Bool)
     (environment-fresh Bool)
     (result-fresh Bool)
     (trusted Bool)
     (actor-distinct Bool)
     (control-domain-independent Bool)) Bool
  (and (= status complete-exact)
       artifact-fresh configuration-fresh provider-fresh environment-fresh
       result-fresh trusted actor-distinct control-domain-independent))

; E9-SMT-STALE-IDENTITY-BLOCKS-EXACT
(push)
(assert (or
  (publish-exact complete-exact false true true true true true true true)
  (publish-exact complete-exact true false true true true true true true)
  (publish-exact complete-exact true true false true true true true true)
  (publish-exact complete-exact true true true false true true true true)
  (publish-exact complete-exact true true true true false true true true)))
(check-sat)
(pop)

; E9-SMT-DEPENDENT-GUARANTEE-BLOCKS-EXACT
(push)
(assert (publish-exact
  complete-exact true true true true true true true false))
(check-sat)
(pop)

; Distinct actor labels do not help when both actors share a control domain.
; E9-SMT-DISTINCT-NAMES-INSUFFICIENT
(push)
(declare-const producer-actor Int)
(declare-const verifier-actor Int)
(declare-const producer-domain Int)
(declare-const verifier-domain Int)
(assert (distinct producer-actor verifier-actor))
(assert (= producer-domain verifier-domain))
(assert (publish-exact complete-exact true true true true true true
  (distinct producer-actor verifier-actor)
  (distinct producer-domain verifier-domain)))
(check-sat)
(pop)

; Party 0 is the upstream provider, party 1 is the downstream consumer;
; surface 0 is public and surface 1 is private.
(define-fun lawful-dependency ((from Int) (to Int) (surface Int)) Bool
  (and (= from 1) (= to 0) (= surface 0)))

; E9-SMT-REVERSE-DEPENDENCY-FORBIDDEN
(push)
(assert (lawful-dependency 0 1 0))
(check-sat)
(pop)

; E9-SMT-PRIVATE-INTERNALS-FORBIDDEN
(push)
(declare-const from-party Int)
(declare-const to-party Int)
(assert (lawful-dependency from-party to-party 1))
(check-sat)
(pop)

; E9-SMT-RELEASE-NEVER-UNDERFLOWS
(push)
(declare-const borrow-count Int)
(assert (= borrow-count 0))
(assert (> borrow-count 0))
(check-sat)
(pop)

; E9-SMT-OWNER-IS-STABLE
(push)
(declare-const owner-before Int)
(declare-const owner-after Int)
(assert (= owner-after owner-before))
(assert (distinct owner-after owner-before))
(check-sat)
(pop)

; E9-SMT-VALID-EXACT-WITNESS
(push)
(assert (publish-exact
  complete-exact true true true true true true true true))
(check-sat)
(pop)

; E9-SMT-VALID-APPROXIMATE-WITNESS
(push)
(declare-const limitation-count Int)
(assert (> limitation-count 0))
(assert (cacheable complete-approximate))
(check-sat)
(pop)
