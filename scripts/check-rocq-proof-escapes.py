#!/usr/bin/env python3
"""Reject unchecked or literally vacuous Rocq proof declarations."""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "proofs/coq"
PROOF_DIRECTORIES = (
    "foundations",
    "wfst",
    "algorithms",
    "abi",
    "optimizer",
    "domain_integration",
    "logict",
    "presburger",
    "sft",
    "symbolic_algebra",
)
HARD_ESCAPE = re.compile(
    r"^[ \t]*(?:Admitted|Abort)\."
    r"|^[ \t]*(?:Axiom|Conjecture|Parameter)[ \t]"
    r"|\badmit\.",
    re.MULTILINE,
)
DECLARATION_WITH_PROOF = re.compile(
    r"^(Theorem|Lemma)\s+([A-Za-z0-9_']+)\s*:(.*?)^Proof\.",
    re.MULTILINE | re.DOTALL,
)
COMMENT = re.compile(r"\(\*.*?\*\)", re.DOTALL)


def line_number(text: str, offset: int) -> int:
    """Return the one-origin source line containing *offset*."""
    return text.count("\n", 0, offset) + 1


violations: list[str] = []
for directory in PROOF_DIRECTORIES:
    for source in sorted((ROOT / directory).glob("*.v")):
        text = source.read_text(encoding="utf-8")
        for match in HARD_ESCAPE.finditer(text):
            violations.append(
                f"{source.relative_to(ROOT)}:{line_number(text, match.start())}: "
                f"unchecked proof escape {match.group(0)!r}"
            )

        for match in DECLARATION_WITH_PROOF.finditer(text):
            proposition = COMMENT.sub("", match.group(3)).strip()
            if proposition == "True.":
                violations.append(
                    f"{source.relative_to(ROOT)}:"
                    f"{line_number(text, match.start())}: "
                    f"{match.group(1)} {match.group(2)} has literal True "
                    "as its entire proposition"
                )

if violations:
    for violation in violations:
        print(f"ERROR: {violation}", file=sys.stderr)
    raise SystemExit(1)

print(
    "No Admitted/Abort/admit, axiom-like declarations, or literal-True "
    "theorem/lemma propositions found."
)
