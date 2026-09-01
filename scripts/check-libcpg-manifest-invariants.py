#!/usr/bin/env python3
"""Generate and validate exhaustive E7 manifest/fact traceability."""

from __future__ import annotations

import argparse
import csv
import hashlib
import re
import subprocess
import sys
from pathlib import Path

sys.dont_write_bytecode = True

from libcpg_manifest_mutants import MUTATIONS

ROOT = Path(__file__).resolve().parents[1]
ROCQ = "proofs/coq/domain_integration/ManifestFactContracts.v"
TLA = "proofs/tla/LibcpgManifestLifecycle.tla"
TLA_CONFIG = ROOT / "proofs/tla/MC/LibcpgManifestLifecycle.cfg"
SMT = "proofs/smt/vco-e7-manifest-facts.smt2"
REGISTRY = ROOT / "proofs/doc/libcpg-manifest-invariants.tsv"
BASELINES = ROOT / "proofs/doc/libcpg-manifest-api-baselines.tsv"
LIBCPG_SUITE = "proofs/required_red/libcpg_manifest/tests/libcpg.rs"
ADAPTER_SUITE = "proofs/required_red/libcpg_manifest/tests/adapter.rs"
COLUMNS = [
    "id", "area", "invariant", "formalism", "artifact", "formal_symbol",
    "proof_strength", "implementation_owner", "property_suite", "property_name",
    "implementation_state", "model_evidence",
]
BASELINE_COLUMNS = [
    "repository", "state", "branch", "commit", "path", "sha256", "protection",
]


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def snake(symbol: str) -> str:
    value = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", symbol)
    return value.replace("-", "_").lower()


def words(symbol: str) -> str:
    return snake(symbol).replace("_", " ")


def owner_for(symbol: str) -> tuple[str, str, str]:
    lowered = symbol.lower()
    adapter_markers = (
        "lowering", "fact_rule", "fact-rule", "lling_llang", "lling-llang",
        "adapter_is", "adapter-owned", "core_dependency", "core-dependency",
        "runtime_envelope_does_not_reverse", "lowered", "many_to_many",
        "many-to-many",
    )
    if any(marker in lowered for marker in adapter_markers):
        return "adapter-boundary", "vinary-libcpg-adapter", ADAPTER_SUITE
    return "manifest-facts", "libcpg", LIBCPG_SUITE


def expected_rows() -> list[dict[str, str]]:
    coq_text = (ROOT / ROCQ).read_text(encoding="utf-8")
    coq = re.findall(r"^(?:Theorem|Lemma)\s+([A-Za-z0-9_']+)", coq_text, re.MULTILINE)
    tla = re.findall(
        r"^(?:INVARIANT|PROPERTY)\s+([A-Za-z0-9_]+)",
        TLA_CONFIG.read_text(encoding="utf-8"),
        re.MULTILINE,
    )
    smt = re.findall(
        r"^; (E7-MF-SMT-[A-Z0-9-]+)\b",
        (ROOT / SMT).read_text(encoding="utf-8"),
        re.MULTILINE,
    )

    mutant_targets = {mutation.target for mutation in MUTATIONS.values()}
    if mutant_targets != set(tla):
        fail(
            "TLA mutation coverage differs from checked properties: "
            f"missing={sorted(set(tla) - mutant_targets)}, "
            f"unexpected={sorted(mutant_targets - set(tla))}"
        )
    if len(mutant_targets) != len(MUTATIONS):
        fail("multiple mutants target the same checked TLA property")
    for name, mutation in MUTATIONS.items():
        if (ROOT / TLA).read_text(encoding="utf-8").count(mutation.needle) != 1:
            fail(f"mutant {name} does not have exactly one causal injection site")

    rows: list[dict[str, str]] = []
    for index, symbol in enumerate(coq, 1):
        area, owner, suite = owner_for(symbol)
        rows.append({
            "id": f"E7-MF-COQ-{index:03d}",
            "area": area,
            "invariant": words(symbol),
            "formalism": "rocq",
            "artifact": ROCQ,
            "formal_symbol": symbol,
            "proof_strength": "rocq-unbounded",
            "implementation_owner": owner,
            "property_suite": suite,
            "property_name": f"prop_e7_mf_coq_{snake(symbol)}",
            "implementation_state": "required-red-before-production",
            "model_evidence": "rocq-proof-term",
        })
    target_to_mutant = {mutation.target: name for name, mutation in MUTATIONS.items()}
    for index, symbol in enumerate(tla, 1):
        area, owner, suite = owner_for(symbol)
        rows.append({
            "id": f"E7-MF-TLA-{index:03d}",
            "area": area,
            "invariant": words(symbol),
            "formalism": "tla+",
            "artifact": TLA,
            "formal_symbol": symbol,
            "proof_strength": "tlc-finite-exhaustive",
            "implementation_owner": owner,
            "property_suite": suite,
            "property_name": f"prop_e7_mf_tla_{snake(symbol)}",
            "implementation_state": "required-red-before-production",
            "model_evidence": f"tlc-mutant:{target_to_mutant[symbol]}",
        })
    for index, symbol in enumerate(smt, 1):
        area, owner, suite = owner_for(symbol)
        rows.append({
            "id": f"E7-MF-SMT-{index:03d}",
            "area": area,
            "invariant": words(symbol.removeprefix("E7-MF-SMT-")),
            "formalism": "smt",
            "artifact": SMT,
            "formal_symbol": symbol,
            "proof_strength": "z3-finite-boundary",
            "implementation_owner": owner,
            "property_suite": suite,
            "property_name": f"prop_{snake(symbol)}",
            "implementation_state": "required-red-before-production",
            "model_evidence": (
                "z3-positive-witness" if "-VALID-" in symbol
                else "z3-unsat-negative-control"
            ),
        })
    return rows


