#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EVIDENCE="$ROOT/target/formal-verification"
LOG_DIR="$EVIDENCE/logs"
TMP_DIR="$EVIDENCE/tmp"
TLC_DIR="$EVIDENCE/tlc"
MUTANT_DIR="$EVIDENCE/mutants"
TLC_TIMEOUT_SECONDS="${TLC_TIMEOUT_SECONDS:-120}"

mkdir -p "$LOG_DIR" "$TMP_DIR" "$TLC_DIR"

# Local formal verification must never run outside a bounded user scope. CI
# runners without user systemd are responsible for an equivalent container or
# job-level limit and must identify themselves explicitly through CI=true.
if [[ "${LLING_LLANG_FORMAL_SCOPED:-0}" != "1" ]]; then
  if command -v systemd-run >/dev/null 2>&1 \
     && systemd-run --user --scope -q true >/dev/null 2>&1; then
    exec systemd-run --user --scope -q \
      -p MemoryMax=4G -p MemorySwapMax=0 -p CPUQuota=400% -p TasksMax=64 \
      --setenv=LLING_LLANG_FORMAL_SCOPED=1 \
      --setenv=TMPDIR="$TMP_DIR" \
      --setenv=CARGO_BUILD_JOBS=1 \
      --setenv=JAVA_TOOL_OPTIONS="-Xmx3g -XX:+UseParallelGC -Djava.io.tmpdir=$TMP_DIR" \
      bash "$0" "$@"
  fi

  if [[ "${CI:-false}" != "true" ]]; then
    echo "ERROR: a user systemd scope is required for the 4 GiB formal gate." >&2
    exit 1
  fi
fi

export TMPDIR="$TMP_DIR"
export CARGO_BUILD_JOBS=1
export JAVA_TOOL_OPTIONS="${JAVA_TOOL_OPTIONS:--Xmx3g -XX:+UseParallelGC -Djava.io.tmpdir=$TMP_DIR}"

if command -v tlc >/dev/null 2>&1; then
  TLC_COMMAND=(tlc)
elif [[ -n "${TLA2TOOLS_JAR:-}" ]]; then
  TLC_COMMAND=(java -jar "$TLA2TOOLS_JAR")
else
  echo "ERROR: TLC not found. Install tlc or set TLA2TOOLS_JAR." >&2
  exit 127
fi

run_tlc() {
  local name="$1"
  local spec="$2"
  local cfg="$3"
  local metadir="$TLC_DIR/$name"
  local log="$LOG_DIR/tlc-$name.log"

  rm -rf "$metadir"
  mkdir -p "$metadir"
  set +e
  timeout "${TLC_TIMEOUT_SECONDS}s" "${TLC_COMMAND[@]}" \
    -workers 1 -metadir "$metadir" -config "$cfg" "$spec" \
    2>&1 | tee "$log"
  local status="${PIPESTATUS[0]}"
  set -e
  rm -rf "$metadir"
  return "$status"
}

run_tlc_expect_failure() {
  local name="$1"
  local spec="$2"
  local cfg="$3"
  local expected="$4"
  local metadir="$TLC_DIR/$name"
  local log="$LOG_DIR/tlc-$name.log"

  rm -rf "$metadir"
  mkdir -p "$metadir"
  set +e
  timeout "${TLC_TIMEOUT_SECONDS}s" "${TLC_COMMAND[@]}" \
    -workers 1 -metadir "$metadir" -config "$cfg" "$spec" \
    2>&1 | tee "$log"
  local status="${PIPESTATUS[0]}"
  set -e
  rm -rf "$metadir"

  if [[ "$status" -eq 0 ]]; then
    echo "ERROR: expected TLC model '$name' to fail, but it passed." >&2
    return 1
  fi
  if [[ "$status" -eq 124 ]]; then
    echo "ERROR: TLC model '$name' exceeded ${TLC_TIMEOUT_SECONDS}s." >&2
    return 1
  fi
  if ! rg -Fq "$expected" "$log"; then
    echo "ERROR: TLC model '$name' failed for an unexpected reason." >&2
    return 1
  fi
}

