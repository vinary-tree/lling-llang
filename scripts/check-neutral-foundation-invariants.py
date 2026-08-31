#!/usr/bin/env python3
"""Generate or validate the exhaustive E9 neutral-foundation traceability ledger."""

from __future__ import annotations

import argparse
import csv
import hashlib
import re
import subprocess
import sys
from pathlib import Path

sys.dont_write_bytecode = True

from neutral_foundation_mutants import MUTATIONS

ROOT = Path(__file__).resolve().parents[1]
ROCQ = "proofs/coq/domain_integration/NeutralFoundationContracts.v"
TLA = "proofs/tla/NeutralFoundationLifecycle.tla"
TLA_CONFIG = ROOT / "proofs/tla/MC/NeutralFoundationLifecycle.cfg"
SMT = "proofs/smt/vco-e9-neutral-foundations.smt2"
REGISTRY = ROOT / "proofs/doc/neutral-foundation-invariants.tsv"
BASELINES = ROOT / "proofs/doc/neutral-foundation-api-baselines.tsv"
RED_ROOT = "proofs/required_red/neutral_foundations/tests"
IDENTITY_SUITE = "proofs/required_red/content_identity/tests/content_identity.rs"
COLUMNS = [
    "id", "area", "invariant", "formalism", "artifact", "formal_symbol",
    "proof_strength", "implementation_owner", "property_suite", "property_name",
    "implementation_state", "model_evidence",
]
BASELINE_COLUMNS = [
    "id", "repository", "state", "branch", "commit", "api_files",
    "aggregate_sha256", "protection",
]


def fail(message: str) -> None:
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def words(symbol: str) -> str:
    value = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", symbol)
    return value.replace("_", " ").replace("-", " ").lower()


def snake(symbol: str) -> str:
    value = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", symbol)
    return value.replace("-", "_").lower()


def implementation_state(owner: str) -> str:
    if owner in {"vinary-requirements", "vinary-doc-lint"}:
        return "required-red-source; execution-blocked-by-protected-baseline"
    return "required-red-before-production"


COQ_GROUPS = [
    ("canonical-wire", "vinary-canonical-json", "canonical_wire.rs", "non_finite_numbers_are_rejected", "malformed_and_budget_outcomes_are_not_success"),
    ("analysis-graph", "vinary-analysis-graph", "analysis_graph.rs", "graph_epistemic_axes_are_orthogonal", "jsonl_limit_exhaustion_is_not_completion"),
    ("runtime", "vinary-runtime", "runtime.rs", "incomplete_result_is_not_cacheable", "process_termination_native_stack_is_constant"),
    ("requirements", "vinary-requirements", "requirements.rs", "revision_preserves_stable_requirement_identity", "history_validation_uses_constant_native_stack"),
    ("assurance", "vinary-assurance", "assurance.rs", "statistics_do_not_discharge_theorem_obligations", "inapplicable_evidence_cannot_verify"),
    ("documentation", "vinary-doc-lint", "documentation.rs", "changed_source_marks_generated_asset_stale", "documentation_traversal_uses_constant_native_stack"),
]

