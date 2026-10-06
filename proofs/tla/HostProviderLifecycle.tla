------------------------ MODULE HostProviderLifecycle ------------------------
(***************************************************************************)
(* Project-neutral VtResource/query_interface provider protocol.           *)
(*                                                                         *)
(* This model joins obligations that cross existing focused models:         *)
(* AbiOwnershipLifecycle (retains), SerialProviderTurnstile (admission),   *)
(* SerialProviderAliasRegistry (context identity), and AbiV2Lifecycle      *)
(* (cancellation and publication). It does not replace their finer proofs.  *)
(*                                                                         *)
(* Each Client owns at most one base resource and one captured snapshot.    *)
(* A single token arena slot is reused with an increasing generation.       *)
(* Callback depth abstracts same-thread reentrancy; callback bodies and     *)
(* foreign pointer bytes are intentionally outside this finite model.       *)
(***************************************************************************)
EXTENDS FiniteSets, Naturals

CONSTANTS Clients, Threads, NONE, ProviderVersion, RequestVersions,
          Mode, HomeThread, SnapshotAliases, Total, MaxPage,
          MaxGeneration, MaxOutput

ASSUME /\ Clients # {}
       /\ Threads # {}
       /\ NONE \notin Clients
       /\ ProviderVersion \in Nat \ {0}
       /\ RequestVersions \subseteq Nat \ {0}
       /\ Mode \in {"Serial", "Parallel", "ThreadBound"}
       /\ HomeThread \in Threads
       /\ SnapshotAliases \in BOOLEAN
       /\ Total \in Nat
       /\ MaxPage \in Nat \ {0}
       /\ MaxGeneration \in Nat \ {0}
       /\ MaxOutput \in Nat \ {0}

VARIABLES baseOwned, snapshotOwned, retains, context, snapshotContext,
          snapshotId, requestedVersion, negotiated, interfaceOutput,
          status, output, errorOutput, generation, slotLive, tokenOwner,
          tokenGeneration, tokenUseCount, callOwner, callDepth, cursor, cancelled,
          pageIndex, pageWritten, pageCapacity, leaseGeneration

vars == <<baseOwned, snapshotOwned, retains, context, snapshotContext,
          snapshotId, requestedVersion, negotiated, interfaceOutput,
          status, output, errorOutput, generation, slotLive, tokenOwner,
          tokenGeneration, tokenUseCount, callOwner, callDepth, cursor, cancelled,
          pageIndex, pageWritten, pageCapacity, leaseGeneration>>

Statuses == {"None", "Ok", "End", "InvalidArgument", "Unsupported",
             "ProviderError", "BatchInUse", "Closed"}
CursorStates == {"None", "Open", "Leased", "Ended", "Closed"}
Active == {t \in Threads : callDepth[t] > 0}
BaseOwners == {c \in Clients : baseOwned[c]}
SnapshotOwners == {c \in Clients : snapshotOwned[c]}
OwnsAny(c) == baseOwned[c] \/ snapshotOwned[c]
ValidToken(c, supplied) ==
  slotLive /\ tokenOwner = c /\ supplied = generation
    /\ snapshotOwned[c]
Admissible(c, t) ==
  /\ snapshotOwned[c]
  /\ negotiated[c]
  /\ IF callDepth[t] > 0
       THEN Mode = "Parallel" /\ callOwner[t] = c /\ callDepth[t] < 2
       ELSE IF Mode = "Serial" THEN Active = {}
            ELSE IF Mode = "ThreadBound" THEN t = HomeThread /\ Active = {}
            ELSE TRUE