make -C "$ROOT/proofs/coq" proof-check 2>&1 | tee "$LOG_DIR/coq-proof-check.log"
make -C "$ROOT/proofs/coq" -j1 2>&1 | tee "$LOG_DIR/coq-build.log"

# ABI invariant registry: every row points at a live specification and, unless
# formal-only, a live regression test.
python3 "$ROOT/scripts/check-abi-invariants.py" \
  2>&1 | tee "$LOG_DIR/abi-invariant-registry.log"
python3 "$ROOT/scripts/check-domain-integration-invariants.py" \
  2>&1 | tee "$LOG_DIR/domain-integration-invariant-registry.log"
python3 "$ROOT/scripts/check-dictionary-surface-invariants.py" \
  2>&1 | tee "$LOG_DIR/dictionary-surface-invariant-registry.log"
"$ROOT/scripts/check-dictionary-surface-required-red.sh"

run_tlc rrwm "$ROOT/proofs/tla/RRWM.tla" "$ROOT/proofs/tla/MC/RRWM.cfg"
run_tlc rrwm-zero "$ROOT/proofs/tla/RRWM.tla" "$ROOT/proofs/tla/MC/RRWMZeroExperts.cfg"
run_tlc rrwm-single "$ROOT/proofs/tla/RRWM.tla" "$ROOT/proofs/tla/MC/RRWMSingleExpert.cfg"

run_tlc lazy-lru "$ROOT/proofs/tla/LazyComposition.tla" "$ROOT/proofs/tla/MC/LazyComposition.cfg"
run_tlc lazy-nocache "$ROOT/proofs/tla/LazyComposition.tla" "$ROOT/proofs/tla/MC/LazyCompositionNoCache.cfg"
run_tlc lazy-cacheall "$ROOT/proofs/tla/LazyComposition.tla" "$ROOT/proofs/tla/MC/LazyCompositionCacheAll.cfg"

run_tlc abi-composition \
  "$ROOT/proofs/tla/AbiCompositionProtocol.tla" \
  "$ROOT/proofs/tla/MC/AbiCompositionProtocol.cfg"

run_tlc cascade "$ROOT/proofs/tla/CascadeOrder.tla" "$ROOT/proofs/tla/MC/CascadeOrder.cfg"
run_tlc cascade-fair "$ROOT/proofs/tla/CascadeOrder.tla" "$ROOT/proofs/tla/MC/CascadeOrderFair.cfg"
run_tlc cascade-overlap "$ROOT/proofs/tla/CascadeOrder.tla" "$ROOT/proofs/tla/MC/CascadeOrderOverlappingAlphabets.cfg"

run_tlc optimizer-lifecycle \
  "$ROOT/proofs/tla/OptimizerLifecycle.tla" \
  "$ROOT/proofs/tla/MC/OptimizerLifecycle.cfg"
run_tlc fuzzy-reference-lifecycle \
  "$ROOT/proofs/tla/FuzzyReferenceLifecycle.tla" \
  "$ROOT/proofs/tla/MC/FuzzyReferenceLifecycle.cfg"
run_tlc dictionary-surface-lifecycle \
  "$ROOT/proofs/tla/DictionarySurfaceLifecycle.tla" \
  "$ROOT/proofs/tla/MC/DictionarySurfaceLifecycle.cfg"
run_tlc lazy-wfst-lifecycle \
  "$ROOT/proofs/tla/LazyWfstLifecycle.tla" \
  "$ROOT/proofs/tla/MC/LazyWfstLifecycle.cfg"
run_tlc abi-ownership-lifecycle \
  "$ROOT/proofs/tla/AbiOwnershipLifecycle.tla" \
  "$ROOT/proofs/tla/MC/AbiOwnershipLifecycle.cfg"