TLA_PROPERTIES = {
    "TypeOK": ("lifecycle", "vinary-runtime", "lifecycle.rs", "prop_type_ok"),
    "NamedProfileIsNotRfc8785": ("canonical-wire", "vinary-canonical-json", "lifecycle.rs", "prop_named_profile_is_not_rfc8785"),
    "WireAndContentIdentityDomainsAreSeparate": ("wire-identity", "vinary-content-identity", "content_identity.rs", "prop_wire_and_content_identity_domains_are_separate"),
    "ProjectionNeverStrengthens": ("analysis-graph", "vinary-analysis-graph", "analysis_graph.rs", "prop_projection_never_strengthens_lifecycle"),
    "PatchCommitRequiresMatchingBase": ("analysis-graph", "vinary-analysis-graph", "analysis_graph.rs", "prop_patch_commit_requires_matching_base"),
    "IncompleteNeverEntersCache": ("runtime", "vinary-runtime", "runtime.rs", "prop_incomplete_never_enters_cache"),
    "RuntimeReleaseRequiresExactCompleteLockedInputs": ("runtime", "vinary-runtime", "runtime.rs", "prop_runtime_release_requires_exact_complete_locked_inputs"),
    "OverflowSpillsOnlyToRepositoryStorage": ("runtime", "vinary-runtime", "runtime.rs", "prop_overflow_spills_only_to_repository_storage"),
    "ResumeRequiresCompatibleCheckpoint": ("runtime", "vinary-runtime", "runtime.rs", "prop_resume_requires_compatible_checkpoint"),
    "TombstonesAreNotActive": ("requirements", "vinary-requirements", "requirements.rs", "prop_tombstones_are_not_active"),
    "SourceAccountingNeverDropsUnclassifiedText": ("requirements", "vinary-requirements", "requirements.rs", "prop_source_accounting_never_drops_unclassified_text"),
    "StatisticsNeverDischargeTheoremObligations": ("assurance", "vinary-assurance", "assurance.rs", "prop_statistics_never_discharge_theorem_obligations"),
    "StaleEvidenceCannotVerify": ("assurance", "vinary-assurance", "assurance.rs", "prop_stale_evidence_cannot_verify"),
    "VerifiedAssuranceRequiresNegativeControl": ("assurance", "vinary-assurance", "assurance.rs", "prop_verified_assurance_requires_negative_control_lifecycle"),
    "VerifiedAssuranceRequiresRevisionAttestation": ("assurance", "vinary-assurance", "assurance.rs", "prop_verified_assurance_requires_revision_attestation"),
    "CheckOnlyLintNeverMutatesDocumentation": ("documentation", "vinary-doc-lint", "documentation.rs", "prop_check_only_lint_never_mutates_documentation"),
    "StaleManifestCannotPassLint": ("documentation", "vinary-doc-lint", "documentation.rs", "prop_stale_manifest_cannot_pass_lint"),
    "ReleaseRequiresEveryNeutralFoundationGate": ("lifecycle", "vinary-runtime", "lifecycle.rs", "prop_release_requires_every_neutral_foundation_gate"),
    "NativeStackBoundIsInputIndependent": ("stack-safety", "multi-foundation", "lifecycle.rs", "prop_native_stack_bound_is_input_independent"),
    "EventuallyTerminal": ("lifecycle", "multi-foundation", "lifecycle.rs", "prop_eventually_terminal"),
}

TLA_MUTANTS = {
    "TypeOK": "type-ok",
    "NamedProfileIsNotRfc8785": "named-profile",
    "WireAndContentIdentityDomainsAreSeparate": "identity-domains",
    "ProjectionNeverStrengthens": "projection-strength",
    "PatchCommitRequiresMatchingBase": "patch-base",
    "IncompleteNeverEntersCache": "incomplete-cache",
    "RuntimeReleaseRequiresExactCompleteLockedInputs": "release-locks",
    "OverflowSpillsOnlyToRepositoryStorage": "repository-spill",
    "ResumeRequiresCompatibleCheckpoint": "checkpoint-resume",
    "TombstonesAreNotActive": "tombstone-active",
    "SourceAccountingNeverDropsUnclassifiedText": "source-accounting",
    "StatisticsNeverDischargeTheoremObligations": "statistics-theorem",
    "StaleEvidenceCannotVerify": "stale-evidence",
    "VerifiedAssuranceRequiresNegativeControl": "negative-control",
    "VerifiedAssuranceRequiresRevisionAttestation": "revision-attestation",
    "CheckOnlyLintNeverMutatesDocumentation": "check-only-mutation",
    "StaleManifestCannotPassLint": "stale-manifest",
    "ReleaseRequiresEveryNeutralFoundationGate": "release-gates",
    "NativeStackBoundIsInputIndependent": "native-stack",
    "EventuallyTerminal": "eventually-terminal",
}


def coq_target(symbol: str, ordered: list[str]) -> tuple[str, str, str]:
    position = ordered.index(symbol)
    for area, owner, suite, first, last in COQ_GROUPS:
        if ordered.index(first) <= position <= ordered.index(last):
            if symbol == "schema_and_content_identity_are_distinct":
                return "wire-identity", "vinary-content-identity", "content_identity.rs"
            return area, owner, suite
    fail(f"unassigned Rocq theorem {symbol}")


