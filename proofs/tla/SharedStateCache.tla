------------------------- MODULE SharedStateCache -------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS Policy, Capacity, Mutation
Workers == {0, 1}
Ids == {0, 1, 2}
MaxRequests == 2
MaxEpoch == 1
MaxRevision == 6

VARIABLES resident, recency, epoch, revision, reference,
          phase, request, captured, expected, candidateSet, candidateOrder,
          requests, outcome, safeAdmission, evicted

vars == <<resident, recency, epoch, revision, reference,
          phase, request, captured, expected, candidateSet, candidateOrder,
          requests, outcome, safeAdmission, evicted>>

Members(s) == {s[i] : i \in 1..Len(s)}
Without(s, id) == SelectSeq(s, LAMBDA x: x # id)
Touched(s, id) == Append(Without(s, id), id)
Bounded(s) == IF Len(s) > Capacity THEN Tail(s) ELSE s
NextOrder(s, id) == IF Policy = "LRU" THEN Bounded(Touched(s, id)) ELSE <<>>
NextSet(ids, order, id) == IF Policy = "All" THEN ids \cup {id}
                          ELSE Members(NextOrder(order, id))
ReferenceAccess(s, id) == IF Policy = "All" THEN Touched(s, id)
                         ELSE IF Policy = "LRU" THEN Bounded(Touched(s, id))
                         ELSE <<>>
CurrentGeneration(w) == captured[w] = epoch \/ Mutation = "StaleAdmission"
MostRecent(s, id) == IF Len(s) = 0 THEN FALSE ELSE s[Len(s)] = id
Reusable(w) == request[w] \in resident /\
                  (Policy = "All" \/ MostRecent(recency, request[w]))

Init == /\ resident = {}
        /\ recency = <<>>
        /\ epoch = 0 /\ revision = 0 /\ reference = <<>>
        /\ phase = [w \in Workers |-> "Idle"]
        /\ request = [w \in Workers |-> 0]
        /\ captured = [w \in Workers |-> 0]
        /\ expected = [w \in Workers |-> 0]
        /\ candidateSet = [w \in Workers |-> {}]
        /\ candidateOrder = [w \in Workers |-> <<>>]
        /\ requests = [w \in Workers |-> 0]
        /\ outcome = [w \in Workers |-> "Pending"]
        /\ safeAdmission = TRUE /\ evicted = FALSE

Read(w, id) ==
    /\ phase[w] = "Idle"
    /\ requests[w] < MaxRequests
    /\ requests' = [requests EXCEPT ![w] = @ + 1]
    /\ outcome' = [outcome EXCEPT ![w] = IF id \in resident THEN "Hit" ELSE "Pending"]
    /\ request' = [request EXCEPT ![w] = id]
    /\ captured' = [captured EXCEPT ![w] = epoch]
    /\ phase' = [phase EXCEPT ![w] =
          IF id \in resident THEN
              IF Policy = "All" \/ MostRecent(recency, id) THEN "Done" ELSE "Ready"
          ELSE "Computing"]
    /\ UNCHANGED <<resident, recency, epoch, revision, reference,
                    expected, candidateSet, candidateOrder, safeAdmission, evicted>>

Complete(w, result) ==
    /\ phase[w] = "Computing"
    /\ result \in {"Valid", "Invalid", "Fault"}
    /\ outcome' = [outcome EXCEPT ![w] = result]
    /\ phase' = [phase EXCEPT ![w] = IF result = "Valid" THEN "Ready" ELSE "Done"]
    /\ UNCHANGED <<resident, recency, epoch, revision, reference, request,
                    captured, expected, candidateSet, candidateOrder,
                    requests, safeAdmission, evicted>>

Prepare(w) ==
    /\ phase[w] = "Ready"
    /\ CurrentGeneration(w) /\ Policy # "None"
    /\ ~Reusable(w)
    /\ expected' = [expected EXCEPT ![w] = revision]
    /\ candidateSet' = [candidateSet EXCEPT ![w] = NextSet(resident, recency, request[w])]
    /\ candidateOrder' = [candidateOrder EXCEPT ![w] = NextOrder(recency, request[w])]
    /\ phase' = [phase EXCEPT ![w] = "Prepared"]
    /\ UNCHANGED <<resident, recency, epoch, revision, reference, request, captured,
                    requests, outcome, safeAdmission, evicted>>

