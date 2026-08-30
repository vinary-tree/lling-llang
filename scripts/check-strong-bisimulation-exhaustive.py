#!/usr/bin/env python3
"""Exhaust the finite strong-bisimulation contract before production changes."""

from __future__ import annotations

import itertools
import sys
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from pathlib import Path

sys.dont_write_bytecode = True

Edge = tuple[int, int, int]
Relation = tuple[bool, ...]
Formula = tuple[str, object]


class ContractViolation(AssertionError):
    """A deliberately checked executable-contract violation."""


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def validate_dense_lts(
    state_count: int, action_count: int, edges: Sequence[Edge]
) -> None:
    if state_count <= 0:
        raise ValueError("state count must be positive")
    if action_count <= 0:
        raise ValueError("action count must be positive")
    for source, label, target in edges:
        if not 0 <= source < state_count:
            raise ValueError("source endpoint is outside the dense state domain")
        if not 0 <= label < action_count:
            raise ValueError("label is outside the dense action domain")
        if not 0 <= target < state_count:
            raise ValueError("target endpoint is outside the dense state domain")


def adjacency(
    state_count: int, edges: Sequence[Edge]
) -> tuple[tuple[tuple[int, int], ...], ...]:
    outgoing: list[list[tuple[int, int]]] = [[] for _ in range(state_count)]
    for source, label, target in edges:
        outgoing[source].append((label, target))
    return tuple(tuple(sorted(set(row))) for row in outgoing)


def transfers(
    relation: Relation,
    state_count: int,
    outgoing: Sequence[Sequence[tuple[int, int]]],
    left: int,
    right: int,
) -> bool:
    return all(
        any(
            right_label == label and relation[target * state_count + right_target]
            for right_label, right_target in outgoing[right]
        )
        for label, target in outgoing[left]
    )


def refine_relation(
    relation: Relation,
    state_count: int,
    outgoing: Sequence[Sequence[tuple[int, int]]],
) -> Relation:
    return tuple(
        relation[left * state_count + right]
        and transfers(relation, state_count, outgoing, left, right)
        and transfers(relation, state_count, outgoing, right, left)
        for left in range(state_count)
        for right in range(state_count)
    )


def relational_fixed_point(
    state_count: int,
    edges: Sequence[Edge],
    colors: Sequence[int],
) -> tuple[Relation, tuple[tuple[Relation, Relation], ...]]:
    outgoing = adjacency(state_count, edges)
    relation = tuple(
        colors[left] == colors[right]
        for left in range(state_count)
        for right in range(state_count)
    )
    trace: list[tuple[Relation, Relation]] = []
    while True:
        refined = refine_relation(relation, state_count, outgoing)
        if refined == relation:
            return relation, tuple(trace)
        trace.append((relation, refined))
        relation = refined


def canonicalize(keys: Sequence[object]) -> tuple[int, ...]:
    ids: dict[object, int] = {}
    blocks: list[int] = []
    for key in keys:
        block = ids.setdefault(key, len(ids))
        blocks.append(block)
    return tuple(blocks)


def relation_from_blocks(blocks: Sequence[int]) -> Relation:
    return tuple(left == right for left in blocks for right in blocks)


class FormulaArena:
    """Hash-consed modal-formula DAG whose evaluator uses no recursion."""

    def __init__(self) -> None:
        self.nodes: list[Formula] = []
        self.ids: dict[Formula, int] = {}

    def intern(self, node: Formula) -> int:
        existing = self.ids.get(node)
        if existing is not None:
            return existing
        identifier = len(self.nodes)
        self.nodes.append(node)
        self.ids[node] = identifier
        return identifier

    def color(self, value: int) -> int:
        return self.intern(("color", value))

    def diamond(self, label: int, child: int) -> int:
        return self.intern(("diamond", (label, child)))

    def negate(self, child: int) -> int:
        return self.intern(("not", child))

    def conjunction(self, children: Iterable[int]) -> int:
        unique = tuple(sorted(set(children)))
        if len(unique) == 1:
            return unique[0]
        return self.intern(("and", unique))

    def evaluate_all(
        self,
        state_count: int,
        outgoing: Sequence[Sequence[tuple[int, int]]],
        colors: Sequence[int],
    ) -> tuple[tuple[bool, ...], ...]:
        values: list[tuple[bool, ...]] = []
        for node, payload in self.nodes:
            if node == "color":
                row = tuple(color == payload for color in colors)
            elif node == "diamond":
                label, child = payload
                row = tuple(
                    any(
                        edge_label == label and values[child][target]
                        for edge_label, target in outgoing[state]
                    )
                    for state in range(state_count)
                )
            elif node == "not":
                row = tuple(not value for value in values[payload])
            elif node == "and":
                row = tuple(
                    all(values[child][state] for child in payload)
                    for state in range(state_count)
                )
            else:
                raise AssertionError(f"unknown formula node: {node}")
            values.append(row)
        return tuple(values)


