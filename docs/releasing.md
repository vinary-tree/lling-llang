# Releasing lling-llang

This guide defines the release operation for the `lling-llang` crate, native
SDK, and `@vinary-tree/lling-llang` JavaScript facade. The current candidate
is `4.0.0-rc.1`.

## Immutable source graph

Create `v4.0.0-rc.1` from the reviewed `release/4.0.0-rc.1` branch commit,
not from the concurrently changing primary worktree. `release/version.json`,
`Cargo.toml`, native metadata, and the npm manifest must agree. Validation
checks out `llattice@v0.1.0` and exact `v4.0.0-rc.1` tags for interop,
libdictenstein, and liblevenshtein.

The tag is a validation trigger, not registry authorization. Its workflow
tests the FFI and property contracts, runs strict Clippy and npm tests, builds
Linux x86-64 and ARM64, macOS ARM64, and Windows x86-64 archives, relocation-
tests the installed CMake packages with shared and static linkage, packs npm,
and creates a checksummed GitHub prerelease.

## Validate, then publish one registry

Manual dispatches must target the immutable tag. `validate-only` creates no
registry mutation; `npm` and `crates-io` each enable only their namesake
protected job.

```bash
gh workflow run release-bindings.yml \
  --repo vinary-tree/lling-llang \
  --ref v4.0.0-rc.1 \
  -f registry=validate-only

gh workflow run release-bindings.yml \
  --repo vinary-tree/lling-llang \
  --ref v4.0.0-rc.1 \
  -f registry=npm
```

Publish crates.io only after its exact Rust dependencies resolve publicly.
Publish npm only after `@vinary-tree/interop` and the shared
`@vinary-tree/vinary-tree` runtime resolve at `4.0.0-rc.1`. The npm job uses
trusted publishing, provenance, the `next` dist-tag, and the protected `npm`
environment.

## Public-byte verification and recovery

Install the npm coordinate in a clean directory and exercise WFST creation,
arc traversal, composition, iteration, and deterministic resource closure.
After that smoke test, move the new scoped package's `latest` tag to the RC,
remove `bootstrap`, and deprecate `0.0.0` as reservation-only.

Tags and published versions are immutable. Rerun validation safely with
`registry=validate-only`. If a registry accepted incorrect bytes, fix the
source and issue `4.0.0-rc.2`; never move the tag or overwrite the version.
