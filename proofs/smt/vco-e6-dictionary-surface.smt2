; Decidable controls for the split dictionary, lattice, fuzzy, and facade
; surface.  Each named block has one result in the companion .expected file.

(set-logic ALL)

(define-fun same-identity
    ((snapshot-a Int) (query-a Int) (normalization-a Int)
     (edit-a Int) (bound-a Int)
     (snapshot-b Int) (query-b Int) (normalization-b Int)
     (edit-b Int) (bound-b Int)) Bool
  (and (= snapshot-a snapshot-b)
       (= query-a query-b)
       (= normalization-a normalization-b)
       (= edit-a edit-b)
       (= bound-a bound-b)))

; E6-DS-SMT-SNAPSHOT — snapshot identity is mandatory.
(push)
(assert (same-identity 0 0 0 0 1 1 0 0 0 1))
(check-sat)
(pop)

; E6-DS-SMT-QUERY — query identity is mandatory.
(push)
(assert (same-identity 0 0 0 0 1 0 1 0 0 1))
(check-sat)
(pop)

; E6-DS-SMT-NORMALIZATION — normalization identity is mandatory.
(push)
(assert (same-identity 0 0 0 0 1 0 0 1 0 1))
(check-sat)
(pop)

; E6-DS-SMT-EDIT — edit-profile identity is mandatory.
(push)
(assert (same-identity 0 0 0 0 1 0 0 0 1 1))
(check-sat)
(pop)

; E6-DS-SMT-BOUND — distance-bound identity is mandatory.
(push)
(assert (same-identity 0 0 0 0 1 0 0 0 0 2))
(check-sat)
(pop)

; E6-DS-SMT-IDENTIFIER — two-sided correspondence forbids two dense IDs for
; one external key.
(push)
(declare-fun dense-for (Int) Int)
(declare-fun external-for (Int) Int)
(assert (forall ((external Int) (dense Int))
  (=> (= (dense-for external) dense) (= (external-for dense) external))))
(assert (= (dense-for 7) 11))
(assert (= (dense-for 7) 12))
(assert (distinct 11 12))
(check-sat)
(pop)

; E6-DS-SMT-DEPENDENCY — a strict dependency rank cannot contain a cycle.
(push)
(declare-const rank-a Int)
(declare-const rank-b Int)
(declare-const rank-c Int)
(assert (< rank-b rank-a))
(assert (< rank-c rank-b))
(assert (< rank-a rank-c))
(check-sat)
(pop)

; E6-DS-SMT-TROPICAL — arithmetic addition fails meet idempotence.
(push)
(assert (= (+ 1 1) 1))
(check-sat)
(pop)

; E6-DS-SMT-NONFINITE — NaN cannot enter a finite-only lawful wrapper.
(push)
(define-fun admitted-number ((numeric-class Int)) Bool
  (= numeric-class 0))
(assert (admitted-number 3))
(check-sat)
(pop)

; E6-DS-SMT-LEFT-BIASED — structural sequence append has a concrete
; noncommutative witness.
(push)
(assert (distinct
  (seq.++ (seq.unit 1) (seq.unit 2))
  (seq.++ (seq.unit 2) (seq.unit 1))))
(check-sat)
(pop)

; E6-DS-SMT-FACADE — exact delegation forbids an observable difference.
(push)
(declare-fun native-adapter (Int) Int)
(declare-fun facade-adapter (Int) Int)
(assert (forall ((index Int))
  (= (facade-adapter index) (native-adapter index))))
(assert (exists ((index Int))
  (distinct (facade-adapter index) (native-adapter index))))
(check-sat)
(pop)

; E6-DS-SMT-BROKEN-FACADE — a transforming facade has a concrete mismatch.
(push)
(declare-const facade-input Bool)
(assert (distinct facade-input (not facade-input)))
(check-sat)
(pop)

; E6-DS-SMT-FIBRATION — an indexed family without lift evidence cannot claim
; a fibration.
(push)
(declare-const has-cartesian-lifts Bool)
(define-fun may-claim-fibration () Bool has-cartesian-lifts)
(assert (not has-cartesian-lifts))
(assert may-claim-fibration)
(check-sat)
(pop)

(define-fun complete-termination ((reason Int)) Bool (= reason 0))

; E6-DS-SMT-CAP — a cap is not exhaustive completion.
(push)
(assert (complete-termination 1))
(check-sat)
(pop)

; E6-DS-SMT-CANCEL — cancellation is not exhaustive completion.
(push)
(assert (complete-termination 2))
(check-sat)
(pop)

; E6-DS-SMT-FAILURE — provider failure is not exhaustive completion.
(push)
(assert (complete-termination 3))
(check-sat)
(pop)
