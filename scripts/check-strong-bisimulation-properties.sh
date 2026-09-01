#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EVIDENCE="$ROOT/target/formal-verification"
LOG_DIR="$EVIDENCE/logs"
TMP_DIR="$EVIDENCE/tmp"
TARGET_DIR="$EVIDENCE/strong-bisimulation-properties-target"
MANIFEST="$ROOT/proofs/properties/strong_bisimulation/Cargo.toml"

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
    echo "ERROR: a user systemd scope is required for the 4 GiB property gate." >&2
    exit 1
  fi
fi

export CARGO_BUILD_JOBS=1
export CARGO_TARGET_DIR="$TARGET_DIR"
export TMPDIR="$TMP_DIR"

python3 "$ROOT/scripts/check-strong-bisimulation-invariants.py" \
  2>&1 | tee "$LOG_DIR/strong-bisimulation-properties-preflight.log" >/dev/null

log="$LOG_DIR/strong-bisimulation-properties.log"
set +e
cargo test --offline --manifest-path "$MANIFEST" \
  2>&1 | tee "$log" >/dev/null
status="${PIPESTATUS[0]}"
set -e

if rg -qi 'failed to download|network failure|could not resolve host|timed out while fetching' "$log"; then
  echo "ERROR: property gate failed because of dependency transport." >&2
  exit 1
fi
if [[ "$status" -ne 0 ]]; then
  echo "ERROR: strong-bisimulation property gate failed with Cargo status $status." >&2
  exit 1
fi
if ! rg -Fq 'test result: ok. 13 passed; 0 failed;' "$log"; then
  echo "ERROR: property gate did not execute all 13 strong-bisimulation contracts." >&2
  exit 1
fi

case "$TARGET_DIR" in
  "$ROOT"/target/formal-verification/strong-bisimulation-properties-target)
    rm -rf "$TARGET_DIR"
    ;;
  *)
    echo "ERROR: refusing to clean an unexpected property target path." >&2
    exit 1
    ;;
esac

echo "Validated all 13 required-green strong-bisimulation properties."
