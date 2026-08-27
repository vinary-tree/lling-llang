#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:-all}"
case "$MODE" in
  all | --rocq-only | --tla-only) ;;
  *)
    echo "Usage: $0 [--rocq-only|--tla-only]" >&2
    exit 2
    ;;
esac

scratch_parent="${PROOF_SCRATCH_ROOT:-$ROOT/target/proofs/scratch}"
mkdir -p "$scratch_parent"
scratch_root="$(mktemp -d "$scratch_parent/verify.XXXXXX")"
java_scratch="$scratch_root/java"
mkdir -p "$java_scratch"
cleanup() {
  rm -rf -- "$scratch_root"
}
trap cleanup EXIT

tlc_cmd() {
  if command -v tlc >/dev/null 2>&1; then
    TLA_JAVA_OPTS="${TLA_JAVA_OPTS:-} -Djava.io.tmpdir=$java_scratch" tlc "$@"
  elif [[ -n "${TLA2TOOLS_JAR:-}" ]]; then
    java -Djava.io.tmpdir="$java_scratch" -jar "$TLA2TOOLS_JAR" "$@"
  else
    echo "ERROR: TLC not found. Install tlc or set TLA2TOOLS_JAR=/path/to/tla2tools.jar." >&2
    return 127
  fi
}

run_tlc() {
  local name="$1"
  local spec="$2"
  local cfg="$3"
  local metadir="$scratch_root/tlc-$name"
  tlc_cmd -metadir "$metadir" -config "$cfg" "$spec"
}

run_tlc_expect_failure() {
  local name="$1"
  local spec="$2"
  local cfg="$3"
  local expected="$4"
  local metadir="$scratch_root/tlc-$name"
  local output="$scratch_root/tlc-$name.out"

  if tlc_cmd -metadir "$metadir" -config "$cfg" "$spec" >"$output" 2>&1; then
    echo "ERROR: expected TLC model '$name' to fail, but it passed." >&2
    return 1
  fi
  if ! grep -Fq "$expected" "$output"; then
    echo "ERROR: TLC model '$name' failed for an unexpected reason." >&2
    cat "$output" >&2
    return 1
  fi
  cat "$output"
}

# Run the Coq build under systemd resource caps when a user scope is actually
# available (local dev — a heavy modular proof must not spike memory/CPU and
# freeze the workstation), and directly otherwise (CI runners have no user
# systemd session, so the probe fails and we fall through cleanly).
capped_make() {
  if command -v systemd-run >/dev/null 2>&1 \
     && systemd-run --user --scope -q true >/dev/null 2>&1; then
    systemd-run --user --scope -q \
      -p MemoryMax=8G -p CPUQuota=1800% -p TasksMax=200 \
      make "$@"
  else
    make "$@"
  fi
}

run_rocq() {
  capped_make -C "$ROOT/proofs/coq" proof-check
  capped_make -C "$ROOT/proofs/coq" -j1
}