def smt_target(symbol: str) -> tuple[str, str, str]:
    suffix = symbol.removeprefix("E9-NF-SMT-")
    if suffix in {"IDENTITY-DOMAINS-SEPARATE", "NONFINITE-NUMBER-REJECTED", "SINK-REJECTION-ATOMIC"}:
        owner = "vinary-content-identity" if suffix == "IDENTITY-DOMAINS-SEPARATE" else "vinary-canonical-json"
        suite = "content_identity.rs" if suffix == "IDENTITY-DOMAINS-SEPARATE" else "canonical_wire.rs"
        return "canonical-wire", owner, suite
    if suffix in {"PROJECTION-NONSTRENGTHENING", "PATCH-BASE-GATE"}:
        return "analysis-graph", "vinary-analysis-graph", "analysis_graph.rs"
    if suffix in {"INCOMPLETE-NOT-CACHEABLE", "EXACT-RELEASE-LOCKS-ALL-INPUTS", "OVERFLOW-SPILLS-TO-REPOSITORY", "RESUME-REQUIRES-COMPATIBLE-CHECKPOINT"}:
        return "runtime", "vinary-runtime", "runtime.rs"
    if suffix in {"TOMBSTONE-NOT-ACTIVE", "UNCLASSIFIED-SOURCE-RETAINED"}:
        return "requirements", "vinary-requirements", "requirements.rs"
    if suffix in {"STATISTICS-NOT-THEOREM", "STALE-EVIDENCE-NOT-VERIFIED", "NEGATIVE-CONTROL-REQUIRED", "ATTESTATION-REVISION-REQUIRED"}:
        return "assurance", "vinary-assurance", "assurance.rs"
    if suffix in {"STALE-MANIFEST-NOT-LINTED", "CHECK-ONLY-NONMUTATING"}:
        return "documentation", "vinary-doc-lint", "documentation.rs"
    if suffix in {"RELEASE-REQUIRES-EVERY-GATE", "NATIVE-STACK-CONSTANT", "VALID-EXACT-RELEASE-WITNESS", "VALID-COMPLETE-APPROXIMATE-CACHE-WITNESS"}:
        return "lifecycle", "multi-foundation", "lifecycle.rs"
    fail(f"unassigned SMT obligation {symbol}")


def expected_rows() -> list[dict[str, str]]:
    coq_text = (ROOT / ROCQ).read_text(encoding="utf-8")
    coq = re.findall(r"^(?:Theorem|Lemma)\s+([A-Za-z0-9_']+)", coq_text, re.MULTILINE)
    tla = re.findall(r"^(?:INVARIANT|PROPERTY)\s+([A-Za-z0-9_]+)", TLA_CONFIG.read_text(encoding="utf-8"), re.MULTILINE)
    smt = re.findall(r"^; (E9-NF-SMT-[A-Z0-9-]+)\b", (ROOT / SMT).read_text(encoding="utf-8"), re.MULTILINE)
    if set(tla) != set(TLA_PROPERTIES):
        fail(f"TLA property routing is stale: {sorted(set(tla) ^ set(TLA_PROPERTIES))}")
    if set(tla) != set(TLA_MUTANTS):
        fail(f"TLA mutation routing is stale: {sorted(set(tla) ^ set(TLA_MUTANTS))}")
    built_mutants = {mutation.target: name for name, mutation in MUTATIONS.items()}
    if built_mutants != {target: name for target, name in TLA_MUTANTS.items()}:
        fail("TLA mutation builders differ from the exhaustive ledger routing")
    tla_text = (ROOT / TLA).read_text(encoding="utf-8")
    for name, mutation in MUTATIONS.items():
        if tla_text.count(mutation.needle) != 1:
            fail(f"TLA mutant {name} no longer has exactly one causal injection site")

    rows: list[dict[str, str]] = []
    for index, symbol in enumerate(coq, 1):
        area, owner, suite = coq_target(symbol, coq)
        rows.append({
            "id": f"E9-NF-COQ-{index:03d}", "area": area, "invariant": words(symbol),
            "formalism": "rocq", "artifact": ROCQ, "formal_symbol": symbol,
            "proof_strength": "rocq-unbounded", "implementation_owner": owner,
            "property_suite": IDENTITY_SUITE if suite == "content_identity.rs" else f"{RED_ROOT}/{suite}", "property_name": f"prop_{symbol}",
            "implementation_state": implementation_state(owner),
            "model_evidence": "rocq-proof-term",
        })
    for index, symbol in enumerate(tla, 1):
        area, owner, suite, prop = TLA_PROPERTIES[symbol]
        rows.append({
            "id": f"E9-NF-TLA-{index:03d}", "area": area, "invariant": words(symbol),
            "formalism": "tla+", "artifact": TLA, "formal_symbol": symbol,
            "proof_strength": "tlc-finite-exhaustive", "implementation_owner": owner,
            "property_suite": IDENTITY_SUITE if suite == "content_identity.rs" else f"{RED_ROOT}/{suite}", "property_name": prop,
            "implementation_state": implementation_state(owner),
            "model_evidence": f"tlc-mutant:{TLA_MUTANTS[symbol]}",
        })
    for index, symbol in enumerate(smt, 1):
        area, owner, suite = smt_target(symbol)
        rows.append({
            "id": f"E9-NF-SMT-{index:03d}", "area": area, "invariant": words(symbol),
            "formalism": "smt", "artifact": SMT, "formal_symbol": symbol,
            "proof_strength": "z3-finite-boundary", "implementation_owner": owner,
            "property_suite": IDENTITY_SUITE if suite == "content_identity.rs" else f"{RED_ROOT}/{suite}", "property_name": f"prop_{snake(symbol)}",
            "implementation_state": implementation_state(owner),
            "model_evidence": "z3-positive-witness" if "-VALID-" in symbol else "z3-unsat-negative-control",
        })
    return rows