@dataclass(frozen=True)
class PartitionCertificate:
    before: tuple[int, ...]
    after: tuple[int, ...]


@dataclass(frozen=True)
class PartitionResult:
    blocks: tuple[int, ...]
    certificate: tuple[PartitionCertificate, ...]
    block_formulas: tuple[int, ...]
    arena: FormulaArena


def partition_refinement(
    state_count: int,
    action_count: int,
    edges: Sequence[Edge],
    colors: Sequence[int],
) -> PartitionResult:
    validate_dense_lts(state_count, action_count, edges)
    if len(colors) != state_count:
        raise ValueError("initial coloring length differs from state count")
    outgoing = adjacency(state_count, edges)
    blocks = canonicalize(colors)
    arena = FormulaArena()
    formula_by_block = tuple(
        arena.color(colors[blocks.index(block)]) for block in range(max(blocks) + 1)
    )
    certificate: list[PartitionCertificate] = []

    while True:
        signatures = tuple(
            (
                blocks[state],
                tuple(
                    sorted(
                        {(label, blocks[target]) for label, target in outgoing[state]}
                    )
                ),
            )
            for state in range(state_count)
        )
        refined = canonicalize(signatures)
        if relation_from_blocks(refined) == relation_from_blocks(blocks):
            return PartitionResult(
                blocks=refined,
                certificate=tuple(certificate),
                block_formulas=formula_by_block,
                arena=arena,
            )

        old_block_count = max(blocks) + 1
        old_formulas = formula_by_block
        new_formulas: list[int] = []
        for new_block in range(max(refined) + 1):
            representative = refined.index(new_block)
            parent = blocks[representative]
            reached = set(signatures[representative][1])
            conjuncts = [old_formulas[parent]]
            for label in range(action_count):
                for target_block in range(old_block_count):
                    diamond = arena.diamond(label, old_formulas[target_block])
                    conjuncts.append(
                        diamond
                        if (label, target_block) in reached
                        else arena.negate(diamond)
                    )
            new_formulas.append(arena.conjunction(conjuncts))

        certificate.append(PartitionCertificate(blocks, refined))
        blocks = refined
        formula_by_block = tuple(new_formulas)


def replay_certificate(
    state_count: int,
    edges: Sequence[Edge],
    colors: Sequence[int],
    certificate: Sequence[PartitionCertificate],
) -> tuple[int, ...]:
    outgoing = adjacency(state_count, edges)
    blocks = canonicalize(colors)
    for entry in certificate:
        if entry.before != blocks:
            raise ContractViolation(
                "certificate chain does not start at the current partition"
            )
        signatures = tuple(
            (
                blocks[state],
                tuple(
                    sorted(
                        {(label, blocks[target]) for label, target in outgoing[state]}
                    )
                ),
            )
            for state in range(state_count)
        )
        expected = canonicalize(signatures)
        if entry.after != expected:
            raise ContractViolation(
                "certificate refinement is not the exact safe splitter result"
            )
        if len(set(entry.after)) <= len(set(entry.before)):
            raise ContractViolation("certificate refinement is not strict")
        blocks = entry.after
    return blocks


def assert_canonical(blocks: Sequence[int]) -> None:
    seen: dict[int, int] = {}
    next_id = 0
    for block in blocks:
        if block not in seen:
            seen[block] = next_id
            next_id += 1
        if seen[block] != block:
            raise ContractViolation(
                "partition block identifiers are not canonical by first state"
            )


