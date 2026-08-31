; Decidable boundary checks for the E6 domain-integration contract.
; Each push/pop block has one expected result in the companion .expected file.

(set-logic ALL)

; Exact fuzzy publication requires every independent premise.
(define-fun publish-exact
    ((same-index Bool)
     (generator-complete Bool)
     (confirmer-sound Bool)
     (confirmer-complete Bool)
     (precision-exact Bool)
     (coverage-complete Bool)) Bool
  (and same-index generator-complete confirmer-sound confirmer-complete
       precision-exact coverage-complete))

; E6-SMT-STALE — a stale snapshot cannot publish exact, even if all other
; evidence exists.
(push)
(assert (publish-exact false true true true true true))
(check-sat)
(pop)

; E6-SMT-INCOMPLETE — an incomplete candidate feed cannot publish exact.
(push)
(assert (publish-exact true false true true true false))
(check-sat)
(pop)

; E6-SMT-CANDIDATE — candidate membership alone has a concrete false-positive
; model.
(push)
(declare-const candidate-member Bool)
(declare-const reference-member Bool)
(assert candidate-member)
(assert (not reference-member))
(check-sat)
(pop)

; E6-SMT-CERTIFICATE — an exact certificate makes candidate-and-confirmation
; equivalent to the reference denotation in this finite Boolean abstraction.
(push)
(declare-const candidate Bool)
(declare-const confirmed Bool)
(declare-const reference Bool)
(assert (=> reference candidate))
(assert (= confirmed reference))
(assert (distinct (and candidate confirmed) reference))
(check-sat)
(pop)

; E6-SMT-TAPES — the typed H/C/L/G chain has exactly the required middle
; domains.
(push)
(declare-const hmm-output Int)
(declare-const context-input Int)
(declare-const context-output Int)
(declare-const lexicon-input Int)
(declare-const lexicon-output Int)
(declare-const grammar-input Int)
(assert (= hmm-output context-input))
(assert (= context-output lexicon-input))
(assert (= lexicon-output grammar-input))
(assert (or (distinct hmm-output context-input)
            (distinct context-output lexicon-input)
            (distinct lexicon-output grammar-input)))
(check-sat)
(pop)

; E6-SMT-TAGS — matching numeric encodings do not establish matching tape
; domains.
(push)
(declare-const phone-code Int)
(declare-const word-code Int)
(assert (= phone-code word-code))
(assert (distinct 1 2)) ; type tags remain different
(check-sat)
(pop)

; E6-SMT-WEIGHTS — reassociation uses the same ordered component weights and
; semiring product.
(push)
(declare-fun times (Int Int) Int)
(declare-const h Int)
(declare-const c Int)
(declare-const l Int)
(declare-const g Int)
(assert (forall ((a Int) (b Int) (d Int))
  (= (times (times a b) d) (times a (times b d)))))
(assert (distinct
  (times (times (times h c) l) g)
  (times h (times c (times l g)))))
(check-sat)
(pop)
