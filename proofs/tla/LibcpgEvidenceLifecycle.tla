---------------------- MODULE LibcpgEvidenceLifecycle ----------------------
(***************************************************************************)
(* One libcpg analysis captures an immutable five-coordinate evidence index, *)
(* emits a candidate report, receives a separately bound guarantee, and may  *)
(* publish CompleteExact only when every binding, trust, independence,        *)
(* precision, and completeness obligation holds.                             *)
(***************************************************************************)
EXTENDS FiniteSets

CONSTANTS Subjects, Snapshots, Configurations, Tools, Environments, Digests,
          Actors, ExpectedSubject, AlternateSubject, ExpectedSnapshot,
          AlternateSnapshot, ExpectedConfiguration, AlternateConfiguration,
          ExpectedTool, AlternateTool, ExpectedEnvironment,
          AlternateEnvironment, ExpectedDigest, AlternateDigest, Producer,
          IndependentVerifier

ASSUME /\ ExpectedSubject \in Subjects
       /\ AlternateSubject \in Subjects
       /\ ExpectedSubject # AlternateSubject
       /\ ExpectedSnapshot \in Snapshots
       /\ AlternateSnapshot \in Snapshots
       /\ ExpectedSnapshot # AlternateSnapshot
       /\ ExpectedConfiguration \in Configurations
       /\ AlternateConfiguration \in Configurations
       /\ ExpectedConfiguration # AlternateConfiguration
       /\ ExpectedTool \in Tools
       /\ AlternateTool \in Tools
       /\ ExpectedTool # AlternateTool
       /\ ExpectedEnvironment \in Environments
       /\ AlternateEnvironment \in Environments
       /\ ExpectedEnvironment # AlternateEnvironment
       /\ ExpectedDigest \in Digests
       /\ AlternateDigest \in Digests
       /\ ExpectedDigest # AlternateDigest
       /\ Producer \in Actors
       /\ IndependentVerifier \in Actors
       /\ Producer # IndependentVerifier

VARIABLES phase,
          requestedSubject, requestedSnapshot, requestedConfiguration,
          requestedTool, requestedEnvironment,
          capturedSubject, capturedSnapshot, capturedConfiguration,
          capturedTool, capturedEnvironment,
          reportPrecision, reportCoverage, reportDigest,
          guaranteeKind, guaranteeSubject, guaranteeSnapshot,
          guaranteeConfiguration, guaranteeTool, guaranteeEnvironment,
          guaranteeDigest, guaranteeVerifier, guaranteeTrusted,
          guaranteeIndependence, outcome

vars ==
  <<phase,
    requestedSubject, requestedSnapshot, requestedConfiguration,
    requestedTool, requestedEnvironment,
    capturedSubject, capturedSnapshot, capturedConfiguration,
    capturedTool, capturedEnvironment,
    reportPrecision, reportCoverage, reportDigest,
    guaranteeKind, guaranteeSubject, guaranteeSnapshot,
    guaranteeConfiguration, guaranteeTool, guaranteeEnvironment,
    guaranteeDigest, guaranteeVerifier, guaranteeTrusted,
    guaranteeIndependence, outcome>>

Phases == {"Uncaptured", "Captured", "Analyzed", "Guaranteed", "Published"}
Precisions == {"Undecided", "Exact", "Approximate"}
Coverages == {"Undecided", "Complete", "Incomplete"}
GuaranteeKinds ==
  {"Undecided", "Valid", "SubjectStale", "SnapshotStale",
   "ConfigurationStale", "ToolStale", "EnvironmentStale",
   "DigestMismatch", "Untrusted", "Dependent", "Self"}
IndependenceKinds == {"Undecided", "Independent", "Dependent"}
Outcomes ==
  {"None", "CompleteExact", "CompleteApproximate", "Incomplete", "Rejected"}

TypeOK ==
  /\ phase \in Phases
  /\ requestedSubject \in Subjects
  /\ requestedSnapshot \in Snapshots
  /\ requestedConfiguration \in Configurations
  /\ requestedTool \in Tools
  /\ requestedEnvironment \in Environments
  /\ capturedSubject \in Subjects
  /\ capturedSnapshot \in Snapshots
  /\ capturedConfiguration \in Configurations
  /\ capturedTool \in Tools
  /\ capturedEnvironment \in Environments
  /\ reportPrecision \in Precisions
  /\ reportCoverage \in Coverages
  /\ reportDigest \in Digests
  /\ guaranteeKind \in GuaranteeKinds
  /\ guaranteeSubject \in Subjects
  /\ guaranteeSnapshot \in Snapshots
  /\ guaranteeConfiguration \in Configurations
  /\ guaranteeTool \in Tools
  /\ guaranteeEnvironment \in Environments
  /\ guaranteeDigest \in Digests
  /\ guaranteeVerifier \in Actors
  /\ guaranteeTrusted \in BOOLEAN
  /\ guaranteeIndependence \in IndependenceKinds
  /\ outcome \in Outcomes

