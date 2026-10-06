#!/usr/bin/env python3
"""Check closure of formal host-provider laws against actual-path properties.

The ABI invariant registry owns each positive model-to-test mapping. This
second ledger requires an explicit negative control for every checked law and
the deliberate parallel-overlap witness. The dictionary-cursor rows point to
the libdictenstein sibling checked out by lling-llang CI.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "proofs/doc/host-provider-invariants.tsv"
ABI_LEDGER = ROOT / "proofs/doc/abi-invariants.tsv"
MODEL = ROOT / "proofs/tla/HostProviderLifecycle.tla"
CONFIGS = (
    ROOT / "proofs/tla/MC/HostProviderLifecycleSerial.cfg",
    ROOT / "proofs/tla/MC/HostProviderLifecycleParallel.cfg",
    ROOT / "proofs/tla/MC/HostProviderLifecycleThreadBound.cfg",
    ROOT / "proofs/tla/MC/HostProviderLifecycleOwners.cfg",
    ROOT / "proofs/tla/MC/HostProviderLifecycleFair.cfg",
    ROOT / "proofs/tla/MC/HostProviderLifecycleParallelWitness.cfg",
)
COLUMNS = (
    "id",
    "formal_symbol",
    "positive_project",
    "positive_path",
    "positive_test",
    "negative_project",
    "negative_path",
    "negative_test",
    "negative_kind",
    "negative_reason",
)
PROJECTS = {"lling-llang": ROOT, "libdictenstein": ROOT.parent / "libdictenstein"}


def checked_symbols(path: Path) -> set[str]:
    symbols: set[str] = set()
    in_block = False
    for line in path.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped == "INVARIANTS":
            in_block = True
            continue
        if in_block:
            if line.startswith("  ") and re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", stripped):
                symbols.add(stripped)
                continue
            in_block = False
        match = re.fullmatch(r"(?:INVARIANT|PROPERTY) ([A-Za-z][A-Za-z0-9_]*)", stripped)
        if match:
            symbols.add(match.group(1))
    return symbols


def test_source(project: str, path: str, name: str, failures: list[str]) -> str:
    root = PROJECTS.get(project)
    if root is None:
        failures.append(f"{name}: unknown test project {project!r}")
        return ""
    pure = PurePosixPath(path)
    if pure.is_absolute() or ".." in pure.parts or pure.suffix != ".rs":
        failures.append(f"{name}: invalid repository-relative test path {path!r}")
        return ""
    source = root / pure
    if not source.is_file():
        failures.append(f"{name}: missing {project}/{path}")
        return ""
    content = source.read_text(encoding="utf-8", errors="replace")
    if not re.search(rf"\bfn\s+{re.escape(name)}\s*\(", content):
        failures.append(f"{name}: test function missing from {project}/{path}")
    return content


def main() -> int:
    failures: list[str] = []
    try:
        model = MODEL.read_text(encoding="utf-8")
        expected = set().union(*(checked_symbols(path) for path in CONFIGS))
        abi_rows = {}
        for line in ABI_LEDGER.read_text(encoding="utf-8").splitlines():
            if line and not line.startswith("#"):
                cells = line.split("\t")
                if cells[0].startswith("LLING-HOST-"):
                    abi_rows[cells[0]] = cells
        rows = []
        for line in LEDGER.read_text(encoding="utf-8").splitlines():
            if line and not line.startswith("#"):
                cells = line.split("\t")
                if len(cells) != len(COLUMNS):
                    failures.append(f"ledger row has {len(cells)} fields, expected {len(COLUMNS)}")
                else:
                    rows.append(dict(zip(COLUMNS, cells)))
    except OSError as error:
        print(f"check-host-provider-invariants: {error}", file=sys.stderr)
        return 1

    seen_ids: set[str] = set()
    seen_symbols: set[str] = set()
    for row in rows:
        row_id = row["id"]
        symbol = row["formal_symbol"]
        if row_id in seen_ids:
            failures.append(f"{row_id}: duplicate ID")
        seen_ids.add(row_id)
        if symbol in seen_symbols:
            failures.append(f"{symbol}: duplicate formal symbol")
        seen_symbols.add(symbol)
        if not re.search(rf"(?m)^{re.escape(symbol)} ==", model):
            failures.append(f"{row_id}: formal symbol {symbol} missing from model")
        abi = abi_rows.get(row_id)
        if abi is None:
            failures.append(f"{row_id}: missing ABI registry row")
        else:
            expected_path = (
                row["positive_path"]
                if row["positive_project"] == "lling-llang"
                else f"../{row['positive_project']}/{row['positive_path']}"
            )
            if (abi[5], abi[7], abi[8]) != (
                symbol,
                expected_path,
                row["positive_test"],
            ):
                failures.append(f"{row_id}: ABI registry mapping differs from host ledger")

        positive = test_source(
            row["positive_project"], row["positive_path"], row["positive_test"], failures
        )
        if positive and (
            "proptest!" not in positive
            or not ("_generated_" in row["positive_test"] or row["positive_test"].startswith("generated_"))
            or not re.search(
                rf"#\[test\]\s*fn\s+{re.escape(row['positive_test'])}\s*\(",
                positive,
            )
        ):
            failures.append(f"{row_id}: positive mapping is not a generated property")
        negative = test_source(
            row["negative_project"], row["negative_path"], row["negative_test"], failures
        )
        kind = row["negative_kind"]
        if kind == "panic-mutant":
            marker = f'#[should_panic(expected = "{row["negative_reason"]}")]'
            if not row["negative_test"].endswith("_mutant_is_detected") or not re.search(
                rf"{re.escape(marker)}\s*fn\s+{re.escape(row['negative_test'])}\s*\(",
                negative,
            ):
                failures.append(f"{row_id}: expected-reason panic mutant missing")
        else:
            failures.append(f"{row_id}: negative control must be a failing mutant")

    if seen_symbols != expected:
        failures.append(
            f"formal coverage mismatch: missing={sorted(expected - seen_symbols)}, "
            f"extra={sorted(seen_symbols - expected)}"
        )
    if seen_ids != set(abi_rows):
        failures.append(
            f"ABI registry closure mismatch: missing={sorted(set(abi_rows) - seen_ids)}, "
            f"extra={sorted(seen_ids - set(abi_rows))}"
        )

    if failures:
        print(f"check-host-provider-invariants: {len(failures)} failure(s)")
        for failure in failures:
            print(f"  FAIL {failure}")
        return 1
    print(f"check-host-provider-invariants: {len(rows)} formal laws/witnesses mapped to generated actual-path properties and intended-failure mutants")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
