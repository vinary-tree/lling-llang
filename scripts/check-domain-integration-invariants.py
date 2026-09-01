#!/usr/bin/env python3
"""Validate exhaustive E6 formal-obligation to future-property mappings."""

from __future__ import annotations

import csv
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "proofs/doc/domain-integration-invariants.tsv"
COQ_ARTIFACTS = (
    "proofs/coq/domain_integration/FuzzyReference.v",
    "proofs/coq/domain_integration/TypedHclg.v",
)
TLA_CONFIG = ROOT / "proofs/tla/MC/FuzzyReferenceLifecycle.cfg"
SMT_ARTIFACT = "proofs/smt/vco-e6-domain-contracts.smt2"
REQUIRED_COLUMNS = {
    "id",
    "area",
    "invariant",
    "formalism",
    "artifact",
    "formal_symbol",
    "proof_strength",
    "property_suite",
    "property_name",
    "implementation_state",
}


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


with REGISTRY.open(encoding="utf-8", newline="") as source:
    reader = csv.DictReader(source, delimiter="\t")
    if set(reader.fieldnames or ()) != REQUIRED_COLUMNS:
        fail(f"unexpected registry columns: {reader.fieldnames}")
    rows = list(reader)

if not rows:
    fail("domain integration invariant registry is empty")

ids: set[str] = set()
properties: set[tuple[str, str]] = set()
symbols_by_artifact: dict[str, set[str]] = {}

for line_number, row in enumerate(rows, start=2):
    missing = sorted(name for name in REQUIRED_COLUMNS if not row[name].strip())
    if missing:
        fail(f"line {line_number} has empty fields: {', '.join(missing)}")

    invariant_id = row["id"]
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

    property_key = (row["property_suite"], row["property_name"])
    if property_key in properties:
        fail(f"duplicate planned property mapping {property_key}")
    properties.add(property_key)
    if not row["property_suite"].startswith("tests/") or not row[
        "property_suite"
    ].endswith(".rs"):
        fail(f"{invariant_id} has a non-Rust property suite path")

    symbols_by_artifact.setdefault(row["artifact"], set()).add(row["formal_symbol"])

# Every theorem and lemma in the E6 Rocq artifacts must be represented.  This
# makes adding a proof without extracting its executable property a gate error.
declaration = re.compile(r"^(?:Theorem|Lemma)\s+([A-Za-z0-9_']+)", re.MULTILINE)
for relative in COQ_ARTIFACTS:
    declared = set(declaration.findall((ROOT / relative).read_text(encoding="utf-8")))
    registered = symbols_by_artifact.get(relative, set())
    omitted = sorted(declared - registered)
    if omitted:
        fail(f"unregistered Rocq obligations in {relative}: {', '.join(omitted)}")

# Every TLC invariant selected by the model configuration must be registered.
tla_invariants = set(
    re.findall(r"^INVARIANT\s+([A-Za-z0-9_]+)", TLA_CONFIG.read_text(encoding="utf-8"), re.MULTILINE)
)
registered_tla = symbols_by_artifact.get("proofs/tla/FuzzyReferenceLifecycle.tla", set())
omitted_tla = sorted(tla_invariants - registered_tla)
if omitted_tla:
    fail(f"unregistered TLC invariants: {', '.join(omitted_tla)}")

# Every named SMT check must have an invariant-registry row.
smt_hooks = set(
    re.findall(r"^; (E6-SMT-[A-Z-]+)\b", (ROOT / SMT_ARTIFACT).read_text(encoding="utf-8"), re.MULTILINE)
)
registered_smt = symbols_by_artifact.get(SMT_ARTIFACT, set())
omitted_smt = sorted(smt_hooks - registered_smt)
if omitted_smt:
    fail(f"unregistered SMT checks: {', '.join(omitted_smt)}")

print(
    f"Validated {len(rows)} E6 invariants: all formal obligations map to "
    "unique pre-implementation Rust properties."
)
