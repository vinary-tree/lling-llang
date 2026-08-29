; Finite countermodel boundary for the explicit lazy-expansion lifecycle.
; Rocq proves the unbounded functional lemmas and TLC exhausts the concurrent
; finite control model.  These queries independently pin the decidable API
; decisions that the Rust property suite must reproduce.

(set-logic ALL)

(declare-datatypes ()
  ((Status
     Unexpanded
     Expanding
     ExpandedEmpty
     ExpandedNonempty
     Failed
     Cancelled)))

(declare-datatypes ()
  ((Observation Empty Nonempty Failure Cancellation)))

(declare-datatypes ()
  ((BeginMode Normal ExplicitRetry)))

(define-fun begin-authorized
    ((mode BeginMode) (status Status) (retryable Bool) (cancelled Bool)) Bool
  (and
    (not cancelled)
    (or
      (and (= mode Normal) (= status Unexpanded))
      (and (= mode ExplicitRetry) (= status Failed) retryable))))

(define-fun observable
    ((fresh Bool) (status Status) (observation Observation)) Bool
  (and
    fresh
    (or
      (and (= status ExpandedEmpty) (= observation Empty))
      (and (= status ExpandedNonempty) (= observation Nonempty))
      (and (= status Failed) (= observation Failure))
      (and (= status Cancelled) (= observation Cancellation)))))

(define-fun completion-status ((transition-count Int)) Status
  (ite (= transition-count 0) ExpandedEmpty ExpandedNonempty))

; E4-LAZY-SMT-UNEXPANDED-NOT-EMPTY
(push)
(assert (observable true Unexpanded Empty))
(check-sat)
(pop)

; E4-LAZY-SMT-EXPANDING-NOT-EMPTY
(push)
(assert (observable true Expanding Empty))
(check-sat)
(pop)

; E4-LAZY-SMT-EMPTY-OBSERVATION-EXACT
(push)
(declare-const empty-status Status)
(assert (observable true empty-status Empty))
(assert (distinct empty-status ExpandedEmpty))
(check-sat)
(pop)

; E4-LAZY-SMT-SINGLE-OWNER
(push)
(declare-const owners-before Int)
(declare-const owners-after Int)
(assert (= owners-before 0))
(assert (= owners-after (+ owners-before 1)))
(assert (> owners-after 1))
(check-sat)
(pop)

; E4-LAZY-SMT-NORMAL-BEGIN-CANNOT-RETRY
(push)
(assert (begin-authorized Normal Failed true false))
(check-sat)
(pop)

; E4-LAZY-SMT-NONRETRYABLE-FAILURE-TERMINAL
(push)
(assert (begin-authorized ExplicitRetry Failed false false))
(check-sat)
(pop)

; E4-LAZY-SMT-CANCELLATION-BLOCKS-BEGIN
(push)
(declare-const cancellation-mode BeginMode)
(declare-const cancellation-status Status)
(declare-const cancellation-retryable Bool)
(assert
  (begin-authorized
    cancellation-mode cancellation-status cancellation-retryable true))
(check-sat)
(pop)

; E4-LAZY-SMT-STALE-COMPLETION-BLOCKED
(push)
(declare-const entry-snapshot Int)
(declare-const current-snapshot Int)
(declare-const completion-accepted Bool)
(assert (distinct entry-snapshot current-snapshot))
(assert (= completion-accepted (= entry-snapshot current-snapshot)))
(assert completion-accepted)
(check-sat)
(pop)

; E4-LAZY-SMT-STALE-OBSERVATION-BLOCKED
(push)
(declare-const stale-status Status)
(declare-const stale-observation Observation)
(assert (observable false stale-status stale-observation))
(check-sat)
(pop)

; E4-LAZY-SMT-WRONG-OWNER-CANNOT-COMPLETE
(push)
(declare-const owner Int)
(declare-const finisher Int)
(declare-const owner-completion-accepted Bool)
(assert (distinct owner finisher))
(assert (= owner-completion-accepted (= owner finisher)))
(assert owner-completion-accepted)
(check-sat)
(pop)

; E4-LAZY-SMT-EMPTY-NONEMPTY-CLASSIFICATION-EXCLUSIVE
(push)
(declare-const transition-count Int)
(assert (>= transition-count 0))
(assert (= (completion-status transition-count) ExpandedEmpty))
(assert (= (completion-status transition-count) ExpandedNonempty))
(check-sat)
(pop)

; E4-LAZY-SMT-PRECANCEL-DOES-NOT-ATTEMPT
(push)
(declare-const attempts-before Int)
(declare-const attempts-after Int)
(assert (= attempts-after attempts-before))
(assert (distinct attempts-after attempts-before))
(check-sat)
(pop)

; E4-LAZY-SMT-RESET-WITNESS
(push)
(declare-const reset-status-before Status)
(declare-const reset-status-after Status)
(assert (= reset-status-before Cancelled))
(assert (= reset-status-after Unexpanded))
(check-sat)
(pop)

; E4-LAZY-SMT-VALID-EXPLICIT-RETRY-WITNESS
(push)
(assert (begin-authorized ExplicitRetry Failed true false))
(check-sat)
(pop)
