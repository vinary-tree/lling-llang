#!/usr/bin/env python3
"""Generate deliberate host-provider protocol faults for TLC negative checks.

Each replacement is anchored to one exact source fragment. A change to the
model must update the corresponding fault injection rather than silently
dropping a negative control.
"""

from __future__ import annotations

import argparse
from pathlib import Path


MUTATIONS = {
    "missing-snapshot-retain": (
        "  /\\ snapshotId' = [snapshotId EXCEPT ![c] = 1]\n"
        "  /\\ retains' = retains + 1",
        "  /\\ snapshotId' = [snapshotId EXCEPT ![c] = 1]\n"
        "  /\\ retains' = retains",
    ),
    "unsafe-serial-admission": (
        'ELSE IF Mode = "Serial" THEN Active = {}',
        'ELSE IF Mode = "Serial" THEN TRUE',
    ),
    "page-capacity-lie": (
        "pageCapacity' = [pageCapacity EXCEPT ![c] = capacity]",
        "pageCapacity' = [pageCapacity EXCEPT ![c] = 0]",
    ),
    "error-publishes-output": (
        "![c] = IF succeeded THEN @ + 1 ELSE @]",
        "![c] = IF succeeded THEN @ + 1 ELSE @ + 1]",
    ),
    "accepts-stale-token": (
        "  /\\ IF ValidToken(c, supplied)\n"
        '       THEN status\' = [status EXCEPT ![c] = "Ok"]',
        "  /\\ IF slotLive /\\ tokenOwner = c /\\ snapshotOwned[c]\n"
        '       THEN status\' = [status EXCEPT ![c] = "Ok"]',
    ),
}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("model", type=Path)
    parser.add_argument("output_root", type=Path)
    args = parser.parse_args()
    source = args.model.read_text(encoding="utf-8")
    for name, (original, faulty) in MUTATIONS.items():
        matches = source.count(original)
        if matches != 1:
            raise ValueError(f"{name}: expected one source anchor, found {matches}")
        destination = args.output_root / name / args.model.name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(source.replace(original, faulty), encoding="utf-8")


if __name__ == "__main__":
    main()
