; Decidable countermodels for the categorical optimizer and ABI contracts.
; Each push/pop block has one expected result in vco-e4-contracts.expected.

(set-logic ALL)

(declare-datatypes () ((Precision Exact Approximate)))
(declare-datatypes () ((Completeness Complete Incomplete)))

(define-fun precision-meet ((left Precision) (right Precision)) Precision
  (ite (and (= left Exact) (= right Exact)) Exact Approximate))

(define-fun completeness-meet
    ((left Completeness) (right Completeness)) Completeness
  (ite (and (= left Complete) (= right Complete)) Complete Incomplete))

; An approximate input cannot yield an exact composite claim.
(push)
(declare-const precision-right Precision)
(assert (= (precision-meet Approximate precision-right) Exact))
(check-sat)
(pop)

; An incomplete input cannot yield a complete composite claim.
(push)
(declare-const completeness-left Completeness)
(assert (= (completeness-meet completeness-left Incomplete) Complete))
(check-sat)
(pop)

; Erasing the output-tape type admits a concrete false-positive match.
(push)
(declare-const left-input Int)
(declare-const left-output Int)
(declare-const right-input Int)
(assert (= left-input right-input))
(assert (distinct left-output right-input))
(check-sat)
(pop)

; A valid release requires positive ownership, so release-at-zero is impossible.
(push)
(declare-const retain-count Int)
(assert (= retain-count 0))
(assert (> retain-count 0))
(check-sat)
(pop)

; A successful provenance commit cannot use a noncanonical sequence number.
(push)
(declare-const expected-sequence Int)
(declare-const actual-sequence Int)
(assert (distinct expected-sequence actual-sequence))
(assert (= expected-sequence actual-sequence))
(check-sat)
(pop)

; Terminal cancellation and publication are mutually exclusive outcomes.
(push)
(declare-const cancelled Bool)
(declare-const published Bool)
(assert cancelled)
(assert published)
(assert (=> cancelled (not published)))
(check-sat)
(pop)
