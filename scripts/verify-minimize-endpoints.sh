#!/bin/sh
# Reproducible, RSS-bounded acceptance for the fail-closed minimizer boundary.
set -eu

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
log_dir="$repo_dir/target/verification/minimize-endpoint"
tmp_dir="$repo_dir/target/tmp/minimize-endpoint"
mkdir -p "$log_dir" "$tmp_dir"

cd "$repo_dir/proofs/coq"
systemd-run --user --scope \
    -p MemoryMax=4G -p MemoryHigh=3G -p MemorySwapMax=0 -p CPUQuota=100% \
    --setenv="TMPDIR=$tmp_dir" \
    make -j1 -f Makefile CoqMakefile \
    > "$log_dir/rocq-makefile.log" 2>&1
systemd-run --user --scope \
    -p MemoryMax=4G -p MemoryHigh=3G -p MemorySwapMax=0 -p CPUQuota=100% \
    --setenv="TMPDIR=$tmp_dir" \
    make -j1 -f CoqMakefile algorithms/MinimizeEndpointValidation.vo \
    > "$log_dir/rocq.log" 2>&1
systemd-run --user --scope \
    -p MemoryMax=4G -p MemoryHigh=3G -p MemorySwapMax=0 -p CPUQuota=100% \
    --setenv="TMPDIR=$tmp_dir" \
    coqchk -Q . LlingLlang LlingLlang.algorithms.MinimizeEndpointValidation \
    > "$log_dir/coqchk.log" 2>&1

cd "$repo_dir"
systemd-run --user --scope \
    -p MemoryMax=4G -p MemoryHigh=3G -p MemorySwapMax=0 -p CPUQuota=200% \
    --setenv="TMPDIR=$tmp_dir" --setenv=CARGO_BUILD_JOBS=2 \
    --setenv=CARGO_INCREMENTAL=0 --setenv=RUST_TEST_THREADS=2 \
    cargo test --all-targets > "$log_dir/all-targets-test.log" 2>&1
systemd-run --user --scope \
    -p MemoryMax=4G -p MemoryHigh=3G -p MemorySwapMax=0 -p CPUQuota=200% \
    --setenv="TMPDIR=$tmp_dir" --setenv=CARGO_BUILD_JOBS=2 \
    --setenv=CARGO_INCREMENTAL=0 --setenv=RUST_TEST_THREADS=2 \
    cargo test --doc > "$log_dir/doc-tests.log" 2>&1