Init ==
  /\ phase = "Uncaptured"
  /\ requestedSubject = ExpectedSubject
  /\ requestedSnapshot = ExpectedSnapshot
  /\ requestedConfiguration = ExpectedConfiguration
  /\ requestedTool = ExpectedTool
  /\ requestedEnvironment = ExpectedEnvironment
  /\ capturedSubject = ExpectedSubject
  /\ capturedSnapshot = ExpectedSnapshot
  /\ capturedConfiguration = ExpectedConfiguration
  /\ capturedTool = ExpectedTool
  /\ capturedEnvironment = ExpectedEnvironment
  /\ reportPrecision = "Undecided"
  /\ reportCoverage = "Undecided"
  /\ reportDigest = ExpectedDigest
  /\ guaranteeKind = "Undecided"
  /\ guaranteeSubject = ExpectedSubject
  /\ guaranteeSnapshot = ExpectedSnapshot
  /\ guaranteeConfiguration = ExpectedConfiguration
  /\ guaranteeTool = ExpectedTool
  /\ guaranteeEnvironment = ExpectedEnvironment
  /\ guaranteeDigest = ExpectedDigest
  /\ guaranteeVerifier = IndependentVerifier
  /\ guaranteeTrusted = FALSE
  /\ guaranteeIndependence = "Undecided"
  /\ outcome = "None"

Capture ==
  /\ phase = "Uncaptured"
  /\ capturedSubject' = requestedSubject
  /\ capturedSnapshot' = requestedSnapshot
  /\ capturedConfiguration' = requestedConfiguration
  /\ capturedTool' = requestedTool
  /\ capturedEnvironment' = requestedEnvironment
  /\ phase' = "Captured"
  /\ UNCHANGED
       <<requestedSubject, requestedSnapshot, requestedConfiguration,
         requestedTool, requestedEnvironment,
         reportPrecision, reportCoverage, reportDigest,
         guaranteeKind, guaranteeSubject, guaranteeSnapshot,
         guaranteeConfiguration, guaranteeTool, guaranteeEnvironment,
         guaranteeDigest, guaranteeVerifier, guaranteeTrusted,
         guaranteeIndependence, outcome>>

Analyze(precision, coverage) ==
  /\ phase = "Captured"
  /\ precision \in {"Exact", "Approximate"}
  /\ coverage \in {"Complete", "Incomplete"}
  /\ reportPrecision' = precision
  /\ reportCoverage' = coverage
  /\ reportDigest' = ExpectedDigest
  /\ phase' = "Analyzed"
  /\ UNCHANGED
       <<requestedSubject, requestedSnapshot, requestedConfiguration,
         requestedTool, requestedEnvironment,
         capturedSubject, capturedSnapshot, capturedConfiguration,
         capturedTool, capturedEnvironment,
         guaranteeKind, guaranteeSubject, guaranteeSnapshot,
         guaranteeConfiguration, guaranteeTool, guaranteeEnvironment,
         guaranteeDigest, guaranteeVerifier, guaranteeTrusted,
         guaranteeIndependence, outcome>>

IssueGuarantee(kind) ==
  /\ phase = "Analyzed"
  /\ kind \in GuaranteeKinds \ {"Undecided"}
  /\ guaranteeKind' = kind
  /\ guaranteeSubject' =
       IF kind = "SubjectStale" THEN AlternateSubject ELSE capturedSubject
  /\ guaranteeSnapshot' =
       IF kind = "SnapshotStale" THEN AlternateSnapshot ELSE capturedSnapshot
  /\ guaranteeConfiguration' =
       IF kind = "ConfigurationStale"
         THEN AlternateConfiguration ELSE capturedConfiguration
  /\ guaranteeTool' =
       IF kind = "ToolStale" THEN AlternateTool ELSE capturedTool
  /\ guaranteeEnvironment' =
       IF kind = "EnvironmentStale"
         THEN AlternateEnvironment ELSE capturedEnvironment
  /\ guaranteeDigest' =
       IF kind = "DigestMismatch" THEN AlternateDigest ELSE reportDigest
  /\ guaranteeVerifier' =
       IF kind = "Self" THEN Producer ELSE IndependentVerifier
  /\ guaranteeTrusted' = (kind # "Untrusted")
  /\ guaranteeIndependence' =
       IF kind \in {"Dependent", "Self"} THEN "Dependent" ELSE "Independent"
  /\ phase' = "Guaranteed"
  /\ UNCHANGED
       <<requestedSubject, requestedSnapshot, requestedConfiguration,
         requestedTool, requestedEnvironment,
         capturedSubject, capturedSnapshot, capturedConfiguration,
         capturedTool, capturedEnvironment,
         reportPrecision, reportCoverage, reportDigest, outcome>>