Publish(w) ==
    /\ phase[w] = "Prepared"
    /\ CurrentGeneration(w) /\ expected[w] = revision
    /\ revision < MaxRevision
    /\ resident' = candidateSet[w] /\ recency' = candidateOrder[w]
    /\ reference' = ReferenceAccess(reference, request[w])
    /\ safeAdmission' = (safeAdmission /\ captured[w] = epoch
                           /\ outcome[w] \in {"Valid", "Hit"})
    /\ evicted' = (evicted \/ (resident \ candidateSet[w] # {}))
    /\ revision' = revision + 1
    /\ phase' = [phase EXCEPT ![w] = "Done"]
    /\ UNCHANGED <<epoch, request, captured, expected, candidateSet, candidateOrder,
                    requests, outcome>>

Retry(w) ==
    /\ phase[w] = "Prepared"
    /\ CurrentGeneration(w) /\ expected[w] # revision
    /\ phase' = [phase EXCEPT ![w] = "Ready"]
    /\ UNCHANGED <<resident, recency, epoch, revision, reference, request,
                    captured, expected, candidateSet, candidateOrder,
                    requests, outcome, safeAdmission, evicted>>

ReturnUncached(w) ==
    /\ phase[w] \in {"Ready", "Prepared"}
    /\ (captured[w] # epoch \/ Policy = "None")
    /\ phase' = [phase EXCEPT ![w] = "Done"]
    /\ UNCHANGED <<resident, recency, epoch, revision, reference, request,
                    captured, expected, candidateSet, candidateOrder,
                    requests, outcome, safeAdmission, evicted>>

Again(w) ==
    /\ phase[w] = "Done" /\ requests[w] < MaxRequests
    /\ phase' = [phase EXCEPT ![w] = "Idle"]
    /\ request' = [request EXCEPT ![w] = 0]
    /\ captured' = [captured EXCEPT ![w] = 0]
    /\ expected' = [expected EXCEPT ![w] = 0]
    /\ candidateSet' = [candidateSet EXCEPT ![w] = {}]
    /\ candidateOrder' = [candidateOrder EXCEPT ![w] = <<>>]
    /\ outcome' = [outcome EXCEPT ![w] = "Pending"]
    /\ UNCHANGED <<resident, recency, epoch, revision, reference,
                    requests, safeAdmission, evicted>>

Reuse(w) ==
    /\ phase[w] = "Ready" /\ CurrentGeneration(w) /\ Reusable(w)
    /\ phase' = [phase EXCEPT ![w] = "Done"]
    /\ UNCHANGED <<resident, recency, epoch, revision, reference, request,
                    captured, expected, candidateSet, candidateOrder,
                    requests, outcome, safeAdmission, evicted>>

Clear == /\ epoch < MaxEpoch /\ revision < MaxRevision
         /\ resident' = {} /\ recency' = <<>> /\ reference' = <<>>
         /\ epoch' = epoch + 1 /\ revision' = revision + 1
         /\ UNCHANGED <<phase, request, captured, expected, candidateSet, candidateOrder,
                         requests, outcome, safeAdmission, evicted>>

Next == Clear \/ \E w \in Workers:
    (\E id \in Ids: Read(w, id)) \/
    (\E result \in {"Valid", "Invalid", "Fault"}: Complete(w, result)) \/
    Prepare(w) \/ Publish(w) \/ Retry(w) \/ ReturnUncached(w) \/ Again(w) \/ Reuse(w)

Spec == Init /\ [][Next]_vars

TypeOK == /\ resident \subseteq Ids
          /\ Members(recency) \subseteq Ids /\ Len(recency) <= Cardinality(Ids)
          /\ epoch \in 0..MaxEpoch /\ revision \in 0..MaxRevision
          /\ phase \in [Workers -> {"Idle", "Computing", "Ready", "Prepared", "Done"}]
          /\ request \in [Workers -> Ids]
          /\ captured \in [Workers -> 0..MaxEpoch]
          /\ expected \in [Workers -> 0..MaxRevision]
          /\ candidateSet \in [Workers -> SUBSET Ids]
          /\ requests \in [Workers -> 0..MaxRequests]
          /\ outcome \in [Workers -> {"Pending", "Hit", "Valid", "Invalid", "Fault"}]
          /\ safeAdmission \in BOOLEAN /\ evicted \in BOOLEAN
NoStaleOrFailedAdmission == safeAdmission
ResidencyMatchesReference == resident = Members(reference)
ExactLruOrder == Policy = "LRU" => recency = reference
BoundedResidency == Policy = "LRU" => Cardinality(resident) <= Capacity
OneRecencyRecord == Policy = "LRU" =>
    /\ Len(recency) = Cardinality(resident) /\ Members(recency) = resident
NoCacheResidency == Policy = "None" => resident = {} /\ recency = <<>>
NoRecencyForCacheAll == Policy = "All" => recency = <<>>
\* Intentionally false invariant used only by the reachability configuration.
NoEvictionWitness == ~evicted
=============================================================================
