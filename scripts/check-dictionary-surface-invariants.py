#!/usr/bin/env python3
"""Validate the E6 dictionary-surface proof and required-red test registry."""

from __future__ import annotations

import csv
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "proofs/doc/dictionary-surface-invariants.tsv"
BASELINES = ROOT / "proofs/doc/dictionary-surface-api-baselines.tsv"
COQ = "proofs/coq/domain_integration/DictionarySurface.v"
TLA = "proofs/tla/DictionarySurfaceLifecycle.tla"
TLA_CONFIG = ROOT / "proofs/tla/MC/DictionarySurfaceLifecycle.cfg"
SMT = "proofs/smt/vco-e6-dictionary-surface.smt2"
COLUMNS = [
    "id", "area", "invariant", "formalism", "artifact", "formal_symbol",
    "proof_strength", "implementation_owner", "property_suite", "property_name",
    "implementation_state",
]
BASELINE_COLUMNS = [
    "id", "package", "version", "source_ref", "commit", "api_files",
    "archive_sha256", "architectural_role",
]
OWNERS = {
    "libdictenstein", "liblevenshtein", "llattice", "lling-llang",
    "libdictenstein-llattice", "vinary-dictionary-pipeline", "duallity",
}
EXPECTED_BASELINES = {
    "BASE-LIBDICT-RC5": (
        "libdictenstein", "4.0.0-rc.5", "v4.0.0-rc.5",
        "1cf21a1ef1861ca074ded8b63ed17c98c9fd6c7c",
        "Cargo.toml;src/lib.rs;src/bindings.rs;src/bindings/entries.rs",
        "11185d98bc883ba437d2c0981dc0c54dcf6ed0d2da19296145600d6bec111c80",
    ),
    "BASE-LEV-RC5": (
        "liblevenshtein", "4.0.0-rc.5", "v4.0.0-rc.5",
        "a08279410e572f0c932b1887a1906aba6fdcece4",
        "Cargo.toml;src/lib.rs;src/transducer/algorithm.rs;src/transducer/query.rs;src/transducer/ordered_query.rs;src/transducer/query_result.rs;src/dictionary/mod.rs;src/dictionary/node_adapter.rs;src/dictionary/phonetic_normalized.rs",
        "5a5c45fc8558a8f085ba89c24e741f1250f39446934f32c6af8a5efe51e0b847",
    ),
    "BASE-DUALLITY-RC5": (
        "duallity", "4.0.0-rc.5", "v4.0.0-rc.5",
        "387521f2e2c40ea1abc14e267c35f6006291b703",
        "Cargo.toml;src/lib.rs;src/backend.rs;src/bindings.rs",
        "8a30a0f141e29d8727173863ed8f27d5532a47350cc3f4f23b4983e3e80cf583",
    ),
    "BASE-LLING-RC5": (
        "lling-llang", "4.0.0-rc.5", "v4.0.0-rc.5",
        "d4cdb40540338c901addb7c28b932f2d9222a151",
        "Cargo.toml;src/lib.rs;src/backend/traits.rs;src/lattice_bridge.rs",
        "31f62e0f2fa3ea3fd1c57951eeb5159f3826bb0cf426d698b94eccb0809d0eb2",
    ),
    "BASE-LLATTICE-V01": (
        "llattice", "0.1.0", "v0.1.0",
        "9a35c0f08cf6dbd5f6cb8c72a431c1ccd849a095",
        "Cargo.toml;src/lib.rs",
        "ba3aa48e3cfacc72426f47a16c4ee5135f48417759a30b31866ce7217f65a195",
    ),
    "BASE-LLATTICE-V2": (
        "llattice", "unreleased-v2-candidate",
        "e123c8711aaff177c14b2b5852af06bd07ba3dc2",
        "e123c8711aaff177c14b2b5852af06bd07ba3dc2",
        "Cargo.toml;src/lib.rs;src/impls.rs;src/laws.rs",
        "fd7ca3d02f42562c8e1f0875223a3040d1f8cf1b8f52d2232ef37676b10b6fef",
    ),
}


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


with REGISTRY.open(encoding="utf-8", newline="") as source:
    reader = csv.DictReader(source, delimiter="\t")
    if reader.fieldnames != COLUMNS:
        fail(f"unexpected invariant columns: {reader.fieldnames}")
    rows = list(reader)

