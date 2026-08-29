#!/usr/bin/env python3
"""Validate exhaustive E9 formal-obligation to Rust-property mappings."""

from __future__ import annotations

import csv
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "proofs/doc/provider-boundary-invariants.tsv"
ROCQ_ARTIFACTS = (
    "proofs/coq/domain_integration/ProviderResult.v",
    "proofs/coq/domain_integration/CanonicalArtifact.v",
    "proofs/coq/domain_integration/ProviderBoundary.v",
)
TLA_ARTIFACT = "proofs/tla/ProviderBoundaryLifecycle.tla"
TLA_CONFIG = ROOT / "proofs/tla/MC/ProviderBoundaryLifecycle.cfg"
SMT_ARTIFACT = "proofs/smt/vco-e9-provider-boundary.smt2"
REQUIRED_COLUMNS = {
    "id",
    "area",
    "invariant",
    "formalism",
    "artifact",
    "formal_symbol",
    "declaration_kind",
    "proof_strength",
    "property_suite",
    "property_name",
    "implementation_state",
}
ROCQ_DECLARATION = re.compile(
    r"^(Theorem|Lemma|Definition|Record|Inductive)\s+([A-Za-z0-9_']+)",
    re.MULTILINE,
)


def fail(message: str) -> None:
    """Report a registry contract violation and terminate."""
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


with REGISTRY.open(encoding="utf-8", newline="") as source:
    reader = csv.DictReader(source, delimiter="\t")
    if set(reader.fieldnames or ()) != REQUIRED_COLUMNS:
        fail(f"unexpected registry columns: {reader.fieldnames}")
    rows = list(reader)

if not rows:
    fail("E9 invariant registry is empty")

ids: set[str] = set()
properties: set[tuple[str, str]] = set()
registered: dict[str, set[tuple[str, str]]] = {}

for line_number, row in enumerate(rows, start=2):
    missing = sorted(name for name in REQUIRED_COLUMNS if not row[name].strip())
    if missing:
        fail(f"line {line_number} has empty fields: {', '.join(missing)}")

    invariant_id = row["id"]
    if not invariant_id.startswith("E9-"):
        fail(f"unexpected non-E9 identifier {invariant_id}")
    if invariant_id in ids:
        fail(f"duplicate invariant id {invariant_id}")
    ids.add(invariant_id)

    if row["implementation_state"] != "required-red-before-production":
        fail(f"{invariant_id} is not gated before production implementation")

    artifact = ROOT / row["artifact"]
    if not artifact.is_file():
        fail(f"{invariant_id} references missing artifact {row['artifact']}")
    if row["formal_symbol"] not in artifact.read_text(encoding="utf-8"):
        fail(f"{invariant_id} symbol {row['formal_symbol']!r} is absent")

    suite = row["property_suite"]
    if not suite.startswith("tests/") or not suite.endswith(".rs"):
        fail(f"{invariant_id} has a non-Rust property suite path")
    if not row["property_name"].startswith("prop_"):
        fail(f"{invariant_id} property does not use the prop_ naming contract")

    property_key = (suite, row["property_name"])
    if property_key in properties:
        fail(f"duplicate planned property mapping {property_key}")
    properties.add(property_key)

    declaration_key = (row["declaration_kind"], row["formal_symbol"])
    artifact_declarations = registered.setdefault(row["artifact"], set())
    if declaration_key in artifact_declarations:
        fail(f"duplicate formal mapping {row['artifact']}::{declaration_key}")
    artifact_declarations.add(declaration_key)

for relative in ROCQ_ARTIFACTS:
    text = (ROOT / relative).read_text(encoding="utf-8")
    declared = {
        (kind.lower(), symbol) for kind, symbol in ROCQ_DECLARATION.findall(text)
    }
    mapped = registered.get(relative, set())
    omitted = sorted(declared - mapped)
    unexpected = sorted(mapped - declared)
    if omitted:
        fail(f"unregistered Rocq declarations in {relative}: {omitted}")
    if unexpected:
        fail(f"spurious Rocq declarations in {relative}: {unexpected}")

tla_invariants = {
    ("invariant", symbol)
    for symbol in re.findall(
        r"^INVARIANT\s+([A-Za-z0-9_]+)",
        TLA_CONFIG.read_text(encoding="utf-8"),
        re.MULTILINE,
    )
}
mapped_tla = registered.get(TLA_ARTIFACT, set())
if tla_invariants != mapped_tla:
    fail(
        "TLC invariant registry mismatch: "
        f"missing={sorted(tla_invariants - mapped_tla)}, "
        f"unexpected={sorted(mapped_tla - tla_invariants)}"
    )

smt_queries = {
    ("query", symbol)
    for symbol in re.findall(
        r"^; (E9-SMT-[A-Z0-9-]+)\b",
        (ROOT / SMT_ARTIFACT).read_text(encoding="utf-8"),
        re.MULTILINE,
    )
}
mapped_smt = registered.get(SMT_ARTIFACT, set())
if smt_queries != mapped_smt:
    fail(
        "SMT query registry mismatch: "
        f"missing={sorted(smt_queries - mapped_smt)}, "
        f"unexpected={sorted(mapped_smt - smt_queries)}"
    )

known_artifacts = {*ROCQ_ARTIFACTS, TLA_ARTIFACT, SMT_ARTIFACT}
unknown_artifacts = sorted(set(registered) - known_artifacts)
if unknown_artifacts:
    fail(f"registry contains unknown formal artifacts: {unknown_artifacts}")

print(
    f"Validated {len(rows)} E9 obligations: every Rocq declaration, "
    "TLC invariant, and named SMT query maps to a unique "
    "required-red pre-production Rust property."
)
