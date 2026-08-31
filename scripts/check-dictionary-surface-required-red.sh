#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/proofs/required_red/dictionary_surface/Cargo.toml"
LOG_DIR="$ROOT/target/formal-verification/logs"
LOG="$LOG_DIR/dictionary-surface-required-red.log"
mkdir -p "$LOG_DIR"

set +e
cargo test --manifest-path "$MANIFEST" --no-run 2>&1 | tee "$LOG"
status="${PIPESTATUS[0]}"
set -e

if [[ "$status" -ne 101 ]]; then
  echo "ERROR: required-red dictionary surface returned $status rather than Cargo's 101." >&2
  exit 1
fi
if ! rg -q 'failed to load manifest for dependency .(libdictenstein-llattice|vinary-dictionary-pipeline).' "$LOG"; then
  echo "ERROR: required-red failure was not an absent canonical adapter crate." >&2
  exit 1
fi
if rg -q 'failed to get|failed to download|Could not resolve host|timed out' "$LOG"; then
  echo "ERROR: network or registry failure cannot satisfy the required-red gate." >&2
  exit 1
fi

echo "Required-red dictionary properties are blocked only by an absent canonical adapter crate."
