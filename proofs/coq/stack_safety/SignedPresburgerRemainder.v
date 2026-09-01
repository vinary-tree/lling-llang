(** * Signed two's-complement Presburger remainder transitions

    The remainder construction consumes ordinary low bits by subtracting their
    coefficient sum and dividing the residual by two.  The final bit of a signed
    two's-complement word has negative positional weight, so it is not an
    ordinary low bit: if [r] is the residual immediately before the sign bit and
    [s] is the coefficient sum selected by that bit vector, acceptance is
    exactly [0 <= r + s].

    The first theorem proves preservation of the quotient/remainder invariant
    for an ordinary bit.  The second proves the signed terminal rule equivalent
    to the original linear inequality.  The final counterexample demonstrates
    why the former unsigned final transition is unsound.  No axiom, admission,
    parameter, or proof escape is used.
*)

From Stdlib Require Import Lia.
From Stdlib Require Import Psatz.
From Stdlib Require Import ZArith.
Open Scope Z_scope.

Theorem ordinary_bit_preserves_residual_invariant :
  forall bound lower scale remainder residual bit_sum next parity,
    0 < scale ->
    bound - lower = scale * residual + remainder ->
    0 <= remainder < scale ->
    residual - bit_sum = 2 * next + parity ->
    0 <= parity < 2 ->
    bound - (lower + scale * bit_sum) =
      (2 * scale) * next + (scale * parity + remainder) /\
    0 <= scale * parity + remainder < 2 * scale.
Proof.
  intros bound lower scale remainder residual bit_sum next parity
    Hscale Hinvariant Hremainder Hnext Hparity.
  split; nia.
Qed.

Theorem signed_terminal_transition_is_exact :
  forall bound lower scale remainder residual sign_sum,
    0 < scale ->
    bound - lower = scale * residual + remainder ->
    0 <= remainder < scale ->
    (0 <= residual + sign_sum <->
     lower - scale * sign_sum <= bound).
Proof.
  intros bound lower scale remainder residual sign_sum
    Hscale Hinvariant Hremainder.
  split; intro Hterminal; nia.
Qed.

Example unsigned_final_transition_rejects_valid_minus_one :
  0 <= (-1) + 1 /\
  ~ (0 <= ((-1) - 1) / 2).
Proof.
  split.
  - lia.
  - change (~ (0 <= -1))%Z.
    lia.
Qed.
