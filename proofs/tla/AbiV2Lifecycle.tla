--------------------------- MODULE AbiV2Lifecycle ---------------------------
(***************************************************************************)
(* Additive typed-ABI-v2 lifecycle. Fixed-width request metadata is         *)
(* validated before an operation runs. Precision, completeness,            *)
(* applicability, termination, and evidence authority are independent.     *)
(* Cancellation is sticky, budget exhaustion cannot publish, opaque ABI v1  *)
(* inputs cannot obtain typed evidence, and every published result remains  *)
(* bound to the validated snapshot/context pair.                            *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, Sequences

CONSTANT MaxBudget

VARIABLES phase,
          requestValid, typedInput, identityFresh,
          cancelRequested, remaining,
          precision, completeness, applicability, termination, evidence,
          outcomeWritten, resourcePresent, evidencePresent, authoritative,
          resourceHandleLive, evidenceHandleLive,
          boundSnapshot, boundContext, resultSnapshot, resultContext

vars == <<phase,
          requestValid, typedInput, identityFresh,
          cancelRequested, remaining,
          precision, completeness, applicability, termination, evidence,
          outcomeWritten, resourcePresent, evidencePresent, authoritative,
          resourceHandleLive, evidenceHandleLive,
          boundSnapshot, boundContext, resultSnapshot, resultContext>>

Phases == {"Idle", "Running", "Succeeded", "Cancelled",
           "BudgetExhausted", "Failed", "Rejected"}
Precisions == {"None", "Exact", "Approximate", "Unknown"}
Completenesses == {"None", "Complete", "Incomplete"}
Applicabilities == {"None", "Applicable", "Unsupported", "Unknown"}
Terminations == {"None", "Succeeded", "Cancelled", "BudgetExhausted", "Failed"}
EvidenceStates == {"None", "Candidate", "Verified", "Stale", "Invalid"}

TypeOK ==
  /\ phase \in Phases
  /\ requestValid \in BOOLEAN
  /\ typedInput \in BOOLEAN
  /\ identityFresh \in BOOLEAN
  /\ cancelRequested \in BOOLEAN
  /\ remaining \in 0..MaxBudget
  /\ precision \in Precisions
  /\ completeness \in Completenesses
  /\ applicability \in Applicabilities
  /\ termination \in Terminations
  /\ evidence \in EvidenceStates
  /\ outcomeWritten \in BOOLEAN
  /\ resourcePresent \in BOOLEAN
  /\ evidencePresent \in BOOLEAN
  /\ authoritative \in BOOLEAN
  /\ resourceHandleLive \in BOOLEAN
  /\ evidenceHandleLive \in BOOLEAN
  /\ boundSnapshot \in 0..2
  /\ boundContext \in 0..2
  /\ resultSnapshot \in 0..2
  /\ resultContext \in 0..2

Init ==
  /\ phase = "Idle"
  /\ requestValid = FALSE
  /\ typedInput = FALSE
  /\ identityFresh = FALSE
  /\ cancelRequested = FALSE
  /\ remaining = 0
  /\ precision = "None"
  /\ completeness = "None"
  /\ applicability = "None"
  /\ termination = "None"
  /\ evidence = "None"
  /\ outcomeWritten = FALSE
  /\ resourcePresent = FALSE
  /\ evidencePresent = FALSE
  /\ authoritative = FALSE
  /\ resourceHandleLive = FALSE
  /\ evidenceHandleLive = FALSE
  /\ boundSnapshot = 0
  /\ boundContext = 0
  /\ resultSnapshot = 0
  /\ resultContext = 0

Begin(valid, typed, fresh, budget, snapshot, context) ==
  /\ phase = "Idle"
  /\ valid \in BOOLEAN
  /\ typed \in BOOLEAN
  /\ fresh \in BOOLEAN
  /\ budget \in 0..MaxBudget
  /\ snapshot \in 1..2
  /\ context \in 1..2
  /\ requestValid' = valid
  /\ typedInput' = typed
  /\ identityFresh' = fresh
  /\ cancelRequested' = FALSE
  /\ remaining' = budget
  /\ boundSnapshot' = snapshot
  /\ boundContext' = context
  /\ resultSnapshot' = 0
  /\ resultContext' = 0
  /\ resourcePresent' = FALSE
  /\ evidencePresent' = FALSE
  /\ authoritative' = FALSE
  /\ resourceHandleLive' = FALSE
  /\ evidenceHandleLive' = FALSE
  /\ IF ~valid \/ ~fresh
        THEN /\ phase' = "Rejected"
             /\ precision' = "None"
             /\ completeness' = "None"
             /\ applicability' = "None"
             /\ termination' = "None"
             /\ evidence' = "None"
             /\ outcomeWritten' = FALSE
        ELSE IF budget = 0
          THEN /\ phase' = "BudgetExhausted"
               /\ precision' = "Unknown"
               /\ completeness' = "Incomplete"
               /\ applicability' = "Applicable"
               /\ termination' = "BudgetExhausted"
               /\ evidence' = "None"
               /\ outcomeWritten' = TRUE
          ELSE /\ phase' = "Running"
               /\ precision' = "None"
               /\ completeness' = "None"
               /\ applicability' = "Applicable"
               /\ termination' = "None"
               /\ evidence' = "None"
               /\ outcomeWritten' = FALSE

RequestCancel ==
  /\ phase = "Running"
  /\ ~cancelRequested
  /\ cancelRequested' = TRUE
  /\ UNCHANGED <<phase, requestValid, typedInput, identityFresh, remaining,
                  precision, completeness, applicability, termination, evidence,
                  outcomeWritten, resourcePresent, evidencePresent, authoritative,
                  resourceHandleLive, evidenceHandleLive,
                  boundSnapshot, boundContext, resultSnapshot, resultContext>>

Consume ==
  /\ phase = "Running"
  /\ ~cancelRequested
  /\ remaining > 1
  /\ remaining' = remaining - 1
  /\ UNCHANGED <<phase, requestValid, typedInput, identityFresh,
                  cancelRequested, precision, completeness, applicability,
                  termination, evidence, outcomeWritten, resourcePresent,
                  evidencePresent, authoritative, resourceHandleLive,
                  evidenceHandleLive, boundSnapshot, boundContext,
                  resultSnapshot, resultContext>>

CompleteExact ==
  /\ phase = "Running"
  /\ ~cancelRequested
  /\ remaining > 0
  /\ phase' = "Succeeded"
  /\ precision' = "Exact"
  /\ completeness' = "Complete"
  /\ applicability' = "Applicable"
  /\ termination' = "Succeeded"
  /\ evidence' = IF typedInput THEN "Verified" ELSE "None"
  /\ outcomeWritten' = TRUE
  /\ resourcePresent' = TRUE
  /\ evidencePresent' = typedInput
  /\ authoritative' = typedInput
  /\ resourceHandleLive' = TRUE
  /\ evidenceHandleLive' = typedInput
  /\ resultSnapshot' = boundSnapshot
  /\ resultContext' = boundContext
  /\ UNCHANGED <<requestValid, typedInput, identityFresh, cancelRequested,
                  remaining, boundSnapshot, boundContext>>

CompleteExactIncomplete ==
  /\ phase = "Running"
  /\ ~cancelRequested
  /\ remaining > 0
  /\ phase' = "Succeeded"
  /\ precision' = "Exact"
  /\ completeness' = "Incomplete"
  /\ applicability' = "Applicable"
  /\ termination' = "Succeeded"
  /\ evidence' = IF typedInput THEN "Candidate" ELSE "None"
  /\ outcomeWritten' = TRUE
  /\ resourcePresent' = TRUE
  /\ evidencePresent' = typedInput
  /\ authoritative' = FALSE
  /\ resourceHandleLive' = TRUE
  /\ evidenceHandleLive' = typedInput
  /\ resultSnapshot' = boundSnapshot
  /\ resultContext' = boundContext
  /\ UNCHANGED <<requestValid, typedInput, identityFresh, cancelRequested,
                  remaining, boundSnapshot, boundContext>>

CompleteApproximate ==
  /\ phase = "Running"
  /\ ~cancelRequested
  /\ remaining > 0
  /\ phase' = "Succeeded"
  /\ precision' = "Approximate"
  /\ completeness' = "Complete"
  /\ applicability' = "Applicable"
  /\ termination' = "Succeeded"
  /\ evidence' = IF typedInput THEN "Candidate" ELSE "None"
  /\ outcomeWritten' = TRUE
  /\ resourcePresent' = TRUE
  /\ evidencePresent' = typedInput
  /\ authoritative' = FALSE
  /\ resourceHandleLive' = TRUE
  /\ evidenceHandleLive' = typedInput
  /\ resultSnapshot' = boundSnapshot
  /\ resultContext' = boundContext
  /\ UNCHANGED <<requestValid, typedInput, identityFresh, cancelRequested,
                  remaining, boundSnapshot, boundContext>>

ObserveCancellation ==
  /\ phase = "Running"
  /\ cancelRequested
  /\ phase' = "Cancelled"
  /\ precision' = "Unknown"
  /\ completeness' = "Incomplete"
  /\ applicability' = "Applicable"
  /\ termination' = "Cancelled"
  /\ evidence' = "None"
  /\ outcomeWritten' = TRUE
  /\ resourcePresent' = FALSE
  /\ evidencePresent' = FALSE
  /\ authoritative' = FALSE
  /\ resourceHandleLive' = FALSE
  /\ evidenceHandleLive' = FALSE
  /\ resultSnapshot' = 0
  /\ resultContext' = 0
  /\ UNCHANGED <<requestValid, typedInput, identityFresh, cancelRequested,
                  remaining, boundSnapshot, boundContext>>

ExhaustBudget ==
  /\ phase = "Running"
  /\ ~cancelRequested
  /\ remaining = 1
  /\ phase' = "BudgetExhausted"
  /\ remaining' = 0
  /\ precision' = "Unknown"
  /\ completeness' = "Incomplete"
  /\ applicability' = "Applicable"
  /\ termination' = "BudgetExhausted"
  /\ evidence' = "None"
  /\ outcomeWritten' = TRUE
  /\ resourcePresent' = FALSE
  /\ evidencePresent' = FALSE
  /\ authoritative' = FALSE
  /\ resourceHandleLive' = FALSE
  /\ evidenceHandleLive' = FALSE
  /\ resultSnapshot' = 0
  /\ resultContext' = 0
  /\ UNCHANGED <<requestValid, typedInput, identityFresh, cancelRequested,
                  boundSnapshot, boundContext>>

Fail ==
  /\ phase = "Running"
  /\ phase' = "Failed"
  /\ precision' = "Unknown"
  /\ completeness' = "Incomplete"
  /\ applicability' = "Applicable"
  /\ termination' = "Failed"
  /\ evidence' = "None"
  /\ outcomeWritten' = TRUE
  /\ resourcePresent' = FALSE
  /\ evidencePresent' = FALSE
  /\ authoritative' = FALSE
  /\ resourceHandleLive' = FALSE
  /\ evidenceHandleLive' = FALSE
  /\ resultSnapshot' = 0
  /\ resultContext' = 0
  /\ UNCHANGED <<requestValid, typedInput, identityFresh, cancelRequested,
                  remaining, boundSnapshot, boundContext>>

ReleaseResource ==
  /\ phase = "Succeeded"
  /\ resourceHandleLive
  /\ resourceHandleLive' = FALSE
  /\ UNCHANGED <<phase, requestValid, typedInput, identityFresh,
                  cancelRequested, remaining, precision, completeness,
                  applicability, termination, evidence, outcomeWritten,
                  resourcePresent, evidencePresent, authoritative,
                  evidenceHandleLive, boundSnapshot, boundContext,
                  resultSnapshot, resultContext>>

ReleaseEvidence ==
  /\ phase = "Succeeded"
  /\ evidenceHandleLive
  /\ evidenceHandleLive' = FALSE
  /\ UNCHANGED <<phase, requestValid, typedInput, identityFresh,
                  cancelRequested, remaining, precision, completeness,
                  applicability, termination, evidence, outcomeWritten,
                  resourcePresent, evidencePresent, authoritative,
                  resourceHandleLive, boundSnapshot, boundContext,
                  resultSnapshot, resultContext>>

Next ==
  \/ \E valid \in BOOLEAN, typed \in BOOLEAN, fresh \in BOOLEAN,
        budget \in 0..MaxBudget, snapshot \in 1..2, context \in 1..2 :
       Begin(valid, typed, fresh, budget, snapshot, context)
  \/ RequestCancel
  \/ Consume
  \/ CompleteExact
  \/ CompleteExactIncomplete
  \/ CompleteApproximate
  \/ ObserveCancellation
  \/ ExhaustBudget
  \/ Fail
  \/ ReleaseResource
  \/ ReleaseEvidence

Spec == Init /\ [][Next]_vars

(***************************************************************************)
(* Safety invariants                                                       *)
(***************************************************************************)

RejectedRequestNeverRuns ==
  phase = "Rejected" => ~outcomeWritten /\ ~resourcePresent /\ ~evidencePresent

TerminalNonSuccessNeverPublishes ==
  phase \in {"Cancelled", "BudgetExhausted", "Failed", "Rejected"} =>
    ~resourcePresent /\ ~evidencePresent

CancellationNeverPublishes ==
  termination = "Cancelled" =>
    outcomeWritten /\ completeness = "Incomplete"
    /\ ~resourcePresent /\ ~evidencePresent

BudgetExhaustionNeverPublishes ==
  termination = "BudgetExhausted" =>
    outcomeWritten /\ completeness = "Incomplete"
    /\ ~resourcePresent /\ ~evidencePresent

EvidenceRequiresResource == evidencePresent => resourcePresent

OpaqueV1NeverHasTypedEvidence == ~typedInput => evidence # "Verified"

AuthoritativeRequiresVerifiedEvidence ==
  authoritative =>
    resourcePresent /\ evidencePresent
    /\ precision = "Exact" /\ completeness = "Complete"
    /\ applicability = "Applicable" /\ termination = "Succeeded"
    /\ evidence = "Verified" /\ typedInput

PublishedIdentityIsFreshAndBound ==
  resourcePresent =>
    identityFresh
    /\ resultSnapshot = boundSnapshot
    /\ resultContext = boundContext

LiveHandlesWerePublished ==
  /\ resourceHandleLive => resourcePresent
  /\ evidenceHandleLive => evidencePresent

CancelledStateIsSticky == phase = "Cancelled" => cancelRequested

=============================================================================
