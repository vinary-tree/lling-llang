#!/usr/bin/env python3
"""Validate the exhaustive lling-llang stack-safety disposition ledger.

The ledger is a closed-world acceptance artifact: 91 pgmcp/libcpg direct
recursion candidates, five mutual strongly connected components (SCCs), and
41 recursive owned-type lifecycles.  This checker rejects row loss, discovery
drift, empty assurance fields, unapproved dispositions, weak deep-test
evidence, missing referenced artifacts, and non-immutable final revisions.

During development only, ``--allow-worktree`` admits the explicit
``WORKTREE_DERIVED_FROM_...`` revision marker.  Final acceptance deliberately
has no equivalent escape hatch: every row must name a 40-hex Git commit.

The ``--self-test`` mode exercises causal in-memory mutants and never writes a
temporary file.  The script is standard-library-only.
"""

from __future__ import annotations

import argparse
import copy
import csv
import hashlib
import io
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "proofs" / "doc" / "stack-safety-dispositions.tsv"

COLUMNS = [
    "id",
    "kind",
    "discovery_run",
    "baseline_revision",
    "baseline_file",
    "baseline_line",
    "symbol",
    "scc",
    "feature_profile",
    "reachability",
    "depth_driver",
    "observable_order",
    "sharing_cycle_policy",
    "cancellation_cap",
    "baseline_stack_risk",
    "machine",
    "shallow_oracle",
    "formal_obligations",
    "property_laws",
    "deep_wide_lifecycle_test",
    "complexity",
    "allocation_plan",
    "final_revision",
    "disposition",
    "source_evidence",
]

BASELINE = "bc26797854444aa0ec38d06bd3ba991a79bd3da7"
RUN = "44bdd341-48f4-46bf-8829-92988abf55b1"
DISCOVERY_DIGEST = "3dab5e67ab143d57957522816eb736ff459a67a93ca34832149193e4ffdf7df9"
DISCOVERY_FIELDS = (
    "id",
    "kind",
    "discovery_run",
    "baseline_revision",
    "baseline_file",
    "baseline_line",
    "symbol",
    "scc",
    "source_evidence",
)
FINAL_REVISION = re.compile(r"^[0-9a-f]{40}$")
WORKTREE_REVISION = re.compile(r"^WORKTREE_DERIVED_FROM_[0-9a-f]{40}$")
ARTIFACT = re.compile(r"(?:src|tests|scripts)/[^\s:+]+\.(?:rs|py)")
FORMAL_ARTIFACT = re.compile(r"[A-Za-z][A-Za-z0-9_-]*\.(?:v|tla|smt2)")
FORBIDDEN = re.compile(r"\b(?:TODO|TBD|FIXME|placeholder|stub|unknown)\b", re.IGNORECASE)

FALSE_DIRECT = {
    "D002",
    *{f"D{index:03d}" for index in range(4, 16)},
    "D018",
    "D020",
    "D022",
    "D023",
    *{f"D{index:03d}" for index in range(27, 38)},
    "D050",
    "D054",
    "D055",
    "D062",
    "D074",
    "D081",
    "D084",
    "D086",
}
FALSE_IDS = FALSE_DIRECT | {"S001"}
FIXED_IDS = {"D003"}
EXTERNAL_IDS = {"L021", "L022"}
DISPOSITIONS = {
    "flattened",
    "fixed-bound-with-proof",
    "false-positive-with-source-evidence",
    "external-boundary",
    "test-only-oracle",
}


def expected_ids() -> set[str]:
    return (
        {f"D{index:03d}" for index in range(1, 92)}
        | {f"S{index:03d}" for index in range(1, 6)}
        | {f"L{index:03d}" for index in range(1, 42)}
    )


