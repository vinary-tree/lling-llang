---------------------- MODULE DictionarySurfaceLifecycle ----------------------
(***************************************************************************)
(* Bounded lifecycle for the split dictionary/fuzzy adapter.  One capture  *)
(* fixes the snapshot, normalization profile, edit profile, and bound.      *)
(* Candidate confirmation is independent, caps are incomplete, and the     *)
(* duallity facade publishes exactly the native adapter observation.        *)
(***************************************************************************)
EXTENDS FiniteSets

CONSTANTS Terms, ReferenceTerms, CompleteFeed, CappedFeed,
          ExpectedSnapshot, NextSnapshot,
          ExpectedNormalization, NextNormalization,
          ExpectedEditProfile, NextEditProfile,
          ExpectedBound, NextBound, FalsePositive

ASSUME /\ Terms # {}
       /\ ReferenceTerms \subseteq Terms
       /\ CompleteFeed \subseteq Terms
       /\ CappedFeed \subseteq Terms
       /\ ReferenceTerms \subseteq CompleteFeed
       /\ ~(ReferenceTerms \subseteq CappedFeed)
       /\ FalsePositive \in Terms \ ReferenceTerms
       /\ ExpectedSnapshot # NextSnapshot
       /\ ExpectedNormalization # NextNormalization
       /\ ExpectedEditProfile # NextEditProfile
       /\ ExpectedBound # NextBound

VARIABLES phase,
          capturedSnapshot, capturedNormalization, capturedEditProfile, capturedBound,
          feedSnapshot, feedNormalization, feedEditProfile, feedBound,
          feed, pending, checked, accepted,
          coverage, precision, termination, outcome,
          nativePublished, facadePublished

vars == <<phase,
          capturedSnapshot, capturedNormalization, capturedEditProfile, capturedBound,
          feedSnapshot, feedNormalization, feedEditProfile, feedBound,
          feed, pending, checked, accepted,
          coverage, precision, termination, outcome,
          nativePublished, facadePublished>>

Phases == {"Uncaptured", "Generating", "Confirming", "Completed", "Published"}
CoverageKinds == {"Undecided", "Complete", "Incomplete"}
PrecisionKinds == {"Undecided", "Exact", "Approximate"}
TerminationKinds == {"Undecided", "Exhausted", "Capped", "Cancelled", "Failed"}
Outcomes == {"None", "CompleteExact", "CompleteApproximate", "Incomplete"}

TypeOK ==
  /\ phase \in Phases
  /\ capturedSnapshot \in {ExpectedSnapshot, NextSnapshot}
  /\ capturedNormalization \in {ExpectedNormalization, NextNormalization}
  /\ capturedEditProfile \in {ExpectedEditProfile, NextEditProfile}
  /\ capturedBound \in {ExpectedBound, NextBound}
  /\ feedSnapshot \in {ExpectedSnapshot, NextSnapshot}
  /\ feedNormalization \in {ExpectedNormalization, NextNormalization}
  /\ feedEditProfile \in {ExpectedEditProfile, NextEditProfile}
  /\ feedBound \in {ExpectedBound, NextBound}
  /\ feed \subseteq Terms
  /\ pending \subseteq Terms
  /\ checked \subseteq Terms
  /\ accepted \subseteq Terms
  /\ nativePublished \subseteq Terms
  /\ facadePublished \subseteq Terms
  /\ coverage \in CoverageKinds
  /\ precision \in PrecisionKinds
  /\ termination \in TerminationKinds
  /\ outcome \in Outcomes

Init ==
  /\ phase = "Uncaptured"
  /\ capturedSnapshot = ExpectedSnapshot
  /\ capturedNormalization = ExpectedNormalization
  /\ capturedEditProfile = ExpectedEditProfile
  /\ capturedBound = ExpectedBound
  /\ feedSnapshot = ExpectedSnapshot
  /\ feedNormalization = ExpectedNormalization
  /\ feedEditProfile = ExpectedEditProfile
  /\ feedBound = ExpectedBound
  /\ feed = {}
  /\ pending = {}
  /\ checked = {}
  /\ accepted = {}
  /\ coverage = "Undecided"
  /\ precision = "Undecided"
  /\ termination = "Undecided"
  /\ outcome = "None"
  /\ nativePublished = {}
  /\ facadePublished = {}

Capture ==
  /\ phase = "Uncaptured"
  /\ phase' = "Generating"
  /\ capturedSnapshot' = ExpectedSnapshot
  /\ capturedNormalization' = ExpectedNormalization
  /\ capturedEditProfile' = ExpectedEditProfile
  /\ capturedBound' = ExpectedBound
  /\ UNCHANGED <<feedSnapshot, feedNormalization, feedEditProfile, feedBound,
                  feed, pending, checked, accepted, coverage, precision,
                  termination, outcome, nativePublished, facadePublished>>

