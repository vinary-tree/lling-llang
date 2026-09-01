#!/usr/bin/env python3
"""Inject causal faults into the exhaustive strong-bisimulation checker."""

from __future__ import annotations

import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "scripts/check-strong-bisimulation-exhaustive.py"
MUTANT_ROOT = ROOT / "target/formal-verification/mutants/strong-bisimulation"


@dataclass(frozen=True)
class Mutation:
    needle: str
    replacement: str
    expected: str
    replace_count: int = 1


MUTATIONS = {
    "accept-malformed-source": Mutation(
        "if not 0 <= source < state_count:",
        "if not 0 <= source <= state_count:",
        "malformed source was accepted",
    ),
    "accept-malformed-target": Mutation(
        "if not 0 <= target < state_count:",
        "if not 0 <= target <= state_count:",
        "malformed target was accepted",
    ),
    "accept-malformed-label": Mutation(
        "if not 0 <= label < action_count:",
        "if not 0 <= label <= action_count:",
        "malformed label was accepted",
    ),
    "ignore-initial-colors": Mutation(
        "    blocks = canonicalize(colors)\n    arena = FormulaArena()",
        "    blocks = canonicalize([0] * state_count)\n    arena = FormulaArena()",
        "partition refinement differs from the independent relational fixed point",
    ),
    "one-way-transfer": Mutation(
        "        and transfers(relation, state_count, outgoing, right, left)\n",
        "        and True\n",
        "partition refinement differs from the independent relational fixed point",
    ),
    "premature-stability": Mutation(
        "        if relation_from_blocks(refined) == relation_from_blocks(blocks):",
        "        if True or relation_from_blocks(refined) == relation_from_blocks(blocks):",
        "certificate replay did not reconstruct the canonical partition",
    ),
    "duplicate-sensitive-signature": Mutation(
        "{(label, blocks[target]) for label, target in outgoing[state]}",
        "[(label, blocks[target]) for label, target in outgoing[state]]",
        "partition refinement differs from the independent relational fixed point",
        replace_count=2,
    ),
    "unsound-modal-negation": Mutation(
        "row = tuple(not value for value in values[payload])",
        "row = tuple(value for value in values[payload])",
        "modal negation control was not semantically exact",
    ),
    "accept-forged-certificate": Mutation(
        "        if entry.after != expected:",
        "        if False and entry.after != expected:",
        "forged certificate was accepted",
    ),
    "accept-noncanonical-ids": Mutation(
        "        if seen[block] != block:",
        "        if False and seen[block] != block:",
        "noncanonical block identifiers were accepted",
    ),
}


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def inject(source: str, name: str, mutation: Mutation) -> str:
    occurrences = source.count(mutation.needle)
    if occurrences != mutation.replace_count:
        fail(
            f"mutant {name} expected {mutation.replace_count} injection "
            f"site(s), found {occurrences}"
        )
    return source.replace(
        mutation.needle,
        mutation.replacement,
        mutation.replace_count,
    )


def main() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    if MUTANT_ROOT.exists():
        shutil.rmtree(MUTANT_ROOT)
    MUTANT_ROOT.mkdir(parents=True)

    try:
        for name, mutation in MUTATIONS.items():
            path = MUTANT_ROOT / f"{name}.py"
            path.write_text(
                inject(source, name, mutation),
                encoding="utf-8",
            )
            completed = subprocess.run(
                [sys.executable, str(path)],
                cwd=ROOT,
                capture_output=True,
                text=True,
                timeout=30,
                check=False,
            )
            output = completed.stdout + completed.stderr
            if completed.returncode == 0:
                fail(f"mutant {name} survived the exhaustive contract")
            if mutation.expected not in output:
                fail(
                    f"mutant {name} died for an unexpected reason; "
                    f"wanted {mutation.expected!r}, got:\n{output}"
                )
            print(f"killed: {name}: {mutation.expected}")
    finally:
        if MUTANT_ROOT.exists():
            shutil.rmtree(MUTANT_ROOT)

    print(f"Killed all {len(MUTATIONS)} strong-bisimulation mutants.")


if __name__ == "__main__":
    main()