rm -rf "$MUTANT_DIR"
mkdir -p \
  "$MUTANT_DIR/lazy" \
  "$MUTANT_DIR/abi-composition" \
  "$MUTANT_DIR/rrwm" \
  "$MUTANT_DIR/cascade" \
  "$MUTANT_DIR/optimizer" \
  "$MUTANT_DIR/domain-integration" \
  "$MUTANT_DIR/dictionary-surface" \
  "$MUTANT_DIR/lazy-wfst" \
  "$MUTANT_DIR/abi-ownership"

cp "$ROOT/proofs/tla/LazyComposition.tla" "$MUTANT_DIR/lazy/LazyComposition.tla"
cp "$ROOT/proofs/tla/MC/LazyCompositionNoCache.cfg" "$MUTANT_DIR/lazy/LazyCompositionNoCache.cfg"
perl -0pi -e 's/IF CacheMode = "NoCache" THEN\n        \{\}/IF CacheMode = "NoCache" THEN\n        cache \\cup \{state\}/' \
  "$MUTANT_DIR/lazy/LazyComposition.tla"
run_tlc_expect_failure lazy-nocache-mutant \
  "$MUTANT_DIR/lazy/LazyComposition.tla" \
  "$MUTANT_DIR/lazy/LazyCompositionNoCache.cfg" \
  "Invariant MemoryBounded is violated."

cp "$ROOT/proofs/tla/AbiCompositionProtocol.tla" \
  "$MUTANT_DIR/abi-composition/AbiCompositionProtocol.tla"
cp "$ROOT/proofs/tla/MC/AbiCompositionProtocol.cfg" \
  "$MUTANT_DIR/abi-composition/AbiCompositionProtocol.cfg"
perl -0pi -e 's/\/\\ pc'"'"' = \[pc EXCEPT !\[t\] = "callProviders"\]\n  \/\\ UNCHANGED <<regWriter, cacheWriter>>/\/\\ regWriter = NONE\n  \/\\ regWriter'"'"' = t\n  \/\\ pc'"'"' = [pc EXCEPT ![t] = "callProviders"]\n  \/\\ UNCHANGED cacheWriter/' \
  "$MUTANT_DIR/abi-composition/AbiCompositionProtocol.tla"
run_tlc_expect_failure abi-composition-mutant \
  "$MUTANT_DIR/abi-composition/AbiCompositionProtocol.tla" \
  "$MUTANT_DIR/abi-composition/AbiCompositionProtocol.cfg" \
  "Invariant NoCallbackUnderRegWrite is violated."

cp "$ROOT/proofs/tla/RRWM.tla" "$MUTANT_DIR/rrwm/RRWM.tla"
cp "$ROOT/proofs/tla/MC/RRWM.cfg" "$MUTANT_DIR/rrwm/RRWM.cfg"
perl -0pi -e 's/MaxTotalLoss \+ 1 - nextExpertLosses\[i\]/MaxTotalLoss + 1 - expertLosses[i]/' \
  "$MUTANT_DIR/rrwm/RRWM.tla"
run_tlc_expect_failure rrwm-weight-mutant \
  "$MUTANT_DIR/rrwm/RRWM.tla" "$MUTANT_DIR/rrwm/RRWM.cfg" \
  "Invariant WeightsExact is violated."

cp "$ROOT/proofs/tla/CascadeOrder.tla" "$MUTANT_DIR/cascade/CascadeOrder.tla"
cp "$ROOT/proofs/tla/MC/CascadeOrderOverlappingAlphabets.cfg" \
  "$MUTANT_DIR/cascade/CascadeOrderOverlappingAlphabets.cfg"
perl -0pi -e 's/^    \/\x5c AllowedNext\(c1, c2\)\n//m' \
  "$MUTANT_DIR/cascade/CascadeOrder.tla"
