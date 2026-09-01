#!/usr/bin/env bash
# Dynamic-analysis harness for the scalar-WFST resource-ABI boundary (wave W8).
#
# Runs lling-llang's OWN FFI/ABI integration suites under AddressSanitizer (+
# LeakSanitizer) and ThreadSanitizer to catch, at the FFI boundary, the classes
# of defect that safe Rust and `catch_unwind` cannot: use-after-free,
# out-of-bounds access, leaks of retained/leased provider snapshots, and data
# races across the call gate. These exercise the same LLING-BRIDGE / retain-
# ledger invariants the correspondence tests pin, but dynamically at machine
# level:
#
#   - tests/ffi_out_pointer_safety.rs   -- the null-out leak/orphan class
#       (LLING-B5 heap + double snapshot-retain leak; LLING-B6 orphan state);
#       LeakSanitizer is the machine-level backstop for the retain ledgers.
#   - tests/ffi_incompatible_resources.rs, tests/ffi_builder_matrix.rs
#       -- adversarial provider + builder argument matrices (LLING-BRIDGE-4
#       weight-domain rejection, status mapping) run under ASan.
#   - tests/ffi_roundtrip_proptest.rs, tests/ffi_lazy_composition_correspondence.rs
#       -- proptest round-trip and lazy-composition correspondence under ASan.
#   - tests/ffi_lazy_expansion_metrics.rs -- laziness/retain metrics under ASan.
#   - tests/ffi_concurrent_composition_stress.rs -- concurrent composition /
#       shared-snapshot walks (std::thread::scope); the primary ThreadSanitizer
#       target for races across the composition call gate.
#
# The suites are the PROJECT'S OWN boundary tests (in-repo providers under
# tests/support/); no dependent crate is pulled in.
#
# Requires a nightly toolchain with the `rust-src` component (for `-Zbuild-std`,
# which rebuilds std -- and every dependency, including the registry-resolved
# `vinary-tree-interop` crate at the ABI boundary -- with the sanitizer
# runtime). `--target` is passed so host build scripts and proc-macros
# (moniker-derive) are NOT instrumented; only the target test binary is.
#
# The FFI suites are gated `#![cfg(feature = "ffi")]` (proptest suites additionally
# on `test-utils`), so the feature set mirrors the repo's FFI CI invocation
# exactly: `--no-default-features --features "ffi test-utils"`. `--no-default-features`
# drops the default `smt-z3` feature so the system libz3 link -- out of the ABI
# boundary -- is not dragged into the sanitizer build.
#
# The repo's x86_64-linux codegen baseline (`-C target-feature=+aes,+sse2`, set
# in .cargo/config.toml) is re-applied here because an env `RUSTFLAGS` fully
# REPLACES config `rustflags`; without it the sanitizer build would silently
# drop the baseline the rest of CI compiles against. Override via
# SANITIZER_BASELINE_RUSTFLAGS (e.g. `-C target-feature=+aes,+neon` on aarch64).
#
# Usage:
#   scripts/run-sanitizers.sh                 # asan+lsan then tsan, whole suite
#   SANITIZER_ONLY=address scripts/run-sanitizers.sh --test ffi_out_pointer_safety
#   SANITIZER_ONLY=thread  scripts/run-sanitizers.sh --test ffi_concurrent_composition_stress
#   SANITIZER_NIGHTLY=nightly-2026-04-21 scripts/run-sanitizers.sh
set -euo pipefail

TARGET="${SANITIZER_TARGET:-x86_64-unknown-linux-gnu}"
NIGHTLY="${SANITIZER_NIGHTLY:-nightly}"
FEATURES="${SANITIZER_FEATURES:-ffi test-utils}"
ONLY="${SANITIZER_ONLY:-address thread}"
BASELINE_RUSTFLAGS="${SANITIZER_BASELINE_RUSTFLAGS:--C target-feature=+aes,+sse2}"

run_one() {
  local san="$1"; shift
  echo "== ${san}sanitizer =="
  RUSTFLAGS="-Zsanitizer=${san} ${BASELINE_RUSTFLAGS}" \
  RUSTDOCFLAGS="-Zsanitizer=${san} ${BASELINE_RUSTFLAGS}" \
  ASAN_OPTIONS="detect_leaks=1:detect_stack_use_after_return=1" \
    cargo +"$NIGHTLY" test -Zbuild-std \
      --target "$TARGET" --no-default-features --features "$FEATURES" "$@"
}

for san in $ONLY; do
  run_one "$san" "$@"
done

echo "sanitizers: all requested runs completed cleanly"
