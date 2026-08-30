#!/usr/bin/env python3
"""Validate exhaustive WPDS portability proof-to-property traceability."""

from __future__ import annotations

import csv
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "proofs/doc/wpds-portability-invariants.tsv"
ROCQ_ARTIFACT = "proofs/coq/portability/WpdsPortability.v"
TLA_ARTIFACT = "proofs/tla/WpdsPortabilityLifecycle.tla"
TLA_CONFIG = ROOT / "proofs/tla/MC/WpdsPortabilityLifecycle.cfg"
SMT_ARTIFACT = "proofs/smt/vco-e4-wpds-portability.smt2"
SMT_EXPECTED = ROOT / "proofs/smt/vco-e4-wpds-portability.expected"
KANI_ARTIFACT = "proofs/kani/wpds_portability_model.rs"
VERIFY_SCRIPT = ROOT / "proofs/verify.sh"
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
PROPERTY_DECLARATION = re.compile(r"^[ \t]*fn\s+(prop_[A-Za-z0-9_]+)\s*\(", re.MULTILINE)


def fail(message: str) -> None:
    """Report one traceability violation and terminate."""
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def declarations() -> dict[str, set[tuple[str, str]]]:
    """Extract every proof obligation governed by this formal tranche."""
    rocq = (ROOT / ROCQ_ARTIFACT).read_text(encoding="utf-8")
    tla = TLA_CONFIG.read_text(encoding="utf-8")
    smt = (ROOT / SMT_ARTIFACT).read_text(encoding="utf-8")
    kani = (ROOT / KANI_ARTIFACT).read_text(encoding="utf-8")
    return {
        ROCQ_ARTIFACT: {
            (kind.lower(), symbol)
            for kind, symbol in re.findall(
                r"^[ \t]*(Theorem|Lemma)\s+([A-Za-z0-9_']+)",
                rocq,
                re.MULTILINE,
            )
        },
        TLA_ARTIFACT: {
            ("invariant", symbol)
            for symbol in re.findall(
                r"^INVARIANT\s+([A-Za-z0-9_]+)", tla, re.MULTILINE
            )
        },
        SMT_ARTIFACT: {
            ("query", symbol)
            for symbol in re.findall(
                r'^\(echo\s+"\[([^]]+)\]"\)', smt, re.MULTILINE
            )
        },
        KANI_ARTIFACT: {
            ("harness", symbol)
            for symbol in re.findall(
                r"#\[kani::proof\](?:\s*#\[[^]]+\])*\s*fn\s+([A-Za-z0-9_]+)",
                kani,
            )
        },
    }


with REGISTRY.open(encoding="utf-8", newline="") as source:
    reader = csv.DictReader(source, delimiter="\t")
    if set(reader.fieldnames or ()) != REQUIRED_COLUMNS:
        fail(f"unexpected registry columns: {reader.fieldnames}")
    rows = list(reader)

if not rows:
    fail("WPDS portability invariant registry is empty")

ids: set[str] = set()
mapped: dict[str, set[tuple[str, str]]] = {}
mapped_properties: dict[str, set[str]] = {}

for line_number, row in enumerate(rows, start=2):
    missing = sorted(name for name in REQUIRED_COLUMNS if not row[name].strip())
    if missing:
        fail(f"line {line_number} has empty fields: {', '.join(missing)}")

    invariant_id = row["id"]
    if not invariant_id.startswith("E4-WPDS-"):
        fail(f"unexpected invariant identifier {invariant_id}")
    if invariant_id in ids:
        fail(f"duplicate invariant identifier {invariant_id}")
    ids.add(invariant_id)

    if row["implementation_state"] != "required-red-before-production":
        fail(f"{invariant_id} is not gated before production implementation")

    artifact = ROOT / row["artifact"]
    if not artifact.is_file():
        fail(f"{invariant_id} references missing artifact {row['artifact']}")
    if row["formal_symbol"] not in artifact.read_text(encoding="utf-8"):
        fail(f"{invariant_id} symbol {row['formal_symbol']!r} is absent")

    formal_key = (row["declaration_kind"], row["formal_symbol"])
    artifact_mappings = mapped.setdefault(row["artifact"], set())
    if formal_key in artifact_mappings:
        fail(f"duplicate formal mapping {row['artifact']}::{formal_key}")
    artifact_mappings.add(formal_key)

    suite = row["property_suite"]
    property_name = row["property_name"]
    if not suite.startswith("tests/") or not suite.endswith(".rs"):
        fail(f"{invariant_id} has a non-Rust property suite path")
    if not property_name.startswith("prop_"):
        fail(f"{invariant_id} property lacks the prop_ naming contract")
    suite_path = ROOT / suite
    if not suite_path.is_file():
        fail(f"{invariant_id} references missing property suite {suite}")
    properties = set(PROPERTY_DECLARATION.findall(suite_path.read_text(encoding="utf-8")))
    if property_name not in properties:
        fail(f"{invariant_id} property {property_name!r} is absent")
    mapped_properties.setdefault(suite, set()).add(property_name)

expected = declarations()
if set(mapped) != set(expected):
    fail(
        "formal artifact registry mismatch: "
        f"missing={sorted(set(expected) - set(mapped))}, "
        f"unexpected={sorted(set(mapped) - set(expected))}"
    )
for artifact, declared in expected.items():
    registered = mapped[artifact]
    if declared != registered:
        fail(
            f"{artifact} registry mismatch: "
            f"missing={sorted(declared - registered)}, "
            f"unexpected={sorted(registered - declared)}"
        )

for suite, registered in mapped_properties.items():
    declared = set(PROPERTY_DECLARATION.findall((ROOT / suite).read_text(encoding="utf-8")))
    if declared != registered:
        fail(
            f"property registry mismatch in {suite}: "
            f"missing={sorted(declared - registered)}, "
            f"unexpected={sorted(registered - declared)}"
        )

smt_queries = {symbol for _, symbol in expected[SMT_ARTIFACT]}
expected_tags = set(re.findall(r"^\[([^]]+)\]$", SMT_EXPECTED.read_text(encoding="utf-8"), re.MULTILINE))
if smt_queries != expected_tags:
    fail(
        "SMT expected-output coverage mismatch: "
        f"missing={sorted(smt_queries - expected_tags)}, "
        f"unexpected={sorted(expected_tags - smt_queries)}"
    )

verify = VERIFY_SCRIPT.read_text(encoding="utf-8")
required_gate_fragments = {
    "run_tlc wpds-portability",
    "wpds-portability-duplicate-mutant",
    "wpds-portability-identity-mutant",
    "wpds-portability-cancellation-mutant",
    "z3-vco-e4-wpds-portability.log",
    "verify-wpds-portability-bounded.sh",
}
missing_gate_fragments = sorted(fragment for fragment in required_gate_fragments if fragment not in verify)
if missing_gate_fragments:
    fail(f"formal gate omits WPDS checks: {missing_gate_fragments}")

print(
    f"Validated {len(rows)} WPDS portability obligations across Rocq, TLC, "
    "Z3, and Kani; every obligation maps to a required-red Rust property."
)
