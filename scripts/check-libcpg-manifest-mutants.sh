#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EVIDENCE="$ROOT/target/formal-verification"
LOG_DIR="$EVIDENCE/logs"
TMP_DIR="$EVIDENCE/tmp"
MUTANT_DIR="$EVIDENCE/mutants/libcpg-manifest"
TIMEOUT_SECONDS="${TLC_TIMEOUT_SECONDS:-120}"

mkdir -p "$LOG_DIR" "$TMP_DIR"

if [[ "${LLING_LLANG_FORMAL_SCOPED:-0}" != "1" ]]; then
  if command -v systemd-run >/dev/null 2>&1 \
     && systemd-run --user --scope -q true >/dev/null 2>&1; then
    exec systemd-run --user --scope -q --expand-environment=no \
      -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=64 \
      --setenv=LLING_LLANG_FORMAL_SCOPED=1 \
      --setenv=TMPDIR="$TMP_DIR" \
      --setenv=JAVA_TOOL_OPTIONS="-Djava.awt.headless=true -Xmx3g -XX:+UseParallelGC -Djava.io.tmpdir=$TMP_DIR" \
      bash "$0"
  fi
  if [[ "${CI:-false}" != "true" ]]; then
    echo "ERROR: a user systemd scope is required for the 4 GiB mutant gate." >&2
    exit 1
  fi
fi

export TMPDIR="$TMP_DIR"
export JAVA_TOOL_OPTIONS="${JAVA_TOOL_OPTIONS:--Djava.awt.headless=true -Xmx3g -XX:+UseParallelGC -Djava.io.tmpdir=$TMP_DIR}"

if command -v tlc >/dev/null 2>&1; then
  TLC_COMMAND=(tlc)
elif [[ -n "${TLA2TOOLS_JAR:-}" ]]; then
  TLC_COMMAND=(java -jar "$TLA2TOOLS_JAR")
else
  echo "ERROR: TLC not found. Install tlc or set TLA2TOOLS_JAR." >&2
  exit 127
fi

case "$MUTANT_DIR" in
  "$ROOT"/target/formal-verification/mutants/libcpg-manifest) rm -rf "$MUTANT_DIR" ;;
  *) echo "ERROR: refusing to clean an unexpected mutant path." >&2; exit 1 ;;
esac
mkdir -p "$MUTANT_DIR"

count=0
while IFS=$'\t' read -r name target kind; do
  output="$MUTANT_DIR/$name"
  log="$LOG_DIR/tlc-libcpg-manifest-$name-mutant.log"
  python3 "$ROOT/scripts/libcpg_manifest_mutants.py" \
    "$name" \
    "$ROOT/proofs/tla/LibcpgManifestLifecycle.tla" \
    "$ROOT/proofs/tla/MC/LibcpgManifestLifecycle.cfg" \
    "$output"
  mkdir -p "$output/states"

  set +e
  timeout "${TIMEOUT_SECONDS}s" "${TLC_COMMAND[@]}" -workers 1 \
    -metadir "$output/states" \
    -config "$output/LibcpgManifestLifecycle.cfg" \
    "$output/LibcpgManifestLifecycle.tla" \
    2>&1 | tee "$log" >/dev/null
  status="${PIPESTATUS[0]}"
  set -e

  if [[ "$status" -eq 0 ]]; then
    echo "ERROR: mutant '$name' survived property '$target'." >&2
    exit 1
  fi
  if [[ "$status" -eq 124 ]]; then
    echo "ERROR: mutant '$name' exceeded ${TIMEOUT_SECONDS}s." >&2
    exit 1
  fi
  if [[ "$kind" == "property" ]]; then
    expected="Temporal properties were violated."
    if ! LC_ALL=C grep -aFq -- "$expected" "$log"; then
      echo "ERROR: mutant '$name' failed for an unexpected reason." >&2
      tail -n 30 "$log" >&2
      exit 1
    fi
  else
    expected="Invariant $target is violated"
    constant_expected="invariant of $target is equal to FALSE"
    if ! LC_ALL=C grep -aFq -- "$expected" "$log" \
       && ! LC_ALL=C grep -aFq -- "$constant_expected" "$log"; then
      echo "ERROR: mutant '$name' failed for an unexpected reason." >&2
      tail -n 30 "$log" >&2
      exit 1
    fi
  fi
  count=$((count + 1))
  echo "killed: $name -> $target"
done < <(python3 "$ROOT/scripts/libcpg_manifest_mutants.py" --list)

if [[ "$count" -ne 26 ]]; then
  echo "ERROR: expected 26 exhaustive manifest mutants; ran $count." >&2
  exit 1
fi

case "$MUTANT_DIR" in
  "$ROOT"/target/formal-verification/mutants/libcpg-manifest) rm -rf "$MUTANT_DIR" ;;
  *) echo "ERROR: refusing to clean an unexpected mutant path." >&2; exit 1 ;;
esac

echo "Killed all $count libcpg manifest lifecycle mutants."
