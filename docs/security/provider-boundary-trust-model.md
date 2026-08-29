# Provider Boundary Trust Model

This document defines the security boundary for provider-generated artifacts
and assurance claims. The companion
[provider-neutral contract](../optimization/provider-boundary-contract.md)
defines the algebra and lifecycle; this document identifies adversarial inputs,
authority, rejection behavior, and residual assumptions.

## Protected claims

The boundary protects five claims:

1. an exact result was complete and exact when produced;
2. evidence refers to the requested immutable inputs and execution context;
3. an exact guarantee is independent of the candidate producer;
4. incomplete results never enter a complete-result cache; and
5. native resources are neither double-released nor released while borrowed.

## Trust domains

An actor identifier answers “which actor spoke?” A control-domain identifier
answers “which authority controls that actor?” Independence is decided on the
second question. This prevents a producer from creating a second actor label
and using it to validate its own report.

The policy relation must conservatively group actors sharing any relevant
administrative authority, signing key, process boundary, deployment pipeline,
or mutable evidence store. Mere process separation is insufficient when both
processes share control.

## Threats and mandatory responses

| Threat | Required response |
|---|---|
| provider labels an incomplete result exact | reject exact publication; preserve reason, partial payload, and checkpoint |
| adapter drops approximation limitations | reject the adapted result as invalid |
| artifact bytes disagree with the URI or declared digest | reject before evidence evaluation |
| configuration, provider, environment, input, or result identity changes | classify prior evidence as stale |
| verifier has a distinct name but shares the producer's control domain | reject exact publication |
| untrusted verifier signs a fresh result | reject exact publication |
| incomplete result reaches cache insertion | reject the cache operation |
| consumer requests provider-private state | reject the dependency at design and compile boundaries |
| provider depends on consumer internals | reject the reverse dependency |
| release occurs at borrow count zero | return failure without decrementing |
| destruction occurs while borrowed or by the consumer | reject destruction |

## Fail-closed ordering

Validation proceeds from cheap structural checks to authority-bearing checks.
The purpose is to minimize attacker-controlled work while returning a stable,
non-secret-dependent reason class.

```text
VALIDATE-EXACT(requested, candidate, guarantee)
  REQUIRE candidate.status = CompleteExact
  REQUIRE VALID-ARTIFACT(candidate.artifact)
  REQUIRE candidate.binding = requested
  REQUIRE guarantee.binding = requested
  REQUIRE guarantee.trusted under current policy
  REQUIRE candidate.producer.control_domain
          differs from guarantee.verifier.control_domain
  ACCEPT exact publication
```

Every failed requirement rejects. No later check may repair an earlier failure.
Diagnostics may identify the failed coordinate, but they must not disclose
private provider state or secret digest material.

## Dependency threat boundary

lling-llang is independent of libcpg. lling-llang never loads, links against,
imports, or reaches into libcpg as part of this campaign. An optional libcpg
adapter may call lling-llang's public API. That adapter cannot receive privileged
access merely because both projects use the same provider-neutral foundation
crate.

Shared crates own only generic vocabulary and mechanics. Product semantics,
private caches, scheduling policy, internal graphs, and optimization state stay
with their respective projects.

## Resource safety

All provider and evidence control paths use finite iterative state machines.
Variable-size manifests, limitations, and diagnostic collections live on the
heap and are traversed iteratively. Resource limits must be checked before
allocation and charged monotonically. Cancellation or a resource limit yields
`Incomplete`; it never supplies evidence of completion.

Foreign calls execute without registry write locks or other global locks held.
Opaque handle ownership is stable across borrow and release transitions. The
reference-counting background is documented by
[Collins 1960](../BIBLIOGRAPHY.md).

## Audit evidence

Acceptance evidence must include:

- the exact provider descriptor identity;
- the complete six-coordinate evidence binding;
- the candidate completion class and limitations;
- verifier identity, control domain, policy version, and trust decision;
- the named property covering the decision;
- the positive TLC state count and all negative-control failures; and
- resource-cap settings used by verification.

Logs and generated model metadata remain below persistent repository-local
`target/formal-verification/`. They must never use memory-backed temporary
storage.

## Residual assumptions

The formal model does not prove cryptographic collision resistance, operating
system process isolation, correctness of a third-party signature
implementation, or honesty of policy configuration. It proves that once
identities and policy decisions enter the boundary, stale, dependent,
approximate, and incomplete claims cannot satisfy the exact-publication rules.
