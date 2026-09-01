# Releasing lling-llang

This guide defines the release operation for the `lling-llang` crate, native
SDK, and `@vinary-tree/lling-llang` JavaScript facade. The current candidate
is `4.0.0-rc.6`.

## Immutable source graph

Create `v4.0.0-rc.6` from the reviewed `release/4.0.0-rc.6` branch commit,
not from the concurrently changing primary worktree. `release/version.json`,
`Cargo.toml`, native metadata, and the npm manifest must agree. Validation
checks out `llattice@v0.1.0` and exact `v4.0.0-rc.6` tags for interop,
libdictenstein, and liblevenshtein.

The repository synchronizer owns every family entry in `Cargo.lock`. A
read-only invocation rejects stale entries, and all validation/package builds
use `--locked` without changing the reviewed lockfile.

The tag creates an immutable source boundary but triggers no workflow. A
manual `validate-only` dispatch tests the FFI and property contracts, runs
strict Clippy and npm tests, builds Linux x86-64 and ARM64, macOS ARM64, and
Windows x86-64 archives, relocation-tests the installed CMake packages with
shared and static linkage, packs npm, and creates a checksummed GitHub
prerelease.

## Validate, then publish one registry

Manual dispatches must target the immutable tag. `validate-only` creates no
registry mutation; `npm` and `crates-io` each enable only their namesake
protected job.

The checksummed GitHub prerelease is also a repository mutation. Its
`github-release` environment requires an operator review and a `v*` tag policy;
it stores no secret and gates only the job-scoped `GITHUB_TOKEN`.

The RC.5 train starts from the canonical source tag recorded as
`publication.sourceTag` in `release/version.json`. The workflow grants
`id-token: write` only to the crates.io job, obtains a short-lived token with
`rust-lang/crates-io-auth-action@v1`, and revokes that token after the job. If
a workflow-only correction is required before a coordinate is published, use
the next positive `v4.0.0-rc.6-release.N` tag; never move an existing tag.

```bash
gh workflow run release-bindings.yml \
  --repo vinary-tree/lling-llang \
  --ref v4.0.0-rc.6 \
  -f registry=validate-only

gh workflow run release-bindings.yml \
  --repo vinary-tree/lling-llang \
  --ref v4.0.0-rc.6 \
  -f registry=npm
```

Use the same reviewed source ref with `registry=crates-io`. A corrective ref may
publish only while the exact coordinate remains absent; a public RC.5 artifact
must never be rebuilt or overwritten.

Publish crates.io only after its exact Rust dependencies resolve publicly.
Publish npm only after `@vinary-tree/vinary-tree-interop` and the shared
`@vinary-tree/javascript-runtime` resolve at `4.0.0-rc.6`. The npm job uses
trusted publishing, provenance, the `next` dist-tag, and the protected `npm`
environment.

## Public-byte verification and recovery

Install the npm coordinate in a clean directory and exercise WFST creation,
arc traversal, composition, iteration, and deterministic resource closure.
After that smoke test, move the new scoped package's `latest` tag to the RC,
remove `bootstrap`, and deprecate `0.0.0` as reservation-only.

Tags and published versions are immutable. Rerun validation safely with
`registry=validate-only`. If a registry accepted incorrect bytes, fix the
source and issue the next unused candidate; never move the tag or overwrite
the version.
