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
    exec systemd-run --user --scope -q --expand-environment=no \
      -p MemoryMax=4G -p MemorySwapMax=0 -p CPUQuota=100% -p TasksMax=64 \
      --setenv=LLING_LLANG_FORMAL_SCOPED=1 \
      --setenv=TMPDIR="$TMP_DIR" \
      --setenv=CARGO_BUILD_JOBS=1 \
      --setenv=JAVA_TOOL_OPTIONS="-Djava.awt.headless=true -Xmx3g -XX:+UseParallelGC -Djava.io.tmpdir=$TMP_DIR" \
      bash "$0" "$@"
  fi

  if [[ "${CI:-false}" != "true" ]]; then
    echo "ERROR: a user systemd scope is required for the 4 GiB formal gate." >&2
    exit 1
  fi
fi

export TMPDIR="$TMP_DIR"
export CARGO_BUILD_JOBS=1
export JAVA_TOOL_OPTIONS="${JAVA_TOOL_OPTIONS:--Djava.awt.headless=true -Xmx3g -XX:+UseParallelGC -Djava.io.tmpdir=$TMP_DIR}"

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
python3 "$ROOT/scripts/check-libcpg-assurance-invariants.py" \
  2>&1 | tee "$LOG_DIR/libcpg-assurance-invariant-registry.log"
python3 "$ROOT/scripts/check-libcpg-manifest-invariants.py" \
  2>&1 | tee "$LOG_DIR/libcpg-manifest-invariant-registry.log"
python3 "$ROOT/scripts/check-provider-boundary-invariants.py" \
  2>&1 | tee "$LOG_DIR/provider-boundary-invariant-registry.log"
python3 "$ROOT/scripts/check-neutral-foundation-invariants.py" \
  2>&1 | tee "$LOG_DIR/neutral-foundation-invariant-registry.log"
python3 "$ROOT/scripts/check-strong-bisimulation-invariants.py" \
  2>&1 | tee "$LOG_DIR/strong-bisimulation-invariant-registry.log"
python3 -B "$ROOT/scripts/check-stack-safety-dispositions.py" \
  2>&1 | tee "$LOG_DIR/stack-safety-disposition-registry.log"
python3 -B "$ROOT/scripts/check-stack-safety-dispositions.py" --self-test \
  2>&1 | tee "$LOG_DIR/stack-safety-disposition-self-test.log"

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
run_tlc stack-balanced \
  "$ROOT/proofs/tla/StackMachineProtocol.tla" \
  "$ROOT/proofs/tla/MC/StackMachineBalanced.cfg"
run_tlc stack-deep \
  "$ROOT/proofs/tla/StackMachineProtocol.tla" \
  "$ROOT/proofs/tla/MC/StackMachineDeep.cfg"
run_tlc stack-short-circuit \
  "$ROOT/proofs/tla/StackMachineProtocol.tla" \
  "$ROOT/proofs/tla/MC/StackMachineShortCircuit.cfg"

run_tlc optimizer-lifecycle \
  "$ROOT/proofs/tla/OptimizerLifecycle.tla" \
  "$ROOT/proofs/tla/MC/OptimizerLifecycle.cfg"
run_tlc fuzzy-reference-lifecycle \
  "$ROOT/proofs/tla/FuzzyReferenceLifecycle.tla" \
  "$ROOT/proofs/tla/MC/FuzzyReferenceLifecycle.cfg"
run_tlc libcpg-evidence-lifecycle \
  "$ROOT/proofs/tla/LibcpgEvidenceLifecycle.tla" \
  "$ROOT/proofs/tla/MC/LibcpgEvidenceLifecycle.cfg"
run_tlc libcpg-manifest-lifecycle \
  "$ROOT/proofs/tla/LibcpgManifestLifecycle.tla" \
  "$ROOT/proofs/tla/MC/LibcpgManifestLifecycle.cfg"
run_tlc provider-boundary-lifecycle \
  "$ROOT/proofs/tla/ProviderBoundaryLifecycle.tla" \
  "$ROOT/proofs/tla/MC/ProviderBoundaryLifecycle.cfg"
run_tlc lazy-wfst-lifecycle \
  "$ROOT/proofs/tla/LazyWfstLifecycle.tla" \
  "$ROOT/proofs/tla/MC/LazyWfstLifecycle.cfg"
run_tlc abi-ownership-lifecycle \
  "$ROOT/proofs/tla/AbiOwnershipLifecycle.tla" \
  "$ROOT/proofs/tla/MC/AbiOwnershipLifecycle.cfg"