def parse_ledger(text: str) -> tuple[list[dict[str, str]], list[str]]:
    failures: list[str] = []
    material = "\n".join(line for line in text.splitlines() if not line.startswith("#"))
    reader = csv.DictReader(io.StringIO(material), delimiter="\t")
    if reader.fieldnames != COLUMNS:
        failures.append(f"header is {reader.fieldnames!r}, expected {COLUMNS!r}")
        return [], failures
    rows: list[dict[str, str]] = []
    for line_number, row in enumerate(reader, start=6):
        if None in row:
            failures.append(f"line {line_number}: excess cells {row[None]!r}")
            continue
        rows.append({column: row[column] for column in COLUMNS})
    return rows, failures


def discovery_digest(rows: list[dict[str, str]]) -> str:
    payload = "\n".join(
        "\t".join(row[field] for field in DISCOVERY_FIELDS) for row in rows
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def referenced_paths(value: str) -> set[str]:
    return set(ARTIFACT.findall(value))


def validate_rows(
    rows: list[dict[str, str]],
    *,
    allow_worktree: bool,
    verify_paths: bool = True,
) -> list[str]:
    failures: list[str] = []
    ids = [row["id"] for row in rows]
    actual_ids = set(ids)
    required_ids = expected_ids()
    if len(ids) != len(actual_ids):
        duplicates = sorted({row_id for row_id in ids if ids.count(row_id) > 1})
        failures.append(f"duplicate ids: {duplicates}")
    if actual_ids != required_ids:
        failures.append(
            f"id closure differs: missing={sorted(required_ids - actual_ids)}, "
            f"extra={sorted(actual_ids - required_ids)}"
        )
    if len(rows) != 137:
        failures.append(f"row count is {len(rows)}, expected 137")
    digest = discovery_digest(rows)
    if digest != DISCOVERY_DIGEST:
        failures.append(f"discovery projection digest is {digest}, expected {DISCOVERY_DIGEST}")

    for row in rows:
        row_id = row["id"]
        for column in COLUMNS:
            value = row[column].strip()
            if not value:
                failures.append(f"{row_id}: empty {column}")
            if FORBIDDEN.search(value):
                failures.append(f"{row_id}: forbidden incompletion marker in {column}")

        expected_kind = (
            "direct" if row_id.startswith("D") else
            "mutual-scc" if row_id.startswith("S") else
            "owned-lifecycle"
        )
        if row["kind"] != expected_kind:
            failures.append(f"{row_id}: kind {row['kind']!r}, expected {expected_kind!r}")
        expected_run = RUN if expected_kind != "owned-lifecycle" else "rust-analyzer-213-file-type-audit"
        if row["discovery_run"] != expected_run:
            failures.append(f"{row_id}: unexpected discovery_run")
        if row["baseline_revision"] != BASELINE:
            failures.append(f"{row_id}: unexpected baseline revision")
        try:
            if int(row["baseline_line"]) <= 0:
                raise ValueError
        except ValueError:
            failures.append(f"{row_id}: baseline_line is not positive")

        disposition = row["disposition"]
        if disposition not in DISPOSITIONS:
            failures.append(f"{row_id}: unapproved disposition {disposition!r}")
        required_disposition = (
            "false-positive-with-source-evidence" if row_id in FALSE_IDS else
            "fixed-bound-with-proof" if row_id in FIXED_IDS else
            "external-boundary" if row_id in EXTERNAL_IDS else
            "flattened"
        )
        if disposition != required_disposition:
            failures.append(
                f"{row_id}: disposition {disposition!r}, expected {required_disposition!r}"
            )

        revision = row["final_revision"]
        if not FINAL_REVISION.fullmatch(revision):
            if not (allow_worktree and WORKTREE_REVISION.fullmatch(revision)):
                failures.append(f"{row_id}: final_revision is not an immutable commit")

        if disposition == "flattened" and "deep" not in row["deep_wide_lifecycle_test"].lower():
            failures.append(f"{row_id}: flattened row lacks explicit deep acceptance evidence")
        if disposition == "false-positive-with-source-evidence":
            if "not applicable: no recursion" != row["deep_wide_lifecycle_test"]:
                failures.append(f"{row_id}: false positive claims a recursion test")
            if "source" not in row["formal_obligations"]:
                failures.append(f"{row_id}: false positive lacks source proof")
        if disposition == "fixed-bound-with-proof":
            if "rank" not in row["formal_obligations"] or "<=2" not in row["complexity"]:
                failures.append(f"{row_id}: fixed bound lacks rank/depth proof")

        if verify_paths:
            baseline_path = ROOT / row["baseline_file"].split("|")[0]
            if not baseline_path.is_file():
                failures.append(f"{row_id}: baseline file missing: {baseline_path.relative_to(ROOT)}")
            for field in ("shallow_oracle", "deep_wide_lifecycle_test"):
                for relative in referenced_paths(row[field]):
                    if not (ROOT / relative).is_file():
                        failures.append(f"{row_id}: missing {field} path {relative}")
            for name in FORMAL_ARTIFACT.findall(row["formal_obligations"]):
                matches = list((ROOT / "proofs").rglob(name))
                if len(matches) != 1:
                    failures.append(
                        f"{row_id}: formal artifact {name} resolves {len(matches)} times"
                    )
    return failures


def accepted_copy(rows: list[dict[str, str]]) -> list[dict[str, str]]:
    accepted = copy.deepcopy(rows)
    for row in accepted:
        row["final_revision"] = "a" * 40
        if row["disposition"] == "flattened" and "deep" not in row["deep_wide_lifecycle_test"].lower():
            row["deep_wide_lifecycle_test"] = "deep acceptance: " + row["deep_wide_lifecycle_test"]
    return accepted


def self_test(rows: list[dict[str, str]]) -> list[str]:
    failures: list[str] = []
    accepted = accepted_copy(rows)
    if messages := validate_rows(accepted, allow_worktree=False, verify_paths=True):
        failures.append(f"positive control failed: {messages}")

    mutants: list[tuple[str, list[dict[str, str]], str]] = []
    mutants.append(("missing row", accepted[:-1], "id closure differs"))
    duplicate = copy.deepcopy(accepted)
    duplicate.append(copy.deepcopy(duplicate[0]))
    mutants.append(("duplicate row", duplicate, "duplicate ids"))
    blank = copy.deepcopy(accepted)
    blank[0]["machine"] = ""
    mutants.append(("blank assurance", blank, "empty machine"))
    drift = copy.deepcopy(accepted)
    drift[0]["baseline_file"] = "src/lib.rs"
    mutants.append(("discovery drift", drift, "discovery projection digest"))
    wrong_disposition = copy.deepcopy(accepted)
    wrong_disposition[0]["disposition"] = "external-boundary"
    mutants.append(("wrong disposition", wrong_disposition, "expected 'flattened'"))
    weak = copy.deepcopy(accepted)
    weak[0]["deep_wide_lifecycle_test"] = "shallow only"
    mutants.append(("weak evidence", weak, "lacks explicit deep"))
    mutable_revision = copy.deepcopy(accepted)
    mutable_revision[0]["final_revision"] = "WORKTREE_DERIVED_FROM_" + BASELINE
    mutants.append(("mutable revision", mutable_revision, "not an immutable commit"))

    for name, mutant, expected in mutants:
        messages = validate_rows(mutant, allow_worktree=False, verify_paths=False)
        if not any(expected in message for message in messages):
            failures.append(f"{name} mutant survived; messages={messages}")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--allow-worktree", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if not LEDGER.is_file():
        print(f"check-stack-safety-dispositions: missing {LEDGER.relative_to(ROOT)}")
        return 1
    rows, failures = parse_ledger(LEDGER.read_text(encoding="utf-8"))
    if args.self_test and not failures:
        failures.extend(self_test(rows))
    if not args.self_test:
        failures.extend(validate_rows(rows, allow_worktree=args.allow_worktree))

    if failures:
        print(f"check-stack-safety-dispositions: {len(failures)} failure(s)")
        for failure in failures:
            print(f"  FAIL {failure}")
        return 1
    mode = "self-tests" if args.self_test else "ledger rows"
    print(f"check-stack-safety-dispositions: 137 {mode} verified")
    return 0


if __name__ == "__main__":
    sys.exit(main())