TypeOK ==
  /\ baseOwned \in [Clients -> BOOLEAN]
  /\ snapshotOwned \in [Clients -> BOOLEAN]
  /\ retains \in 0..(2 * Cardinality(Clients))
  /\ context \in [Clients -> 0..1]
  /\ snapshotContext \in [Clients -> 0..2]
  /\ snapshotId \in [Clients -> 0..1]
  /\ requestedVersion \in [Clients -> RequestVersions \cup {0}]
  /\ negotiated \in [Clients -> BOOLEAN]
  /\ interfaceOutput \in [Clients -> 0..1]
  /\ status \in [Clients -> Statuses]
  /\ output \in [Clients -> 0..MaxOutput]
  /\ errorOutput \in [Clients -> 0..MaxOutput]
  /\ generation \in 0..MaxGeneration
  /\ slotLive \in BOOLEAN
  /\ tokenOwner \in Clients \cup {NONE}
  /\ tokenGeneration \in [Clients -> 0..MaxGeneration]
  /\ tokenUseCount \in [Clients -> 0..1]
  /\ callOwner \in [Threads -> Clients \cup {NONE}]
  /\ callDepth \in [Threads -> 0..2]
  /\ cursor \in [Clients -> CursorStates]
  /\ cancelled \in [Clients -> BOOLEAN]
  /\ pageIndex \in [Clients -> 0..Total]
  /\ pageWritten \in [Clients -> 0..MaxPage]
  /\ pageCapacity \in [Clients -> 0..MaxPage]
  /\ leaseGeneration \in [Clients -> 0..MaxGeneration]

Init ==
  /\ baseOwned = [c \in Clients |-> FALSE]
  /\ snapshotOwned = [c \in Clients |-> FALSE]
  /\ retains = 0
  /\ context = [c \in Clients |-> 0]
  /\ snapshotContext = [c \in Clients |-> 0]
  /\ snapshotId = [c \in Clients |-> 0]
  /\ requestedVersion = [c \in Clients |-> 0]
  /\ negotiated = [c \in Clients |-> FALSE]
  /\ interfaceOutput = [c \in Clients |-> 0]
  /\ status = [c \in Clients |-> "None"]
  /\ output = [c \in Clients |-> 0]
  /\ errorOutput = [c \in Clients |-> 0]
  /\ generation = 0
  /\ slotLive = FALSE
  /\ tokenOwner = NONE
  /\ tokenGeneration = [c \in Clients |-> 0]
  /\ tokenUseCount = [c \in Clients |-> 0]
  /\ callOwner = [t \in Threads |-> NONE]
  /\ callDepth = [t \in Threads |-> 0]
  /\ cursor = [c \in Clients |-> "None"]
  /\ cancelled = [c \in Clients |-> FALSE]
  /\ pageIndex = [c \in Clients |-> 0]
  /\ pageWritten = [c \in Clients |-> 0]
  /\ pageCapacity = [c \in Clients |-> 0]
  /\ leaseGeneration = [c \in Clients |-> 0]

Acquire(c) ==
  /\ ~OwnsAny(c) /\ context[c] = 0
  /\ baseOwned' = [baseOwned EXCEPT ![c] = TRUE]
  /\ context' = [context EXCEPT ![c] = 1]
  /\ retains' = retains + 1
  /\ status' = [status EXCEPT ![c] = "Ok"]
  /\ UNCHANGED <<snapshotOwned, snapshotContext, snapshotId,
       requestedVersion, negotiated, interfaceOutput, output, errorOutput,
       generation, slotLive, tokenOwner, tokenGeneration, tokenUseCount, callOwner,
       callDepth, cursor, cancelled, pageIndex, pageWritten,
       pageCapacity, leaseGeneration>>

CloneBase(source, target) ==
  /\ source # target
  /\ baseOwned[source]
  /\ ~OwnsAny(target)
  /\ baseOwned' = [baseOwned EXCEPT ![target] = TRUE]
  /\ context' = [context EXCEPT ![target] = context[source]]
  /\ retains' = retains + 1
  /\ status' = [status EXCEPT ![target] = "Ok"]
  /\ UNCHANGED <<snapshotOwned, snapshotContext, snapshotId,
       requestedVersion, negotiated, interfaceOutput, output, errorOutput,
       generation, slotLive, tokenOwner, tokenGeneration, tokenUseCount, callOwner,
       callDepth, cursor, cancelled, pageIndex, pageWritten,
       pageCapacity, leaseGeneration>>

