#!/usr/bin/env python3
"""Validate exhaustive lazy-expansion formal-obligation/property mappings."""

from __future__ import annotations

import csv
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "proofs/doc/lazy-expansion-invariants.tsv"
ROCQ_ARTIFACT = "proofs/coq/wfst/LazyExpansion.v"
TLA_ARTIFACT = "proofs/tla/LazyExpansionLifecycle.tla"
TLA_CONFIG = ROOT / "proofs/tla/MC/LazyExpansionLifecycle.cfg"
SMT_ARTIFACT = "proofs/smt/vco-e4-lazy-expansion.smt2"
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
PROPERTY_DECLARATION = re.compile(
    r"^[ \t]*property!\(\s*(prop_[A-Za-z0-9_]+)",
    re.MULTILINE,
)


def fail(message: str) -> None:
    """Report a registry contract violation and terminate."""
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def property_declarations(source: str) -> set[str]:
    """Extract property macro names across stable rustfmt layouts."""
    return set(PROPERTY_DECLARATION.findall(source))


parser_regression = """
property!(prop_inline, any::<bool>(), |_value| { Ok(()) });
property!(
    prop_multiline,
    any::<bool>(),
    |_value| { Ok(()) }
);
"""
if property_declarations(parser_regression) != {"prop_inline", "prop_multiline"}:
    fail("internal property parser does not cover inline and multiline rustfmt layouts")


with REGISTRY.open(encoding="utf-8", newline="") as source:
    reader = csv.DictReader(source, delimiter="\t")
    if set(reader.fieldnames or ()) != REQUIRED_COLUMNS:
        fail(f"unexpected registry columns: {reader.fieldnames}")
    rows = list(reader)

if not rows:
    fail("lazy-expansion invariant registry is empty")

ids: set[str] = set()
properties: set[tuple[str, str]] = set()
registered: dict[str, set[tuple[str, str]]] = {}

for line_number, row in enumerate(rows, start=2):
    missing = sorted(name for name in REQUIRED_COLUMNS if not row[name].strip())
    if missing:
        fail(f"line {line_number} has empty fields: {', '.join(missing)}")

    invariant_id = row["id"]
    if not invariant_id.startswith("E4-LAZY-"):
        fail(f"unexpected identifier {invariant_id}")
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
        fail(f"{invariant_id} property lacks the prop_ naming contract")
    suite_path = ROOT / suite
    if not suite_path.is_file():
        fail(f"{invariant_id} references missing property suite {suite}")
    if not re.search(
        rf"\b{re.escape(row['property_name'])}\b",
        suite_path.read_text(encoding="utf-8"),
    ):
        fail(f"{invariant_id} property {row['property_name']!r} is absent")

    property_key = (suite, row["property_name"])
    if property_key in properties:
        fail(f"duplicate planned property mapping {property_key}")
    properties.add(property_key)

    declaration_key = (row["declaration_kind"], row["formal_symbol"])
    artifact_declarations = registered.setdefault(row["artifact"], set())
    if declaration_key in artifact_declarations:
        fail(f"duplicate formal mapping {row['artifact']}::{declaration_key}")
    artifact_declarations.add(declaration_key)

rocq_text = (ROOT / ROCQ_ARTIFACT).read_text(encoding="utf-8")
rocq_declarations = {
    (kind.lower(), symbol) for kind, symbol in ROCQ_DECLARATION.findall(rocq_text)
}
mapped_rocq = registered.get(ROCQ_ARTIFACT, set())
if rocq_declarations != mapped_rocq:
    fail(
        "Rocq registry mismatch: "
        f"missing={sorted(rocq_declarations - mapped_rocq)}, "
        f"unexpected={sorted(mapped_rocq - rocq_declarations)}"
    )

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
        r"^; (E4-LAZY-SMT-[A-Z0-9-]+)\b",
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

known_artifacts = {ROCQ_ARTIFACT, TLA_ARTIFACT, SMT_ARTIFACT}
unknown_artifacts = sorted(set(registered) - known_artifacts)
if unknown_artifacts:
    fail(f"registry contains unknown formal artifacts: {unknown_artifacts}")

for suite in sorted({suite for suite, _ in properties}):
    declared_properties = {
        (suite, name)
        for name in property_declarations((ROOT / suite).read_text(encoding="utf-8"))
    }
    mapped_properties = {entry for entry in properties if entry[0] == suite}
    if declared_properties != mapped_properties:
        fail(
            f"property registry mismatch in {suite}: "
            f"missing={sorted(declared_properties - mapped_properties)}, "
            f"unexpected={sorted(mapped_properties - declared_properties)}"
        )

print(
    f"Validated {len(rows)} lazy-expansion obligations: every Rocq "
    "declaration, TLC invariant, and named SMT query maps to a unique "
    "required-red pre-production Rust property."
)
