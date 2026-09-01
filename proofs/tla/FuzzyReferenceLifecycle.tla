----------------------- MODULE FuzzyReferenceLifecycle -----------------------
(***************************************************************************)
(* Bounded lifecycle for one fuzzy dictionary query.  A query captures one *)
(* immutable snapshot, chooses a candidate feed, confirms every candidate   *)
(* independently against the exact reference set, and classifies the final  *)
(* outcome without promoting approximation or incompleteness.               *)
(***************************************************************************)
EXTENDS FiniteSets

CONSTANTS Terms, ReferenceTerms, CompleteFeed, ExpectedSnapshot, NextSnapshot

ASSUME /\ Terms # {}
       /\ ReferenceTerms \subseteq Terms
       /\ CompleteFeed \subseteq Terms
       /\ ReferenceTerms \subseteq CompleteFeed
       /\ ExpectedSnapshot # NextSnapshot

VARIABLES phase, liveSnapshot, capturedSnapshot, feed, pending, checked,
          accepted, coverage, precision, outcome

vars == <<phase, liveSnapshot, capturedSnapshot, feed, pending, checked,
          accepted, coverage, precision, outcome>>

Phases == {"Uncaptured", "Generating", "Confirming", "Completed", "Published"}
CoverageKinds == {"Undecided", "Complete", "Incomplete"}
PrecisionKinds == {"Undecided", "Exact", "Approximate"}
Outcomes == {"None", "CompleteExact", "CompleteApproximate", "Incomplete"}

TypeOK ==
  /\ phase \in Phases
  /\ liveSnapshot \in {ExpectedSnapshot, NextSnapshot}
  /\ capturedSnapshot \in {ExpectedSnapshot, NextSnapshot}
  /\ feed \subseteq Terms
  /\ pending \subseteq Terms
  /\ checked \subseteq Terms
  /\ accepted \subseteq Terms
  /\ coverage \in CoverageKinds
  /\ precision \in PrecisionKinds
  /\ outcome \in Outcomes

Init ==
  /\ phase = "Uncaptured"
  /\ liveSnapshot = ExpectedSnapshot
  /\ capturedSnapshot = ExpectedSnapshot
  /\ feed = {}
  /\ pending = {}
  /\ checked = {}
  /\ accepted = {}
  /\ coverage = "Undecided"
  /\ precision = "Undecided"
  /\ outcome = "None"

Capture ==
  /\ phase = "Uncaptured"
  /\ capturedSnapshot' = liveSnapshot
  /\ phase' = "Generating"
  /\ UNCHANGED <<liveSnapshot, feed, pending, checked, accepted,
                  coverage, precision, outcome>>

SelectFeed(selectedCoverage, selectedPrecision, selectedFeed) ==
  /\ phase = "Generating"
  /\ selectedCoverage \in {"Complete", "Incomplete"}
  /\ selectedPrecision \in {"Exact", "Approximate"}
  /\ selectedFeed \subseteq Terms
  /\ IF selectedCoverage = "Complete"
        THEN ReferenceTerms \subseteq selectedFeed
        ELSE ~(ReferenceTerms \subseteq selectedFeed)
  /\ coverage' = selectedCoverage
  /\ precision' = selectedPrecision
  /\ feed' = selectedFeed
  /\ pending' = selectedFeed
  /\ checked' = {}
  /\ accepted' = {}
  /\ phase' = "Confirming"
  /\ UNCHANGED <<liveSnapshot, capturedSnapshot, outcome>>

(** Confirmation order is nondeterministic, but the accepted set is exactly *)
(** the checked candidates that belong to the captured reference denotation. *)
Confirm(term) ==
  /\ phase = "Confirming"
  /\ term \in pending
  /\ pending' = pending \ {term}
  /\ checked' = checked \cup {term}
  /\ accepted' = IF term \in ReferenceTerms
                    THEN accepted \cup {term}
                    ELSE accepted
  /\ UNCHANGED <<phase, liveSnapshot, capturedSnapshot, feed,
                  coverage, precision, outcome>>

(** Mutation of the live dictionary is legal after capture and cannot alter *)
(** the captured revision used by confirmation. *)
MutateLive ==
  /\ phase \in {"Generating", "Confirming", "Completed", "Published"}
  /\ liveSnapshot = ExpectedSnapshot
  /\ liveSnapshot' = NextSnapshot
  /\ UNCHANGED <<phase, capturedSnapshot, feed, pending, checked,
                  accepted, coverage, precision, outcome>>

Finish ==
  /\ phase = "Confirming"
  /\ pending = {}
  /\ phase' = "Completed"
  /\ outcome' =
       IF coverage = "Incomplete" THEN "Incomplete"
       ELSE IF precision = "Exact" THEN "CompleteExact"
       ELSE "CompleteApproximate"
  /\ UNCHANGED <<liveSnapshot, capturedSnapshot, feed, pending, checked,
                  accepted, coverage, precision>>

Publish ==
  /\ phase = "Completed"
  /\ phase' = "Published"
  /\ UNCHANGED <<liveSnapshot, capturedSnapshot, feed, pending, checked,
                  accepted, coverage, precision, outcome>>

(** A published result is terminal.  The explicit self-loop lets TLC check *)
(** terminal states without treating intended quiescence as a deadlock. *)
Quiesce ==
  /\ phase = "Published"
  /\ UNCHANGED vars

Next ==
  \/ Capture
  \/ \E selectedCoverage \in {"Complete", "Incomplete"},
        selectedPrecision \in {"Exact", "Approximate"},
        selectedFeed \in SUBSET Terms :
       SelectFeed(selectedCoverage, selectedPrecision, selectedFeed)
  \/ \E term \in Terms : Confirm(term)
  \/ MutateLive
  \/ Finish
  \/ Publish
  \/ Quiesce

Spec == Init /\ [][Next]_vars

(***************************************************************************)
(* Safety invariants                                                        *)
(***************************************************************************)

CaptureIsStable ==
  phase # "Uncaptured" => capturedSnapshot = ExpectedSnapshot

LiveMutationDoesNotChangeCapture ==
  liveSnapshot = NextSnapshot => capturedSnapshot = ExpectedSnapshot

CandidateAccounting ==
  /\ pending \cup checked = feed
  /\ pending \cap checked = {}

AcceptedExactlyCheckedReference ==
  accepted = checked \cap ReferenceTerms

AcceptedSubsetReference == accepted \subseteq ReferenceTerms

CompleteCoverageContainsReference ==
  coverage = "Complete" => ReferenceTerms \subseteq feed

IncompleteCoverageMissesReference ==
  coverage = "Incomplete" => ~(ReferenceTerms \subseteq feed)

ExactOutcomeRequiresEvidence ==
  outcome = "CompleteExact" =>
    /\ coverage = "Complete"
    /\ precision = "Exact"
    /\ capturedSnapshot = ExpectedSnapshot

IncompleteNeverPublishesAsExact ==
  coverage = "Incomplete" => outcome # "CompleteExact"

ApproximateNeverPublishesAsExact ==
  precision = "Approximate" => outcome # "CompleteExact"

CompleteExactEqualsReference ==
  outcome = "CompleteExact" => accepted = ReferenceTerms

PublishedHasClassifiedOutcome ==
  phase = "Published" => outcome # "None"

=============================================================================