run_tlc_expect_failure cascade-order-mutant \
  "$MUTANT_DIR/cascade/CascadeOrder.tla" \
  "$MUTANT_DIR/cascade/CascadeOrderOverlappingAlphabets.cfg" \
  "Invariant OrderingConstraints is violated."

cp "$ROOT/proofs/tla/OptimizerLifecycle.tla" \
  "$MUTANT_DIR/optimizer/OptimizerLifecycle.tla"
cp "$ROOT/proofs/tla/MC/OptimizerLifecycle.cfg" \
  "$MUTANT_DIR/optimizer/OptimizerLifecycle.cfg"
perl -0pi -e 's/provenance'"'"' = Append\(provenance, nextSequence\)/provenance'"'"' = Append(provenance, CHOOSE node \\in finished : TRUE)/' \
  "$MUTANT_DIR/optimizer/OptimizerLifecycle.tla"
run_tlc_expect_failure optimizer-provenance-mutant \
  "$MUTANT_DIR/optimizer/OptimizerLifecycle.tla" \
  "$MUTANT_DIR/optimizer/OptimizerLifecycle.cfg" \
  "Invariant ProvenanceIsCanonicalPrefix is violated."

cp "$ROOT/proofs/tla/FuzzyReferenceLifecycle.tla" \
  "$MUTANT_DIR/domain-integration/FuzzyReferenceLifecycle.tla"
cp "$ROOT/proofs/tla/MC/FuzzyReferenceLifecycle.cfg" \
  "$MUTANT_DIR/domain-integration/FuzzyReferenceLifecycle.cfg"
perl -0pi -e 's/ELSE accepted\n/ELSE accepted \\cup {term}\n/' \
  "$MUTANT_DIR/domain-integration/FuzzyReferenceLifecycle.tla"
run_tlc_expect_failure fuzzy-reference-confirmation-mutant \
  "$MUTANT_DIR/domain-integration/FuzzyReferenceLifecycle.tla" \
  "$MUTANT_DIR/domain-integration/FuzzyReferenceLifecycle.cfg" \
  "Invariant AcceptedExactlyCheckedReference is violated."

cp "$ROOT/proofs/tla/MC/DictionarySurfaceLifecycle.cfg" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.cfg"

cp "$ROOT/proofs/tla/DictionarySurfaceLifecycle.tla" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla"
perl -0pi -e "s/feedSnapshot' = capturedSnapshot/feedSnapshot' = NextSnapshot/" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla"
run_tlc_expect_failure dictionary-surface-stale-snapshot-mutant \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.cfg" \
  "Invariant CandidateIdentityMatchesCapture is violated."

cp "$ROOT/proofs/tla/DictionarySurfaceLifecycle.tla" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla"
perl -0pi -e "s/feedNormalization' = capturedNormalization/feedNormalization' = NextNormalization/" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla"
run_tlc_expect_failure dictionary-surface-normalization-mutant \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.cfg" \
  "Invariant CandidateIdentityMatchesCapture is violated."

cp "$ROOT/proofs/tla/DictionarySurfaceLifecycle.tla" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla"
perl -0pi -e "s/feedEditProfile' = capturedEditProfile/feedEditProfile' = NextEditProfile/" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla"
run_tlc_expect_failure dictionary-surface-edit-profile-mutant \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.cfg" \
  "Invariant CandidateIdentityMatchesCapture is violated."

cp "$ROOT/proofs/tla/DictionarySurfaceLifecycle.tla" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla"
perl -0pi -e "s/feedBound' = capturedBound/feedBound' = NextBound/" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla"
run_tlc_expect_failure dictionary-surface-bound-mutant \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.cfg" \
  "Invariant CandidateIdentityMatchesCapture is violated."

