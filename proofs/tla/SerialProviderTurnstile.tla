--------------------- MODULE SerialProviderTurnstile ----------------------
(***************************************************************************)
(* LLING-GATE-1: the per-captured-provider serial callback turnstile.       *)
(*                                                                         *)
(* `owner` abstracts the atomic admission bit; `waiters` abstracts the     *)
(* SeqCst registered-waiter count. A waiter acquires `parking`, rechecks   *)
(* `owner`, and only then atomically releases `parking` while sleeping.     *)
(* Release clears `owner` before taking that SAME parking mutex to notify. *)
(* Hence release-before-recheck makes the waiter acquire immediately,      *)
(* whereas recheck-before-release makes notification follow sleep.          *)
(* Spurious wakeups merely restart the check. Independent providers have   *)
(* distinct owner, waiter, and parking state. Same-thread recursion never  *)
(* joins the waiters: it is rejected while the original call stays active. *)
(*                                                                         *)
(* Source: src/bindings.rs SerialProviderCallGate::{enter,drop}.           *)
(* Tests:  src/bindings.rs tests::serial_provider_gate_* and               *)
(*         tests/ffi_concurrent_composition_stress.rs.                      *)
(***************************************************************************)
EXTENDS FiniteSets, Naturals

CONSTANTS Threads, Providers, NONE

VARIABLES pc, target, owner, waiters, parking, notifyPending, recursed,
          nestedRejected
vars == <<pc, target, owner, waiters, parking, notifyPending,
          recursed, nestedRejected>>

Phases == {"idle", "check", "registered", "recheck", "parked",
           "active", "done"}
WaitingPhases == {"registered", "recheck", "parked"}
Parked(p) == {t \in Threads : pc[t] = "parked" /\ target[t] = p}
RunnableWaiter(p) ==
  {t \in Threads : pc[t] \in {"registered", "recheck"} /\ target[t] = p}

TypeOK ==
  /\ pc \in [Threads -> Phases]
  /\ target \in [Threads -> Providers \cup {NONE}]
  /\ owner \in [Providers -> Threads \cup {NONE}]
  /\ waiters \in [Providers -> SUBSET Threads]
  /\ parking \in [Providers -> Threads \cup {NONE}]
  /\ notifyPending \in [Providers -> BOOLEAN]
  /\ recursed \in [Threads -> BOOLEAN]
  /\ nestedRejected \in [Threads -> BOOLEAN]

Init ==
  /\ pc = [t \in Threads |-> "idle"]
  /\ target = [t \in Threads |-> NONE]
  /\ owner = [p \in Providers |-> NONE]
  /\ waiters = [p \in Providers |-> {}]
  /\ parking = [p \in Providers |-> NONE]
  /\ notifyPending = [p \in Providers |-> FALSE]
  /\ recursed = [t \in Threads |-> FALSE]
  /\ nestedRejected = [t \in Threads |-> FALSE]

Begin(t, p) ==
  /\ pc[t] = "idle"
  /\ pc' = [pc EXCEPT ![t] = "check"]
  /\ target' = [target EXCEPT ![t] = p]
  /\ UNCHANGED <<owner, waiters, parking, notifyPending, recursed,
                  nestedRejected>>

FastAdmit(t) ==
  /\ pc[t] = "check"
  /\ owner[target[t]] = NONE
  /\ owner' = [owner EXCEPT ![target[t]] = t]
  /\ pc' = [pc EXCEPT ![t] = "active"]
  /\ UNCHANGED <<target, waiters, parking, notifyPending, recursed,
                  nestedRejected>>

Register(t) ==
  /\ pc[t] = "check"
  /\ owner[target[t]] # NONE
  /\ waiters' = [waiters EXCEPT ![target[t]] = @ \cup {t}]
  /\ pc' = [pc EXCEPT ![t] = "registered"]
  /\ UNCHANGED <<target, owner, parking, notifyPending, recursed,
                  nestedRejected>>

AcquireParking(t) ==
  /\ pc[t] = "registered"
  /\ parking[target[t]] = NONE
  /\ parking' = [parking EXCEPT ![target[t]] = t]
  /\ pc' = [pc EXCEPT ![t] = "recheck"]
  /\ UNCHANGED <<target, owner, waiters, notifyPending, recursed,
                  nestedRejected>>

RecheckAdmit(t) ==
  /\ pc[t] = "recheck"
  /\ parking[target[t]] = t
  /\ owner[target[t]] = NONE
  /\ owner' = [owner EXCEPT ![target[t]] = t]
  /\ waiters' = [waiters EXCEPT ![target[t]] = @ \ {t}]
  /\ parking' = [parking EXCEPT ![target[t]] = NONE]
  /\ pc' = [pc EXCEPT ![t] = "active"]
  /\ UNCHANGED <<target, notifyPending, recursed, nestedRejected>>

Sleep(t) ==
  /\ pc[t] = "recheck"
  /\ parking[target[t]] = t
  /\ owner[target[t]] # NONE
  /\ parking' = [parking EXCEPT ![target[t]] = NONE]
  /\ pc' = [pc EXCEPT ![t] = "parked"]
  /\ UNCHANGED <<target, owner, waiters, notifyPending, recursed,
                  nestedRejected>>

SpuriousWake(t) ==
  /\ pc[t] = "parked"
  /\ pc' = [pc EXCEPT ![t] = "registered"]
  /\ UNCHANGED <<target, owner, waiters, parking, notifyPending, recursed,
                  nestedRejected>>

