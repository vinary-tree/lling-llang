#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

tlc_cmd() {
  if command -v tlc >/dev/null 2>&1; then
    tlc "$@"
  elif [[ -n "${TLA2TOOLS_JAR:-}" ]]; then
    java -jar "$TLA2TOOLS_JAR" "$@"
  else
    echo "ERROR: TLC not found. Install tlc or set TLA2TOOLS_JAR=/path/to/tla2tools.jar." >&2
    return 127
  fi
}

run_tlc() {
  local name="$1"
  local spec="$2"
  local cfg="$3"
  local metadir="/tmp/lling-llang-tlc-${name}-$$"

  rm -rf "$metadir"
  tlc_cmd -metadir "$metadir" -config "$cfg" "$spec"
}

run_tlc_expect_failure() {
  local name="$1"
  local spec="$2"
  local cfg="$3"
  local expected="$4"
  local metadir="/tmp/lling-llang-tlc-${name}-$$"
  local output="/tmp/lling-llang-tlc-${name}-$$.out"

  rm -rf "$metadir"
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

make -C "$ROOT/proofs/coq" proof-check
make -C "$ROOT/proofs/coq" -j1

run_tlc rrwm "$ROOT/proofs/tla/RRWM.tla" "$ROOT/proofs/tla/MC/RRWM.cfg"
run_tlc rrwm-zero "$ROOT/proofs/tla/RRWM.tla" "$ROOT/proofs/tla/MC/RRWMZeroExperts.cfg"
run_tlc rrwm-single "$ROOT/proofs/tla/RRWM.tla" "$ROOT/proofs/tla/MC/RRWMSingleExpert.cfg"

run_tlc lazy-lru "$ROOT/proofs/tla/LazyComposition.tla" "$ROOT/proofs/tla/MC/LazyComposition.cfg"
run_tlc lazy-nocache "$ROOT/proofs/tla/LazyComposition.tla" "$ROOT/proofs/tla/MC/LazyCompositionNoCache.cfg"
run_tlc lazy-cacheall "$ROOT/proofs/tla/LazyComposition.tla" "$ROOT/proofs/tla/MC/LazyCompositionCacheAll.cfg"

negative_dir="/tmp/lling-llang-negative-lazy-$$"
mkdir -p "$negative_dir"
negative_lazy="$negative_dir/LazyComposition.tla"
negative_cfg="$negative_dir/LazyCompositionNoCache.cfg"
cp "$ROOT/proofs/tla/LazyComposition.tla" "$negative_lazy"
cp "$ROOT/proofs/tla/MC/LazyCompositionNoCache.cfg" "$negative_cfg"
perl -0pi -e 's/IF CacheMode = "NoCache" THEN\n        \{\}/IF CacheMode = "NoCache" THEN\n        cache \\cup \{state\}/' "$negative_lazy"
run_tlc_expect_failure lazy-nocache-mutant "$negative_lazy" "$negative_cfg" \
  "Invariant MemoryBounded is violated."

run_tlc cascade "$ROOT/proofs/tla/CascadeOrder.tla" "$ROOT/proofs/tla/MC/CascadeOrder.cfg"
run_tlc cascade-fair "$ROOT/proofs/tla/CascadeOrder.tla" "$ROOT/proofs/tla/MC/CascadeOrderFair.cfg"
run_tlc cascade-overlap "$ROOT/proofs/tla/CascadeOrder.tla" "$ROOT/proofs/tla/MC/CascadeOrderOverlappingAlphabets.cfg"

negative_rrwm_dir="/tmp/lling-llang-negative-rrwm-$$"
mkdir -p "$negative_rrwm_dir"
negative_rrwm="$negative_rrwm_dir/RRWM.tla"
negative_rrwm_cfg="$negative_rrwm_dir/RRWM.cfg"
cp "$ROOT/proofs/tla/RRWM.tla" "$negative_rrwm"
cp "$ROOT/proofs/tla/MC/RRWM.cfg" "$negative_rrwm_cfg"
perl -0pi -e 's/MaxTotalLoss \+ 1 - nextExpertLosses\[i\]/MaxTotalLoss + 1 - expertLosses[i]/' "$negative_rrwm"
run_tlc_expect_failure rrwm-weight-mutant "$negative_rrwm" "$negative_rrwm_cfg" \
  "Invariant WeightsExact is violated."

negative_cascade_dir="/tmp/lling-llang-negative-cascade-$$"
mkdir -p "$negative_cascade_dir"
negative_cascade="$negative_cascade_dir/CascadeOrder.tla"
negative_cascade_cfg="$negative_cascade_dir/CascadeOrderOverlappingAlphabets.cfg"
cp "$ROOT/proofs/tla/CascadeOrder.tla" "$negative_cascade"
cp "$ROOT/proofs/tla/MC/CascadeOrderOverlappingAlphabets.cfg" "$negative_cascade_cfg"
perl -0pi -e 's/^    \/\x5c AllowedNext\(c1, c2\)\n//m' "$negative_cascade"
run_tlc_expect_failure cascade-order-mutant "$negative_cascade" "$negative_cascade_cfg" \
  "Invariant OrderingConstraints is violated."
