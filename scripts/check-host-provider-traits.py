#!/usr/bin/env python3
"""Verify exhaustive, evidence-backed classification of public Rust traits.

The registry covers every source-level ``pub trait`` declaration in this crate
and in its ``llattice`` path dependency.  A source-level inventory is
intentional: it also records private-module sealing traits, so moving a trait
into or out of the externally implementable surface cannot happen silently.

Usage:
  python3 scripts/check-host-provider-traits.py
  python3 scripts/check-host-provider-traits.py --require-complete
  python3 scripts/check-host-provider-traits.py --llattice-root PATH

The default llattice location is resolved from Cargo.toml.  ``LLATTICE_ROOT``
or ``--llattice-root`` may override it without embedding a workstation path.
"""

from __future__ import annotations

import argparse
import csv
import os
import re
import sys
from pathlib import Path

import tomllib


ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "bindings" / "conformance" / "host-provider-traits.tsv"
REQUIRED_COLUMNS = (
    "crate",
    "source",
    "trait",
    "disposition",
    "state",
    "customer_form",
    "evidence",
    "rationale",
)
ALLOWED_DISPOSITIONS = {
    "provider-capability",
    "provider-resource",
    "consumer-operation",
    "law-declaration",
    "rust-type-system",
    "internal-sealing",
}
ALLOWED_STATES = {"implemented", "missing"}
TRAIT_DECLARATION = re.compile(
    r"^[ \t]*pub(?:\([^)]*\))?[ \t]+(?:unsafe[ \t]+)?trait[ \t]+"
    r"([A-Za-z_][A-Za-z0-9_]*)",
    re.MULTILINE,
)


def fail(message: str) -> None:
    """Report one contract violation and stop without a traceback."""
    print(f"ERROR: {message}", file=sys.stderr)
    raise SystemExit(1)


def dependency_root(name: str, command_line: Path | None) -> Path:
    """Resolve a path dependency from an override or the package manifest."""
    if command_line is not None:
        candidate = command_line
    elif override := os.environ.get(f"{name.upper()}_ROOT"):
        candidate = Path(override)
    else:
        manifest = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        specification = manifest.get("dependencies", {}).get(name)
        if not isinstance(specification, dict) or not isinstance(
            specification.get("path"), str
        ):
            fail(f"Cargo.toml does not define {name!r} as a path dependency")
        candidate = ROOT / specification["path"]
    resolved = candidate.expanduser().resolve()
    if not (resolved / "Cargo.toml").is_file() or not (resolved / "src").is_dir():
        fail(f"{name} root is not a Rust package: {resolved}")
    return resolved


def discover_traits(crate: str, package_root: Path) -> set[tuple[str, str, str]]:
    """Return every source-level public-trait key in deterministic form."""
    traits: set[tuple[str, str, str]] = set()
    for path in sorted((package_root / "src").rglob("*.rs")):
        source = path.read_text(encoding="utf-8")
        relative = path.relative_to(package_root).as_posix()
        for trait in TRAIT_DECLARATION.findall(source):
            key = (crate, relative, trait)
            if key in traits:
                fail(f"duplicate trait declaration key: {key}")
            traits.add(key)
    return traits


def verify_evidence(line_number: int, evidence: str) -> None:
    """Require every semicolon-delimited path#needle claim to be reproducible."""
    claims = [claim.strip() for claim in evidence.split(";") if claim.strip()]
    if not claims:
        fail(f"line {line_number} has no implementation or design evidence")
    for claim in claims:
        relative, separator, needle = claim.partition("#")
        if not separator or not relative or not needle:
            fail(
                f"line {line_number} evidence must use path#needle entries: {claim!r}"
            )
        path = ROOT / relative
        if not path.is_file():
            fail(f"line {line_number} evidence file is missing: {relative}")
        if needle not in path.read_text(encoding="utf-8", errors="replace"):
            fail(
                f"line {line_number} evidence symbol/text {needle!r} "
                f"is absent from {relative}"
            )


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    parser.add_argument(
        "--require-complete",
        action="store_true",
        help="fail while any sensible customer-provider surface is missing",
    )
    parser.add_argument(
        "--llattice-root",
        type=Path,
        help="override the llattice package root",
    )
    arguments = parser.parse_args()

    llattice_root = dependency_root("llattice", arguments.llattice_root)
    discovered = discover_traits("lling-llang", ROOT)
    discovered |= discover_traits("llattice", llattice_root)

    if not REGISTRY.is_file():
        fail(f"trait registry is missing: {REGISTRY.relative_to(ROOT)}")
    with REGISTRY.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source, delimiter="\t")
        if tuple(reader.fieldnames or ()) != REQUIRED_COLUMNS:
            fail(f"unexpected registry columns: {reader.fieldnames}")
        rows = list(reader)

    registered: set[tuple[str, str, str]] = set()
    missing_surfaces: list[str] = []
    for line_number, row in enumerate(rows, start=2):
        empty = [column for column in REQUIRED_COLUMNS if not row[column].strip()]
        if empty:
            fail(f"line {line_number} has empty fields: {', '.join(empty)}")
        key = (row["crate"], row["source"], row["trait"])
        if key in registered:
            fail(f"line {line_number} duplicates trait key {key}")
        registered.add(key)
        if row["disposition"] not in ALLOWED_DISPOSITIONS:
            fail(
                f"line {line_number} has unknown disposition "
                f"{row['disposition']!r}"
            )
        if row["state"] not in ALLOWED_STATES:
            fail(f"line {line_number} has unknown state {row['state']!r}")
        if row["state"] == "missing":
            if row["disposition"] not in {
                "provider-capability",
                "provider-resource",
                "consumer-operation",
            }:
                fail(
                    f"line {line_number} marks non-runtime disposition "
                    f"{row['disposition']!r} as missing"
                )
            missing_surfaces.append("::".join(key))
        if len(row["rationale"].split()) < 8:
            fail(f"line {line_number} rationale is too short to be reviewable")
        verify_evidence(line_number, row["evidence"])

    omitted = sorted(discovered - registered)
    stale = sorted(registered - discovered)
    if omitted:
        fail(f"unclassified public traits: {omitted}")
    if stale:
        fail(f"registry rows without matching public traits: {stale}")
    if arguments.require_complete and missing_surfaces:
        fail(f"missing customer surfaces: {missing_surfaces}")

    state = "complete" if not missing_surfaces else f"{len(missing_surfaces)} gaps recorded"
    print(
        f"Validated {len(rows)} exhaustive trait classifications across "
        f"lling-llang and llattice ({state})."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
