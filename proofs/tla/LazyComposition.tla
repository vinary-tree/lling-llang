---------------------------- MODULE LazyComposition ----------------------------
(* Lazy WFST Composition specification.
 *
 * This module specifies the lazy composition algorithm and verifies
 * that memory usage is bounded by the cache size.
 *
 * Corresponds to src/composition/lazy.rs in lling-llang.
 *
 * Lazy composition computes composed states on-demand, caching results
 * to avoid recomputation. The cache policy determines memory bounds.
 *)

EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
    CacheSize,          \* Maximum number of cached states
    CacheMode,          \* "CacheAll", "Lru", or "NoCache"
    MaxStates1,         \* States in first FST
    MaxStates2,         \* States in second FST
    MaxLabels           \* Maximum label alphabet size

VARIABLES
    cache,              \* Set of computed product states
    worklist,           \* States to be explored
    currentState,       \* Current product state being processed
    accessOrder,        \* Access order for LRU (sequence)
    processed           \* States already processed, independent of cache

vars == <<cache, worklist, currentState, accessOrder, processed>>

(***************************************************************************
 * Type Definitions
 ***************************************************************************)

\* A product state is a tuple (state1, state2, filter)
ProductState == (1..MaxStates1) \X (1..MaxStates2) \X {"None", "Eps1", "Eps2"}

\* Filter states for epsilon handling
FilterState == {"None", "Eps1", "Eps2"}

CacheModes == {"CacheAll", "Lru", "NoCache"}
Label == 1..MaxLabels
MaybeLabel == 0..MaxLabels
SeqToSet(seq) == { seq[i] : i \in 1..Len(seq) }

(***************************************************************************
 * Type Invariants
 ***************************************************************************)

TypeOK ==
    /\ cache \subseteq ProductState
    /\ worklist \subseteq ProductState
    /\ (currentState = "None" \/ currentState \in ProductState)
    /\ accessOrder \in Seq(ProductState)
    /\ processed \subseteq ProductState
    /\ CacheMode \in CacheModes
    /\ CacheSize \in Nat
    /\ MaxStates1 >= 1
    /\ MaxStates2 >= 1
    /\ MaxLabels >= 1

(***************************************************************************
 * Initial State
 ***************************************************************************)

Init ==
    /\ cache = {}
    /\ worklist = {<<1, 1, "None">>}  \* Start at (start1, start2, None)
    /\ currentState = "None"
    /\ accessOrder = <<>>
    /\ processed = {}

(***************************************************************************
 * Helper Functions
 ***************************************************************************)

\* Check if state is in cache
InCache(state) == state \in cache

\* Synthetic bounded transition systems for the two component FSTs.  This is
\* still a finite TLC model, but it now models label matching and epsilon moves
\* instead of just incrementing product-state coordinates.
Transitions1(s) ==
    (IF s < MaxStates1 THEN
        { [to |-> s + 1, inp |-> l, outp |-> l] : l \in Label }
        \cup
        { [to |-> s + 1, inp |-> l, outp |-> 0] : l \in Label }
     ELSE {})

Transitions2(s) ==
    (IF s < MaxStates2 THEN
        { [to |-> s + 1, inp |-> l, outp |-> l] : l \in Label }
        \cup
        { [to |-> s + 1, inp |-> 0, outp |-> l] : l \in Label }
     ELSE {})

CanEps1(filt) == filt # "Eps2"
CanEps2(filt) == filt # "Eps1"
CanMatch(filt) == filt = "None"

NextFilterEps1(filt) == "Eps1"
NextFilterEps2(filt) == "Eps2"
NextFilterMatch(filt) == "None"

\* Get states reachable from a product state by epsilon or matching-label moves.
Successors(state) ==
    LET s1 == state[1]
        s2 == state[2]
        filt == state[3]
    IN
    (IF CanEps1(filt) THEN
        { <<t1.to, s2, NextFilterEps1(filt)>> :
            t1 \in {tr \in Transitions1(s1) : tr.outp = 0} }
     ELSE {}) \cup
    (IF CanEps2(filt) THEN
        { <<s1, t2.to, NextFilterEps2(filt)>> :
            t2 \in {tr \in Transitions2(s2) : tr.inp = 0} }
     ELSE {}) \cup
    (IF CanMatch(filt) THEN
        UNION {
            { <<t1.to, t2.to, NextFilterMatch(filt)>> :
                t2 \in {tr \in Transitions2(s2) :
                    tr.inp \in Label /\ tr.inp = t1.outp} } :
            t1 \in {tr \in Transitions1(s1) : tr.outp \in Label} }
     ELSE {})