Discover(c, version) ==
  /\ baseOwned[c]
  /\ version \in RequestVersions
  /\ IF version <= ProviderVersion
       THEN /\ requestedVersion' =
                 [requestedVersion EXCEPT ![c] = version]
            /\ negotiated' = [negotiated EXCEPT ![c] = TRUE]
            /\ interfaceOutput' = [interfaceOutput EXCEPT ![c] = 1]
            /\ status' = [status EXCEPT ![c] = "Ok"]
       ELSE /\ UNCHANGED <<requestedVersion, negotiated,
                            interfaceOutput>>
            /\ status' = [status EXCEPT ![c] = "Unsupported"]
  /\ errorOutput' = [errorOutput EXCEPT ![c] = output[c]]
  /\ UNCHANGED <<baseOwned, snapshotOwned, retains, context,
       snapshotContext, snapshotId, output, generation, slotLive,
       tokenOwner, tokenGeneration, tokenUseCount, callOwner, callDepth, cursor,
       cancelled, pageIndex, pageWritten, pageCapacity, leaseGeneration>>

Capture(c) ==
  /\ baseOwned[c] /\ negotiated[c] /\ snapshotId[c] = 0
  /\ snapshotOwned' = [snapshotOwned EXCEPT ![c] = TRUE]
  /\ snapshotContext' = [snapshotContext EXCEPT
       ![c] = IF SnapshotAliases THEN context[c] ELSE 2]
  /\ snapshotId' = [snapshotId EXCEPT ![c] = 1]
  /\ retains' = retains + 1
  /\ status' = [status EXCEPT ![c] = "Ok"]
  /\ UNCHANGED <<baseOwned, context, requestedVersion, negotiated,
       interfaceOutput, output, errorOutput, generation, slotLive,
       tokenOwner, tokenGeneration, tokenUseCount, callOwner, callDepth, cursor,
       cancelled, pageIndex, pageWritten, pageCapacity, leaseGeneration>>

ReleaseBase(c) ==
  /\ baseOwned[c]
  /\ baseOwned' = [baseOwned EXCEPT ![c] = FALSE]
  /\ retains' = retains - 1
  /\ status' = [status EXCEPT ![c] = "Ok"]
  /\ UNCHANGED <<snapshotOwned, context, snapshotContext, snapshotId,
       requestedVersion, negotiated, interfaceOutput, output, errorOutput,
       generation, slotLive, tokenOwner, tokenGeneration, tokenUseCount, callOwner,
       callDepth, cursor, cancelled, pageIndex, pageWritten,
       pageCapacity, leaseGeneration>>

ReleaseSnapshot(c) ==
  /\ snapshotOwned[c]
  /\ cursor[c] \in {"None", "Closed"}
  /\ tokenOwner # c
  /\ \A t \in Threads : callOwner[t] # c
  /\ snapshotOwned' = [snapshotOwned EXCEPT ![c] = FALSE]
  /\ retains' = retains - 1
  /\ status' = [status EXCEPT ![c] = "Ok"]
  /\ UNCHANGED <<baseOwned, context, snapshotContext, snapshotId,
       requestedVersion, negotiated, interfaceOutput, output, errorOutput,
       generation, slotLive, tokenOwner, tokenGeneration, tokenUseCount, callOwner,
       callDepth, cursor, cancelled, pageIndex, pageWritten,
       pageCapacity, leaseGeneration>>

(* A safe facade finalizer performs precisely the same ownership transition. *)
Finalizer(c) == ReleaseBase(c) \/ ReleaseSnapshot(c)

IssueToken(c) ==
  /\ snapshotOwned[c] /\ ~slotLive /\ generation < MaxGeneration
  /\ generation' = generation + 1
  /\ tokenGeneration' = [tokenGeneration EXCEPT ![c] = generation + 1]
  /\ slotLive' = TRUE
  /\ tokenOwner' = c
  /\ status' = [status EXCEPT ![c] = "Ok"]
  /\ UNCHANGED <<baseOwned, snapshotOwned, retains, context,
       snapshotContext, snapshotId, requestedVersion, negotiated,
       interfaceOutput, output, errorOutput, tokenUseCount,
       callOwner, callDepth,
       cursor, cancelled, pageIndex, pageWritten, pageCapacity,
       leaseGeneration>>