cp "$ROOT/proofs/tla/DictionarySurfaceLifecycle.tla" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla"
perl -0pi -e 's/THEN "Incomplete"\n       ELSE IF precision/THEN "CompleteExact"\n       ELSE IF precision/' \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla"
run_tlc_expect_failure dictionary-surface-cap-promotion-mutant \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.cfg" \
  "Invariant NonExhaustiveTerminationIsIncomplete is violated."

cp "$ROOT/proofs/tla/DictionarySurfaceLifecycle.tla" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla"
perl -0pi -e "s/facadePublished' = accepted/facadePublished' = accepted \\\\cup {FalsePositive}/" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla"
run_tlc_expect_failure dictionary-surface-facade-mutant \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.tla" \
  "$MUTANT_DIR/dictionary-surface/DictionarySurfaceLifecycle.cfg" \
  "Invariant FacadeEqualsNative is violated."

cp "$ROOT/proofs/tla/LazyWfstLifecycle.tla" \
  "$MUTANT_DIR/lazy-wfst/LazyWfstLifecycle.tla"
cp "$ROOT/proofs/tla/MC/LazyWfstLifecycle.cfg" \
  "$MUTANT_DIR/lazy-wfst/LazyWfstLifecycle.cfg"
perl -0pi -e 's/(\/\\ policy'"'"' = selected\n  )\/\\ cache'"'"' = \{\}/$1\/\\ cache'"'"' = cache/' \
  "$MUTANT_DIR/lazy-wfst/LazyWfstLifecycle.tla"
run_tlc_expect_failure lazy-wfst-policy-mutant \
  "$MUTANT_DIR/lazy-wfst/LazyWfstLifecycle.tla" \
  "$MUTANT_DIR/lazy-wfst/LazyWfstLifecycle.cfg" \
  "Invariant NoCacheHasNoPersistentEntries is violated."

cp "$ROOT/proofs/tla/AbiOwnershipLifecycle.tla" \
  "$MUTANT_DIR/abi-ownership/AbiOwnershipLifecycle.tla"
cp "$ROOT/proofs/tla/MC/AbiOwnershipLifecycle.cfg" \
  "$MUTANT_DIR/abi-ownership/AbiOwnershipLifecycle.cfg"
perl -0pi -e 's/retainCount'"'"' = retainCount - 1/retainCount'"'"' = retainCount/' \
  "$MUTANT_DIR/abi-ownership/AbiOwnershipLifecycle.tla"
run_tlc_expect_failure abi-release-mutant \
  "$MUTANT_DIR/abi-ownership/AbiOwnershipLifecycle.tla" \
  "$MUTANT_DIR/abi-ownership/AbiOwnershipLifecycle.cfg" \
  "Invariant RetainsEqualOwners is violated."

z3 "$ROOT/proofs/smt/vco-e4-contracts.smt2" \
  2>&1 | tee "$LOG_DIR/z3-vco-e4-contracts.log"
diff -u \
  "$ROOT/proofs/smt/vco-e4-contracts.expected" \
  "$LOG_DIR/z3-vco-e4-contracts.log"

z3 "$ROOT/proofs/smt/vco-e6-domain-contracts.smt2" \
  2>&1 | tee "$LOG_DIR/z3-vco-e6-domain-contracts.log"
diff -u \
  "$ROOT/proofs/smt/vco-e6-domain-contracts.expected" \
  "$LOG_DIR/z3-vco-e6-domain-contracts.log"

z3 "$ROOT/proofs/smt/vco-e6-dictionary-surface.smt2" \
  2>&1 | tee "$LOG_DIR/z3-vco-e6-dictionary-surface.log"
diff -u \
  "$ROOT/proofs/smt/vco-e6-dictionary-surface.expected" \
  "$LOG_DIR/z3-vco-e6-dictionary-surface.log"

"$ROOT/proofs/verify-abi-bounded.sh"

rm -rf "$MUTANT_DIR"
echo "Formal verification completed successfully. Evidence logs: $LOG_DIR"
