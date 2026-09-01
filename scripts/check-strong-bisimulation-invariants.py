#!/usr/bin/env python3
"""Generate and validate exhaustive E4 strong-bisimulation traceability."""

from __future__ import annotations

import argparse
import csv
import re
import sys
from pathlib import Path

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[1]
ROCQ = "proofs/coq/algorithms/StrongBisimulation.v"
TLA = "proofs/tla/StrongBisimulationLifecycle.tla"
TLA_CONFIGS = (
    "proofs/tla/MC/StrongBisimulationValid.cfg",
    "proofs/tla/MC/StrongBisimulationInvalidSource.cfg",
    "proofs/tla/MC/StrongBisimulationInvalidTarget.cfg",
    "proofs/tla/MC/StrongBisimulationInvalidLabel.cfg",
)
SMT = "proofs/smt/vco-e4-strong-bisimulation.smt2"
SMT_EXPECTED = ROOT / "proofs/smt/vco-e4-strong-bisimulation.expected"
PROPERTY_SUITE = "proofs/required_red/strong_bisimulation/tests/contracts.rs"
REGISTRY = ROOT / "proofs/doc/strong-bisimulation-invariants.tsv"
COLUMNS = [
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
    "model_evidence",
]

P_EXHAUSTIVE = "prop_exhaustive_small_system_matches_independent_fixed_point"
P_SOURCE = "prop_malformed_source_is_rejected"
P_TARGET = "prop_malformed_target_is_rejected"
P_VECTOR = "prop_color_vector_length_is_total"
P_PERMUTE = "prop_transition_permutation_preserves_canonical_relation"
P_DUPLICATE = "prop_duplicate_transitions_preserve_canonical_relation"
P_RELABEL = "prop_injective_label_relabeling_preserves_relation"
P_CERTIFICATE = "prop_certificate_replay_reconstructs_exact_partition"
P_WITNESS = "prop_non_equivalent_pair_has_sound_distinguishing_witness"
P_ADVERSARIAL = "prop_adversarial_discrete_partition_has_no_whole_rescan"
P_RESOURCES = "prop_resource_account_respects_quasilinear_work_and_linear_heap"
P_STACK = "prop_deep_chain_is_stack_safe_on_small_native_stack"
P_EMPTY = "prop_empty_lts_is_valid_and_canonical"


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def snake(symbol: str) -> str:
    value = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", symbol)
    return value.replace("-", "_").replace("'", "").lower()


def words(symbol: str) -> str:
    return snake(symbol).replace("_", " ")


def rocq_property(symbol: str) -> str:
    lowered = symbol.lower()
    if "canonical_relation_matrix_outer_length" in lowered:
        return P_EMPTY
    if "malformed_source" in lowered or "source_is_indexable" in lowered:
        return P_SOURCE
    if (
        "malformed_target" in lowered
        or "target_is_indexable" in lowered
        or "malformed_endpoint" in lowered
    ):
        return P_TARGET
    if (
        "vector" in lowered
        or "short_" in lowered
        or "long_" in lowered
        or "query_index" in lowered
    ):
        return P_VECTOR
    if "edge_valid" in lowered:
        return P_DUPLICATE
    if "canonical" in lowered or "relabel" in lowered:
        return P_RELABEL
    if "certificate" in lowered or "replayed_" in lowered:
        return P_CERTIFICATE
    if any(
        marker in lowered
        for marker in (
            "formula",
            "witness",
            "saturation",
            "saturated",
            "preimage",
            "class_cert",
        )
    ):
        return P_WITNESS
    if "constant_native_stack" in lowered:
        return P_STACK
    if any(
        marker in lowered for marker in ("work_is_", "heap_is_", "dag_is_", "charge_")
    ):
        return P_RESOURCES
    if "strict_refinement" in lowered or "no_whole_partition_rescans" in lowered:
        return P_ADVERSARIAL
    return P_EXHAUSTIVE


TLA_PROPERTIES = {
    "TypeOK": P_EXHAUSTIVE,
    "InvalidInputRejected": P_SOURCE,
    "IndexedEndpointsValid": P_TARGET,
    "RelationRefinesColors": P_EXHAUSTIVE,
    "RelationIsReflexive": P_EXHAUSTIVE,
    "RelationIsSymmetric": P_EXHAUSTIVE,
    "HistorySound": P_CERTIFICATE,
    "HistoryChains": P_CERTIFICATE,
    "RefinementTerminates": P_ADVERSARIAL,
    "AcceptedStable": P_EXHAUSTIVE,
    "AcceptedMatchesOracle": P_EXHAUSTIVE,
    "CanonicalOutputExact": P_PERMUTE,
    "WitnessComplete": P_WITNESS,
    "WitnessSound": P_WITNESS,
    "EventuallyTerminal": P_STACK,
}


def smt_property(symbol: str) -> str:
    if "MALFORMED-SOURCE" in symbol:
        return P_SOURCE
    if "MALFORMED-TARGET" in symbol:
        return P_TARGET
    if "MALFORMED-LABEL" in symbol:
        return P_VECTOR
    if "VALID-EDGE" in symbol:
        return P_DUPLICATE
    if "SELF-LOOP" in symbol or "STABLE-PARTITION" in symbol:
        return P_EXHAUSTIVE
    if "CERTIFICATE" in symbol or "PROGRESS" in symbol:
        return P_CERTIFICATE
    if "WITNESS" in symbol:
        return P_WITNESS
    if "RELABELING" in symbol:
        return P_RELABEL
    if "STACK" in symbol:
        return P_STACK
    if "WORK" in symbol or "HEAP" in symbol or "RESOURCE" in symbol:
        return P_RESOURCES
    fail(f"SMT control lacks a property mapping: {symbol}")