ReleaseToken(c) ==
  /\ slotLive /\ tokenOwner = c
  /\ slotLive' = FALSE
  /\ tokenOwner' = NONE
  /\ status' = [status EXCEPT ![c] = "Ok"]
  /\ UNCHANGED <<baseOwned, snapshotOwned, retains, context,
       snapshotContext, snapshotId, requestedVersion, negotiated,
       interfaceOutput, output, errorOutput, generation, tokenGeneration, tokenUseCount,
       callOwner, callDepth, cursor, cancelled, pageIndex, pageWritten,
       pageCapacity, leaseGeneration>>

UseToken(c, supplied) ==
  /\ supplied \in 1..generation /\ tokenUseCount[c] < 1
  /\ tokenUseCount' = [tokenUseCount EXCEPT ![c] = @ + 1]
  /\ IF ValidToken(c, supplied)
       THEN status' = [status EXCEPT ![c] = "Ok"]
       ELSE status' = [status EXCEPT ![c] = "InvalidArgument"]
  /\ errorOutput' = [errorOutput EXCEPT ![c] = output[c]]
  /\ UNCHANGED <<baseOwned, snapshotOwned, retains, context,
       snapshotContext, snapshotId, requestedVersion, negotiated,
       interfaceOutput, output, generation, slotLive, tokenOwner,
       tokenGeneration, callOwner, callDepth, cursor, cancelled,
       pageIndex, pageWritten, pageCapacity, leaseGeneration>>

BeginCall(c, t) ==
  /\ Admissible(c, t)
  /\ callOwner' = [callOwner EXCEPT ![t] = c]
  /\ callDepth' = [callDepth EXCEPT ![t] = @ + 1]
  /\ UNCHANGED <<baseOwned, snapshotOwned, retains, context,
       snapshotContext, snapshotId, requestedVersion, negotiated,
       interfaceOutput, status, output, errorOutput, generation,
       slotLive, tokenOwner, tokenGeneration, tokenUseCount, cursor, cancelled,
       pageIndex, pageWritten, pageCapacity, leaseGeneration>>

RejectCall(c, t) ==
  /\ ~Admissible(c, t)
  /\ status' = [status EXCEPT ![c] =
       IF ~OwnsAny(c) THEN "Closed" ELSE "ProviderError"]
  /\ errorOutput' = [errorOutput EXCEPT ![c] = output[c]]
  /\ UNCHANGED <<baseOwned, snapshotOwned, retains, context,
       snapshotContext, snapshotId, requestedVersion, negotiated,
       interfaceOutput, output, generation, slotLive, tokenOwner,
       tokenGeneration, tokenUseCount, callOwner, callDepth, cursor, cancelled,
       pageIndex, pageWritten, pageCapacity, leaseGeneration>>

FinishCall(c, t, succeeded) ==
  /\ callDepth[t] > 0 /\ callOwner[t] = c
  /\ succeeded \in BOOLEAN
  /\ succeeded => output[c] < MaxOutput
  /\ callDepth' = [callDepth EXCEPT ![t] = @ - 1]
  /\ callOwner' = [callOwner EXCEPT
       ![t] = IF callDepth[t] = 1 THEN NONE ELSE c]
  /\ output' = [output EXCEPT
       ![c] = IF succeeded THEN @ + 1 ELSE @]
  /\ status' = [status EXCEPT
       ![c] = IF succeeded THEN "Ok" ELSE "ProviderError"]
  /\ errorOutput' = [errorOutput EXCEPT
       ![c] = IF succeeded THEN @ ELSE output[c]]
  /\ UNCHANGED <<baseOwned, snapshotOwned, retains, context,
       snapshotContext, snapshotId, requestedVersion, negotiated,
       interfaceOutput, generation, slotLive, tokenOwner,
       tokenGeneration, tokenUseCount, cursor, cancelled, pageIndex, pageWritten,
       pageCapacity, leaseGeneration>>

OpenCursor(c) ==
  /\ snapshotOwned[c] /\ negotiated[c] /\ cursor[c] = "None"
  /\ cursor' = [cursor EXCEPT ![c] = "Open"]
  /\ status' = [status EXCEPT ![c] = "Ok"]
  /\ UNCHANGED <<baseOwned, snapshotOwned, retains, context,
       snapshotContext, snapshotId, requestedVersion, negotiated,
       interfaceOutput, output, errorOutput, generation, slotLive,
       tokenOwner, tokenGeneration, tokenUseCount, callOwner, callDepth, cancelled,
       pageIndex, pageWritten, pageCapacity, leaseGeneration>>