run_tlc neutral-foundations \
  "$ROOT/proofs/tla/NeutralFoundationLifecycle.tla" \
  "$ROOT/proofs/tla/MC/NeutralFoundationLifecycle.cfg"
run_tlc strong-bisimulation-valid \
  "$ROOT/proofs/tla/StrongBisimulationLifecycle.tla" \
  "$ROOT/proofs/tla/MC/StrongBisimulationValid.cfg"
run_tlc strong-bisimulation-invalid-source \
  "$ROOT/proofs/tla/StrongBisimulationLifecycle.tla" \
  "$ROOT/proofs/tla/MC/StrongBisimulationInvalidSource.cfg"
run_tlc strong-bisimulation-invalid-target \
  "$ROOT/proofs/tla/StrongBisimulationLifecycle.tla" \
  "$ROOT/proofs/tla/MC/StrongBisimulationInvalidTarget.cfg"
run_tlc strong-bisimulation-invalid-label \
  "$ROOT/proofs/tla/StrongBisimulationLifecycle.tla" \
  "$ROOT/proofs/tla/MC/StrongBisimulationInvalidLabel.cfg"

rm -rf "$MUTANT_DIR"
mkdir -p \
  "$MUTANT_DIR/lazy" \
  "$MUTANT_DIR/abi-composition" \
  "$MUTANT_DIR/rrwm" \
  "$MUTANT_DIR/cascade" \
  "$MUTANT_DIR/optimizer" \
  "$MUTANT_DIR/domain-integration" \
  "$MUTANT_DIR/libcpg-evidence" \
  "$MUTANT_DIR/provider-status" \
  "$MUTANT_DIR/provider-limitations" \
  "$MUTANT_DIR/provider-independence" \
  "$MUTANT_DIR/lazy-wfst" \
  "$MUTANT_DIR/abi-ownership" \
  "$MUTANT_DIR/neutral-foundations"

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

cp "$ROOT/proofs/tla/LibcpgEvidenceLifecycle.tla" \
  "$MUTANT_DIR/libcpg-evidence/LibcpgEvidenceLifecycle.tla"
cp "$ROOT/proofs/tla/MC/LibcpgEvidenceLifecycle.cfg" \
  "$MUTANT_DIR/libcpg-evidence/LibcpgEvidenceLifecycle.cfg"
perl -0pi -e 's/  \/\\ guaranteeIndependence = "Independent"/  \/\\ guaranteeVerifier # Producer/' \
  "$MUTANT_DIR/libcpg-evidence/LibcpgEvidenceLifecycle.tla"
run_tlc_expect_failure libcpg-dependent-evidence-mutant \
  "$MUTANT_DIR/libcpg-evidence/LibcpgEvidenceLifecycle.tla" \
  "$MUTANT_DIR/libcpg-evidence/LibcpgEvidenceLifecycle.cfg" \
  "Invariant DependentGuaranteeBlocksExact is violated."

cp "$ROOT/proofs/tla/ProviderBoundaryLifecycle.tla" \
  "$MUTANT_DIR/provider-status/ProviderBoundaryLifecycle.tla"
cp "$ROOT/proofs/tla/MC/ProviderBoundaryLifecycle.cfg" \
  "$MUTANT_DIR/provider-status/ProviderBoundaryLifecycle.cfg"
perl -0pi -e "s/adaptedStatus' = originalStatus/adaptedStatus' = IF originalStatus = \"Incomplete\" THEN \"CompleteExact\" ELSE originalStatus/" \
  "$MUTANT_DIR/provider-status/ProviderBoundaryLifecycle.tla"
run_tlc_expect_failure provider-status-promotion-mutant \
  "$MUTANT_DIR/provider-status/ProviderBoundaryLifecycle.tla" \
  "$MUTANT_DIR/provider-status/ProviderBoundaryLifecycle.cfg" \
  "Invariant AdaptationPreservesStatus is violated."

cp "$ROOT/proofs/tla/ProviderBoundaryLifecycle.tla" \
  "$MUTANT_DIR/provider-limitations/ProviderBoundaryLifecycle.tla"
cp "$ROOT/proofs/tla/MC/ProviderBoundaryLifecycle.cfg" \
  "$MUTANT_DIR/provider-limitations/ProviderBoundaryLifecycle.cfg"
perl -0pi -e "s/adaptedLimitations' = originalLimitations/adaptedLimitations' = \"None\"/" \
  "$MUTANT_DIR/provider-limitations/ProviderBoundaryLifecycle.tla"
