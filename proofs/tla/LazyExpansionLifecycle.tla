---------------------- MODULE LazyExpansionLifecycle ----------------------
(***************************************************************************)
(* Finite concurrency model for the explicit LazyWfst state lifecycle.      *)
(*                                                                           *)
(* The Rust wrapper uses exclusive mutable access, which is the zero-lock    *)
(* implementation of the single-owner rule modelled here.  Multiple workers  *)
(* represent competing callers before that ownership boundary.  Source       *)
(* snapshots, cooperative cancellation, explicit retry, terminal             *)
(* observations, and cache eligibility are modelled independently of WFST    *)
(* payloads so the checker explores the entire finite control state.          *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS States, Workers, Snapshots, InitialSnapshot, MaxAttempts, NONE

VARIABLES
  phase,
  owners,
  entrySnapshot,
  currentSnapshot,
  retryable,
  attempts,
  observation,
  cacheable,
  cancellationRequested,
  retryBeginAuthorized

vars ==
  <<phase, owners, entrySnapshot, currentSnapshot, retryable, attempts,
    observation, cacheable, cancellationRequested, retryBeginAuthorized>>

Phases ==
  {"Unexpanded", "Expanding", "ExpandedEmpty", "ExpandedNonempty",
   "Failed", "Cancelled"}

Observations == {NONE, "Empty", "Nonempty", "Failure", "Cancellation"}

ExpandedPhases == {"ExpandedEmpty", "ExpandedNonempty"}
TerminalPhases == ExpandedPhases \cup {"Failed", "Cancelled"}

ResetPhases == [state \in States |-> "Unexpanded"]
ResetOwners == [state \in States |-> {}]
ResetRetryable == [state \in States |-> FALSE]
ResetAttempts == [state \in States |-> 0]
ResetObservations == [state \in States |-> NONE]
ResetCacheable == [state \in States |-> FALSE]
ResetAuthorization == [state \in States |-> TRUE]
SnapshotEntries(snapshot) == [state \in States |-> snapshot]

TypeOK ==
  /\ phase \in [States -> Phases]
  /\ owners \in [States -> SUBSET Workers]
  /\ entrySnapshot \in [States -> Snapshots]
  /\ currentSnapshot \in Snapshots
  /\ retryable \in [States -> BOOLEAN]
  /\ attempts \in [States -> 0..MaxAttempts]
  /\ observation \in [States -> Observations]
  /\ cacheable \in [States -> BOOLEAN]
  /\ cancellationRequested \in [Workers -> BOOLEAN]
  /\ retryBeginAuthorized \in [States -> BOOLEAN]

Init ==
  /\ phase = ResetPhases
  /\ owners = ResetOwners
  /\ entrySnapshot = SnapshotEntries(InitialSnapshot)
  /\ currentSnapshot = InitialSnapshot
  /\ retryable = ResetRetryable
  /\ attempts = ResetAttempts
  /\ observation = ResetObservations
  /\ cacheable = ResetCacheable
  /\ cancellationRequested = [worker \in Workers |-> FALSE]
  /\ retryBeginAuthorized = ResetAuthorization

RequestCancellation(worker) ==
  /\ worker \in Workers
  /\ ~cancellationRequested[worker]
  /\ cancellationRequested' =
       [cancellationRequested EXCEPT ![worker] = TRUE]
  /\ UNCHANGED
       <<phase, owners, entrySnapshot, currentSnapshot, retryable, attempts,
         observation, cacheable, retryBeginAuthorized>>

ReplaceCancellationToken(worker) ==
  /\ worker \in Workers
  /\ cancellationRequested[worker]
  /\ cancellationRequested' =
       [cancellationRequested EXCEPT ![worker] = FALSE]
  /\ UNCHANGED
       <<phase, owners, entrySnapshot, currentSnapshot, retryable, attempts,
         observation, cacheable, retryBeginAuthorized>>

BeginNormal(state, worker) ==
  /\ state \in States
  /\ worker \in Workers
  /\ phase[state] = "Unexpanded"
  /\ attempts[state] < MaxAttempts
  /\ ~cancellationRequested[worker]
  /\ owners[state] = {}
  /\ phase' = [phase EXCEPT ![state] = "Expanding"]
  /\ owners' = [owners EXCEPT ![state] = @ \cup {worker}]
  /\ entrySnapshot' =
       [entrySnapshot EXCEPT ![state] = currentSnapshot]
  /\ retryable' = [retryable EXCEPT ![state] = FALSE]
  /\ attempts' = [attempts EXCEPT ![state] = @ + 1]
  /\ observation' = [observation EXCEPT ![state] = NONE]
  /\ cacheable' = [cacheable EXCEPT ![state] = FALSE]
  /\ retryBeginAuthorized' =
       [retryBeginAuthorized EXCEPT ![state] = TRUE]
  /\ UNCHANGED <<currentSnapshot, cancellationRequested>>

BeginRetry(state, worker) ==
  /\ state \in States
  /\ worker \in Workers
  /\ phase[state] = "Failed"
  /\ retryable[state]
  /\ attempts[state] < MaxAttempts
  /\ ~cancellationRequested[worker]
  /\ owners[state] = {}
  /\ phase' = [phase EXCEPT ![state] = "Expanding"]
  /\ owners' = [owners EXCEPT ![state] = @ \cup {worker}]
  /\ entrySnapshot' =
       [entrySnapshot EXCEPT ![state] = currentSnapshot]
  /\ retryable' = [retryable EXCEPT ![state] = FALSE]
  /\ attempts' = [attempts EXCEPT ![state] = @ + 1]
  /\ observation' = [observation EXCEPT ![state] = NONE]
  /\ cacheable' = [cacheable EXCEPT ![state] = FALSE]
  /\ retryBeginAuthorized' =
       [retryBeginAuthorized EXCEPT ![state] = retryable[state]]
  /\ UNCHANGED <<currentSnapshot, cancellationRequested>>

CancelBeforeNormalBegin(state, worker) ==
  /\ state \in States
  /\ worker \in Workers
  /\ phase[state] = "Unexpanded"
  /\ cancellationRequested[worker]
  /\ owners[state] = {}
  /\ phase' = [phase EXCEPT ![state] = "Cancelled"]
  /\ owners' = [owners EXCEPT ![state] = {}]
  /\ entrySnapshot' =
       [entrySnapshot EXCEPT ![state] = currentSnapshot]
  /\ retryable' = [retryable EXCEPT ![state] = FALSE]
  /\ observation' = [observation EXCEPT ![state] = NONE]
  /\ cacheable' = [cacheable EXCEPT ![state] = FALSE]
  /\ retryBeginAuthorized' =
       [retryBeginAuthorized EXCEPT ![state] = TRUE]
  /\ UNCHANGED <<currentSnapshot, attempts, cancellationRequested>>

CancelBeforeRetry(state, worker) ==
  /\ state \in States
  /\ worker \in Workers
  /\ phase[state] = "Failed"
  /\ retryable[state]
  /\ cancellationRequested[worker]
  /\ owners[state] = {}
  /\ phase' = [phase EXCEPT ![state] = "Cancelled"]
  /\ owners' = [owners EXCEPT ![state] = {}]
  /\ entrySnapshot' =
       [entrySnapshot EXCEPT ![state] = currentSnapshot]
  /\ retryable' = [retryable EXCEPT ![state] = FALSE]
  /\ observation' = [observation EXCEPT ![state] = NONE]
  /\ cacheable' = [cacheable EXCEPT ![state] = FALSE]
  /\ retryBeginAuthorized' =
       [retryBeginAuthorized EXCEPT ![state] = retryable[state]]
  /\ UNCHANGED <<currentSnapshot, attempts, cancellationRequested>>

Complete(state, worker, completedPhase) ==
  /\ state \in States
  /\ worker \in Workers
  /\ completedPhase \in ExpandedPhases
  /\ phase[state] = "Expanding"
  /\ owners[state] = {worker}
  /\ entrySnapshot[state] = currentSnapshot
  /\ phase' = [phase EXCEPT ![state] = completedPhase]
  /\ owners' = [owners EXCEPT ![state] = {}]
  /\ retryable' = [retryable EXCEPT ![state] = FALSE]
  /\ observation' = [observation EXCEPT ![state] = NONE]
  /\ cacheable' = [cacheable EXCEPT ![state] = TRUE]
  /\ UNCHANGED
       <<entrySnapshot, currentSnapshot, attempts, cancellationRequested,
         retryBeginAuthorized>>

Fail(state, worker, mayRetry) ==
  /\ state \in States
  /\ worker \in Workers
  /\ mayRetry \in BOOLEAN
  /\ phase[state] = "Expanding"
  /\ owners[state] = {worker}
  /\ entrySnapshot[state] = currentSnapshot
  /\ phase' = [phase EXCEPT ![state] = "Failed"]
  /\ owners' = [owners EXCEPT ![state] = {}]
  /\ retryable' = [retryable EXCEPT ![state] = mayRetry]
  /\ observation' = [observation EXCEPT ![state] = NONE]
  /\ cacheable' = [cacheable EXCEPT ![state] = FALSE]
  /\ UNCHANGED
       <<entrySnapshot, currentSnapshot, attempts, cancellationRequested,
         retryBeginAuthorized>>

Cancel(state, worker) ==
  /\ state \in States
  /\ worker \in Workers
  /\ phase[state] = "Expanding"
  /\ owners[state] = {worker}
  /\ entrySnapshot[state] = currentSnapshot
  /\ phase' = [phase EXCEPT ![state] = "Cancelled"]
  /\ owners' = [owners EXCEPT ![state] = {}]
  /\ retryable' = [retryable EXCEPT ![state] = FALSE]
  /\ observation' = [observation EXCEPT ![state] = NONE]
  /\ cacheable' = [cacheable EXCEPT ![state] = FALSE]
  /\ UNCHANGED
       <<entrySnapshot, currentSnapshot, attempts, cancellationRequested,
         retryBeginAuthorized>>

Observe(state) ==
  /\ state \in States
  /\ entrySnapshot[state] = currentSnapshot
  /\ LET observed ==
       CASE phase[state] = "ExpandedEmpty" -> "Empty"
         [] phase[state] = "ExpandedNonempty" -> "Nonempty"
         [] phase[state] = "Failed" -> "Failure"
         [] phase[state] = "Cancelled" -> "Cancellation"
         [] OTHER -> NONE
     IN /\ observed # NONE
        /\ observation' = [observation EXCEPT ![state] = observed]
  /\ UNCHANGED
       <<phase, owners, entrySnapshot, currentSnapshot, retryable, attempts,
         cacheable, cancellationRequested, retryBeginAuthorized>>

ResetCancelled(state) ==
  /\ state \in States
  /\ phase[state] = "Cancelled"
  /\ phase' = [phase EXCEPT ![state] = "Unexpanded"]
  /\ owners' = [owners EXCEPT ![state] = {}]
  /\ entrySnapshot' =
       [entrySnapshot EXCEPT ![state] = currentSnapshot]
  /\ retryable' = [retryable EXCEPT ![state] = FALSE]
  /\ observation' = [observation EXCEPT ![state] = NONE]
  /\ cacheable' = [cacheable EXCEPT ![state] = FALSE]
  /\ retryBeginAuthorized' =
       [retryBeginAuthorized EXCEPT ![state] = TRUE]
  /\ UNCHANGED <<currentSnapshot, attempts, cancellationRequested>>

ResetFailed(state) ==
  /\ state \in States
  /\ phase[state] = "Failed"
  /\ phase' = [phase EXCEPT ![state] = "Unexpanded"]
  /\ owners' = [owners EXCEPT ![state] = {}]
  /\ entrySnapshot' =
       [entrySnapshot EXCEPT ![state] = currentSnapshot]
  /\ retryable' = [retryable EXCEPT ![state] = FALSE]
  /\ observation' = [observation EXCEPT ![state] = NONE]
  /\ cacheable' = [cacheable EXCEPT ![state] = FALSE]
  /\ retryBeginAuthorized' =
       [retryBeginAuthorized EXCEPT ![state] = TRUE]
  /\ UNCHANGED <<currentSnapshot, attempts, cancellationRequested>>

EvictExpanded(state) ==
  /\ state \in States
  /\ phase[state] \in ExpandedPhases
  /\ phase' = [phase EXCEPT ![state] = "Unexpanded"]
  /\ owners' = [owners EXCEPT ![state] = {}]
  /\ entrySnapshot' =
       [entrySnapshot EXCEPT ![state] = currentSnapshot]
  /\ retryable' = [retryable EXCEPT ![state] = FALSE]
  /\ attempts' = [attempts EXCEPT ![state] = 0]
  /\ observation' = [observation EXCEPT ![state] = NONE]
  /\ cacheable' = [cacheable EXCEPT ![state] = FALSE]
  /\ retryBeginAuthorized' =
       [retryBeginAuthorized EXCEPT ![state] = TRUE]
  /\ UNCHANGED <<currentSnapshot, cancellationRequested>>

RebindSnapshot(nextSnapshot) ==
  /\ nextSnapshot \in Snapshots
  /\ nextSnapshot # currentSnapshot
  /\ currentSnapshot' = nextSnapshot
  /\ phase' = ResetPhases
  /\ owners' = ResetOwners
  /\ entrySnapshot' = SnapshotEntries(nextSnapshot)
  /\ retryable' = ResetRetryable
  /\ attempts' = ResetAttempts
  /\ observation' = ResetObservations
  /\ cacheable' = ResetCacheable
  /\ retryBeginAuthorized' = ResetAuthorization
  /\ UNCHANGED cancellationRequested

ClearAll ==
  /\ phase # ResetPhases \/ observation # ResetObservations
  /\ phase' = ResetPhases
  /\ owners' = ResetOwners
  /\ entrySnapshot' = SnapshotEntries(currentSnapshot)
  /\ retryable' = ResetRetryable
  /\ attempts' = ResetAttempts
  /\ observation' = ResetObservations
  /\ cacheable' = ResetCacheable
  /\ retryBeginAuthorized' = ResetAuthorization
  /\ UNCHANGED <<currentSnapshot, cancellationRequested>>

Next ==
  \/ \E worker \in Workers : RequestCancellation(worker)
  \/ \E worker \in Workers : ReplaceCancellationToken(worker)
  \/ \E state \in States, worker \in Workers : BeginNormal(state, worker)
  \/ \E state \in States, worker \in Workers : BeginRetry(state, worker)
  \/ \E state \in States, worker \in Workers :
       CancelBeforeNormalBegin(state, worker)
  \/ \E state \in States, worker \in Workers :
       CancelBeforeRetry(state, worker)
  \/ \E state \in States, worker \in Workers,
        completedPhase \in ExpandedPhases :
       Complete(state, worker, completedPhase)
  \/ \E state \in States, worker \in Workers, mayRetry \in BOOLEAN :
       Fail(state, worker, mayRetry)
  \/ \E state \in States, worker \in Workers : Cancel(state, worker)
  \/ \E state \in States : Observe(state)
  \/ \E state \in States : ResetCancelled(state)
  \/ \E state \in States : ResetFailed(state)
  \/ \E state \in States : EvictExpanded(state)
  \/ \E nextSnapshot \in Snapshots : RebindSnapshot(nextSnapshot)
  \/ ClearAll

Spec == Init /\ [][Next]_vars

AtMostOneExpansionOwner ==
  \A state \in States : Cardinality(owners[state]) <= 1

OwnerExactlyWhileExpanding ==
  \A state \in States :
    (phase[state] = "Expanding") = (Cardinality(owners[state]) = 1)

ExpandingUsesCapturedSnapshot ==
  \A state \in States :
    phase[state] = "Expanding" => entrySnapshot[state] = currentSnapshot

ObservableStateUsesCurrentSnapshot ==
  \A state \in States :
    observation[state] # NONE =>
      entrySnapshot[state] = currentSnapshot

UnexpandedNeverAppearsEmpty ==
  \A state \in States :
    phase[state] = "Unexpanded" => observation[state] # "Empty"

ExpandingIsUnobservable ==
  \A state \in States :
    phase[state] = "Expanding" => observation[state] = NONE

EmptyObservationIsExact ==
  \A state \in States :
    observation[state] = "Empty" => phase[state] = "ExpandedEmpty"

NonemptyObservationIsExact ==
  \A state \in States :
    observation[state] = "Nonempty" => phase[state] = "ExpandedNonempty"

FailureObservationHasExactStatus ==
  \A state \in States :
    observation[state] = "Failure" => phase[state] = "Failed"

CancellationObservationHasExactStatus ==
  \A state \in States :
    observation[state] = "Cancellation" => phase[state] = "Cancelled"

RetryFlagOnlyOnFailure ==
  \A state \in States : retryable[state] => phase[state] = "Failed"

NonRetryableFailureIsTerminal ==
  \A state \in States : retryBeginAuthorized[state]

ExpandedExactlyCacheable ==
  \A state \in States :
    cacheable[state] = (phase[state] \in ExpandedPhases)

IncompleteStatesAreNotCacheable ==
  \A state \in States :
    phase[state] \in {"Unexpanded", "Expanding", "Failed", "Cancelled"} =>
      ~cacheable[state]

AttemptCountIsBounded ==
  \A state \in States : attempts[state] <= MaxAttempts

CancelledHasNoOwner ==
  \A state \in States :
    phase[state] = "Cancelled" => owners[state] = {}

FailedHasNoOwner ==
  \A state \in States :
    phase[state] = "Failed" => owners[state] = {}

=============================================================================