Page(c, capacity) ==
  /\ cursor[c] = "Open" /\ ~cancelled[c]
  /\ pageIndex[c] < Total
  /\ capacity \in 1..MaxPage
  /\ leaseGeneration[c] < MaxGeneration
  /\ LET amount == IF capacity <= Total - pageIndex[c]
                    THEN capacity ELSE Total - pageIndex[c] IN
       /\ cursor' = [cursor EXCEPT ![c] = "Leased"]
       /\ pageIndex' = [pageIndex EXCEPT ![c] = @ + amount]
       /\ pageWritten' = [pageWritten EXCEPT ![c] = amount]
       /\ pageCapacity' = [pageCapacity EXCEPT ![c] = capacity]
       /\ leaseGeneration' = [leaseGeneration EXCEPT ![c] = @ + 1]
  /\ status' = [status EXCEPT ![c] = "Ok"]
  /\ UNCHANGED <<baseOwned, snapshotOwned, retains, context,
       snapshotContext, snapshotId, requestedVersion, negotiated,
       interfaceOutput, output, errorOutput, generation, slotLive,
       tokenOwner, tokenGeneration, tokenUseCount, callOwner, callDepth, cancelled>>

ReleaseBatch(c) ==
  /\ cursor[c] = "Leased"
  /\ cursor' = [cursor EXCEPT ![c] = "Open"]
  /\ pageWritten' = [pageWritten EXCEPT ![c] = 0]
  /\ pageCapacity' = [pageCapacity EXCEPT ![c] = 0]
  /\ status' = [status EXCEPT ![c] = "Ok"]
  /\ UNCHANGED <<baseOwned, snapshotOwned, retains, context,
       snapshotContext, snapshotId, requestedVersion, negotiated,
       interfaceOutput, output, errorOutput, generation, slotLive,
       tokenOwner, tokenGeneration, tokenUseCount, callOwner, callDepth, cancelled,
       pageIndex, leaseGeneration>>

EndCursor(c) ==
  /\ cursor[c] = "Open" /\ pageIndex[c] = Total
  /\ cursor' = [cursor EXCEPT ![c] = "Ended"]
  /\ status' = [status EXCEPT ![c] = "End"]
  /\ errorOutput' = [errorOutput EXCEPT ![c] = output[c]]
  /\ UNCHANGED <<baseOwned, snapshotOwned, retains, context,
       snapshotContext, snapshotId, requestedVersion, negotiated,
       interfaceOutput, output, generation, slotLive, tokenOwner,
       tokenGeneration, tokenUseCount, callOwner, callDepth, cancelled, pageIndex,
       pageWritten, pageCapacity, leaseGeneration>>

Cancel(c) ==
  /\ cursor[c] \in {"Open", "Leased"}
  /\ IF cursor[c] = "Leased"
       THEN /\ UNCHANGED <<cursor, cancelled>>
            /\ status' = [status EXCEPT ![c] = "BatchInUse"]
       ELSE /\ cursor' = [cursor EXCEPT ![c] = "Ended"]
            /\ cancelled' = [cancelled EXCEPT ![c] = TRUE]
            /\ status' = [status EXCEPT ![c] = "Ok"]
  /\ errorOutput' = [errorOutput EXCEPT ![c] = output[c]]
  /\ UNCHANGED <<baseOwned, snapshotOwned, retains, context,
       snapshotContext, snapshotId, requestedVersion, negotiated,
       interfaceOutput, output, generation, slotLive, tokenOwner,
       tokenGeneration, tokenUseCount, callOwner, callDepth, pageIndex, pageWritten,
       pageCapacity, leaseGeneration>>

CloseCursor(c) ==
  /\ cursor[c] \in {"Open", "Ended"}
  /\ cursor' = [cursor EXCEPT ![c] = "Closed"]
  /\ status' = [status EXCEPT ![c] = "Ok"]
  /\ UNCHANGED <<baseOwned, snapshotOwned, retains, context,
       snapshotContext, snapshotId, requestedVersion, negotiated,
       interfaceOutput, output, errorOutput, generation, slotLive,
       tokenOwner, tokenGeneration, tokenUseCount, callOwner, callDepth, cancelled,
       pageIndex, pageWritten, pageCapacity, leaseGeneration>>

