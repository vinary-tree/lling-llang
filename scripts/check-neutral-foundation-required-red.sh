#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EVIDENCE="$ROOT/target/formal-verification"
LOG_DIR="$EVIDENCE/logs"
TMP_DIR="$EVIDENCE/tmp"
TARGET_DIR="$EVIDENCE/required-red-target"

mkdir -p "$LOG_DIR" "$TMP_DIR"

if [[ "${LLING_LLANG_FORMAL_SCOPED:-0}" != "1" ]]; then
  if command -v systemd-run >/dev/null 2>&1 \
     && systemd-run --user --scope -q true >/dev/null 2>&1; then
    exec systemd-run --user --scope -q --expand-environment=no \
      -p MemoryMax=4G -p MemorySwapMax=0 -p CPUQuota=100% -p TasksMax=64 \
      --setenv=LLING_LLANG_FORMAL_SCOPED=1 \
      --setenv=CARGO_BUILD_JOBS=1 \
      --setenv=CARGO_TARGET_DIR="$TARGET_DIR" \
      --setenv=TMPDIR="$TMP_DIR" \
      bash "$0"
  fi
  if [[ "${CI:-false}" != "true" ]]; then
    echo "ERROR: a user systemd scope is required for the 4 GiB required-red gate." >&2
    exit 1
  fi
fi

export CARGO_BUILD_JOBS=1
export CARGO_TARGET_DIR="$TARGET_DIR"
export TMPDIR="$TMP_DIR"

python3 "$ROOT/scripts/check-neutral-foundation-invariants.py" \
  2>&1 | tee "$LOG_DIR/neutral-foundation-required-red-preflight.log" >/dev/null

reject_noncausal_failure() {
  local log="$1"
  if rg -qi 'failed to download|network failure|could not resolve host|timed out while fetching' "$log"; then
    echo "ERROR: required-red run failed because of network or dependency transport: $log" >&2
    exit 1
  fi
}

run_missing_api_red() {
  local owner="$1"
  local manifest="$2"
  shift 2
  local log="$LOG_DIR/${owner}-required-red.log"
  set +e
  cargo test --locked --offline --manifest-path "$manifest" --no-run 2>&1 | tee "$log" >/dev/null
  local status="${PIPESTATUS[0]}"
  set -e
  if [[ "$status" -ne 101 ]]; then
    echo "ERROR: $owner must fail with Cargo status 101 before its production APIs exist; got $status." >&2
    exit 1
  fi
  reject_noncausal_failure "$log"
  local symbol
  for symbol in "$@"; do
    if ! rg -Fq "$symbol" "$log"; then
      echo "ERROR: $owner required-red log does not identify missing API '$symbol'." >&2
      exit 1
    fi
  done
  echo "required-red: $owner is causally red on its proposed API surface"
}

run_missing_api_red canonical-wire \
  "$ROOT/proofs/required_red/owners/canonical_wire/Cargo.toml" \
  CanonicalProfileId DigestByteSink NumericDomain VINARY_CANONICAL_JSON_V1 \
  SchemaFingerprintDigest

run_missing_api_red analysis-graph \
  "$ROOT/proofs/required_red/owners/analysis_graph/Cargo.toml" \
  ClaimStrength DialectConformance EpistemicAxes JsonlBuilder RelationNode RoleEdge

run_missing_api_red runtime \
  "$ROOT/proofs/required_red/owners/runtime/Cargo.toml" \
  CheckpointCompatibility InputLocks NeutralFoundationRelease OutcomeAxes \
  ProcessTreeTerminator RepositoryBackedSpillPolicy

run_missing_api_red assurance \
  "$ROOT/proofs/required_red/owners/assurance/Cargo.toml" \
  Applicability AssuranceDecision EvidenceAuthority EvidenceContext ObligationKind \
  ReviewerAttestation

run_missing_api_red lifecycle \
  "$ROOT/proofs/required_red/owners/lifecycle/Cargo.toml" \
  InputLocks NeutralFoundationRelease OutcomeAxes

content_log="$LOG_DIR/content-identity-required-red.log"
set +e
cargo test --offline \
  --manifest-path "$ROOT/proofs/required_red/content_identity/Cargo.toml" \
  --no-run 2>&1 | tee "$content_log" >/dev/null
content_status="${PIPESTATUS[0]}"
set -e
if [[ "$content_status" -ne 101 ]] \
   || ! rg -Fq 'failed to load manifest for dependency `vinary-content-identity`' "$content_log" \
   || ! rg -Fq '/vinary-content-identity/Cargo.toml' "$content_log"; then
  echo "ERROR: content identity is not red solely because its independently owned crate is absent." >&2
  exit 1
fi
reject_noncausal_failure "$content_log"
echo "required-red: content-identity is gated by its absent independent crate"

# The exhaustive ledger validates the protected vinary-doc-lint API hash. Its
# one-time diagnostic evidence records both pre-API build blockers; recompiling
# 1.5 GiB of unchanged vendored dependencies would not strengthen that result.
echo "ownership-gated: documentation required-red sources await the protected dependency baseline"

requirements_log="$LOG_DIR/requirements-prebaseline-blocker.log"
set +e
cargo test --offline \
  --manifest-path "$ROOT/proofs/required_red/owners/requirements/Cargo.toml" \
  --no-run 2>&1 | tee "$requirements_log" >/dev/null
requirements_status="${PIPESTATUS[0]}"
set -e
if [[ "$requirements_status" -ne 101 ]] \
   || ! rg -Fq '/vinary-requirements/crates/vinary-test-ir/Cargo.toml' "$requirements_log" \
   || ! rg -Fq 'no targets specified in the manifest' "$requirements_log"; then
  echo "ERROR: requirements did not reproduce its reviewed prebaseline ownership blocker." >&2
  exit 1
fi
reject_noncausal_failure "$requirements_log"
echo "ownership-gated: requirements properties cannot reach the proposed API until vinary-test-ir has a target"

case "$TARGET_DIR" in
  "$ROOT"/target/formal-verification/required-red-target) rm -rf "$TARGET_DIR" ;;
  *) echo "ERROR: refusing to clean an unexpected required-red target path." >&2; exit 1 ;;
esac

echo "Neutral-foundation required-red contracts validated."