run_tla() {
  # ABI invariant registry: every hooked invariant is registered, and every row
  # points at a live spec (and test, unless formal-only).
  python3 "$ROOT/scripts/check-abi-invariants.py"

  run_tlc rrwm "$ROOT/proofs/tla/RRWM.tla" "$ROOT/proofs/tla/MC/RRWM.cfg"
  run_tlc rrwm-zero "$ROOT/proofs/tla/RRWM.tla" "$ROOT/proofs/tla/MC/RRWMZeroExperts.cfg"
  run_tlc rrwm-single "$ROOT/proofs/tla/RRWM.tla" "$ROOT/proofs/tla/MC/RRWMSingleExpert.cfg"

  run_tlc lazy-lru "$ROOT/proofs/tla/LazyComposition.tla" "$ROOT/proofs/tla/MC/LazyComposition.cfg"
  run_tlc lazy-nocache "$ROOT/proofs/tla/LazyComposition.tla" "$ROOT/proofs/tla/MC/LazyCompositionNoCache.cfg"
  run_tlc lazy-cacheall "$ROOT/proofs/tla/LazyComposition.tla" "$ROOT/proofs/tla/MC/LazyCompositionCacheAll.cfg"

  negative_dir="$scratch_root/negative-lazy"
  mkdir -p "$negative_dir"
  negative_lazy="$negative_dir/LazyComposition.tla"
  negative_cfg="$negative_dir/LazyCompositionNoCache.cfg"
  cp "$ROOT/proofs/tla/LazyComposition.tla" "$negative_lazy"
  cp "$ROOT/proofs/tla/MC/LazyCompositionNoCache.cfg" "$negative_cfg"
  perl -0pi -e 's/IF CacheMode = "NoCache" THEN\n        \{\}/IF CacheMode = "NoCache" THEN\n        cache \\cup \{state\}/' "$negative_lazy"
  run_tlc_expect_failure lazy-nocache-mutant "$negative_lazy" "$negative_cfg" \
    "Invariant MemoryBounded is violated."

  run_tlc abi-composition \
    "$ROOT/proofs/tla/AbiCompositionProtocol.tla" \
    "$ROOT/proofs/tla/MC/AbiCompositionProtocol.cfg"

  negative_abicomp_dir="$scratch_root/negative-abicomp"
  mkdir -p "$negative_abicomp_dir"
  negative_abicomp="$negative_abicomp_dir/AbiCompositionProtocol.tla"
  negative_abicomp_cfg="$negative_abicomp_dir/AbiCompositionProtocol.cfg"
  cp "$ROOT/proofs/tla/AbiCompositionProtocol.tla" "$negative_abicomp"
  cp "$ROOT/proofs/tla/MC/AbiCompositionProtocol.cfg" "$negative_abicomp_cfg"
  # Mutant: acquire the registry write lock inside Begin, so a foreign provider
  # callback then runs while the lock is held -- the exact defect LLING-COMP-5
  # forbids (src/bindings.rs calls the providers before acquiring the lock).
  perl -0pi -e 's/\/\\ pc'"'"' = \[pc EXCEPT !\[t\] = "callProviders"\]\n  \/\\ UNCHANGED <<regWriter, cacheWriter>>/\/\\ regWriter = NONE\n  \/\\ regWriter'"'"' = t\n  \/\\ pc'"'"' = [pc EXCEPT ![t] = "callProviders"]\n  \/\\ UNCHANGED cacheWriter/' "$negative_abicomp"
  run_tlc_expect_failure abi-composition-mutant "$negative_abicomp" "$negative_abicomp_cfg" \
    "Invariant NoCallbackUnderRegWrite is violated."

  run_tlc cascade "$ROOT/proofs/tla/CascadeOrder.tla" "$ROOT/proofs/tla/MC/CascadeOrder.cfg"
  run_tlc cascade-fair "$ROOT/proofs/tla/CascadeOrder.tla" "$ROOT/proofs/tla/MC/CascadeOrderFair.cfg"
  run_tlc cascade-overlap "$ROOT/proofs/tla/CascadeOrder.tla" "$ROOT/proofs/tla/MC/CascadeOrderOverlappingAlphabets.cfg"

  negative_rrwm_dir="$scratch_root/negative-rrwm"
  mkdir -p "$negative_rrwm_dir"
  negative_rrwm="$negative_rrwm_dir/RRWM.tla"
  negative_rrwm_cfg="$negative_rrwm_dir/RRWM.cfg"
  cp "$ROOT/proofs/tla/RRWM.tla" "$negative_rrwm"
  cp "$ROOT/proofs/tla/MC/RRWM.cfg" "$negative_rrwm_cfg"
  perl -0pi -e 's/MaxTotalLoss \+ 1 - nextExpertLosses\[i\]/MaxTotalLoss + 1 - expertLosses[i]/' "$negative_rrwm"
  run_tlc_expect_failure rrwm-weight-mutant "$negative_rrwm" "$negative_rrwm_cfg" \
    "Invariant WeightsExact is violated."

  negative_cascade_dir="$scratch_root/negative-cascade"
  mkdir -p "$negative_cascade_dir"
  negative_cascade="$negative_cascade_dir/CascadeOrder.tla"
  negative_cascade_cfg="$negative_cascade_dir/CascadeOrderOverlappingAlphabets.cfg"
  cp "$ROOT/proofs/tla/CascadeOrder.tla" "$negative_cascade"
  cp "$ROOT/proofs/tla/MC/CascadeOrderOverlappingAlphabets.cfg" "$negative_cascade_cfg"
  perl -0pi -e 's/^    \/\x5c AllowedNext\(c1, c2\)\n//m' "$negative_cascade"
  run_tlc_expect_failure cascade-order-mutant "$negative_cascade" "$negative_cascade_cfg" \
    "Invariant OrderingConstraints is violated."
}

case "$MODE" in
  all)
    run_rocq
    run_tla
    ;;
  --rocq-only) run_rocq ;;
  --tla-only) run_tla ;;
esac
