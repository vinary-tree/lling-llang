; Decidable duals and countermodels for the additive typed ABI v2 contract.
(set-logic ALL)

(echo "[LLING-ABI2-HDR-1]")
; A header cannot be both valid and carry nonzero reserved data.
(push)
(declare-const reserved Int)
(declare-const header-valid Bool)
(assert header-valid)
(assert (distinct reserved 0))
(assert (=> header-valid (= reserved 0)))
(check-sat)
(pop)

(echo "[LLING-ABI2-RAW-1]")
; A raw precision discriminant outside 1..3 is not admitted.
(push)
(declare-const raw-precision Int)
(assert (> raw-precision 3))
(assert (and (<= 1 raw-precision) (<= raw-precision 3)))
(check-sat)
(pop)

(echo "[LLING-ABI2-AUTH-1]")
; An authoritative exact claim requires verified evidence.
(push)
(declare-const authoritative Bool)
(declare-const verified Bool)
(assert authoritative)
(assert (not verified))
(assert (=> authoritative verified))
(check-sat)
(pop)

(echo "[LLING-ABI2-TERM-1]")
; Cancellation and publication are mutually exclusive.
(push)
(declare-const cancelled Bool)
(declare-const published-after-cancel Bool)
(assert cancelled)
(assert published-after-cancel)
(assert (=> cancelled (not published-after-cancel)))
(check-sat)
(pop)

(echo "[LLING-ABI2-TERM-2]")
; Budget exhaustion cannot report complete.
(push)
(declare-const budget-exhausted Bool)
(declare-const complete-after-budget Bool)
(assert budget-exhausted)
(assert complete-after-budget)
(assert (=> budget-exhausted (not complete-after-budget)))
(check-sat)
(pop)

(echo "[LLING-ABI2-AXIS-1]")
; Precision and completeness are independent: exact-but-incomplete is real.
(push)
(declare-const exact Bool)
(declare-const incomplete Bool)
(assert exact)
(assert incomplete)
(check-sat)
(pop)

(echo "[LLING-ABI2-OPAQUE-1]")
; An opaque ABI v1 input cannot authorize typed evidence.
(push)
(declare-const typed-input Bool)
(declare-const typed-evidence Bool)
(assert (not typed-input))
(assert typed-evidence)
(assert (=> typed-evidence typed-input))
(check-sat)
(pop)

(echo "[LLING-ABI2-HDR-2]")
; Additive trailing bytes are accepted when the known prefix is valid.
(push)
(declare-const required-size Int)
(declare-const supplied-size Int)
(assert (= required-size 120))
(assert (> supplied-size required-size))
(assert (>= supplied-size required-size))
(check-sat)
(pop)

