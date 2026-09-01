------------------------ MODULE AbiOwnershipLifecycle ------------------------
(***************************************************************************)
(* Bounded opaque-resource ownership protocol for ABI v1.  Each Owned client *)
(* accounts for exactly one retain.  Clone adds one owner and one retain;     *)
(* transfer moves ownership without changing the count; release removes one. *)
(* Private relayouts preserve the complete public v1 observation.             *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS Clients, MaxObservations

VARIABLES clientState, retainCount, privateLayout, publicVersion,
          resourceIdentity, observations

vars == <<clientState, retainCount, privateLayout, publicVersion,
          resourceIdentity, observations>>

ClientStates == {"Idle", "Owned", "Moved", "Released"}
Owners == {client \in Clients : clientState[client] = "Owned"}
PublicObservation == <<publicVersion, resourceIdentity>>

TypeOK ==
  /\ clientState \in [Clients -> ClientStates]
  /\ retainCount \in Nat
  /\ privateLayout \in Nat
  /\ publicVersion \in Nat
  /\ resourceIdentity \in Nat
  /\ observations \in Seq(Nat \X Nat)

Init ==
  /\ clientState = [client \in Clients |-> "Idle"]
  /\ retainCount = 0
  /\ privateLayout = 0
  /\ publicVersion = 1
  /\ resourceIdentity = 1
  /\ observations = <<>>

Acquire(client) ==
  /\ clientState[client] = "Idle"
  /\ clientState' = [clientState EXCEPT ![client] = "Owned"]
  /\ retainCount' = retainCount + 1
  /\ UNCHANGED <<privateLayout, publicVersion, resourceIdentity, observations>>

Clone(source, target) ==
  /\ source # target
  /\ clientState[source] = "Owned"
  /\ clientState[target] = "Idle"
  /\ clientState' = [clientState EXCEPT ![target] = "Owned"]
  /\ retainCount' = retainCount + 1
  /\ UNCHANGED <<privateLayout, publicVersion, resourceIdentity, observations>>

Transfer(source, target) ==
  /\ source # target
  /\ clientState[source] = "Owned"
  /\ clientState[target] = "Idle"
  /\ clientState' = [clientState EXCEPT
       ![source] = "Moved", ![target] = "Owned"]
  /\ UNCHANGED <<retainCount, privateLayout, publicVersion,
                  resourceIdentity, observations>>

Release(client) ==
  /\ clientState[client] = "Owned"
  /\ retainCount > 0
  /\ clientState' = [clientState EXCEPT ![client] = "Released"]
  /\ retainCount' = retainCount - 1
  /\ UNCHANGED <<privateLayout, publicVersion, resourceIdentity, observations>>

Observe ==
  /\ Len(observations) < MaxObservations
  /\ observations' = Append(observations, PublicObservation)
  /\ UNCHANGED <<clientState, retainCount, privateLayout,
                  publicVersion, resourceIdentity>>

Relayout ==
  /\ privateLayout' = privateLayout + 1
  /\ UNCHANGED <<clientState, retainCount, publicVersion,
                  resourceIdentity, observations>>

Next ==
  \/ \E client \in Clients : Acquire(client)
  \/ \E source, target \in Clients : Clone(source, target)
  \/ \E source, target \in Clients : Transfer(source, target)
  \/ \E client \in Clients : Release(client)
  \/ Observe
  \/ Relayout

Spec == Init /\ [][Next]_vars

RetainsEqualOwners == retainCount = Cardinality(Owners)
NoMovedClientOwns ==
  \A client \in Clients : clientState[client] = "Moved" => client \notin Owners
NoReleasedClientOwns ==
  \A client \in Clients : clientState[client] = "Released" => client \notin Owners
AbiV1IsStable == publicVersion = 1
IdentityIsStable == resourceIdentity = 1
OpaqueObservationsAreStable ==
  \A index \in 1..Len(observations) : observations[index] = <<1, 1>>
ModelConstraint == privateLayout <= 3

=============================================================================
