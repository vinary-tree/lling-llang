#!/usr/bin/env bash
set -euo pipefail

# z3-sys 0.13 ships enum bindings generated for Z3 >= 5.0.0. Ubuntu 24.04's
# libz3-dev is 4.8.12, so CI uses one verified ABI-compatible release instead.
: "${RUNNER_TEMP:?RUNNER_TEMP must be set by GitHub Actions}"
: "${GITHUB_ENV:?GITHUB_ENV must be set by GitHub Actions}"

z3_ci_name=z3-5.0.0-x64-glibc-2.39
z3_ci_archive="$RUNNER_TEMP/$z3_ci_name.zip"
z3_ci_root="$RUNNER_TEMP/$z3_ci_name"
z3_ci_digest=d4922cebc9f0a55629231ec0c62f0bbedf8006eddaed4e68199ad19626b697f6

curl -fsSL --retry 3 \
  --output "$z3_ci_archive" \
  "https://github.com/Z3Prover/z3/releases/download/z3-5.0.0/$z3_ci_name.zip"
echo "$z3_ci_digest  $z3_ci_archive" | sha256sum --check --strict
unzip -q "$z3_ci_archive" -d "$RUNNER_TEMP"
test -f "$z3_ci_root/include/z3.h"
test -f "$z3_ci_root/bin/libz3.so"
[[ "$("$z3_ci_root/bin/z3" --version)" == *"5.0.0"* ]]

# The archive has no pkg-config manifest. Supply one so z3-sys does not pick
# the runner's older system library (even when its separate link override is
# present). Keep the default pkg-config directories available for other crates.
z3_ci_pkgconfig="$z3_ci_root/pkgconfig"
mkdir -p "$z3_ci_pkgconfig"
# pkg-config expands these literal ${prefix} and ${libdir} variables later.
# shellcheck disable=SC2016
printf 'prefix=%s\nlibdir=${prefix}/bin\nincludedir=${prefix}/include\nName: z3\nDescription: Pinned Z3 solver for CI\nVersion: 5.0.0\nLibs: -L${libdir} -lz3\nCflags: -I${includedir}\n' "$z3_ci_root" > "$z3_ci_pkgconfig/z3.pc"
[[ "$(PKG_CONFIG_PATH="$z3_ci_pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" pkg-config --modversion z3)" == "5.0.0" ]]

{
  echo "Z3_SYS_Z3_HEADER=$z3_ci_root/include/z3.h"
  echo "Z3_SYS_Z3_VERSION=5.0.0"
  echo "Z3_LIBRARY_PATH_OVERRIDE=$z3_ci_root/bin"
  echo "PKG_CONFIG_PATH=$z3_ci_pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
  echo "LIBRARY_PATH=$z3_ci_root/bin${LIBRARY_PATH:+:$LIBRARY_PATH}"
  echo "LD_LIBRARY_PATH=$z3_ci_root/bin${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
} >> "$GITHUB_ENV"
