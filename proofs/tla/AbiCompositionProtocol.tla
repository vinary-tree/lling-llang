---------------------- MODULE AbiCompositionProtocol ----------------------
(***************************************************************************)
(* The concurrency protocol of lling-llang's lazy WFST composition         *)
(* (src/bindings.rs, CompositionResource::state). Several reader threads    *)
(* expand product states concurrently through the re-exported vtable. Each  *)
(* thread runs this sequence:                                               *)
(*                                                                          *)
(*   1. call the foreign left/right provider callbacks holding NO           *)
(*      composition lock (phase "callProviders" -- src/bindings.rs:400-401  *)
(*      read the cache/registry only under short-lived read guards that are *)
(*      released before the callbacks);                                     *)
(*   2. take the registry WRITE lock and register neighbour product states  *)
(*      -- pure in-memory work, no provider callbacks (phases "acqReg" and  *)
(*      "register", src/bindings.rs:418-482);                               *)
(*   3. DROP the registry lock (src/bindings.rs:483) BEFORE taking the      *)
(*      cache write lock (:490), so no thread ever holds both.              *)
(*                                                                          *)
(* Safety obligations (LLING-COMP-5 and siblings):                          *)
(*   - a foreign provider callback is never invoked while the registry      *)
(*     write lock is held (a re-entrant provider could otherwise deadlock,  *)
(*     and an untrusted call must not extend a critical section);           *)
(*   - each write lock is mutually exclusive;                               *)
(*   - no thread holds both write locks at once, so there is no lock-order  *)
(*     cycle (and the model is deadlock-free).                              *)
(*                                                                          *)
(* Registry: proofs/doc/abi-invariants.tsv, LLING-COMP-5..6.               *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS Threads, NONE

VARIABLES pc, regWriter, cacheWriter
vars == <<pc, regWriter, cacheWriter>>

Phases == {"idle", "callProviders", "acqReg", "register", "acqCache", "insert", "done"}

TypeOK ==
  /\ pc \in [Threads -> Phases]
  /\ regWriter \in Threads \cup {NONE}
  /\ cacheWriter \in Threads \cup {NONE}

Init ==
  /\ pc = [t \in Threads |-> "idle"]
  /\ regWriter = NONE
  /\ cacheWriter = NONE

\* Begin the cache-miss expansion path: no composition lock is held.
Begin(t) ==
  /\ pc[t] = "idle"
  /\ pc' = [pc EXCEPT ![t] = "callProviders"]
  /\ UNCHANGED <<regWriter, cacheWriter>>

\* The foreign provider callbacks complete. Crucially this phase holds no
\* composition lock, so a re-entrant provider cannot deadlock on the registry.
CallProviders(t) ==
  /\ pc[t] = "callProviders"
  /\ pc' = [pc EXCEPT ![t] = "acqReg"]
  /\ UNCHANGED <<regWriter, cacheWriter>>

\* Acquire the registry write lock (only when free).
AcqReg(t) ==
  /\ pc[t] = "acqReg"
  /\ regWriter = NONE
  /\ regWriter' = t
  /\ pc' = [pc EXCEPT ![t] = "register"]
  /\ UNCHANGED cacheWriter

\* Register neighbour product states (pure, in-memory) and DROP the lock.
Register(t) ==
  /\ pc[t] = "register"
  /\ regWriter = t
  /\ regWriter' = NONE
  /\ pc' = [pc EXCEPT ![t] = "acqCache"]
  /\ UNCHANGED cacheWriter

\* Acquire the cache write lock -- only after the registry lock is released.
AcqCache(t) ==
  /\ pc[t] = "acqCache"
  /\ cacheWriter = NONE
  /\ cacheWriter' = t
  /\ pc' = [pc EXCEPT ![t] = "insert"]
  /\ UNCHANGED regWriter

Insert(t) ==
  /\ pc[t] = "insert"
  /\ cacheWriter = t
  /\ cacheWriter' = NONE
  /\ pc' = [pc EXCEPT ![t] = "done"]
  /\ UNCHANGED regWriter

Finish(t) ==
  /\ pc[t] = "done"
  /\ pc' = [pc EXCEPT ![t] = "idle"]
  /\ UNCHANGED <<regWriter, cacheWriter>>

Next ==
  \E t \in Threads :
    \/ Begin(t)
    \/ CallProviders(t)
    \/ AcqReg(t)
    \/ Register(t)
    \/ AcqCache(t)
    \/ Insert(t)
    \/ Finish(t)

Spec == Init /\ [][Next]_vars

(***************************************************************************)
(* Invariants                                                              *)
(***************************************************************************)

\* The registry write lock is held exactly by the thread in its "register"
\* phase (and the cache lock by the thread in "insert").
RegisterImpliesHolder ==
  \A t \in Threads : pc[t] = "register" => regWriter = t

InsertImpliesHolder ==
  \A t \in Threads : pc[t] = "insert" => cacheWriter = t

\* LLING-COMP-5: no foreign provider callback runs while the registry write
\* lock is held.
NoCallbackUnderRegWrite ==
  \A t \in Threads : pc[t] = "callProviders" => regWriter # t

\* Mutual exclusion of each write lock.
RegisterMutualExclusion ==
  \A s, t \in Threads : (pc[s] = "register" /\ pc[t] = "register") => s = t

CacheMutualExclusion ==
  \A s, t \in Threads : (pc[s] = "insert" /\ pc[t] = "insert") => s = t

\* No thread holds both write locks -- drop-before-acquire means no lock
\* nesting, hence no circular wait (deadlock-freedom is checked separately by
\* TLC over the Next relation).
NeverBothLocks ==
  \A t \in Threads : ~(regWriter = t /\ cacheWriter = t)

===============================================================================
