#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EVIDENCE="$ROOT/target/formal-verification"
LOG_DIR="$EVIDENCE/logs"
TMP_DIR="$EVIDENCE/tmp"
TARGET_DIR="$EVIDENCE/strong-bisimulation-required-red-target"
MANIFEST="$ROOT/proofs/required_red/strong_bisimulation/Cargo.toml"

mkdir -p "$LOG_DIR" "$TMP_DIR"

if [[ "${LLING_LLANG_FORMAL_SCOPED:-0}" != "1" ]]; then
  if command -v systemd-run >/dev/null 2>&1 \
     && systemd-run --user --scope -q true >/dev/null 2>&1; then
    exec systemd-run --user --scope -q --expand-environment=no \
      -p MemoryMax=4G -p MemorySwapMax=0 -p CPUQuota=400% -p TasksMax=64 \
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

python3 "$ROOT/scripts/check-strong-bisimulation-invariants.py" \
  2>&1 | tee "$LOG_DIR/strong-bisimulation-required-red-preflight.log" >/dev/null

log="$LOG_DIR/strong-bisimulation-required-red.log"
set +e
cargo test --offline --manifest-path "$MANIFEST" --no-run \
  2>&1 | tee "$log" >/dev/null
status="${PIPESTATUS[0]}"
set -e

if [[ "$status" -ne 101 ]]; then
  echo "ERROR: strong-bisimulation properties must be red with Cargo status 101; got $status." >&2
  exit 1
fi
if rg -qi 'failed to download|network failure|could not resolve host|timed out while fetching' "$log"; then
  echo "ERROR: required-red failed because of dependency transport." >&2
  exit 1
fi
if ! rg -Fq 'no `CertifiedBisimulation` in `symbolic::bisimulation`' "$log"; then
  echo "ERROR: required-red is not caused by the reviewed missing certified API." >&2
  exit 1
fi

case "$TARGET_DIR" in
  "$ROOT"/target/formal-verification/strong-bisimulation-required-red-target)
    rm -rf "$TARGET_DIR"
    ;;
  *)
    echo "ERROR: refusing to clean an unexpected required-red target path." >&2
    exit 1
    ;;
esac

echo "Validated all 13 causal required-red strong-bisimulation properties."