ChangeRequestedSubject ==
  /\ phase \in {"Captured", "Analyzed", "Guaranteed"}
  /\ requestedSubject = ExpectedSubject
  /\ requestedSubject' = AlternateSubject
  /\ UNCHANGED
       <<phase, requestedSnapshot, requestedConfiguration,
         requestedTool, requestedEnvironment,
         capturedSubject, capturedSnapshot, capturedConfiguration,
         capturedTool, capturedEnvironment,
         reportPrecision, reportCoverage, reportDigest,
         guaranteeKind, guaranteeSubject, guaranteeSnapshot,
         guaranteeConfiguration, guaranteeTool, guaranteeEnvironment,
         guaranteeDigest, guaranteeVerifier, guaranteeTrusted,
         guaranteeIndependence, outcome>>

ChangeRequestedSnapshot ==
  /\ phase \in {"Captured", "Analyzed", "Guaranteed"}
  /\ requestedSnapshot = ExpectedSnapshot
  /\ requestedSnapshot' = AlternateSnapshot
  /\ UNCHANGED
       <<phase, requestedSubject, requestedConfiguration,
         requestedTool, requestedEnvironment,
         capturedSubject, capturedSnapshot, capturedConfiguration,
         capturedTool, capturedEnvironment,
         reportPrecision, reportCoverage, reportDigest,
         guaranteeKind, guaranteeSubject, guaranteeSnapshot,
         guaranteeConfiguration, guaranteeTool, guaranteeEnvironment,
         guaranteeDigest, guaranteeVerifier, guaranteeTrusted,
         guaranteeIndependence, outcome>>

ChangeRequestedConfiguration ==
  /\ phase \in {"Captured", "Analyzed", "Guaranteed"}
  /\ requestedConfiguration = ExpectedConfiguration
  /\ requestedConfiguration' = AlternateConfiguration
  /\ UNCHANGED
       <<phase, requestedSubject, requestedSnapshot,
         requestedTool, requestedEnvironment,
         capturedSubject, capturedSnapshot, capturedConfiguration,
         capturedTool, capturedEnvironment,
         reportPrecision, reportCoverage, reportDigest,
         guaranteeKind, guaranteeSubject, guaranteeSnapshot,
         guaranteeConfiguration, guaranteeTool, guaranteeEnvironment,
         guaranteeDigest, guaranteeVerifier, guaranteeTrusted,
         guaranteeIndependence, outcome>>

ChangeRequestedTool ==
  /\ phase \in {"Captured", "Analyzed", "Guaranteed"}
  /\ requestedTool = ExpectedTool
  /\ requestedTool' = AlternateTool
  /\ UNCHANGED
       <<phase, requestedSubject, requestedSnapshot, requestedConfiguration,
         requestedEnvironment,
         capturedSubject, capturedSnapshot, capturedConfiguration,
         capturedTool, capturedEnvironment,
         reportPrecision, reportCoverage, reportDigest,
         guaranteeKind, guaranteeSubject, guaranteeSnapshot,
         guaranteeConfiguration, guaranteeTool, guaranteeEnvironment,
         guaranteeDigest, guaranteeVerifier, guaranteeTrusted,
         guaranteeIndependence, outcome>>

ChangeRequestedEnvironment ==
  /\ phase \in {"Captured", "Analyzed", "Guaranteed"}
  /\ requestedEnvironment = ExpectedEnvironment
  /\ requestedEnvironment' = AlternateEnvironment
  /\ UNCHANGED
       <<phase, requestedSubject, requestedSnapshot, requestedConfiguration,
         requestedTool,
         capturedSubject, capturedSnapshot, capturedConfiguration,
         capturedTool, capturedEnvironment,
         reportPrecision, reportCoverage, reportDigest,
         guaranteeKind, guaranteeSubject, guaranteeSnapshot,
         guaranteeConfiguration, guaranteeTool, guaranteeEnvironment,
         guaranteeDigest, guaranteeVerifier, guaranteeTrusted,
         guaranteeIndependence, outcome>>

