# Host-provider resource lifecycle

This document specifies the shared resource protocol used when a host program
supplies a dictionary, weighted finite-state transducer (WFST), lattice, or
semiring to a Vinary Tree consumer. The [Vinary Tree Interop C
header](https://github.com/vinary-tree/vinary-tree-interop/blob/19b1774/include/vinary_tree_interop.h)
defines the binary interface. The [finite state
model](../../proofs/tla/HostProviderLifecycle.tla) checks the transitions that
join ownership, version negotiation, snapshots, callbacks, tokens, and entry
cursors. The model adds no public ABI declaration.

![Sequence diagram: a caller transfers an owned resource to a consumer, which validates and retains it, negotiates an interface through a context gate, captures an immutable snapshot, and then uses a bounded WFST page, a generational algebra token, or a leased dictionary cursor before releasing ownership.](../diagrams/architecture/host-provider-lifecycle.svg)

## Vocabulary and authority

| Term | Meaning |
|---|---|
| Resource | The two-word `VtResource`: an opaque `context` pointer and a pointer to its base vtable. The provider owns the allocation behind the context. |
| Base vtable | The versioned `VtResourceVTable` containing `retain`, `release`, and `query_interface`. Its `struct_size` and `abi_version` are checked before a call. |
| Provider | The foreign implementation of the callbacks. It owns context storage and determines its concurrency claim. |
| Consumer | Code that validates raw provider replies and owns every retain it acquires. `lling-llang` is a consumer in [`src/bindings.rs`](../../src/bindings.rs). |
| Snapshot | An owned immutable revision returned by a provider callback. It may retain the live context or return a distinct context. |
| Context gate | The admission object keyed by opaque context identity. It serializes discovery before flags are known and later serial callbacks across aliases; a valid parallel claim permits overlapping interface callbacks. |
| Token | A compact semiring value owned by exactly one retained operation context. An arena-backed token includes a slot and a nonzero generation. Copying its bits does not clone ownership. |
| Cursor and batch lease | An entry cursor streams one snapshot. A successful batch call lends its storage until the matching generation is released. |
| Status | A raw integer reply decoded as `VtStatus` only after validating its discriminant. An error result cannot publish a partial output. |

The base resource contract and its retain law are specified separately by
[`AbiOwnershipLifecycle.tla`](../../proofs/tla/AbiOwnershipLifecycle.tla).
The context gate's waiter protocol and alias identity are specified by
[`SerialProviderTurnstile.tla`](../../proofs/tla/SerialProviderTurnstile.tla)
and
[`SerialProviderAliasRegistry.tla`](../../proofs/tla/SerialProviderAliasRegistry.tla).
The host-provider model checks their *cross-boundary composition*: a callback
or batch operation cannot outlive the snapshot retain that authorizes it.

## Protocol

The consumer validates the base vtable before invoking any provider function.
It obtains one retain for each owned base or snapshot reference. At every
point in the model, the ledger law is
$`\mathrm{retains}=|B|+|S|`$, where $`B`$ is the set of base owners and $`S`$
is the set of snapshot owners. Explicit release and a safe facade finalizer
perform the same decrement. A snapshot retain remains live even if the base
owner releases its reference.

`query_interface` receives an interface identifier and a minimum version. A
supported version returns `Ok` and a validated vtable. An unsupported version
returns `Unsupported` and leaves the output pointer unchanged. An earlier
successful negotiation remains valid if a later request asks for an unavailable
version. The consumer does not infer support from a non-null output left by an
earlier call; it checks the status for *this* request and validates the returned
vtable before use. Capture pins one snapshot identity and either preserves the
base context identity or establishes a new context identity. The context gate
uses the snapshot's actual identity, not the address of a copied handle.

For ordinary operations, an unclaimed provider is serial. A
`PARALLEL_REENTRANT` claim permits concurrent calls and same-thread recursion
for that interface. A `THREAD_BOUND` algebra admits only its owning thread.
The caller must not combine contradictory flag claims; the consumer rejects
them at capture. The model represents one active callback per thread, with
depth two sufficient to exercise nested reentrancy. The exact parked-waiter,
lost-wakeup, and cross-provider-cycle rules remain in the turnstile model and
its generated tests; the host model checks admission mode and live snapshot
ownership at the call boundary.

An arena token is accepted only if its presented generation matches the
current live slot, its context owner matches, and the issuing snapshot is
retained. Reusing a freed slot increases the generation; passing the old
generation must return `InvalidArgument`. The bounded model explicitly
chooses a presented generation independently of the current one, so the stale
case is reachable after reuse. The actual ABI uses provider-defined compact
words and requires clone/release callbacks for owned tokens.

An entry cursor moves through a finite snapshot in pages. For a page with
capacity $`k`$ and $`r`$ remaining entries, the model writes
$`\min(k,r)`$ complete entries, advances by that count, and creates one
generation-tagged lease. No second page starts before `release_batch` settles
the first lease. A call with no remaining entries returns `End`. Cancellation
is sticky, and cancellation during a live batch returns `BatchInUse` without
invalidating the outstanding lease. A caller closes the cursor after its lease
has settled. The consumer checks raw counts, bounds, and progress before
exposing a page; provider-owned batch memory expires on release.

Every callback reports a raw status. Only `Ok` publishes the result of a
successful operation. `End`, `Unsupported`, `InvalidArgument`, `Closed`,
`BatchInUse`, and `ProviderError` leave the previous output intact. The model
records that output at the error transition and checks equality afterward.
The [ABI security model](https://github.com/vinary-tree/vinary-tree-interop/blob/19b1774/docs/security-model.md)
specifies the further byte-level validation required for pointers, lengths,
reserved fields, labels, and out-of-range status integers.

## Model checks and limits

The [verification script](../../proofs/verify.sh) runs five finite
configurations and a non-vacuous parallel-overlap witness. The checked
parameters are deliberately explicit:

| Configuration | Clients | Threads | Snapshot context | Entries | Token generations | Distinct states in the local bounded run |
|---|---:|---:|---|---:|---:|---:|
| Serial | 1 | 2 | aliases base | 1 | 2 | 4,528 |
| Parallel | 1 | 2 | distinct | 1 | 2 | 12,880 |
| ThreadBound | 1 | 2 | distinct | 1 | 2 | 3,136 |
| Owners | 2 | 1 | aliases base | 0 | 1 | 605,488 |
| Fair | 1 | 1 | aliases base | 1 | 2 | 3,136 |

Each modeled client acquires one base resource and captures one snapshot.
Those finite bounds keep the combined state space checkable. The broader
owner transfer/clone law, multiple snapshot aliases, and exact waiter handoff
are checked by the focused models linked above. The positive checks establish
safety only within their declared bounds; they are not an unbounded proof of
arbitrary provider code or allocator behavior.

The Fair configuration also checks `CursorEventuallyClosed`. Its assumption
is weak fairness for the *caller's* `release_batch` and `close` actions: if one
remains continuously enabled, the caller eventually performs it. Without
that assumption a caller may retain a batch forever, and neither the ABI nor
the provider can force closure. No fairness is assumed for arbitrary foreign
callbacks that block without returning.

Five exact-source fault mutations test that the safety checks are sensitive
to their intended failures:

| Injected fault | Expected failed check |
|---|---|
| Omit the snapshot retain | `RetainsEqualOwners` |
| Admit a second serial callback | `SerialAdmission` |
| Report zero capacity for a nonempty page | `PageBounded` |
| Change the output on a failed callback | `ErrorDoesNotPublish` |
| Accept an old token generation after slot reuse | `TokenUseStatusLaw` |

The separate parallel witness intentionally violates `NoParallelOverlap`.
That counterexample demonstrates that the `Parallel` configuration reaches
real overlapping callback states instead of passing only because callbacks
never start.

## Boundary of the abstraction

The model treats `retain` and `release` as atomic, infallible calls because
their C signatures have no status channel. It does not dereference foreign
pointers, assert algebraic laws, or establish that a malicious native library
cannot corrupt its own address space. Those behaviors need executable
provider/consumer tests and the validation duties in the ABI security model.
`THREAD_BOUND` is represented as one owning thread; handoff across host
runtimes must be qualified by each binding. The model's `ProviderError` is an
abstract contained failure; a panic or foreign exception must be contained by
the producing language binding before it reaches the C ABI.
