#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EVIDENCE="$ROOT/target/formal-verification"
LOG_DIR="$EVIDENCE/logs"
TMP_DIR="$EVIDENCE/tmp"
mkdir -p "$LOG_DIR" "$TMP_DIR"

if [[ "${LLING_LLANG_ABI_BOUNDED_SCOPED:-0}" != "1" ]]; then
  if command -v systemd-run >/dev/null 2>&1 \
     && systemd-run --user --scope -q true >/dev/null 2>&1; then
    exec systemd-run --user --scope -q \
      -p MemoryMax=2G -p MemorySwapMax=0 -p CPUQuota=200% -p TasksMax=48 \
      --setenv=LLING_LLANG_ABI_BOUNDED_SCOPED=1 \
      --setenv=TMPDIR="$TMP_DIR" \
      bash "$0" "$@"
  fi

  if [[ "${CI:-false}" != "true" ]]; then
    echo "ERROR: a user systemd scope is required for the 2 GiB ABI check." >&2
    exit 1
  fi
fi

export TMPDIR="$TMP_DIR"
timeout 120s kani "$ROOT/proofs/kani/abi_ownership_model.rs" \
  --target-dir "$EVIDENCE/kani-target" \
  --jobs 1 \
  2>&1 | tee "$LOG_DIR/kani-abi-ownership.log"
