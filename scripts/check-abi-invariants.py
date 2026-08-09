#!/usr/bin/env python3
"""Validate the lling-llang ABI invariant registry (proofs/doc/abi-invariants.tsv).

The registry is the traceability spine of the scalar-WFST ABI verification
program: every invariant row names its formal home (spec_path/spec_name), its
strength on the proof ladder, its mirroring executable test (test_path/
test_name, or "-" for a formal-only algebra row proved in Rocq/TLA+), and the
gates that run them. This checker enforces that the registry never drifts from
the artifacts it points at:

  1. column shape and unique, well-formed invariant ids;
  2. strength values drawn from the documented ladder;
  3. every spec_path exists and contains its spec_name verbatim (Coq theorem /
     TLA+ property name, or SMT echo label);
  4. every test-backed row (test_path != "-") names a real test function, and a
     formal-only row (test_path == "-") is a Rocq/TLA+/SMT proof, never
     test-pinned;
  5. test-pinned rows justify themselves in the law column;
  6. hook <-> registry closure: every VT-*/LLING-* id referenced by an
     INVARIANT-HOOK comment under tests/ resolves to a registry row, and every
     TEST-BACKED registry row (test_path != "-") is hooked by at least one test
     (formal-only rows are exempt -- their evidence is the proof itself).

Adapted from the sibling liblevenshtein-rust scripts/check-abi-invariants.py;
the one intentional difference is rule 6's formal-only exemption, since several
lling-llang invariants (the weight-domain semirings, the status algebra) are
pure Rocq/TLA+ results with no ABI-boundary test to mirror.

Stdlib-only; exit 1 on any failure. Invoked by proofs/verify.sh and CI.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REGISTRY = ROOT / "proofs" / "doc" / "abi-invariants.tsv"

COLUMNS = [
    "id",
    "layer",
    "law",
    "spec_kind",
    "spec_path",
    "spec_name",
    "strength",
    "test_path",
    "test_name",
    "gate",
]
STRENGTHS = {
    "rocq-unbounded",
    "tlaps-unbounded",
    "apalache-inductive",
    "verus",
    "smt-dual",
    "tlc-bounded",
    "test-pinned",
}
PROOF_STRENGTHS = STRENGTHS - {"test-pinned"}
SPEC_KINDS = {"tla", "coq", "smt", "verus", "test"}
ID_PATTERN = re.compile(r"^[A-Z]+(?:-[A-Z0-9]+)*-\d+$")
HOOK_PATTERN = re.compile(r"INVARIANT-HOOK:\s*([A-Z]+(?:-[A-Z0-9]+)*-\d+(?:\.\.\d+)?)")


class Failures:
    def __init__(self) -> None:
        self.messages: list[str] = []

    def add(self, message: str) -> None:
        self.messages.append(message)


def parse_rows(failures: Failures) -> list[dict[str, str]]:
    if not REGISTRY.is_file():
        failures.add(f"registry missing: {REGISTRY.relative_to(ROOT)}")
        return []
    rows: list[dict[str, str]] = []
    for line_number, line in enumerate(
        REGISTRY.read_text(encoding="utf-8").splitlines(), start=1
    ):
        if not line.strip() or line.startswith("#"):
            continue
        cells = line.split("\t")
        if len(cells) != len(COLUMNS):
            failures.add(
                f"line {line_number}: {len(cells)} columns, expected {len(COLUMNS)}"
            )
            continue
        rows.append(dict(zip(COLUMNS, cells)))
    return rows


def expand_hook(hook: str) -> list[str]:
    """`LLING-BRIDGE-1..4` expands to LLING-BRIDGE-1 ... 4; plain ids pass."""
    if ".." not in hook:
        return [hook]
    prefix, _, tail = hook.rpartition("-")
    low, _, high = tail.partition("..")
    return [f"{prefix}-{index}" for index in range(int(low), int(high) + 1)]


def main() -> int:
    failures = Failures()
    rows = parse_rows(failures)

    seen_ids: set[str] = set()
    test_backed_ids: set[str] = set()
    for row in rows:
        row_id = row["id"]
        if not ID_PATTERN.match(row_id):
            failures.add(f"{row_id}: malformed invariant id")
        if row_id in seen_ids:
            failures.add(f"{row_id}: duplicate id")
        seen_ids.add(row_id)

        if row["strength"] not in STRENGTHS:
            failures.add(f"{row_id}: unknown strength {row['strength']!r}")
        if row["spec_kind"] not in SPEC_KINDS:
            failures.add(f"{row_id}: unknown spec_kind {row['spec_kind']!r}")
        if row["strength"] == "test-pinned":
            if row["spec_kind"] != "test":
                failures.add(f"{row_id}: test-pinned rows must have spec_kind=test")
            if "exhaustively pinned" not in row["law"]:
                failures.add(
                    f"{row_id}: test-pinned rows must justify themselves in the "
                    "law column ('exhaustively pinned ...')"
                )
        if not row["gate"].strip():
            failures.add(f"{row_id}: empty gate column")

        spec_path = ROOT / row["spec_path"]
        if not spec_path.is_file():
            failures.add(f"{row_id}: spec_path missing: {row['spec_path']}")
        else:
            spec_text = spec_path.read_text(encoding="utf-8", errors="replace")
            needle = row["spec_name"]
            if row["spec_kind"] == "smt" and "[" in needle:
                needle = needle[needle.index("[") : needle.index("]") + 1]
            if needle.split(" ")[0] not in spec_text:
                failures.add(
                    f"{row_id}: spec_name {row['spec_name']!r} not found in "
                    f"{row['spec_path']}"
                )

        if row["test_path"] == "-":
            # Formal-only row: a pure Rocq/TLA+/SMT result with no ABI test.
            if row["strength"] not in PROOF_STRENGTHS:
                failures.add(
                    f"{row_id}: formal-only rows (test_path '-') must carry a "
                    "proof strength, not test-pinned"
                )
            if row["test_name"] != "-":
                failures.add(
                    f"{row_id}: formal-only rows must set test_name to '-'"
                )
        else:
            test_backed_ids.add(row_id)
            test_path = ROOT / row["test_path"]
            if not test_path.is_file():
                failures.add(f"{row_id}: test_path missing: {row['test_path']}")
            else:
                test_text = test_path.read_text(encoding="utf-8", errors="replace")
                if not re.search(rf"fn {re.escape(row['test_name'])}\b", test_text):
                    failures.add(
                        f"{row_id}: test fn {row['test_name']!r} not found in "
                        f"{row['test_path']}"
                    )

    # Hooks <-> registry closure.
    hooked: set[str] = set()
    hook_sources: dict[str, list[str]] = {}
    tests_dir = ROOT / "tests"
    if tests_dir.is_dir():
        for path in sorted(tests_dir.rglob("*.rs")):
            text = path.read_text(encoding="utf-8", errors="replace")
            for match in HOOK_PATTERN.finditer(text):
                for invariant_id in expand_hook(match.group(1)):
                    hooked.add(invariant_id)
                    hook_sources.setdefault(invariant_id, []).append(
                        str(path.relative_to(ROOT))
                    )
    for invariant_id in sorted(hooked - seen_ids):
        failures.add(
            f"hook references unregistered invariant {invariant_id} "
            f"(in {', '.join(hook_sources[invariant_id])})"
        )
    # Every test-backed row must be reachable from a hook (formal-only exempt).
    for invariant_id in sorted(test_backed_ids - hooked):
        failures.add(
            f"test-backed invariant {invariant_id} has no INVARIANT-HOOK"
        )

    if failures.messages:
        print(f"check-abi-invariants: {len(failures.messages)} failure(s)")
        for message in failures.messages:
            print(f"  FAIL {message}")
        return 1
    print(
        f"check-abi-invariants: {len(rows)} invariants verified "
        f"(specs, tests, hooks, and ladder all consistent)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
