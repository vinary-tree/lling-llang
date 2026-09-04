"""End-to-end Python conformance for native and host-defined resources."""

from __future__ import annotations

import struct
import unittest
from concurrent.futures import ThreadPoolExecutor
from contextlib import ExitStack

import lling_llang as lling


class HostWfst:
    """Immutable two-state WFST used to cross Python/native ownership."""

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
    """Finite max/min lattice whose canonical representation is big-endian."""

    def __init__(self, value: int) -> None:
        self.value = value

    @staticmethod
    def _value(other: lling.LatticeOperand) -> int:
        local = other.python_value()
        if isinstance(local, MaxMin):
            return local.value
        return struct.unpack(">q", other.stable_bytes())[0]

    def join(self, other: lling.LatticeOperand) -> MaxMin:
        return MaxMin(max(self.value, self._value(other)))

    def meet(self, other: lling.LatticeOperand) -> MaxMin:
        return MaxMin(min(self.value, self._value(other)))

    def equal(self, other: lling.LatticeOperand) -> bool:
        return self.value == self._value(other)

    def stable_bytes(self) -> bytes:
        return struct.pack(">q", self.value)

    def diagnostic(self) -> str:
        return f"MaxMin({self.value})"


class ProbabilitySemiring:
    """Probability algebra with every optional dynamic capability."""

    @staticmethod
    def _value(value: object) -> float:
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            raise TypeError("probability weights must be real scalars")
        return float(value)

    def zero(self) -> float:
        return 0.0

    def one(self) -> float:
        return 1.0

    def plus(self, left: object, right: object) -> float:
        return self._value(left) + self._value(right)

    def times(self, left: object, right: object) -> float:
        return self._value(left) * self._value(right)

    def equal(self, left: object, right: object) -> bool:
        return struct.pack(">d", self._value(left)) == struct.pack(
            ">d", self._value(right)
        )

    def approximately_equal(self, left: object, right: object, epsilon: float) -> bool:
        return abs(self._value(left) - self._value(right)) <= epsilon

    def natural_order(self, left: object, right: object) -> lling.SemiringOrder:
        difference = self._value(left) - self._value(right)
        if difference < 0:
            return lling.SemiringOrder.BETTER
        if difference > 0:
            return lling.SemiringOrder.WORSE
        return lling.SemiringOrder.EQUAL

    def stable_bytes(self, value: object) -> bytes:
        return struct.pack(">d", self._value(value))

    def diagnostic(self, value: object | None = None) -> str:
        return "probability" if value is None else repr(self._value(value))

    def divide(self, dividend: object, divisor: object) -> float | None:
        denominator = self._value(divisor)
        return None if denominator == 0 else self._value(dividend) / denominator

    def left_divide(self, value: object, divisor: object) -> float | None:
        return self.divide(value, divisor)

    def star(self, value: object) -> float | None:
        scalar = self._value(value)
        return 1 / (1 - scalar) if abs(scalar) < 1 else None

    def numerical_value(self, value: object) -> float:
        return self._value(value)

    def quantize(self, value: object, epsilon: float) -> int:
        return round(self._value(value) / epsilon)

    def to_probability(self, value: object) -> float:
        return self._value(value)


