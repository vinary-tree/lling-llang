------------------------- MODULE SharedCacheSlots -------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

\* Representation refinement, separate from SharedStateCache's publication
\* protocol. Completed accesses are atomic; Pause/Resume tests re-resolving an
\* external ID after another access has reused its former internal slot.
\* Runtime correspondence: byId projects entries[id].slot; rows projects
\* links[slot].id. previous/next project fields of the same link vector.
\* Immutable payload values are abstracted away.
CONSTANTS Capacity, Mutation
Ids == {0, 1, 2}
MaxSteps == 6

VARIABLES cache, reference, steps, phase, capturedId, capturedSlot
vars == <<cache, reference, steps, phase, capturedId, capturedSlot>>

Members(s) == {s[i] : i \in 1..Len(s)}
Without(s, id) == SelectSeq(s, LAMBDA x: x # id)
ReferenceAccess(s, id) ==
    LET touched == Append(Without(s, id), id)
    IN IF Len(touched) > Capacity THEN Tail(touched) ELSE touched

Empty == [rows |-> <<>>, previous |-> <<>>, next |-> <<>>,
          head |-> 0, tail |-> 0, byId |-> [id \in {} |-> 0]]

\* Touch changes only links and endpoints. In the non-tail case, at least two
\* slots exist. Compose neighbor-field updates instead of writing stale rows.
Touch(s, slot) ==
    IF slot = s.tail THEN s
    ELSE LET predecessor == s.previous[slot]
             successor == s.next[slot]
             detachedPrevious == [s.previous EXCEPT ![successor] = predecessor]
             detachedNext == IF predecessor = 0 THEN s.next
                             ELSE [s.next EXCEPT ![predecessor] = successor]
         IN [s EXCEPT
              !.previous = [detachedPrevious EXCEPT ![slot] = s.tail],
              !.next = [detachedNext EXCEPT ![s.tail] = slot, ![slot] = 0],
              !.head = IF predecessor = 0 THEN successor ELSE s.head,
              !.tail = slot]

AppendResident(s, id) ==
    LET slot == Len(s.rows) + 1
        linkedNext == IF s.tail = 0 THEN s.next
                      ELSE [s.next EXCEPT ![s.tail] = slot]
    IN [rows |-> Append(s.rows, id),
        previous |-> Append(s.previous, s.tail),
        next |-> Append(linkedNext, 0),
        head |-> IF s.head = 0 THEN slot ELSE s.head,
        tail |-> slot,
        byId |-> s.byId @@ (id :> slot)]

ReplaceOldest(s, id) ==
    LET slot == s.head
        victim == s.rows[slot]
        ids == (DOMAIN s.byId \ {victim}) \cup {id}
        replacement == [s EXCEPT
            !.rows = IF Mutation = "StaleReverseId" THEN s.rows
                     ELSE [s.rows EXCEPT ![slot] = id],
            !.byId = [key \in ids |-> IF key = id THEN slot ELSE s.byId[key]]]
    IN Touch(replacement, slot)

Accessed(s, id) ==
    IF id \in DOMAIN s.byId THEN Touch(s, s.byId[id])
    ELSE IF Len(s.rows) < Capacity THEN AppendResident(s, id)
    ELSE ReplaceOldest(s, id)

RECURSIVE Walk(_, _, _)
Walk(s, slot, remaining) ==
    IF slot = 0 \/ remaining = 0 THEN <<>>
    ELSE <<s.rows[slot]>> \o Walk(s, s.next[slot], remaining - 1)

Init == /\ cache = Empty /\ reference = <<>> /\ steps = 0
        /\ phase = "Idle" /\ capturedId = 0 /\ capturedSlot = 0

Access(id) ==
    /\ steps < MaxSteps
    /\ cache' = Accessed(cache, id)
    /\ reference' = ReferenceAccess(reference, id)
    /\ steps' = steps + 1
    /\ UNCHANGED <<phase, capturedId, capturedSlot>>

Pause(id) ==
    /\ phase = "Idle" /\ id \in DOMAIN cache.byId
    /\ cache.byId[id] # cache.tail
    /\ capturedId' = id /\ capturedSlot' = cache.byId[id]
    /\ phase' = "Paused"
    /\ UNCHANGED <<cache, reference, steps>>

Resume ==
    /\ phase = "Paused" /\ steps < MaxSteps
    /\ cache' = IF Mutation = "StaleSlot"
                THEN Touch(cache, capturedSlot)
                ELSE Accessed(cache, capturedId)
    /\ reference' = ReferenceAccess(reference, capturedId)
    /\ steps' = steps + 1 /\ phase' = "Done"
    /\ UNCHANGED <<capturedId, capturedSlot>>

Next == (\E id \in Ids: Access(id) \/ Pause(id)) \/ Resume
Spec == Init /\ [][Next]_vars

RepresentationInvariant ==
    LET count == Len(cache.rows)
        slots == 1..count
        order == Walk(cache, cache.head, count)
    IN /\ count <= Capacity
       /\ Len(cache.previous) = count /\ Len(cache.next) = count
       /\ Members(cache.rows) \subseteq Ids
       /\ Cardinality(Members(cache.rows)) = count
       /\ DOMAIN cache.byId = Members(cache.rows)
       /\ \A slot \in slots: cache.byId[cache.rows[slot]] = slot
       /\ \A slot \in slots:
             /\ cache.previous[slot] \in 0..count
             /\ cache.next[slot] \in 0..count
       /\ IF count = 0 THEN cache.head = 0 /\ cache.tail = 0
          ELSE /\ cache.head \in slots /\ cache.tail \in slots
               /\ cache.previous[cache.head] = 0
               /\ cache.next[cache.tail] = 0
       /\ \A slot \in slots:
             /\ (cache.next[slot] # 0 =>
                    cache.previous[cache.next[slot]] = slot)
             /\ (cache.previous[slot] # 0 =>
                    cache.next[cache.previous[slot]] = slot)
       /\ Len(order) = count /\ Members(order) = Members(cache.rows)

SlotOrderRefinement == Walk(cache, cache.head, Len(cache.rows)) = reference
=============================================================================