run_tlc_expect_failure provider-limitation-loss-mutant \
  "$MUTANT_DIR/provider-limitations/ProviderBoundaryLifecycle.tla" \
  "$MUTANT_DIR/provider-limitations/ProviderBoundaryLifecycle.cfg" \
  "Invariant AdaptationPreservesLimitations is violated."

cp "$ROOT/proofs/tla/ProviderBoundaryLifecycle.tla" \
  "$MUTANT_DIR/provider-independence/ProviderBoundaryLifecycle.tla"
cp "$ROOT/proofs/tla/MC/ProviderBoundaryLifecycle.cfg" \
  "$MUTANT_DIR/provider-independence/ProviderBoundaryLifecycle.cfg"
perl -0pi -e 's|  /\\ guaranteeDomain # ProducerDomain|  /\\ guaranteeActor # ProducerActor|' \
  "$MUTANT_DIR/provider-independence/ProviderBoundaryLifecycle.tla"
run_tlc_expect_failure provider-dependent-guarantee-mutant \
  "$MUTANT_DIR/provider-independence/ProviderBoundaryLifecycle.tla" \
  "$MUTANT_DIR/provider-independence/ProviderBoundaryLifecycle.cfg" \
  "Invariant DependentGuaranteeBlocksExact is violated."

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

run_neutral_mutant() {
  local name="$1"
  local target="$2"
  local expected="Invariant $target is violated"
  local output="$MUTANT_DIR/neutral-foundations/$name"
  python3 "$ROOT/scripts/neutral_foundation_mutants.py" \
    "$name" \
    "$ROOT/proofs/tla/NeutralFoundationLifecycle.tla" \
    "$ROOT/proofs/tla/MC/NeutralFoundationLifecycle.cfg" \
    "$output"
  run_tlc_expect_failure "neutral-$name-mutant" \
    "$output/NeutralFoundationLifecycle.tla" \
    "$output/NeutralFoundationLifecycle.cfg" \
    "$expected"
}

run_neutral_mutant type-ok TypeOK
run_neutral_mutant named-profile NamedProfileIsNotRfc8785
run_neutral_mutant identity-domains WireAndContentIdentityDomainsAreSeparate
run_neutral_mutant projection-strength ProjectionNeverStrengthens
run_neutral_mutant patch-base PatchCommitRequiresMatchingBase
run_neutral_mutant incomplete-cache IncompleteNeverEntersCache
run_neutral_mutant release-locks RuntimeReleaseRequiresExactCompleteLockedInputs
run_neutral_mutant repository-spill OverflowSpillsOnlyToRepositoryStorage
run_neutral_mutant checkpoint-resume ResumeRequiresCompatibleCheckpoint
run_neutral_mutant tombstone-active TombstonesAreNotActive
run_neutral_mutant source-accounting SourceAccountingNeverDropsUnclassifiedText
run_neutral_mutant statistics-theorem StatisticsNeverDischargeTheoremObligations
run_neutral_mutant stale-evidence StaleEvidenceCannotVerify
run_neutral_mutant negative-control VerifiedAssuranceRequiresNegativeControl
run_neutral_mutant revision-attestation VerifiedAssuranceRequiresRevisionAttestation
run_neutral_mutant check-only-mutation CheckOnlyLintNeverMutatesDocumentation
run_neutral_mutant stale-manifest StaleManifestCannotPassLint
run_neutral_mutant release-gates ReleaseRequiresEveryNeutralFoundationGate
run_neutral_mutant native-stack NativeStackBoundIsInputIndependent

eventually_output="$MUTANT_DIR/neutral-foundations/eventually-terminal"
python3 "$ROOT/scripts/neutral_foundation_mutants.py" \
  eventually-terminal \
  "$ROOT/proofs/tla/NeutralFoundationLifecycle.tla" \
  "$ROOT/proofs/tla/MC/NeutralFoundationLifecycle.cfg" \
  "$eventually_output"
run_tlc_expect_failure neutral-eventually-terminal-mutant \
  "$eventually_output/NeutralFoundationLifecycle.tla" \
  "$eventually_output/NeutralFoundationLifecycle.cfg" \
  "Temporal properties were violated."

"$ROOT/scripts/check-libcpg-manifest-mutants.sh"
python3 "$ROOT/scripts/check-strong-bisimulation-exhaustive.py" \
  2>&1 | tee "$LOG_DIR/strong-bisimulation-exhaustive.log"
