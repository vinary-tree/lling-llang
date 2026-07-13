# Archived Documentation

This directory holds **frozen historical records** — dated, append-only scientific
documents that are preserved *verbatim* rather than rewritten. They predate the
documentation overhaul that migrated the rest of `docs/` to MathJax LaTeX, so they
deliberately retain the **older Unicode-math-in-backticks notation** (e.g. `` `⊕` ``,
`` `0̄` ``, `` `O(∣V∣ + ∣E∣)` ``). Rewriting a dated ledger's notation would alter the
record it exists to preserve, so these files are intentionally left as written.

> **Do not rewrite these documents.** Correct a typo only if it is unambiguously a
> transcription slip; never restate results, re-run-and-replace benchmark numbers, or
> "modernize" the prose. New findings belong in a *new* dated entry, not an edit to an
> old one.

## Contents

| Document | What it is | Last active |
|---|---|---|
| [`journal.md`](journal.md) | The scientific optimization journal — baseline, per-hypothesis benchmark results, statistical-significance tests, and post-mortems, following the project's optimize-by-hypothesis methodology. | Appended through 2026-07 |
| [`implementation-ledger/`](implementation-ledger/index.md) | The phase-by-phase implementation ledger (a single logical document split into [index](implementation-ledger/index.md) + phases 1–7) recording how each WFST feature was built. | Appended through 2026-06 |
| [`industry-standard-and-sota-review.md`](industry-standard-and-sota-review.md) | A point-in-time industry-standard / state-of-the-art optimization review, with per-item "implemented" / "benchmarked-and-retained" outcomes. | Snapshot as of 2026-07-04 |

## Provenance

These files were previously located under `docs/optimization/`. They were moved here
unchanged (`git mv`) during the July 2026 documentation overhaul. Their internal
cross-links (e.g. `implementation-ledger/phase-4` → `../../architecture/…`) resolve
identically from this location because the directory depth was preserved.

The **living** optimization documentation — the pedagogical technique guides
([lookahead](../optimization/lookahead.md), [n-gram back-off](../optimization/ngram-backoff.md),
[token grouping](../optimization/token-grouping.md)) — remains under
[`../optimization/`](../optimization/) and follows the current MathJax conventions in
[`../STYLE.md`](../STYLE.md).
