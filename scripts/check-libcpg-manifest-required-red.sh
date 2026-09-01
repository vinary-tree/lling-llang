#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EVIDENCE="$ROOT/target/formal-verification"
LOG_DIR="$EVIDENCE/logs"
TMP_DIR="$EVIDENCE/tmp"
TARGET_DIR="$EVIDENCE/libcpg-manifest-required-red-target"

mkdir -p "$LOG_DIR" "$TMP_DIR"

if [[ "${LLING_LLANG_FORMAL_SCOPED:-0}" != "1" ]]; then
  if command -v systemd-run >/dev/null 2>&1 \
     && systemd-run --user --scope -q true >/dev/null 2>&1; then
    exec systemd-run --user --scope -q --expand-environment=no \
      -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=64 \
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

python3 "$ROOT/scripts/check-libcpg-manifest-invariants.py" \
  2>&1 | tee "$LOG_DIR/libcpg-manifest-required-red-preflight.log" >/dev/null

reject_transport_failure() {
  local log="$1"
  if rg -qi 'failed to download|network failure|could not resolve host|timed out while fetching' "$log"; then
    echo "ERROR: required-red failed because of dependency transport: $log" >&2
    exit 1
  fi
}

manifest_log="$LOG_DIR/libcpg-manifest-required-red.log"
set +e
cargo test --offline \
  --manifest-path "$ROOT/proofs/required_red/owners/libcpg_manifest/Cargo.toml" \
  --no-run 2>&1 | tee "$manifest_log" >/dev/null
manifest_status="${PIPESTATUS[0]}"
set -e
if [[ "$manifest_status" -ne 101 ]]; then
  echo "ERROR: libcpg manifest properties must be red with Cargo status 101; got $manifest_status." >&2
  exit 1
fi
reject_transport_failure "$manifest_log"
for symbol in \
  CacheCompatibility DenseFactIndex DurableFactKey ExtractionCoverage \
  ExtractorManifest FeatureHistory HistoricalFeatureId ManifestScenario \
  PortableFactSnapshot SourceFactEvidence; do
  if ! rg -Fq "$symbol" "$manifest_log"; then
    echo "ERROR: libcpg required-red log does not identify missing API '$symbol'." >&2
    exit 1
  fi
done
echo "required-red: all 98 libcpg properties are blocked by the reviewed missing manifest/fact API"

adapter_log="$LOG_DIR/vinary-libcpg-adapter-required-red.log"
set +e
cargo test --offline \
  --manifest-path "$ROOT/proofs/required_red/owners/libcpg_adapter/Cargo.toml" \
  --no-run 2>&1 | tee "$adapter_log" >/dev/null
adapter_status="${PIPESTATUS[0]}"
set -e
if [[ "$adapter_status" -ne 101 ]] \
   || ! rg -Fq 'failed to load manifest for dependency `vinary-libcpg-adapter`' "$adapter_log" \
   || ! rg -Fq '/vinary-libcpg-adapter/Cargo.toml' "$adapter_log"; then
  echo "ERROR: adapter properties are not red solely because the independent adapter crate is absent." >&2
  exit 1
fi
reject_transport_failure "$adapter_log"
echo "required-red: all 15 adapter properties are blocked by the absent independently owned crate"

case "$TARGET_DIR" in
  "$ROOT"/target/formal-verification/libcpg-manifest-required-red-target) rm -rf "$TARGET_DIR" ;;
  *) echo "ERROR: refusing to clean an unexpected required-red target path." >&2; exit 1 ;;
esac

echo "Validated all 113 causal required-red manifest/fact properties."