def row(
    identifier: str,
    area: str,
    symbol: str,
    formalism: str,
    artifact: str,
    strength: str,
    property_name: str,
    evidence: str,
) -> dict[str, str]:
    return {
        "id": identifier,
        "area": area,
        "invariant": words(symbol),
        "formalism": formalism,
        "artifact": artifact,
        "formal_symbol": symbol,
        "proof_strength": strength,
        "property_suite": PROPERTY_SUITE,
        "property_name": property_name,
        "implementation_state": "required-red-before-production",
        "model_evidence": evidence,
    }


def expected_rows() -> list[dict[str, str]]:
    rocq_symbols = re.findall(
        r"^(?:Theorem|Lemma)\s+([A-Za-z0-9_']+)",
        (ROOT / ROCQ).read_text(encoding="utf-8"),
        re.MULTILINE,
    )
    if not rocq_symbols:
        fail("no Rocq theorems were discovered")

    tla_symbols: list[str] = []
    for config in TLA_CONFIGS:
        for symbol in re.findall(
            r"^(?:INVARIANT|PROPERTY)\s+([A-Za-z0-9_]+)",
            (ROOT / config).read_text(encoding="utf-8"),
            re.MULTILINE,
        ):
            if symbol not in tla_symbols:
                tla_symbols.append(symbol)
    if set(tla_symbols) != set(TLA_PROPERTIES):
        fail(
            "TLA registry mapping differs from configured checks: "
            f"missing={sorted(set(tla_symbols) - set(TLA_PROPERTIES))}, "
            f"stale={sorted(set(TLA_PROPERTIES) - set(tla_symbols))}"
        )

    smt_symbols = re.findall(
        r"^; (E4-BIS-SMT-[A-Z0-9-]+)\b",
        (ROOT / SMT).read_text(encoding="utf-8"),
        re.MULTILINE,
    )
    expected_verdicts = SMT_EXPECTED.read_text(encoding="utf-8").splitlines()
    if len(smt_symbols) != len(expected_verdicts):
        fail("SMT controls and expected verdicts have different lengths")

    rows: list[dict[str, str]] = []
    for index, symbol in enumerate(rocq_symbols, 1):
        rows.append(
            row(
                f"E4-BIS-ROCQ-{index:03d}",
                "unbounded-semantics",
                symbol,
                "rocq",
                ROCQ,
                "rocq-universal",
                rocq_property(symbol),
                "kernel-checked-proof-term",
            )
        )
    for index, symbol in enumerate(tla_symbols, 1):
        rows.append(
            row(
                f"E4-BIS-TLA-{index:03d}",
                "finite-lifecycle",
                symbol,
                "tla+",
                TLA,
                "tlc-finite-exhaustive",
                TLA_PROPERTIES[symbol],
                "four-config-exhaustive-state-space",
            )
        )
    for index, (symbol, verdict) in enumerate(
        zip(smt_symbols, expected_verdicts, strict=True),
        1,
    ):
        rows.append(
            row(
                f"E4-BIS-SMT-{index:03d}",
                "decidable-boundary",
                symbol,
                "smt",
                SMT,
                "z3-decidable-control",
                smt_property(symbol),
                f"z3-expected-{verdict}",
            )
        )
    return rows


def write_registry(rows: list[dict[str, str]]) -> None:
    with REGISTRY.open("w", encoding="utf-8", newline="") as destination:
        writer = csv.DictWriter(
            destination,
            fieldnames=COLUMNS,
            delimiter="\t",
            lineterminator="\n",
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
        fail(
            "strong-bisimulation invariant ledger is stale; "
            "run this script with --write"
        )

    ids = [entry["id"] for entry in actual]
    if len(ids) != len(set(ids)):
        fail("duplicate invariant identifier")
    for entry in actual:
        if not (ROOT / entry["artifact"]).is_file():
            fail(f"missing formal artifact: {entry['artifact']}")
        if not (ROOT / entry["property_suite"]).is_file():
            fail(f"missing property suite: {entry['property_suite']}")

    property_text = (ROOT / PROPERTY_SUITE).read_text(encoding="utf-8")
    defined = set(
        re.findall(r"^\s*fn\s+(prop_[A-Za-z0-9_]+)", property_text, re.MULTILINE)
    )
    referenced = {entry["property_name"] for entry in actual}
    if defined != referenced:
        fail(
            "required-red property coverage differs from the formal ledger: "
            f"unreferenced={sorted(defined - referenced)}, "
            f"missing={sorted(referenced - defined)}"
        )

    mutant_text = (ROOT / "scripts/strong_bisimulation_mutants.py").read_text(
        encoding="utf-8"
    )
    mutant_names = re.findall(
        r'^    "([a-z0-9-]+)": Mutation\(', mutant_text, re.MULTILINE
    )
    if len(mutant_names) != 10 or len(mutant_names) != len(set(mutant_names)):
        fail("the mutation registry must contain exactly ten unique controls")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--write",
        action="store_true",
        help="rewrite the canonical TSV before validating it",
    )
    args = parser.parse_args()
    expected = expected_rows()
    if args.write:
        write_registry(expected)
    validate_registry(expected)
    print(
        "Strong-bisimulation invariant ledger validated: "
        f"{len(expected)} formal rows, "
        f"{len({row['property_name'] for row in expected})} properties."
    )


if __name__ == "__main__":
    main()