if not rows:
    fail("dictionary-surface invariant registry is empty")

ids: set[str] = set()
properties: set[tuple[str, str]] = set()
symbols: dict[str, set[str]] = {}

for line_number, row in enumerate(rows, start=2):
    missing = [column for column in COLUMNS if not row[column].strip()]
    if missing:
        fail(f"line {line_number} has empty fields: {', '.join(missing)}")
    if row["id"] in ids:
        fail(f"duplicate invariant id {row['id']}")
    ids.add(row["id"])
    if row["implementation_owner"] not in OWNERS:
        fail(f"{row['id']} has unapproved owner {row['implementation_owner']}")
    if row["implementation_state"] != "required-red-before-production":
        fail(f"{row['id']} bypasses the required-red gate")

    artifact = ROOT / row["artifact"]
    if not artifact.is_file():
        fail(f"{row['id']} references missing artifact {row['artifact']}")
    if row["formal_symbol"] not in artifact.read_text(encoding="utf-8"):
        fail(f"{row['id']} references absent symbol {row['formal_symbol']}")
    symbols.setdefault(row["artifact"], set()).add(row["formal_symbol"])

    suite = ROOT / row["property_suite"]
    if not suite.is_file():
        fail(f"{row['id']} references missing property suite {row['property_suite']}")
    property_key = (row["property_suite"], row["property_name"])
    if property_key in properties:
        fail(f"duplicate property mapping {property_key}")
    properties.add(property_key)
    if not re.search(rf"\bfn\s+{re.escape(row['property_name'])}\b", suite.read_text(encoding="utf-8")):
        fail(f"{row['id']} property {row['property_name']} is absent")

declared = set(re.findall(
    r"^(?:Theorem|Lemma)\s+([A-Za-z0-9_']+)",
    (ROOT / COQ).read_text(encoding="utf-8"),
    re.MULTILINE,
))
omitted = sorted(declared - symbols.get(COQ, set()))
if omitted:
    fail(f"unregistered Rocq obligations: {', '.join(omitted)}")

configured_tla = set(re.findall(
    r"^INVARIANT\s+([A-Za-z0-9_]+)",
    TLA_CONFIG.read_text(encoding="utf-8"),
    re.MULTILINE,
))
omitted = sorted(configured_tla - symbols.get(TLA, set()))
if omitted:
    fail(f"unregistered TLC invariants: {', '.join(omitted)}")

named_smt = set(re.findall(
    r"^; (E6-DS-SMT-[A-Z0-9-]+)\b",
    (ROOT / SMT).read_text(encoding="utf-8"),
    re.MULTILINE,
))
omitted = sorted(named_smt - symbols.get(SMT, set()))
if omitted:
    fail(f"unregistered SMT checks: {', '.join(omitted)}")

with BASELINES.open(encoding="utf-8", newline="") as source:
    reader = csv.DictReader(source, delimiter="\t")
    if reader.fieldnames != BASELINE_COLUMNS:
        fail(f"unexpected API-baseline columns: {reader.fieldnames}")
    baselines = list(reader)

actual_baseline_ids = {row["id"] for row in baselines}
if actual_baseline_ids != set(EXPECTED_BASELINES):
    fail(f"API baseline IDs changed: {sorted(actual_baseline_ids)}")

for row in baselines:
    expected = EXPECTED_BASELINES[row["id"]]
    actual = (
        row["package"], row["version"], row["source_ref"], row["commit"],
        row["api_files"], row["archive_sha256"],
    )
    if actual != expected:
        fail(f"{row['id']} identity or digest differs from the reviewed baseline")
    if not re.fullmatch(r"[0-9a-f]{40}", row["commit"]):
        fail(f"{row['id']} has a malformed commit")
    if not re.fullmatch(r"[0-9a-f]{64}", row["archive_sha256"]):
        fail(f"{row['id']} has a malformed archive digest")
    files = row["api_files"].split(";")
    if len(files) != len(set(files)) or any(not file_name for file_name in files):
        fail(f"{row['id']} has duplicate or empty API files")

print(
    f"Validated {len(rows)} dictionary-surface obligations, "
    f"{len(properties)} required-red properties, and {len(baselines)} API baselines."
)