SelectFeed(selectedFeed, selectedCoverage, selectedTermination) ==
  /\ phase = "Generating"
  /\ selectedFeed \in {CompleteFeed, CappedFeed}
  /\ selectedCoverage \in {"Complete", "Incomplete"}
  /\ selectedTermination \in {"Exhausted", "Capped"}
  /\ IF selectedFeed = CompleteFeed
        THEN /\ selectedCoverage = "Complete"
             /\ selectedTermination = "Exhausted"
        ELSE /\ selectedCoverage = "Incomplete"
             /\ selectedTermination \in {"Capped", "Cancelled", "Failed"}
  /\ feedSnapshot' = capturedSnapshot
  /\ feedNormalization' = capturedNormalization
  /\ feedEditProfile' = capturedEditProfile
  /\ feedBound' = capturedBound
  /\ feed' = selectedFeed
  /\ pending' = selectedFeed
  /\ checked' = {}
  /\ accepted' = {}
  /\ coverage' = selectedCoverage
  /\ precision' = "Exact"
  /\ termination' = selectedTermination
  /\ phase' = "Confirming"
  /\ UNCHANGED <<capturedSnapshot, capturedNormalization,
                  capturedEditProfile, capturedBound, outcome,
                  nativePublished, facadePublished>>

Confirm(term) ==
  /\ phase = "Confirming"
  /\ term \in pending
  /\ pending' = pending \ {term}
  /\ checked' = checked \cup {term}
  /\ accepted' = IF term \in ReferenceTerms
                    THEN accepted \cup {term}
                    ELSE accepted
  /\ UNCHANGED <<phase,
                  capturedSnapshot, capturedNormalization,
                  capturedEditProfile, capturedBound,
                  feedSnapshot, feedNormalization, feedEditProfile, feedBound,
                  feed, coverage, precision, termination, outcome,
                  nativePublished, facadePublished>>

Finish ==
  /\ phase = "Confirming"
  /\ pending = {}
  /\ phase' = "Completed"
  /\ outcome' =
       IF termination # "Exhausted" \/ coverage = "Incomplete"
         THEN "Incomplete"
       ELSE IF precision = "Exact"
         THEN "CompleteExact"
         ELSE "CompleteApproximate"
  /\ UNCHANGED <<capturedSnapshot, capturedNormalization,
                  capturedEditProfile, capturedBound,
                  feedSnapshot, feedNormalization, feedEditProfile, feedBound,
                  feed, pending, checked, accepted, coverage, precision,
                  termination, nativePublished, facadePublished>>

HaltNonExhaustive ==
  /\ phase = "Confirming"
  /\ termination \in {"Capped", "Cancelled", "Failed"}
  /\ phase' = "Completed"
  /\ outcome' = "Incomplete"
  /\ UNCHANGED <<capturedSnapshot, capturedNormalization,
                  capturedEditProfile, capturedBound,
                  feedSnapshot, feedNormalization, feedEditProfile, feedBound,
                  feed, pending, checked, accepted, coverage, precision,
                  termination, nativePublished, facadePublished>>

Publish ==
  /\ phase = "Completed"
  /\ phase' = "Published"
  /\ nativePublished' = accepted
  /\ facadePublished' = accepted
  /\ UNCHANGED <<capturedSnapshot, capturedNormalization,
                  capturedEditProfile, capturedBound,
                  feedSnapshot, feedNormalization, feedEditProfile, feedBound,
                  feed, pending, checked, accepted, coverage, precision,
                  termination, outcome>>

Quiesce ==
  /\ phase = "Published"
  /\ UNCHANGED vars

Next ==
  \/ Capture
  \/ \E selectedFeed \in {CompleteFeed, CappedFeed},
        selectedCoverage \in {"Complete", "Incomplete"},
        selectedTermination \in {"Exhausted", "Capped", "Cancelled", "Failed"} :
       SelectFeed(selectedFeed, selectedCoverage, selectedTermination)
  \/ \E term \in Terms : Confirm(term)
  \/ Finish
  \/ HaltNonExhaustive
  \/ Publish
  \/ Quiesce

Spec == Init /\ [][Next]_vars

CandidateIdentityMatchesCapture ==
  phase \in {"Confirming", "Completed", "Published"} =>
    /\ feedSnapshot = capturedSnapshot
    /\ feedNormalization = capturedNormalization
    /\ feedEditProfile = capturedEditProfile
    /\ feedBound = capturedBound

CandidateAccounting ==
  /\ pending \cup checked = feed
  /\ pending \cap checked = {}

AcceptedExactlyCheckedReference == accepted = checked \cap ReferenceTerms
AcceptedSubsetReference == accepted \subseteq ReferenceTerms

CompleteCoverageContainsReference ==
  coverage = "Complete" => ReferenceTerms \subseteq feed

NonExhaustiveTerminationIsIncomplete ==
  termination \in {"Capped", "Cancelled", "Failed"} =>
    outcome \in {"None", "Incomplete"}

CompleteExactRequiresExhaustion ==
  outcome = "CompleteExact" =>
    /\ termination = "Exhausted"
    /\ coverage = "Complete"
    /\ precision = "Exact"

CompleteExactEqualsReference ==
  outcome = "CompleteExact" => accepted = ReferenceTerms

FacadeEqualsNative ==
  phase = "Published" => facadePublished = nativePublished

PublishedEqualsAccepted ==
  phase = "Published" => nativePublished = accepted

=============================================================================
