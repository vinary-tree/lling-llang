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
    MaxStates1,         \* States in first FST
    MaxStates2,         \* States in second FST
    MaxLabels           \* Maximum label alphabet size

VARIABLES
    cache,              \* Set of computed product states
    worklist,           \* States to be explored
    currentState,       \* Current product state being processed
    accessOrder         \* Access order for LRU (sequence)

vars == <<cache, worklist, currentState, accessOrder>>

(***************************************************************************
 * Type Definitions
 ***************************************************************************)

\* A product state is a tuple (state1, state2, filter)
ProductState == (1..MaxStates1) \X (1..MaxStates2) \X {"None", "Eps1", "Eps2"}

\* Filter states for epsilon handling
FilterState == {"None", "Eps1", "Eps2"}

(***************************************************************************
 * Type Invariants
 ***************************************************************************)

TypeOK ==
    /\ cache \subseteq ProductState
    /\ worklist \subseteq ProductState
    /\ (currentState = "None" \/ currentState \in ProductState)
    /\ accessOrder \in Seq(ProductState)

(***************************************************************************
 * Initial State
 ***************************************************************************)

Init ==
    /\ cache = {}
    /\ worklist = {<<1, 1, "None">>}  \* Start at (start1, start2, None)
    /\ currentState = "None"
    /\ accessOrder = <<>>

(***************************************************************************
 * Helper Functions
 ***************************************************************************)

\* Check if state is in cache
InCache(state) == state \in cache

\* Get states reachable from a product state in this bounded monotone model.
Successors(state) ==
    LET s1 == state[1]
        s2 == state[2]
        filt == state[3]
    IN
    (IF s1 < MaxStates1 THEN {<<s1 + 1, s2, filt>>} ELSE {}) \cup
    (IF s2 < MaxStates2 THEN {<<s1, s2 + 1, filt>>} ELSE {})

\* Add state to cache, evicting if necessary
AddToCache(state) ==
    IF CacheSize = 0 THEN
        {}
    ELSE IF Cardinality(cache) < CacheSize THEN
        cache \cup {state}
    ELSE
        \* Bounded eviction: remove one cached state before inserting.
        LET evicted == CHOOSE old \in cache : TRUE
        IN (cache \ {evicted}) \cup {state}

(***************************************************************************
 * Cache Policies
 ***************************************************************************)

\* Cache-All policy: never evict (bounded by input sizes)
CacheAllPolicy == CacheSize >= MaxStates1 * MaxStates2 * 3

\* LRU policy: evict least recently used when full
LruPolicy == TRUE  \* Always applicable

\* No-Cache policy: don't cache (recompute each time)
NoCachePolicy == CacheSize = 0

(***************************************************************************
 * State Transitions
 ***************************************************************************)

\* Process a state from the worklist
ProcessState ==
    /\ worklist # {}
    /\ LET state == CHOOSE s \in worklist : TRUE
       IN
       /\ currentState' = state
       \* Compute successors
       /\ LET succs == Successors(state)
              newStates == { s \in succs : ~InCache(s) }
          IN
          /\ worklist' = (worklist \ {state}) \cup newStates
          \* Add current state to cache
          /\ cache' = AddToCache(state)
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

\* No state is processed twice (with caching)
NoDuplicateProcessing ==
    \A state \in cache : state \notin worklist

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

\* With fairness, composition terminates
THEOREM FairSpec => EventuallyComplete

=============================================================================
