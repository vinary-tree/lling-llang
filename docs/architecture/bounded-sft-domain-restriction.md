# Bounded SFT domain restriction

Domain restriction keeps an SFT's input/output relation only for input
words accepted by a symbolic finite automaton (SFA) over the same input
alphabet. `symbolic::bounded_restrict::BoundedSftRestriction` constructs
the reachable SFT/SFA product and retains each SFT output function
unchanged. The result is a transducer, not an output-language automaton.

An SFT transition and an input-SFA transition can be paired exactly when
their input-guard conjunction is satisfiable. Their product transition
uses that conjunction and the original SFT output function; the product
state accepts only if both component states accept. Thus the restricted
relation is the original SFT relation intersected with the SFA's input
language, without collapsing nondeterministic output alternatives.

This conjunction is valid only when the two algebra *instances* interpret
predicates identically. `ExactAlgebraSemantics::same_semantics` is a
soundness contract, not an equality check on Rust types. Character-class
algebras have one common Unicode universe; bounded-integer interval
algebras require matching lower and upper bounds. Mismatched instances
return `IncompatibleAlgebras` before any product work. Other algebras may
implement the trait with their own exact compatibility proof.

```text
source SFT state ---- SFT input guard/output function ---- target SFT state
       |                       AND                               |
input SFA state ---- input-SFA guard -------------------- target SFA state
       |                                                         |
 product source ----- conjoined guard/original output --> product target
```

The algorithm indexes both source transition vectors incrementally,
seeds sorted initial-state pairs, and processes the reachable product
frontier iteratively:

```text
for each product state in discovery order:
    preflight one state visit
    for each SFT outgoing edge in source order:
        for each input-SFA outgoing edge in source order:
            intersect input guards and discard empty intersections
            validate reachable targets
            retain the exact SFT output function
    calculate work, arc and retained-heap costs for the entire batch
    poll cancellation and charge atomically
    append first-discovered product states and ordered transitions
return Complete only after the frontier is empty
```

`OperationLimits` bound accepted product-state visits, indexed and
inspected arcs, abstract work, caller-metered logical heap, and elapsed
time. The predicate and output meters supplied to `run` account for
payloads beyond the inline types; constant-output vector storage is also
charged. They must be pure and stable across resume. Logical heap does
not bound allocator slack, transient batch storage or process RSS. A
failed batch leaves the partial product graph and frontier unchanged.
`Incomplete` remains distinct from `Complete`; dynamic results cannot
enter the generic complete-only cache. Resume needs the exact checkpoint
and the same live frontier, state map, sources and result. A checkpoint
alone does not serialize the operation.

This path rejects malformed reachable endpoints and stale content
bindings rather than publishing a false-complete transducer. Its
callback output functions are preserved by `Arc` clone and must remain
semantically stable under the caller's SFT binding. The legacy
`restrict_domain` convenience method remains unbounded and does not
provide this resource or malformed-source contract.

`tests/bounded_sft_restriction.rs` checks the relation against independent
shallow source-path enumeration, all five output-function variants,
input-guard narrowing, mismatched instance semantics, resource and
cancellation interruption, atomicity, resume, cache exclusion, typed
malformed targets and a 2,048-state product walk on a 128 KiB thread
stack.
