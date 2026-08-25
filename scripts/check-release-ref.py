#!/usr/bin/env python3
"""Validate canonical and append-only corrective RC release refs."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REGISTRIES = frozenset({"validate-only", "npm", "crates-io"})


def validate(ref: str, ref_name: str, registry: str, version: str) -> None:
    if registry not in REGISTRIES:
        raise ValueError(f"unknown release registry: {registry}")
    if ref != f"refs/tags/{ref_name}":
        raise ValueError("manual releases must target an immutable tag")

    canonical = f"v{version}"
    if ref_name == canonical:
        return
    if re.fullmatch(rf"{re.escape(canonical)}-release\.[1-9][0-9]*", ref_name):
        return
    raise ValueError(
        f"expected {canonical} or a positive numbered corrective release tag; "
        f"got {ref_name}"
    )


def self_test(version: str) -> None:
    workflow = (ROOT / ".github/workflows/release-bindings.yml").read_text(
        encoding="utf-8"
    )
    if "environment: github-release" not in workflow:
        raise AssertionError(
            "GitHub prerelease publication lacks a protected environment"
        )
    canonical = f"v{version}"
    for registry in REGISTRIES:
        validate(f"refs/tags/{canonical}", canonical, registry, version)
        corrective = f"{canonical}-release.1"
        validate(f"refs/tags/{corrective}", corrective, registry, version)

    rejected = (
        (f"refs/heads/{canonical}", canonical, "validate-only"),
        (f"refs/tags/{canonical}-release.0", f"{canonical}-release.0", "crates-io"),
        (f"refs/tags/{canonical}-release.01", f"{canonical}-release.01", "npm"),
        (f"refs/tags/{canonical}-release", f"{canonical}-release", "validate-only"),
        (f"refs/tags/{canonical}-release.1", f"{canonical}-release.1", "unknown"),
    )
    for ref, ref_name, registry in rejected:
        try:
            validate(ref, ref_name, registry, version)
        except ValueError:
            continue
        raise AssertionError(f"accepted forbidden release dispatch: {ref_name}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ref")
    parser.add_argument("--ref-name")
    parser.add_argument("--registry")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    model = json.loads((ROOT / "release/version.json").read_text(encoding="utf-8"))
    version = model.get("canonical")
    if not isinstance(version, str):
        raise TypeError("release/version.json requires a string canonical version")

    if args.self_test:
        self_test(version)
        print("release-ref authority self-test passed")
        return 0
    if not all(
        isinstance(value, str) for value in (args.ref, args.ref_name, args.registry)
    ):
        parser.error("--ref, --ref-name, and --registry are required")
    try:
        validate(args.ref, args.ref_name, args.registry, version)
    except ValueError as error:
        parser.error(str(error))
    print(version)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