class ApiTests(unittest.TestCase):
    def test_typed_abi_layouts_validation_and_cancellation(self) -> None:
        self.assertEqual(lling.abi_version(), lling.ABI_VERSION)
        self.assertGreaterEqual(lling.api_revision(), lling.API_REVISION)

        descriptor = lling.WfstDescriptor(
            input_tape=lling.Id128(bytes([0x11]) * 16),
            output_tape=lling.Id128(bytes([0x22]) * 16),
            algebra=lling.Id128(bytes([0x33]) * 16),
            snapshot=lling.Id128(bytes([0x44]) * 16),
            context=lling.Digest256(bytes([0x55]) * 32),
            flags=(
                lling.DescriptorFlag.SIGNATURE_KNOWN
                | lling.DescriptorFlag.SNAPSHOT_PRESENT
                | lling.DescriptorFlag.CONTEXT_PRESENT
            ),
        )
        lling.validate_header(
            descriptor.header,
            required_size=120,
            known_flags=int(
                lling.DescriptorFlag.SIGNATURE_KNOWN
                | lling.DescriptorFlag.SNAPSHOT_PRESENT
                | lling.DescriptorFlag.CONTEXT_PRESENT
            ),
        )
        self.assertTrue(lling.typed_evidence_allowed(descriptor))
        self.assertTrue(lling.identity_matches(descriptor, descriptor))

        budget = lling.Budget(max_states=100, max_work=1_000)
        self.assertIs(lling.validate_budget(budget), budget)
        self.assertEqual(
            budget.header.flags,
            lling.BudgetFlag.STATES | lling.BudgetFlag.WORK,
        )
        outcome = lling.Outcome(
            precision=lling.Precision.EXACT,
            completeness=lling.Completeness.COMPLETE,
            applicability=lling.Applicability.APPLICABLE,
            termination=lling.Termination.SUCCEEDED,
            evidence=lling.EvidenceState.VERIFIED,
            states=2,
            arcs=1,
            bytes_used=64,
            work=3,
        )
        self.assertTrue(
            lling.authoritative_exact(
                outcome, resource_present=True, evidence_present=True
            )
        )

        with lling.Cancellation() as cancellation:
            self.assertIsNone(cancellation.reason)
            cancellation.request(lling.CancellationReason.REQUESTED)
            cancellation.request(lling.CancellationReason.DEADLINE)
            self.assertEqual(cancellation.reason, lling.CancellationReason.REQUESTED)

    def test_builder_import_and_host_provider_lazy_composition(self) -> None:
        with lling.WfstBuilder(size_hint=2) as builder:
            first = builder.add_state()
            second = builder.add_state()
            self.assertEqual((first, second), (0, 1))
            builder.set_start(first).set_final(second)
            builder.add_arc(first, "a", "b", second, 0.5)
            left = builder.build()

        with left:
            self.assertEqual(left.start, 0)
            self.assertEqual(len(left), 2)
            self.assertEqual(left.state(1), lling.ScalarWfstState(0.0, ()))
            with lling.import_wfst(left) as imported:
                self.assertEqual(imported.start, 0)
                self.assertEqual(imported.arcs(0)[0].output_label, ord("b"))

            host = lling.ScalarWfstResource(
                lambda: HostWfst(), lazy=False, acyclic=True
            )
            product = lling.compose(left, host)
            host.close()
            with product:
                arc = product.arcs(product.start)[0]
                self.assertEqual(arc.input_label, ord("a"))
                self.assertEqual(arc.output_label, ord("c"))
                self.assertEqual(arc.weight, 1.25)
                final_state = product.state(arc.target_state)
                self.assertIsNotNone(final_state)
                if final_state is None:
                    self.fail("composed target state disappeared after arc traversal")
                self.assertEqual(final_state.final_weight, 0.125)

    def test_failed_build_does_not_consume_builder(self) -> None:
        with lling.WfstBuilder() as builder:
            state = builder.add_state()
            with self.assertRaises(lling.NativeError) as failure:
                builder.build()
            self.assertEqual(failure.exception.status, lling.Status.INVALID_ARGUMENT)
            builder.set_start(state).set_final(state)
            with builder.build() as graph:
                self.assertEqual(graph.start, state)

    def test_host_lattice_is_retained_batched_and_law_checked(self) -> None:
        options = lling.LatticeOptions(lling.DomainId.ascii("test.maxmin.v1.."))
        providers = [
            lling.LatticeResource(MaxMin(value), options) for value in (2, 7, 4)
        ]
        values = [lling.LatticeValue(provider) for provider in providers]
        for provider in providers:
            provider.close()

        with ExitStack() as stack:
            for value in values:
                stack.enter_context(value)
            joined = stack.enter_context(values[0] | values[1])
            met = stack.enter_context(values[0] & values[1])
            joined_many = stack.enter_context(values[0].join_many(values[1:]))
            met_many = stack.enter_context(values[1].meet_many((values[0], values[2])))
            self.assertEqual(struct.unpack(">q", joined.stable_bytes())[0], 7)
            self.assertEqual(struct.unpack(">q", met.stable_bytes())[0], 2)
            self.assertEqual(joined, joined_many)
            self.assertEqual(met, met_many)
            self.assertEqual(joined.diagnostic(), "MaxMin(7)")
            self.assertEqual(joined.domain_id, options.domain_id)
            self.assertTrue(joined.flags & lling.LatticeFlag.BATCH)
            lling.validate_lattice_laws(values)

    def test_host_semiring_optional_capabilities_and_context_identity(self) -> None:
        options = lling.SemiringOptions(
            lling.DomainId.ascii("test.prob.v1...."),
            properties=(
                lling.SemiringProperty.HASHABLE
                | lling.SemiringProperty.ZERO_SUM_FREE
                | lling.SemiringProperty.COMMUTATIVE_TIMES
                | lling.SemiringProperty.NONNEGATIVE
            ),
        )
        provider = lling.SemiringResource(ProbabilitySemiring(), options)
        context = lling.SemiringContext(provider)
        provider.close()

        with ExitStack() as stack:
            stack.enter_context(context)
            zero = stack.enter_context(context.zero())
            one = stack.enter_context(context.one())
            sum_weight = stack.enter_context(zero + one)
            product = stack.enter_context(one * one)
            batch_sum = stack.enter_context(context.plus_many((zero, one)))
            batch_product = stack.enter_context(context.times_many((one, one)))
            quotient = context.divide(product, one)
            self.assertIsNotNone(quotient)
            if quotient is None:
                self.fail("division by one unexpectedly returned no weight")
            stack.enter_context(quotient)
            clone = stack.enter_context(one.clone())

            self.assertEqual(sum_weight, one)
            self.assertEqual(batch_sum, one)
            self.assertEqual(batch_product, one)
            self.assertEqual(context.diagnostic(), "probability")
            self.assertEqual(one.diagnostic(), "1.0")
            self.assertTrue(context.approximately_equal(product, one, 1e-12))
            self.assertEqual(
                context.natural_order(zero, one), lling.SemiringOrder.BETTER
            )
            self.assertEqual(context.numerical_value(product), 1.0)
            self.assertEqual(context.probability(product), 1.0)
            self.assertEqual(context.quantize(product, 0.25), 4)
            self.assertIsNone(context.closure_bound)
            self.assertEqual(struct.unpack(">d", clone.stable_bytes())[0], 1.0)
            self.assertIsNone(context.divide(one, zero))
            self.assertIsNone(context.star(one))
            context.validate_laws((zero, one), epsilon=1e-12)

            with (
                ThreadPoolExecutor(max_workers=1) as executor,
                self.assertRaises(lling.NativeError),
            ):
                executor.submit(lambda: context.properties).result()


if __name__ == "__main__":
    unittest.main()