python3 "$ROOT/scripts/strong_bisimulation_mutants.py" \
  2>&1 | tee "$LOG_DIR/strong-bisimulation-mutants.log"

z3 "$ROOT/proofs/smt/vco-e4-contracts.smt2" \
  2>&1 | tee "$LOG_DIR/z3-vco-e4-contracts.log"
diff -u \
  "$ROOT/proofs/smt/vco-e4-contracts.expected" \
  "$LOG_DIR/z3-vco-e4-contracts.log"

z3 "$ROOT/proofs/smt/vco-e4-strong-bisimulation.smt2" \
  2>&1 | tee "$LOG_DIR/z3-vco-e4-strong-bisimulation.log"
diff -u \
  "$ROOT/proofs/smt/vco-e4-strong-bisimulation.expected" \
  "$LOG_DIR/z3-vco-e4-strong-bisimulation.log"

z3 "$ROOT/proofs/smt/vco-e6-domain-contracts.smt2" \
  2>&1 | tee "$LOG_DIR/z3-vco-e6-domain-contracts.log"
diff -u \
  "$ROOT/proofs/smt/vco-e6-domain-contracts.expected" \
  "$LOG_DIR/z3-vco-e6-domain-contracts.log"

z3 "$ROOT/proofs/smt/vco-e7-libcpg-assurance.smt2" \
  2>&1 | tee "$LOG_DIR/z3-vco-e7-libcpg-assurance.log"
diff -u \
  "$ROOT/proofs/smt/vco-e7-libcpg-assurance.expected" \
  "$LOG_DIR/z3-vco-e7-libcpg-assurance.log"

z3 "$ROOT/proofs/smt/vco-e7-manifest-facts.smt2" \
  2>&1 | tee "$LOG_DIR/z3-vco-e7-manifest-facts.log"
diff -u \
  "$ROOT/proofs/smt/vco-e7-manifest-facts.expected" \
  "$LOG_DIR/z3-vco-e7-manifest-facts.log"

z3 "$ROOT/proofs/smt/vco-e9-provider-boundary.smt2" \
  2>&1 | tee "$LOG_DIR/z3-vco-e9-provider-boundary.log"
diff -u \
  "$ROOT/proofs/smt/vco-e9-provider-boundary.expected" \
  "$LOG_DIR/z3-vco-e9-provider-boundary.log"

z3 "$ROOT/proofs/smt/vco-e9-neutral-foundations.smt2" \
  2>&1 | tee "$LOG_DIR/z3-vco-e9-neutral-foundations.log"
diff -u \
  "$ROOT/proofs/smt/vco-e9-neutral-foundations.expected" \
  "$LOG_DIR/z3-vco-e9-neutral-foundations.log"

z3 "$ROOT/proofs/smt/stack-safety-ranks.smt2" \
  2>&1 | tee "$LOG_DIR/z3-stack-safety-ranks.log"

"$ROOT/proofs/verify-abi-bounded.sh"
"$ROOT/scripts/check-neutral-foundation-required-red.sh"
"$ROOT/scripts/check-libcpg-manifest-required-red.sh"
"$ROOT/scripts/check-strong-bisimulation-properties.sh"

if ! command -v vinary-doc-lint >/dev/null 2>&1; then
  echo "ERROR: vinary-doc-lint is required for formal documentation acceptance." >&2
  exit 127
fi
vinary-doc-lint check \
  "$ROOT/docs/optimization/certified-strong-bisimulation-contract.md" \
  "$ROOT/docs/optimization/formal-verification.md" \
  "$ROOT/proofs/doc/proof-status.md" \
  "$ROOT/docs/BIBLIOGRAPHY.md" \
  "$ROOT/docs/README.md" \
  "$ROOT/docs/diagrams/README.md" \
  2>&1 | tee "$LOG_DIR/strong-bisimulation-doc-lint.log"
vinary-doc-lint --diagram-tools check \
  "$ROOT/docs/optimization/certified-strong-bisimulation-contract.md" \
  "$ROOT/docs/diagrams/optimization/strong-bisimulation-flow.puml" \
  "$ROOT/docs/diagrams/optimization/strong-bisimulation-evidence.puml" \
  "$ROOT/docs/diagrams/optimization/strong-bisimulation-flow.svg" \
  "$ROOT/docs/diagrams/optimization/strong-bisimulation-evidence.svg" \
  2>&1 | tee "$LOG_DIR/strong-bisimulation-diagram-lint.log"

rm -rf "$MUTANT_DIR"
echo "Formal verification completed successfully. Evidence logs: $LOG_DIR"