RejectRecursion(t) ==
  /\ pc[t] = "active"
  /\ ~recursed[t]
  /\ recursed' = [recursed EXCEPT ![t] = TRUE]
  /\ UNCHANGED <<pc, target, owner, waiters, parking, notifyPending,
                  nestedRejected>>

(* A callback already inside provider A may enter idle provider B, but if  *)
(* B is busy, waiting could form a cross-thread A -> B -> A cycle. The real *)
(* gate rejects that nested contention before registration.                *)
RejectNestedBusy(t, p) ==
  /\ pc[t] = "active"
  /\ p # target[t]
  /\ owner[p] # NONE
  /\ ~nestedRejected[t]
  /\ nestedRejected' = [nestedRejected EXCEPT ![t] = TRUE]
  /\ UNCHANGED <<pc, target, owner, waiters, parking, notifyPending,
                  recursed>>

Release(t) ==
  /\ pc[t] = "active"
  /\ owner[target[t]] = t
  /\ owner' = [owner EXCEPT ![target[t]] = NONE]
  /\ notifyPending' = [notifyPending EXCEPT ![target[t]] = (waiters[target[t]] # {})]
  /\ pc' = [pc EXCEPT ![t] = "done"]
  /\ UNCHANGED <<target, waiters, parking, recursed, nestedRejected>>

(* Notification can only pass the per-provider parking mutex after a       *)
(* check-to-sleep transition releases it. If nobody has slept yet, the     *)
(* registered waiter will observe the released owner on its recheck.       *)
Notify(p) ==
  /\ notifyPending[p]
  /\ parking[p] = NONE
  /\ IF Parked(p) # {}
        THEN \E t \in Parked(p) : pc' = [pc EXCEPT ![t] = "registered"]
        ELSE UNCHANGED pc
  /\ notifyPending' = [notifyPending EXCEPT ![p] = FALSE]
  /\ UNCHANGED <<target, owner, waiters, parking, recursed,
                  nestedRejected>>

Finish(t) ==
  /\ pc[t] = "done"
  /\ pc' = [pc EXCEPT ![t] = "idle"]
  /\ target' = [target EXCEPT ![t] = NONE]
  /\ recursed' = [recursed EXCEPT ![t] = FALSE]
  /\ nestedRejected' = [nestedRejected EXCEPT ![t] = FALSE]
  /\ UNCHANGED <<owner, waiters, parking, notifyPending>>

Next ==
  \/ \E t \in Threads : \E p \in Providers : Begin(t, p)
  \/ \E t \in Threads : FastAdmit(t) \/ Register(t) \/ AcquireParking(t)
       \/ RecheckAdmit(t) \/ Sleep(t) \/ SpuriousWake(t)
       \/ RejectRecursion(t) \/ Release(t) \/ Finish(t)
  \/ \E t \in Threads : \E p \in Providers : RejectNestedBusy(t, p)
  \/ \E p \in Providers : Notify(p)

Spec == Init /\ [][Next]_vars

(* A callback owns exactly its own provider, never the parking mutex.      *)
ActiveOwnsAdmission ==
  \A t \in Threads : pc[t] = "active" => owner[target[t]] = t
NoCallbackUnderParkingMutex ==
  \A t \in Threads : pc[t] = "active" =>
    \A p \in Providers : parking[p] # t
NoProviderCallbackOverlap ==
  \A p \in Providers :
    \A s, t \in Threads :
      (pc[s] = "active" /\ pc[t] = "active" /\
       target[s] = p /\ target[t] = p) => s = t

(* Registration is preserved through sleep/spurious wake and removed only  *)
(* after acquisition. Therefore the release-side count cannot omit an      *)
(* already parked thread.                                                  *)
WaiterCountExact ==
  \A p \in Providers :
    waiters[p] = {t \in Threads : target[t] = p /\ pc[t] \in WaitingPhases}
ParkingOwnerExact ==
  \A p \in Providers :
    \A t \in Threads : parking[p] = t =>
      (pc[t] = "recheck" /\ target[t] = p)

(* If nobody owns the admission bit and a thread is asleep, either a       *)
(* notification is pending or another waiter is runnable and can acquire. *)
(* With a finite callback and fair scheduling, this precludes an orphaned   *)
(* sleeper; no scheduling fairness is needed for the safety check itself.  *)
NoLostWakeup ==
  \A p \in Providers :
    (owner[p] = NONE /\ Parked(p) # {}) =>
      (notifyPending[p] \/ RunnableWaiter(p) # {})

RecursionNeverParks ==
  \A t \in Threads : recursed[t] =>
    (pc[t] \in {"active", "done"} /\ t \notin waiters[target[t]])

NestedCycleNeverParks ==
  \A t \in Threads : nestedRejected[t] =>
    (pc[t] \in {"active", "done"} /\
     \A p \in Providers : t \notin waiters[p])

(* This intentionally false property witnesses an actually parked waiter; *)
(* NoLostWakeup is therefore checked on reachable non-vacuous states.      *)
NoParkedState == \A t \in Threads : pc[t] # "parked"

(* This is intentionally false in the two-provider configuration. TLC's   *)
(* expected counterexample witnesses that distinct providers can both be  *)
(* active, so the gate is not accidentally resource-wide.                 *)
IndependentOverlapAbsent ==
  \A s, t \in Threads :
    (pc[s] = "active" /\ pc[t] = "active" /\ s # t) =>
      target[s] = target[t]

=============================================================================