def write_registry(rows: list[dict[str, str]]) -> None:
    with REGISTRY.open("w", encoding="utf-8", newline="") as destination:
        writer = csv.DictWriter(
            destination, fieldnames=COLUMNS, delimiter="\t", lineterminator="\n"
        )
        writer.writeheader()
        writer.writerows(rows)


def validate_registry(expected: list[dict[str, str]]) -> None:
    with REGISTRY.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source, delimiter="\t")
        if reader.fieldnames != COLUMNS:
            fail(f"unexpected invariant columns: {reader.fieldnames}")
        actual = list(reader)
    if actual != expected:
        fail("manifest/fact invariant ledger is stale; run with --write")

    ids: set[str] = set()
    property_keys: set[tuple[str, str]] = set()
    expected_by_suite: dict[str, set[str]] = {}
    for row in actual:
        if row["id"] in ids:
            fail(f"duplicate invariant id {row['id']}")
        ids.add(row["id"])
        artifact = ROOT / row["artifact"]
        if not artifact.is_file() or row["formal_symbol"] not in artifact.read_text(encoding="utf-8"):
            fail(f"{row['id']} references absent formal evidence")
        if row["implementation_state"] != "required-red-before-production":
            fail(f"{row['id']} weakens the formal-before-production gate")
        key = (row["property_suite"], row["property_name"])
        if key in property_keys:
            fail(f"duplicate required-red property mapping {key}")
        property_keys.add(key)
        expected_by_suite.setdefault(row["property_suite"], set()).add(row["property_name"])

    for suite_name, names in expected_by_suite.items():
        suite = ROOT / suite_name
        if not suite.is_file():
            fail(f"required-red property suite is absent: {suite_name}")
        source = suite.read_text(encoding="utf-8")
        macro = "adapter_property" if suite_name == ADAPTER_SUITE else "manifest_property"
        actual_names = set(re.findall(rf"{macro}!\(\s*([a-z0-9_]+)", source))
        if actual_names != names:
            fail(
                f"{suite_name} property set differs: "
                f"missing={sorted(names - actual_names)}, "
                f"unexpected={sorted(actual_names - names)}"
            )


def validate_baselines() -> tuple[int, int]:
    with BASELINES.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source, delimiter="\t")
        if reader.fieldnames != BASELINE_COLUMNS:
            fail(f"unexpected baseline columns: {reader.fieldnames}")
        rows = list(reader)
    if not rows:
        fail("libcpg API baseline registry is empty")

    repositories: set[str] = set()
    file_count = 0
    for row in rows:
        repositories.add(row["repository"])
        repository = ROOT.parent / row["repository"]
        if row["state"] == "absent-required-red":
            if repository.exists():
                fail(f"{row['repository']} now exists and requires a fresh ownership audit")
            if any(row[field] != "NONE" for field in ("branch", "commit", "path", "sha256")):
                fail(f"absent repository row is not canonical: {row['repository']}")
            continue
        if row["state"] != "committed-dirty-protected":
            fail(f"unknown protected baseline state {row['state']}")
        branch = subprocess.run(
            ["git", "branch", "--show-current"], cwd=repository,
            check=True, capture_output=True, text=True,
        ).stdout.strip()
        commit = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=repository,
            check=True, capture_output=True, text=True,
        ).stdout.strip()
        if branch != row["branch"] or commit != row["commit"]:
            fail(f"{row['repository']} moved from its reviewed branch/commit baseline")
        path = repository / row["path"]
        if not path.is_file():
            fail(f"protected API path is absent: {row['repository']}/{row['path']}")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != row["sha256"]:
            fail(f"protected API path changed: {row['repository']}/{row['path']}")
        if row["protection"] != "read-only; ownership handoff required before implementation":
            fail(f"baseline protection weakened for {row['repository']}/{row['path']}")
        file_count += 1
    return len(repositories), file_count


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true", help="rewrite the deterministic ledger")
    args = parser.parse_args()
    rows = expected_rows()
    if args.write:
        write_registry(rows)
    validate_registry(rows)
    repositories, files = validate_baselines()
    print(
        f"Validated {len(rows)} exhaustive obligations and required-red properties, "
        f"26 causal mutants, {files} protected files, and {repositories} repository baselines."
    )


if __name__ == "__main__":
    main()
