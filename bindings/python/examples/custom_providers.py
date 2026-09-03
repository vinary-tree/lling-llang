"""Build, compose, and consume Python-defined Vinary Tree resources."""

from __future__ import annotations

import math
import struct

import lling_llang as lling


class RewriteBC:
    """A two-state transducer that rewrites one b label to c."""

    def start(self) -> int:
        return 0

    def num_states(self) -> int:
        return 2

    def state(self, state: int) -> lling.ScalarWfstState | None:
        if state == 0:
            return lling.ScalarWfstState(
                None,
                (lling.ScalarWfstArc("b", "c", 1, 0.75),),
            )
        if state == 1:
            return lling.ScalarWfstState(0.125, ())
        return None


class MaxMin:
    """One immutable value in the integer max/min lattice."""

    def __init__(self, value: int) -> None:
        self.value = value

    @staticmethod
    def _other_value(other: lling.LatticeOperand) -> int:
        local = other.python_value()
        if isinstance(local, MaxMin):
            return local.value
        return struct.unpack(">q", other.stable_bytes())[0]

    def join(self, other: lling.LatticeOperand) -> MaxMin:
        return MaxMin(max(self.value, self._other_value(other)))

    def meet(self, other: lling.LatticeOperand) -> MaxMin:
        return MaxMin(min(self.value, self._other_value(other)))

    def equal(self, other: lling.LatticeOperand) -> bool:
        return self.value == self._other_value(other)

    def stable_bytes(self) -> bytes:
        return struct.pack(">q", self.value)

    def diagnostic(self) -> str:
        return f"MaxMin({self.value})"


class Tropical:
    """The tropical semiring over Python floating-point values."""

    def zero(self) -> float:
        return float("inf")

    def one(self) -> float:
        return 0.0

    def plus(self, left: object, right: object) -> float:
        return min(float(left), float(right))

    def times(self, left: object, right: object) -> float:
        return float(left) + float(right)

    def equal(self, left: object, right: object) -> bool:
        return float(left) == float(right)

    def approximately_equal(self, left: object, right: object, epsilon: float) -> bool:
        left_value = float(left)
        right_value = float(right)
        return left_value == right_value or abs(left_value - right_value) <= epsilon

    def natural_order(self, left: object, right: object) -> lling.SemiringOrder:
        if float(left) < float(right):
            return lling.SemiringOrder.BETTER
        if float(left) > float(right):
            return lling.SemiringOrder.WORSE
        return lling.SemiringOrder.EQUAL

    def stable_bytes(self, value: object) -> bytes:
        return struct.pack(">d", float(value))

    def diagnostic(self, value: object | None = None) -> str:
        return "tropical" if value is None else repr(float(value))

    def numerical_value(self, value: object) -> float:
        return float(value)

    def quantize(self, value: object, epsilon: float) -> int:
        return round(float(value) / epsilon)

    def to_probability(self, value: object) -> float:
        return math.exp(-float(value))


def compose_custom_wfst() -> None:
    with lling.WfstBuilder(size_hint=2) as builder:
        source = builder.add_state()
        target = builder.add_state()
        builder.set_start(source).set_final(target)
        builder.add_arc(source, "a", "b", target, 0.5)
        left = builder.build()

    rewrite = RewriteBC()
    with (
        left,
        lling.ScalarWfstResource(lambda: rewrite, lazy=False, acyclic=True) as right,
        lling.compose(left, right) as product,
    ):
        arc = product.arcs(product.start)[0]
        assert arc.input_label == ord("a")
        assert arc.output_label == ord("c")
        assert arc.weight == 1.25


def consume_custom_lattice() -> None:
    options = lling.LatticeOptions(lling.DomainId.ascii("demo.maxmin.v1.."))
    with lling.LatticeResource(MaxMin(2), options) as low:
        low_value = lling.LatticeValue(low)
    with lling.LatticeResource(MaxMin(7), options) as high:
        high_value = lling.LatticeValue(high)

    with low_value, high_value, low_value.join(high_value) as maximum:
        assert struct.unpack(">q", maximum.stable_bytes())[0] == 7
        lling.validate_lattice_laws((low_value, high_value))


def consume_custom_semiring() -> None:
    options = lling.SemiringOptions(
        lling.DomainId.ascii("demo.tropical.v1"),
        properties=(
            lling.SemiringProperty.IDEMPOTENT_PLUS
            | lling.SemiringProperty.ZERO_SUM_FREE
            | lling.SemiringProperty.COMMUTATIVE_TIMES
            | lling.SemiringProperty.TOTALLY_ORDERED
        ),
    )
    with lling.SemiringResource(Tropical(), options) as provider:
        context = lling.SemiringContext(provider)

    with context, context.zero() as zero, context.one() as one:
        with one + zero as best:
            assert best == one
            assert context.numerical_value(best) == 0.0
        context.validate_laws((zero, one))


if __name__ == "__main__":
    compose_custom_wfst()
    consume_custom_lattice()
    consume_custom_semiring()