IndexFresh ==
  /\ capturedSubject = requestedSubject
  /\ capturedSnapshot = requestedSnapshot
  /\ capturedConfiguration = requestedConfiguration
  /\ capturedTool = requestedTool
  /\ capturedEnvironment = requestedEnvironment

GuaranteeBound ==
  /\ guaranteeSubject = capturedSubject
  /\ guaranteeSnapshot = capturedSnapshot
  /\ guaranteeConfiguration = capturedConfiguration
  /\ guaranteeTool = capturedTool
  /\ guaranteeEnvironment = capturedEnvironment
  /\ guaranteeDigest = reportDigest

ValidExactEvidence ==
  /\ reportPrecision = "Exact"
  /\ reportCoverage = "Complete"
  /\ IndexFresh
  /\ GuaranteeBound
  /\ guaranteeTrusted
  /\ guaranteeIndependence = "Independent"
  /\ guaranteeVerifier # Producer

Publish ==
  /\ phase = "Guaranteed"
  /\ phase' = "Published"
  /\ outcome' =
       IF ValidExactEvidence THEN "CompleteExact"
       ELSE IF reportCoverage = "Incomplete" THEN "Incomplete"
       ELSE IF reportPrecision = "Approximate" THEN "CompleteApproximate"
       ELSE "Rejected"
  /\ UNCHANGED
       <<requestedSubject, requestedSnapshot, requestedConfiguration,
         requestedTool, requestedEnvironment,
         capturedSubject, capturedSnapshot, capturedConfiguration,
         capturedTool, capturedEnvironment,
         reportPrecision, reportCoverage, reportDigest,
         guaranteeKind, guaranteeSubject, guaranteeSnapshot,
         guaranteeConfiguration, guaranteeTool, guaranteeEnvironment,
         guaranteeDigest, guaranteeVerifier, guaranteeTrusted,
         guaranteeIndependence>>

Quiesce ==
  /\ phase = "Published"
  /\ UNCHANGED vars

Next ==
  \/ Capture
  \/ \E precision \in {"Exact", "Approximate"},
        coverage \in {"Complete", "Incomplete"} :
       Analyze(precision, coverage)
  \/ \E kind \in GuaranteeKinds \ {"Undecided"} : IssueGuarantee(kind)
  \/ ChangeRequestedSubject
  \/ ChangeRequestedSnapshot
  \/ ChangeRequestedConfiguration
  \/ ChangeRequestedTool
  \/ ChangeRequestedEnvironment
  \/ Publish
  \/ Quiesce

Spec == Init /\ [][Next]_vars

CapturedIndexIsStable ==
  phase # "Uncaptured" =>
    /\ capturedSubject = ExpectedSubject
    /\ capturedSnapshot = ExpectedSnapshot
    /\ capturedConfiguration = ExpectedConfiguration
    /\ capturedTool = ExpectedTool
    /\ capturedEnvironment = ExpectedEnvironment

ExactPublicationRequiresAllEvidence ==
  outcome = "CompleteExact" => ValidExactEvidence

SubjectStalenessBlocksExact ==
  capturedSubject # requestedSubject => outcome # "CompleteExact"

SnapshotStalenessBlocksExact ==
  capturedSnapshot # requestedSnapshot => outcome # "CompleteExact"

ConfigurationStalenessBlocksExact ==
  capturedConfiguration # requestedConfiguration => outcome # "CompleteExact"

ToolStalenessBlocksExact ==
  capturedTool # requestedTool => outcome # "CompleteExact"

EnvironmentStalenessBlocksExact ==
  capturedEnvironment # requestedEnvironment => outcome # "CompleteExact"

GuaranteeIndexMismatchBlocksExact ==
  ~(GuaranteeBound) => outcome # "CompleteExact"

DigestMismatchBlocksExact ==
  guaranteeDigest # reportDigest => outcome # "CompleteExact"

UntrustedGuaranteeBlocksExact ==
  ~guaranteeTrusted => outcome # "CompleteExact"

DependentGuaranteeBlocksExact ==
  guaranteeIndependence = "Dependent" => outcome # "CompleteExact"

SelfConfirmationBlocksExact ==
  guaranteeVerifier = Producer => outcome # "CompleteExact"

DistinctNamesAreInsufficient ==
  /\ guaranteeVerifier # Producer
  /\ guaranteeIndependence = "Dependent"
  => outcome # "CompleteExact"

IncompleteNeverPublishesExact ==
  reportCoverage = "Incomplete" => outcome # "CompleteExact"

ApproximateNeverPublishesExact ==
  reportPrecision = "Approximate" => outcome # "CompleteExact"

PublishedHasClassifiedOutcome ==
  phase = "Published" => outcome # "None"

=============================================================================