def check_case(
    state_count: int,
    action_count: int,
    edges: tuple[Edge, ...],
    colors: tuple[int, ...],
) -> None:
    oracle, relation_trace = relational_fixed_point(state_count, edges, colors)
    candidate = partition_refinement(state_count, action_count, edges, colors)
    candidate_relation = relation_from_blocks(candidate.blocks)
    if candidate_relation != oracle:
        fail(
            "partition refinement differs from the independent relational "
            f"fixed point: states={state_count} edges={edges} colors={colors}"
        )
    if refine_relation(oracle, state_count, adjacency(state_count, edges)) != oracle:
        fail("independent oracle did not reach a fixed point")
    if any(
        before == after
        or any(after[index] and not before[index] for index in range(len(before)))
        for before, after in relation_trace
    ):
        fail("relational oracle trace is not strictly descending")
    assert_canonical(candidate.blocks)

    replayed = replay_certificate(state_count, edges, colors, candidate.certificate)
    if replayed != candidate.blocks:
        fail("certificate replay did not reconstruct the canonical partition")

    values = candidate.arena.evaluate_all(
        state_count, adjacency(state_count, edges), colors
    )
    for left in range(state_count):
        formula = candidate.block_formulas[candidate.blocks[left]]
        for right in range(state_count):
            related = oracle[left * state_count + right]
            if related and not values[formula][right]:
                fail("characteristic formula split an oracle-related pair")
            if not related and (not values[formula][left] or values[formula][right]):
                fail("modal DAG did not distinguish a separated pair")

    reversed_result = partition_refinement(
        state_count, action_count, tuple(reversed(edges)), colors
    )
    if reversed_result.blocks != candidate.blocks:
        fail("transition permutation changed canonical output")
    duplicated_result = partition_refinement(
        state_count, action_count, edges + edges, colors
    )
    if duplicated_result.blocks != candidate.blocks:
        fail("duplicate transitions changed canonical output")
    relabeled_edges = tuple(
        (source, 2 * label + 1, target) for source, label, target in edges
    )
    relabeled_result = partition_refinement(
        state_count,
        2 * action_count + 1,
        relabeled_edges,
        colors,
    )
    if relabeled_result.blocks != candidate.blocks:
        fail("injective label relabeling changed canonical output")


def endpoint_controls() -> None:
    malformed = (
        ("source", 2, 1, ((2, 0, 0),)),
        ("target", 2, 1, ((0, 0, 2),)),
        ("label", 2, 1, ((0, 1, 0),)),
    )
    for name, state_count, action_count, edges in malformed:
        try:
            validate_dense_lts(state_count, action_count, edges)
        except ValueError:
            continue
        fail(f"malformed {name} was accepted")


def negative_controls() -> None:
    forged = (
        PartitionCertificate(
            before=(0, 0),
            after=(0, 1),
        ),
    )
    try:
        replay_certificate(2, (), (0, 0), forged)
    except ContractViolation:
        pass
    else:
        fail("forged certificate was accepted")

    try:
        assert_canonical((1, 1))
    except ContractViolation:
        pass
    else:
        fail("noncanonical block identifiers were accepted")

    arena = FormulaArena()
    atom = arena.color(0)
    negated = arena.negate(atom)
    values = arena.evaluate_all(2, ((), ()), (0, 1))
    if values[negated] != (False, True):
        fail("modal negation control was not semantically exact")


def exhaustive_cases() -> int:
    checked = 0
    for state_count, action_count in ((1, 1), (2, 2), (3, 1)):
        candidates = tuple(
            itertools.product(
                range(state_count),
                range(action_count),
                range(state_count),
            )
        )
        for edge_mask in range(1 << len(candidates)):
            edges = tuple(
                edge
                for index, edge in enumerate(candidates)
                if edge_mask & (1 << index)
            )
            for colors in itertools.product(range(2), repeat=state_count):
                check_case(
                    state_count,
                    action_count,
                    edges,
                    colors,
                )
                checked += 1
    return checked


def source_audit() -> None:
    source = Path(__file__).read_text(encoding="utf-8")
    forbidden = (
        "TO" + "DO",
        "FIX" + "ME",
        "pass" + "  #",
        "Recursion" + "Error",
    )
    for token in forbidden:
        if token in source:
            fail(f"executable oracle contains forbidden token {token!r}")


def main() -> None:
    source_audit()
    endpoint_controls()
    negative_controls()
    checked = exhaustive_cases()
    print(
        "Strong-bisimulation executable oracle passed "
        f"{checked} exhaustive LTS/color cases with stack-safe modal witnesses."
    )


if __name__ == "__main__":
    main()
