#!/usr/bin/env bash
set -euo pipefail

# The z3 0.21 Rust binding requires libz3 >= 4.8.17. Ubuntu 24.04's
# libz3-dev is 4.8.12, so CI uses one verified official release instead.
: "${RUNNER_TEMP:?RUNNER_TEMP must be set by GitHub Actions}"
: "${GITHUB_ENV:?GITHUB_ENV must be set by GitHub Actions}"

z3_ci_name=z3-4.16.0-x64-glibc-2.39
z3_ci_archive="$RUNNER_TEMP/$z3_ci_name.zip"
z3_ci_root="$RUNNER_TEMP/$z3_ci_name"
z3_ci_digest=7288c49a5bd6dbafd7b0b0d1f65956b91672da24b08f09242919af159be3418e

curl -fsSL --retry 3 \
  --output "$z3_ci_archive" \
  "https://github.com/Z3Prover/z3/releases/download/z3-4.16.0/$z3_ci_name.zip"
echo "$z3_ci_digest  $z3_ci_archive" | sha256sum --check --strict
unzip -q "$z3_ci_archive" -d "$RUNNER_TEMP"
test -f "$z3_ci_root/include/z3.h"
test -f "$z3_ci_root/bin/libz3.so"
[[ "$("$z3_ci_root/bin/z3" --version)" == *"4.16.0"* ]]

{
  echo "Z3_SYS_Z3_HEADER=$z3_ci_root/include/z3.h"
  echo "Z3_LIBRARY_PATH_OVERRIDE=$z3_ci_root/bin"
  echo "LIBRARY_PATH=$z3_ci_root/bin${LIBRARY_PATH:+:$LIBRARY_PATH}"
  echo "LD_LIBRARY_PATH=$z3_ci_root/bin${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
} >> "$GITHUB_ENV"
