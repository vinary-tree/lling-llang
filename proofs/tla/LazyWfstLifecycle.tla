-------------------------- MODULE LazyWfstLifecycle --------------------------
(***************************************************************************)
(* Abstract state machine for LazyWfstWrapper cache-policy transitions.      *)
(* A state has at most one live representation: persistent cache or the      *)
(* single transient slot used by NoCache and zero-capacity LRU.  The model   *)
(* also covers policy changes, explicit cache clearing, LRU eviction, and    *)
(* saturating computation-accounting at a finite model bound.                *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS States, Capacity, MaxComputations, NONE

VARIABLES policy, cache, transient, accessOrder, computedCount
vars == <<policy, cache, transient, accessOrder, computedCount>>

Policies == {"CacheAll", "Lru", "NoCache"}
UsesTransient(selected) ==
  selected = "NoCache" \/ (selected = "Lru" /\ Capacity = 0)

SeqToSet(sequence) == {sequence[index] : index \in 1..Len(sequence)}
FiniteOrders ==
  UNION {[1..length -> States] : length \in 0..Cardinality(States)}

ValidLruOrder(sequence, cached) ==
  /\ sequence \in FiniteOrders
  /\ SeqToSet(sequence) = cached
  /\ Len(sequence) = Cardinality(cached)

TypeOK ==
  /\ policy \in Policies
  /\ cache \subseteq States
  /\ transient \in States \cup {NONE}
  /\ accessOrder \in Seq(States)
  /\ computedCount \in 0..MaxComputations

Init ==
  /\ policy = "CacheAll"
  /\ cache = {}
  /\ transient = NONE
  /\ accessOrder = <<>>
  /\ computedCount = 0

Available(state) == state \in cache \/ transient = state

Expand(state) ==
  /\ state \in States
  /\ ~Available(state)
  /\ computedCount < MaxComputations
  /\ IF UsesTransient(policy)
        THEN /\ cache' = {}
             /\ transient' = state
        ELSE IF policy = "CacheAll"
          THEN /\ cache' = cache \cup {state}
               /\ transient' = NONE
          ELSE /\ Capacity > 0
               /\ LET retained ==
                    IF Cardinality(cache) < Capacity
                    THEN cache
                    ELSE cache \ {Head(accessOrder)}
                  IN /\ cache' = retained \cup {state}
                     /\ accessOrder' =
                          Append(
                            SelectSeq(accessOrder,
                              LAMBDA cached : cached \in retained),
                            state)
               /\ transient' = NONE
  /\ IF policy # "Lru" \/ UsesTransient(policy)
        THEN accessOrder' = <<>>
        ELSE TRUE
  /\ computedCount' = computedCount + 1
  /\ UNCHANGED policy

Observe(state) ==
  /\ state \in States
  /\ Available(state)
  /\ IF policy = "Lru" /\ state \in cache
        THEN accessOrder' =
               Append(
                 SelectSeq(accessOrder, LAMBDA cached : cached # state),
                 state)
        ELSE UNCHANGED accessOrder
  /\ UNCHANGED <<policy, cache, transient, computedCount>>

ClearCache ==
  /\ (cache # {} \/ transient # NONE)
  /\ cache' = {}
  /\ transient' = NONE
  /\ accessOrder' = <<>>
  /\ UNCHANGED <<policy, computedCount>>

SetTransientPolicy(selected) ==
  /\ selected \in Policies
  /\ selected # policy
  /\ UsesTransient(selected)
  /\ policy' = selected
  /\ cache' = {}
  /\ transient' = NONE
  /\ accessOrder' = <<>>
  /\ UNCHANGED computedCount

SetCacheAll ==
  /\ policy # "CacheAll"
  /\ policy' = "CacheAll"
  /\ transient' = NONE
  /\ accessOrder' = <<>>
  /\ UNCHANGED <<cache, computedCount>>

SetPositiveLru ==
  /\ Capacity > 0
  /\ policy # "Lru"
  /\ \E retained \in SUBSET cache :
       /\ IF Cardinality(cache) <= Capacity
             THEN retained = cache
             ELSE Cardinality(retained) = Capacity
       /\ policy' = "Lru"
       /\ cache' = retained
       /\ transient' = NONE
       /\ ValidLruOrder(accessOrder', retained)
       /\ UNCHANGED computedCount

Next ==
  \/ \E state \in States : Expand(state)
  \/ \E state \in States : Observe(state)
  \/ ClearCache
  \/ \E selected \in Policies : SetTransientPolicy(selected)
  \/ SetCacheAll
  \/ SetPositiveLru

Spec == Init /\ [][Next]_vars

NoCacheHasNoPersistentEntries == policy = "NoCache" => cache = {}
ZeroLruHasNoPersistentEntries ==
  policy = "Lru" /\ Capacity = 0 => cache = {}
PositiveLruIsBounded ==
  policy = "Lru" /\ Capacity > 0 => Cardinality(cache) <= Capacity
TransientIsNotDuplicated == transient = NONE \/ transient \notin cache
CacheAllHasNoTransientEntry == policy = "CacheAll" => transient = NONE
ComputationCountIsBounded == computedCount <= MaxComputations
LruOrderIsExact == policy = "Lru" /\ Capacity > 0 =>
  ValidLruOrder(accessOrder, cache)
NonLruOrderIsEmpty == policy # "Lru" => accessOrder = <<>>

=============================================================================