Next ==
  \/ \E c \in Clients : Acquire(c) \/ ReleaseBase(c)
       \/ ReleaseSnapshot(c) \/ Finalizer(c) \/ Capture(c)
       \/ IssueToken(c) \/ ReleaseToken(c)
       \/ OpenCursor(c) \/ ReleaseBatch(c) \/ EndCursor(c)
       \/ Cancel(c) \/ CloseCursor(c)
       \/ \E version \in RequestVersions : Discover(c, version)
       \/ \E supplied \in 1..MaxGeneration : UseToken(c, supplied)
       \/ \E t \in Threads : BeginCall(c, t) \/ RejectCall(c, t)
            \/ \E succeeded \in BOOLEAN : FinishCall(c, t, succeeded)
  \/ \E source, target \in Clients : CloneBase(source, target)
  \/ \E c \in Clients : \E capacity \in 1..MaxPage : Page(c, capacity)

Spec == Init /\ [][Next]_vars

(* The provider cannot force a caller to release a lease or close a cursor. *)
(* These are explicit caller-scheduling assumptions for the liveness check. *)
FairSpec ==
  Spec /\
  \A c \in Clients :
    WF_vars(ReleaseBatch(c)) /\ WF_vars(CloseCursor(c))

RetainsEqualOwners ==
  retains = Cardinality(BaseOwners) + Cardinality(SnapshotOwners)
NegotiationSound ==
  \A c \in Clients : negotiated[c] =>
    interfaceOutput[c] = 1 /\
    requestedVersion[c] > 0 /\ requestedVersion[c] <= ProviderVersion
SnapshotIdentityPinned ==
  \A c \in Clients : snapshotOwned[c] =>
    snapshotId[c] = 1 /\
    snapshotContext[c] = (IF SnapshotAliases THEN context[c] ELSE 2)
NoCallbackAfterClose ==
  \A t \in Threads : callDepth[t] > 0 => snapshotOwned[callOwner[t]]
CallOwnerExact ==
  \A t \in Threads : (callDepth[t] = 0) <=> (callOwner[t] = NONE)
SerialAdmission == Mode = "Serial" => Cardinality(Active) <= 1
ThreadAffinity == Mode = "ThreadBound" => Active \subseteq {HomeThread}
ThreadBoundAdmission == Mode = "ThreadBound" => Cardinality(Active) <= 1
TokenGenerationSound ==
  slotLive => tokenOwner \in Clients /\
    generation > 0 /\ tokenGeneration[tokenOwner] = generation
StaleTokenNeverValid ==
  \A c \in Clients : tokenGeneration[c] > 0 /\
    tokenGeneration[c] < generation =>
      ~ValidToken(c, tokenGeneration[c])
SafeTokenNext ==
  Next /\
  \A c \in Clients :
    \A supplied \in 1..MaxGeneration :
      UseToken(c, supplied) =>
        status'[c] =
          (IF ValidToken(c, supplied) THEN "Ok" ELSE "InvalidArgument")
TokenUseStatusLaw == [][SafeTokenNext]_vars
PageBounded ==
  \A c \in Clients : pageIndex[c] <= Total /\
    pageWritten[c] <= pageCapacity[c] /\
    (cursor[c] = "Leased" => pageWritten[c] > 0)
CancelledNeverOpen ==
  \A c \in Clients : cancelled[c] => cursor[c] \in {"Ended", "Closed"}
ErrorDoesNotPublish ==
  \A c \in Clients : status[c] \in
    {"End", "InvalidArgument", "Unsupported", "ProviderError",
     "BatchInUse", "Closed"} => output[c] = errorOutput[c]

(* Deliberately false for an honest parallel provider. The expected TLC    *)
(* counterexample proves that the parallel configuration reaches overlap.   *)
NoParallelOverlap == Cardinality(Active) <= 1

CursorEventuallyClosed ==
  \A c \in Clients :
    [](cursor[c] \in {"Open", "Leased", "Ended"} =>
       <> (cursor[c] = "Closed"))

=============================================================================
