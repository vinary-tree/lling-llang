#!/usr/bin/env bash
#
# Deterministic positive/negative contract test for doc-math-prescan.raku.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

scanner="scripts/doc-math-prescan.raku"
clean_fixture="scripts/fixtures/doc-math/clean.txt"
invalid_fixture="scripts/fixtures/doc-math/invalid.txt"

command -v raku >/dev/null 2>&1 || {
  echo "error: raku not found on PATH" >&2
  exit 2
}

clean_output="$(raku "$scanner" --lint "$clean_fixture")"
if [[ -n "$clean_output" ]]; then
  echo "error: clean documentation fixture produced findings:" >&2
  printf '%s\n' "$clean_output" >&2
  exit 1
fi

set +e
invalid_output="$(raku "$scanner" --lint "$invalid_fixture" 2>&1)"
invalid_status=$?
set -e

if [[ $invalid_status -ne 1 ]]; then
  echo "error: invalid documentation fixture exited $invalid_status instead of 1" >&2
  printf '%s\n' "$invalid_output" >&2
  exit 1
fi

expected_kinds=(
  backticked-unicode-math
  bare-O
  bare-unicode-math
  code-wrapped-dollar-math
  empty-code-span
  letter-abuts-open
  table-column-mismatch
  unbalanced-inline-math
)

for kind in "${expected_kinds[@]}"; do
  if ! grep -Fq ": $kind:" <<<"$invalid_output"; then
    echo "error: invalid fixture did not exercise $kind" >&2
    printf '%s\n' "$invalid_output" >&2
    exit 1
  fi
done

finding_count="$(grep -c '^[^:]*:[0-9][0-9]*:' <<<"$invalid_output")"
if [[ $finding_count -ne 9 ]]; then
  echo "error: invalid fixture produced $finding_count findings instead of 9" >&2
  printf '%s\n' "$invalid_output" >&2
  exit 1
fi

echo "doc-math-prescan: clean and adversarial fixture contracts pass"
