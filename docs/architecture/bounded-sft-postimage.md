# Bounded exact SFT post-image

The post-image of an input language under a symbolic finite transducer
(SFT) is the set of all output words produced by accepted SFT runs on
accepted input words. `symbolic::bounded_postimage::BoundedSftPostimage`
constructs that language by exploring reachable pairs of input-automaton
and SFT states. A product state accepts only when both components accept.

The result is an `OutputAutomaton`, an NFA with **explicit epsilon edges**.
A plain `SymbolicAutomaton` has only single-symbol predicate transitions:
using a `TRUE` guard for epsilon consumes a symbol, and using one guard for
a two-symbol constant loses an output position. The previous `post_image`
method made those incorrect exactness claims and has been removed. A
constant output of length two now forms two guarded edges through one
intermediate state; an empty output makes one epsilon edge.

```text
input SFA --compatible guards-- SFT transition
      |                              |
      +--------- product state ------+
                    |
           exact image oracle cases
                    |
        epsilon edge or predicate chain
                    |
            output automaton
```

For one input guard `g` and output function `f`, an
`ExactOutputPostimageOracle` returns a finite union of *rectangular cases*.
Each case has an input subguard and an output word of predicates. A case
asserts that every word satisfying that predicate sequence is an output
of `f` for each input in the subguard; the union must contain **all and
only** outputs for inputs satisfying `g`. Overlapping cases preserve
nondeterminism. The adapter discards unsatisfiable input intersections
and unsatisfiable output predicates; it does not replace an unavailable
case with a broad guard. The oracle and content semantics must be bound
to stable nonzero digests supplied by the caller.

`SameAlgebraPostimageOracle` covers these functions exactly when the input
and output use the same Boolean algebra and output constants have exact
singleton predicates:

- Epsilon returns an empty predicate word.
- A constant sequence returns one singleton predicate per element,
  including zero and multiple elements.
- Identity returns the intersected input guard as its output predicate,
  but only if both algebra instances interpret predicates identically.
  Different bounded-integer universes are rejected explicitly.
- Opaque map and flat-map closures return typed `UnrepresentableOutput`.
  A caller with an exact finite relational decomposition may supply its
  own oracle and bind that semantic assumption separately.

`ExactSingletonPredicate` is implemented for the character-class and
bounded-integer-interval algebras. An integer outside an interval algebra's
universe has no singleton there and causes `UnrepresentableConstant`; no
word outside the declared alphabet is silently accepted. Other algebras
can implement this trait when exact singletons exist. The built-in oracle
does not infer closure behavior from finite tests.

The construction is deterministic in source-transition order and initial
state order:

```text
index SFT and input-SFA transitions, one charged arc at a time
sort and seed all initial state pairs
while the product frontier is nonempty:
    preflight a product-state visit
    intersect each pair of outgoing input guards
    ask the exact oracle for finite output-word cases
    validate reachable targets and calculate all output-chain costs
    poll cancellation and charge the entire expansion atomically
    append first-discovered product states and output chains
return Complete only when the frontier is empty
```

Indexing, source-pair inspections, product-state visits, output edges,
intermediate states, abstract work and retained logical heap are charged
through the shared operation contract. Caller-metered predicate payloads
are added to the heap charge; allocator slack, transient oracle storage
and process RSS are not bounded by that logical meter. Time and
cancellation are checked at progress boundaries. A rejected expansion
does not mutate the frontier or output graph. `Incomplete` is not a
complete-language assertion, and dynamic results are excluded from the
generic complete-only cache. Resumption requires the exact checkpoint
and the same live in-memory frontier, state map and result graph; a
checkpoint alone cannot reconstruct the operation.

`OutputAutomaton::accepts` evaluates concrete words by iterative epsilon
closure after the initial state set and after each consumed symbol. It
uses no native recursion. Custom Boolean algebras and oracles must uphold
their semantic contracts; an arbitrary oracle invocation itself is not
preemptible. `tests/bounded_sft_postimage.rs` compares shallow outputs
against independent recursive source-path enumeration, tests epsilon and
multi-symbol constants, a known-exact custom computed-output oracle,
unsupported-output rejection, limits, cancellation, resumption, malformed
targets and cache exclusion.