\* Add state to cache, evicting if necessary
AddToCache(state) ==
    IF CacheMode = "NoCache" THEN
        {}
    ELSE IF CacheMode = "CacheAll" THEN
        cache \cup {state}
    ELSE IF CacheSize = 0 THEN
        {}
    ELSE IF Cardinality(cache) < CacheSize THEN
        cache \cup {state}
    ELSE
        \* LRU eviction: remove the least-recently accessed cached state.
        LET cachedOrder == SelectSeq(accessOrder, LAMBDA s : s \in cache)
            evicted ==
                IF Len(cachedOrder) = 0
                THEN CHOOSE old \in cache : TRUE
                ELSE cachedOrder[1]
        IN (cache \ {evicted}) \cup {state}

(***************************************************************************
 * Cache Policies
 ***************************************************************************)

\* Cache-All policy: never evict (bounded by finite product-state space)
CacheAllPolicy == CacheMode = "CacheAll"

\* LRU policy: evict least recently used when full
LruPolicy == CacheMode = "Lru"

\* No-Cache policy: keep no composed states in the cache.  The separate
\* processed set is termination bookkeeping, not cache storage.
NoCachePolicy == CacheMode = "NoCache"

(***************************************************************************
 * State Transitions
 ***************************************************************************)

\* Process a state from the worklist
ProcessState ==
    /\ worklist # {}
    /\ \E state \in worklist :
       /\ currentState' = state
	       \* Compute successors
	       /\ LET succs == Successors(state)
	              newStates == { s \in succs : s \notin processed /\ s # state }
	          IN
	          /\ worklist' = (worklist \ {state}) \cup newStates
	          \* Add current state to cache
	          /\ cache' = AddToCache(state)
	          /\ processed' = processed \cup {state}
	          \* Update access order for LRU
	          /\ accessOrder' = Append(
	               SelectSeq(accessOrder, LAMBDA s : s # state),
	               state)

\* Mark completion when worklist is empty
Complete ==
    /\ worklist = {}
    /\ UNCHANGED vars

\* Next state
Next == ProcessState \/ Complete

(***************************************************************************
 * Invariants and Properties
 ***************************************************************************)

\* MAIN INVARIANT: Memory is bounded by cache size
MemoryBounded ==
    IF CacheMode = "CacheAll" THEN
        Cardinality(cache) <= Cardinality(ProductState)
    ELSE
        Cardinality(cache) <= CacheSize

\* All cached states are valid product states
CacheValid ==
    \A state \in cache :
        /\ state[1] \in 1..MaxStates1
        /\ state[2] \in 1..MaxStates2
        /\ state[3] \in FilterState

\* Worklist only contains valid states
WorklistValid ==
    \A state \in worklist :
        /\ state[1] \in 1..MaxStates1
        /\ state[2] \in 1..MaxStates2
	        /\ state[3] \in FilterState

\* No state is processed twice; this is independent of whether cache entries
\* have been evicted.
NoDuplicateProcessing ==
    processed \cap worklist = {}

NoCacheEmpty ==
    CacheMode = "NoCache" => cache = {}

ProcessedValid ==
    processed \subseteq ProductState

AccessOrderValid ==
    /\ \A i \in 1..Len(accessOrder) : accessOrder[i] \in processed
    /\ \A i, j \in 1..Len(accessOrder) :
        i # j => accessOrder[i] # accessOrder[j]

CacheCoveredByAccessOrder ==
    cache \subseteq SeqToSet(accessOrder)

(***************************************************************************
 * Liveness Properties
 ***************************************************************************)

\* Eventually, the worklist becomes empty (termination)
\* This requires the composition to be finite (no infinite paths)
EventuallyComplete ==
    <>(worklist = {})

(***************************************************************************
 * Specification
 ***************************************************************************)

Spec == Init /\ [][Next]_vars

\* Fairness for termination
Fairness == WF_vars(ProcessState)

FairSpec == Spec /\ Fairness

(***************************************************************************
 * Theorems to Verify
 ***************************************************************************)

\* Safety: memory is always bounded
THEOREM Spec => []MemoryBounded

\* Type correctness
THEOREM Spec => []TypeOK

\* Cache validity
THEOREM Spec => []CacheValid

\* Processed-set validity
THEOREM Spec => []ProcessedValid

\* No-cache mode really keeps no cached states
THEOREM Spec => []NoCacheEmpty

\* Access-order bookkeeping remains consistent with processed/cache sets
THEOREM Spec => []AccessOrderValid
THEOREM Spec => []CacheCoveredByAccessOrder

\* With fairness, composition terminates
THEOREM FairSpec => EventuallyComplete

=============================================================================