def write_registry(rows: list[dict[str, str]]) -> None:
    with REGISTRY.open("w", encoding="utf-8", newline="") as destination:
        writer = csv.DictWriter(destination, fieldnames=COLUMNS, delimiter="\t", lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


EXPECTED_BASELINES = {
    "BASE-CANONICAL-UNBORN": ("vinary-canonical-json", "unborn-protected", "main", "NONE", "Cargo.toml;src/lib.rs;src/budget.rs;src/encode.rs;src/parse.rs;src/tape.rs;crates/vinary-wire-schema/src/lib.rs", "4c45ea1449cbe5f837b60fae5e809f2d4ed80f19595a4241008b121369a85008"),
    "BASE-GRAPH-UNBORN": ("vinary-analysis-graph", "unborn-protected", "main", "NONE", "Cargo.toml;src/lib.rs;src/dialect.rs;src/interchange.rs;src/patch.rs;src/validation.rs", "92d2e30b49e2f5b8fe1e2144b8b510a41f0dede51f7eca52e1f1ea9f4c2f3104"),
    "BASE-RUNTIME-UNBORN": ("vinary-runtime", "unborn-protected", "main", "NONE", "Cargo.toml;src/lib.rs;src/outcome.rs;src/cache.rs;src/process.rs;src/protocol.rs;src/artifact.rs", "779638ac0080c8db3569e983dd8e0f070f8c55ef1c493626a8cfe8c54e69d7b5"),
    "BASE-REQUIREMENTS-UNBORN": ("vinary-requirements", "unborn-protected", "main", "NONE", "Cargo.toml;src/lib.rs;docs/source-accounting.md", "002b8b24216d508880ddcb6d047ff10aa77c66aa535df3730e2c306eeed6cba9"),
    "BASE-ASSURANCE-UNBORN": ("vinary-assurance", "unborn-protected", "main", "NONE", "Cargo.toml;src/lib.rs;src/wire.rs;docs/epistemology.md", "7e1846c614e22ae3d445533947e7657777b214636754f1df91886c3197191285"),
    "BASE-DOCLINT-DIRTY": ("vinary-doc-lint", "committed-dirty-protected", "main", "1c77ea8965f586dbb4c26bd8b645a95921ff8780", "Cargo.toml;crates/vinary-doc-lint/Cargo.toml;crates/vinary-doc-lint/src/model.rs;crates/vinary-doc-lint/src/diagram.rs;crates/vinary-doc-lint/src/rule/mod.rs;docs/formal-assurance.md;formal/traceability.toml", "f6aef240fbd155de58d5f3983da20d661cd51990b37bf032220d5ecbf4ffcd02"),
    "BASE-CONTENT-IDENTITY-ABSENT": ("vinary-content-identity", "absent-required-red", "NONE", "NONE", "Cargo.toml;src/lib.rs", "ABSENT"),
}


def validate_baselines() -> int:
    with BASELINES.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source, delimiter="\t")
        if reader.fieldnames != BASELINE_COLUMNS:
            fail(f"unexpected API baseline columns: {reader.fieldnames}")
        rows = list(reader)
    if {row["id"] for row in rows} != set(EXPECTED_BASELINES):
        fail("API baseline identity set changed")
    for row in rows:
        expected = EXPECTED_BASELINES[row["id"]]
        actual = tuple(row[column] for column in BASELINE_COLUMNS[1:7])
        if actual != expected:
            fail(f"{row['id']} differs from the reviewed API baseline")
        if row["protection"] != "read-only; ownership handoff required before implementation":
            fail(f"{row['id']} weakens protected prototype ownership")
        repository = ROOT.parent / row["repository"]
        if row["state"] == "absent-required-red":
            if repository.exists():
                fail(f"{row['id']} is no longer absent and requires a new ownership review")
            continue
        if not repository.is_dir():
            fail(f"{row['id']} protected repository is absent")
        branch = subprocess.run(
            ["git", "branch", "--show-current"],
            cwd=repository,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        if branch != row["branch"]:
            fail(f"{row['id']} branch changed from the reviewed baseline")
        head = subprocess.run(
            ["git", "rev-parse", "--verify", "HEAD"],
            cwd=repository,
            check=False,
            capture_output=True,
            text=True,
        )
        actual_commit = head.stdout.strip() if head.returncode == 0 else "NONE"
        if actual_commit != row["commit"]:
            fail(f"{row['id']} commit changed from the reviewed baseline")
        aggregate_input = bytearray()
        for relative in row["api_files"].split(";"):
            source = repository / relative
            if not source.is_file():
                fail(f"{row['id']} API file is absent: {relative}")
            digest = hashlib.sha256(source.read_bytes()).hexdigest()
            aggregate_input.extend(f"{digest}  {relative}\n".encode())
        aggregate = hashlib.sha256(aggregate_input).hexdigest()
        if aggregate != row["aggregate_sha256"]:
            fail(f"{row['id']} protected API surface changed from the reviewed baseline")
    return len(rows)


def validate_registry(expected: list[dict[str, str]]) -> None:
    with REGISTRY.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source, delimiter="\t")
        if reader.fieldnames != COLUMNS:
            fail(f"unexpected invariant columns: {reader.fieldnames}")
        actual = list(reader)
    if actual != expected:
        fail("neutral-foundation invariant ledger is stale; run this script with --write")
    property_keys: set[tuple[str, str]] = set()
    for row in actual:
        artifact = ROOT / row["artifact"]
        suite = ROOT / row["property_suite"]
        if not artifact.is_file() or row["formal_symbol"] not in artifact.read_text(encoding="utf-8"):
            fail(f"{row['id']} references absent formal evidence")
        if not suite.is_file():
            fail(f"{row['id']} references absent property suite")
        key = (row["property_suite"], row["property_name"])
        if key in property_keys:
            fail(f"duplicate property mapping {key}")
        property_keys.add(key)
        if not re.search(rf"\bfn\s+{re.escape(row['property_name'])}\b", suite.read_text(encoding="utf-8")):
            fail(f"{row['id']} property {row['property_name']} is absent")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true", help="rewrite the deterministic ledger")
    args = parser.parse_args()
    rows = expected_rows()
    if args.write:
        write_registry(rows)
    validate_registry(rows)
    baselines = validate_baselines()
    manifests = [
        ROOT / "proofs/required_red/neutral_foundations/Cargo.toml",
        ROOT / "proofs/required_red/content_identity/Cargo.toml",
    ]
    if any(not manifest.is_file() for manifest in manifests):
        fail("required-red Cargo manifests are absent")
    if "vinary-content-identity" not in manifests[1].read_text(encoding="utf-8"):
        fail("domain-separated identity was not retargeted to its neutral leaf")
    print(f"Validated {len(rows)} exhaustive obligations, {len(rows)} required-red properties, and {baselines} protected API baselines.")


if __name__ == "__main__":
    main()
