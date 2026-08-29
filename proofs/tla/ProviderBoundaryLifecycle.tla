---------------------- MODULE ProviderBoundaryLifecycle ----------------------
(***************************************************************************)
(* Provider-neutral result, identity, assurance, cache, and native-handle   *)
(* lifecycle. A downstream adapter may transform payload representation but *)
(* must preserve completion class and limitations. Exact publication needs  *)
(* a fresh, trusted guarantee from an independently controlled verifier.     *)
(***************************************************************************)
EXTENDS FiniteSets, Naturals

CONSTANTS ArtifactIds, ConfigurationIds, ProviderIds, EnvironmentIds,
          ResultDigests, Actors, Domains,
          ExpectedArtifact, AlternateArtifact,
          ExpectedConfiguration, AlternateConfiguration,
          ExpectedProvider, AlternateProvider,
          ExpectedEnvironment, AlternateEnvironment,
          ExpectedResultDigest, AlternateResultDigest,
          ProducerActor, IndependentVerifierActor,
          ProducerDomain, IndependentVerifierDomain

ASSUME /\ ExpectedArtifact \in ArtifactIds
       /\ AlternateArtifact \in ArtifactIds
       /\ ExpectedArtifact # AlternateArtifact
       /\ ExpectedConfiguration \in ConfigurationIds
       /\ AlternateConfiguration \in ConfigurationIds
       /\ ExpectedConfiguration # AlternateConfiguration
       /\ ExpectedProvider \in ProviderIds
       /\ AlternateProvider \in ProviderIds
       /\ ExpectedProvider # AlternateProvider
       /\ ExpectedEnvironment \in EnvironmentIds
       /\ AlternateEnvironment \in EnvironmentIds
       /\ ExpectedEnvironment # AlternateEnvironment
       /\ ExpectedResultDigest \in ResultDigests
       /\ AlternateResultDigest \in ResultDigests
       /\ ExpectedResultDigest # AlternateResultDigest
       /\ ProducerActor \in Actors
       /\ IndependentVerifierActor \in Actors
       /\ ProducerActor # IndependentVerifierActor
       /\ ProducerDomain \in Domains
       /\ IndependentVerifierDomain \in Domains
       /\ ProducerDomain # IndependentVerifierDomain

VARIABLES phase,
          requestedArtifact, requestedConfiguration,
          requestedProvider, requestedEnvironment,
          capturedArtifact, capturedConfiguration,
          capturedProvider, capturedEnvironment,
          resultDigest, originalStatus, adaptedStatus,
          originalLimitations, adaptedLimitations,
          incompleteReason, checkpointPresent,
          guaranteeKind, guaranteeArtifact, guaranteeConfiguration,
          guaranteeProvider, guaranteeEnvironment, guaranteeResultDigest,
          guaranteeActor, guaranteeDomain, guaranteeTrusted,
          cacheClass, borrows, released, outcome

vars ==
  <<phase,
    requestedArtifact, requestedConfiguration,
    requestedProvider, requestedEnvironment,
    capturedArtifact, capturedConfiguration,
    capturedProvider, capturedEnvironment,
    resultDigest, originalStatus, adaptedStatus,
    originalLimitations, adaptedLimitations,
    incompleteReason, checkpointPresent,
    guaranteeKind, guaranteeArtifact, guaranteeConfiguration,
    guaranteeProvider, guaranteeEnvironment, guaranteeResultDigest,
    guaranteeActor, guaranteeDomain, guaranteeTrusted,
    cacheClass, borrows, released, outcome>>

Phases ==
  {"Registered", "Captured", "Returned", "Adapted", "Guaranteed",
   "Terminal", "Released"}
Statuses == {"Undecided", "CompleteExact", "CompleteApproximate", "Incomplete"}
Limitations == {"Undecided", "None", "Declared"}
IncompleteReasons == {"Undecided", "None", "ResourceLimit"}
GuaranteeKinds ==
  {"Undecided", "Valid", "ArtifactStale", "ConfigurationStale",
   "ProviderStale", "EnvironmentStale", "ResultDigestMismatch",
   "Untrusted", "Dependent", "Self"}
CacheClasses == {"None", "Exact", "Approximate"}
Outcomes ==
  {"None", "CompleteExact", "CompleteApproximate", "Incomplete", "Rejected"}

TypeOK ==
  /\ phase \in Phases
  /\ requestedArtifact \in ArtifactIds
  /\ requestedConfiguration \in ConfigurationIds
  /\ requestedProvider \in ProviderIds
  /\ requestedEnvironment \in EnvironmentIds
  /\ capturedArtifact \in ArtifactIds
  /\ capturedConfiguration \in ConfigurationIds
  /\ capturedProvider \in ProviderIds
  /\ capturedEnvironment \in EnvironmentIds
  /\ resultDigest \in ResultDigests
  /\ originalStatus \in Statuses
  /\ adaptedStatus \in Statuses
  /\ originalLimitations \in Limitations
  /\ adaptedLimitations \in Limitations
  /\ incompleteReason \in IncompleteReasons
  /\ checkpointPresent \in BOOLEAN
  /\ guaranteeKind \in GuaranteeKinds
  /\ guaranteeArtifact \in ArtifactIds
  /\ guaranteeConfiguration \in ConfigurationIds
  /\ guaranteeProvider \in ProviderIds
  /\ guaranteeEnvironment \in EnvironmentIds
  /\ guaranteeResultDigest \in ResultDigests
  /\ guaranteeActor \in Actors
  /\ guaranteeDomain \in Domains
  /\ guaranteeTrusted \in BOOLEAN
  /\ cacheClass \in CacheClasses
  /\ borrows \in 0..1
  /\ released \in BOOLEAN
  /\ outcome \in Outcomes

Init ==
  /\ phase = "Registered"
  /\ requestedArtifact = ExpectedArtifact
  /\ requestedConfiguration = ExpectedConfiguration
  /\ requestedProvider = ExpectedProvider
  /\ requestedEnvironment = ExpectedEnvironment
  /\ capturedArtifact = ExpectedArtifact
  /\ capturedConfiguration = ExpectedConfiguration
  /\ capturedProvider = ExpectedProvider
  /\ capturedEnvironment = ExpectedEnvironment
  /\ resultDigest = ExpectedResultDigest
  /\ originalStatus = "Undecided"
  /\ adaptedStatus = "Undecided"
  /\ originalLimitations = "Undecided"
  /\ adaptedLimitations = "Undecided"
  /\ incompleteReason = "Undecided"
  /\ checkpointPresent = FALSE
  /\ guaranteeKind = "Undecided"
  /\ guaranteeArtifact = ExpectedArtifact
  /\ guaranteeConfiguration = ExpectedConfiguration
  /\ guaranteeProvider = ExpectedProvider
  /\ guaranteeEnvironment = ExpectedEnvironment
  /\ guaranteeResultDigest = ExpectedResultDigest
  /\ guaranteeActor = IndependentVerifierActor
  /\ guaranteeDomain = IndependentVerifierDomain
  /\ guaranteeTrusted = FALSE
  /\ cacheClass = "None"
  /\ borrows = 0
  /\ released = FALSE
  /\ outcome = "None"

Capture ==
  /\ phase = "Registered"
  /\ capturedArtifact' = requestedArtifact
  /\ capturedConfiguration' = requestedConfiguration
  /\ capturedProvider' = requestedProvider
  /\ capturedEnvironment' = requestedEnvironment
  /\ borrows' = 1
  /\ phase' = "Captured"
  /\ UNCHANGED
       <<requestedArtifact, requestedConfiguration,
         requestedProvider, requestedEnvironment,
         resultDigest, originalStatus, adaptedStatus,
         originalLimitations, adaptedLimitations,
         incompleteReason, checkpointPresent,
         guaranteeKind, guaranteeArtifact, guaranteeConfiguration,
         guaranteeProvider, guaranteeEnvironment, guaranteeResultDigest,
         guaranteeActor, guaranteeDomain, guaranteeTrusted,
         cacheClass, released, outcome>>

Return(status) ==
  /\ phase = "Captured"
  /\ status \in {"CompleteExact", "CompleteApproximate", "Incomplete"}
  /\ originalStatus' = status
  /\ adaptedStatus' = status
  /\ originalLimitations' =
       IF status = "CompleteApproximate" THEN "Declared" ELSE "None"
  /\ adaptedLimitations' =
       IF status = "CompleteApproximate" THEN "Declared" ELSE "None"
  /\ incompleteReason' =
       IF status = "Incomplete" THEN "ResourceLimit" ELSE "None"
  /\ checkpointPresent' = (status = "Incomplete")
  /\ resultDigest' = ExpectedResultDigest
  /\ phase' = "Returned"
  /\ UNCHANGED
       <<requestedArtifact, requestedConfiguration,
         requestedProvider, requestedEnvironment,
         capturedArtifact, capturedConfiguration,
         capturedProvider, capturedEnvironment,
         guaranteeKind, guaranteeArtifact, guaranteeConfiguration,
         guaranteeProvider, guaranteeEnvironment, guaranteeResultDigest,
         guaranteeActor, guaranteeDomain, guaranteeTrusted,
         cacheClass, borrows, released, outcome>>

Adapt ==
  /\ phase = "Returned"
  /\ adaptedStatus' = originalStatus
  /\ adaptedLimitations' = originalLimitations
  /\ phase' = "Adapted"
  /\ UNCHANGED
       <<requestedArtifact, requestedConfiguration,
         requestedProvider, requestedEnvironment,
         capturedArtifact, capturedConfiguration,
         capturedProvider, capturedEnvironment,
         resultDigest, originalStatus, originalLimitations,
         incompleteReason, checkpointPresent,
         guaranteeKind, guaranteeArtifact, guaranteeConfiguration,
         guaranteeProvider, guaranteeEnvironment, guaranteeResultDigest,
         guaranteeActor, guaranteeDomain, guaranteeTrusted,
         cacheClass, borrows, released, outcome>>

IssueGuarantee(kind) ==
  /\ phase = "Adapted"
  /\ kind \in GuaranteeKinds \ {"Undecided"}
  /\ guaranteeKind' = kind
  /\ guaranteeArtifact' =
       IF kind = "ArtifactStale" THEN AlternateArtifact ELSE capturedArtifact
  /\ guaranteeConfiguration' =
       IF kind = "ConfigurationStale"
       THEN AlternateConfiguration ELSE capturedConfiguration
  /\ guaranteeProvider' =
       IF kind = "ProviderStale" THEN AlternateProvider ELSE capturedProvider
  /\ guaranteeEnvironment' =
       IF kind = "EnvironmentStale"
       THEN AlternateEnvironment ELSE capturedEnvironment
  /\ guaranteeResultDigest' =
       IF kind = "ResultDigestMismatch"
       THEN AlternateResultDigest ELSE resultDigest
  /\ guaranteeActor' =
       IF kind = "Self" THEN ProducerActor ELSE IndependentVerifierActor
  /\ guaranteeDomain' =
       IF kind \in {"Dependent", "Self"}
       THEN ProducerDomain ELSE IndependentVerifierDomain
  /\ guaranteeTrusted' = (kind # "Untrusted")
  /\ phase' = "Guaranteed"
  /\ UNCHANGED
       <<requestedArtifact, requestedConfiguration,
         requestedProvider, requestedEnvironment,
         capturedArtifact, capturedConfiguration,
         capturedProvider, capturedEnvironment,
         resultDigest, originalStatus, adaptedStatus,
         originalLimitations, adaptedLimitations,
         incompleteReason, checkpointPresent,
         cacheClass, borrows, released, outcome>>

CapturedFresh ==
  /\ capturedArtifact = requestedArtifact
  /\ capturedConfiguration = requestedConfiguration
  /\ capturedProvider = requestedProvider
  /\ capturedEnvironment = requestedEnvironment

GuaranteeFresh ==
  /\ guaranteeArtifact = requestedArtifact
  /\ guaranteeConfiguration = requestedConfiguration
  /\ guaranteeProvider = requestedProvider
  /\ guaranteeEnvironment = requestedEnvironment
  /\ guaranteeResultDigest = resultDigest

ExactEligible ==
  /\ adaptedStatus = "CompleteExact"
  /\ CapturedFresh
  /\ GuaranteeFresh
  /\ guaranteeTrusted
  /\ guaranteeActor # ProducerActor
  /\ guaranteeDomain # ProducerDomain

Publish ==
  /\ phase = "Guaranteed"
  /\ outcome' =
       IF ExactEligible THEN "CompleteExact"
       ELSE IF /\ adaptedStatus = "CompleteApproximate"
               /\ adaptedLimitations = "Declared"
            THEN "CompleteApproximate"
       ELSE IF adaptedStatus = "Incomplete" THEN "Incomplete"
       ELSE "Rejected"
  /\ cacheClass' =
       IF ExactEligible THEN "Exact"
       ELSE IF /\ adaptedStatus = "CompleteApproximate"
               /\ adaptedLimitations = "Declared"
            THEN "Approximate"
       ELSE "None"
  /\ phase' = "Terminal"
  /\ UNCHANGED
       <<requestedArtifact, requestedConfiguration,
         requestedProvider, requestedEnvironment,
         capturedArtifact, capturedConfiguration,
         capturedProvider, capturedEnvironment,
         resultDigest, originalStatus, adaptedStatus,
         originalLimitations, adaptedLimitations,
         incompleteReason, checkpointPresent,
         guaranteeKind, guaranteeArtifact, guaranteeConfiguration,
         guaranteeProvider, guaranteeEnvironment, guaranteeResultDigest,
         guaranteeActor, guaranteeDomain, guaranteeTrusted,
         borrows, released>>

Release ==
  /\ phase = "Terminal"
  /\ borrows = 1
  /\ borrows' = 0
  /\ released' = TRUE
  /\ phase' = "Released"
  /\ UNCHANGED
       <<requestedArtifact, requestedConfiguration,
         requestedProvider, requestedEnvironment,
         capturedArtifact, capturedConfiguration,
         capturedProvider, capturedEnvironment,
         resultDigest, originalStatus, adaptedStatus,
         originalLimitations, adaptedLimitations,
         incompleteReason, checkpointPresent,
         guaranteeKind, guaranteeArtifact, guaranteeConfiguration,
         guaranteeProvider, guaranteeEnvironment, guaranteeResultDigest,
         guaranteeActor, guaranteeDomain, guaranteeTrusted,
         cacheClass, outcome>>

ChangeRequestedArtifact ==
  /\ phase \in {"Captured", "Returned", "Adapted", "Guaranteed"}
  /\ requestedArtifact = ExpectedArtifact
  /\ requestedArtifact' = AlternateArtifact
  /\ UNCHANGED
       <<phase, requestedConfiguration, requestedProvider, requestedEnvironment,
         capturedArtifact, capturedConfiguration, capturedProvider,
         capturedEnvironment, resultDigest, originalStatus, adaptedStatus,
         originalLimitations, adaptedLimitations, incompleteReason,
         checkpointPresent, guaranteeKind, guaranteeArtifact,
         guaranteeConfiguration, guaranteeProvider, guaranteeEnvironment,
         guaranteeResultDigest, guaranteeActor, guaranteeDomain,
         guaranteeTrusted, cacheClass, borrows, released, outcome>>

ChangeRequestedConfiguration ==
  /\ phase \in {"Captured", "Returned", "Adapted", "Guaranteed"}
  /\ requestedConfiguration = ExpectedConfiguration
  /\ requestedConfiguration' = AlternateConfiguration
  /\ UNCHANGED
       <<phase, requestedArtifact, requestedProvider, requestedEnvironment,
         capturedArtifact, capturedConfiguration, capturedProvider,
         capturedEnvironment, resultDigest, originalStatus, adaptedStatus,
         originalLimitations, adaptedLimitations, incompleteReason,
         checkpointPresent, guaranteeKind, guaranteeArtifact,
         guaranteeConfiguration, guaranteeProvider, guaranteeEnvironment,
         guaranteeResultDigest, guaranteeActor, guaranteeDomain,
         guaranteeTrusted, cacheClass, borrows, released, outcome>>

ChangeRequestedProvider ==
  /\ phase \in {"Captured", "Returned", "Adapted", "Guaranteed"}
  /\ requestedProvider = ExpectedProvider
  /\ requestedProvider' = AlternateProvider
  /\ UNCHANGED
       <<phase, requestedArtifact, requestedConfiguration, requestedEnvironment,
         capturedArtifact, capturedConfiguration, capturedProvider,
         capturedEnvironment, resultDigest, originalStatus, adaptedStatus,
         originalLimitations, adaptedLimitations, incompleteReason,
         checkpointPresent, guaranteeKind, guaranteeArtifact,
         guaranteeConfiguration, guaranteeProvider, guaranteeEnvironment,
         guaranteeResultDigest, guaranteeActor, guaranteeDomain,
         guaranteeTrusted, cacheClass, borrows, released, outcome>>

ChangeRequestedEnvironment ==
  /\ phase \in {"Captured", "Returned", "Adapted", "Guaranteed"}
  /\ requestedEnvironment = ExpectedEnvironment
  /\ requestedEnvironment' = AlternateEnvironment
  /\ UNCHANGED
       <<phase, requestedArtifact, requestedConfiguration, requestedProvider,
         capturedArtifact, capturedConfiguration, capturedProvider,
         capturedEnvironment, resultDigest, originalStatus, adaptedStatus,
         originalLimitations, adaptedLimitations, incompleteReason,
         checkpointPresent, guaranteeKind, guaranteeArtifact,
         guaranteeConfiguration, guaranteeProvider, guaranteeEnvironment,
         guaranteeResultDigest, guaranteeActor, guaranteeDomain,
         guaranteeTrusted, cacheClass, borrows, released, outcome>>

Stop ==
  /\ phase = "Released"
  /\ UNCHANGED vars

Next ==
  Capture
  \/ \E status \in {"CompleteExact", "CompleteApproximate", "Incomplete"}:
       Return(status)
  \/ Adapt
  \/ \E kind \in GuaranteeKinds \ {"Undecided"}: IssueGuarantee(kind)
  \/ Publish
  \/ Release
  \/ ChangeRequestedArtifact
  \/ ChangeRequestedConfiguration
  \/ ChangeRequestedProvider
  \/ ChangeRequestedEnvironment
  \/ Stop

Spec == Init /\ [][Next]_vars

CapturedIdentityIsStable ==
  phase # "Registered" =>
    /\ capturedArtifact = ExpectedArtifact
    /\ capturedConfiguration = ExpectedConfiguration
    /\ capturedProvider = ExpectedProvider
    /\ capturedEnvironment = ExpectedEnvironment

AdaptationPreservesStatus ==
  phase \in {"Adapted", "Guaranteed", "Terminal", "Released"} =>
    adaptedStatus = originalStatus

AdaptationPreservesLimitations ==
  phase \in {"Adapted", "Guaranteed", "Terminal", "Released"} =>
    adaptedLimitations = originalLimitations

ApproximationCarriesLimitations ==
  adaptedStatus = "CompleteApproximate" => adaptedLimitations = "Declared"

IncompletePreservesReason ==
  adaptedStatus = "Incomplete" =>
    /\ incompleteReason = "ResourceLimit"
    /\ checkpointPresent

IncompleteNotCacheable ==
  adaptedStatus = "Incomplete" => cacheClass = "None"

ExactPublicationRequiresExactResult ==
  outcome = "CompleteExact" => adaptedStatus = "CompleteExact"

ExactPublicationRequiresFreshBinding ==
  outcome = "CompleteExact" => CapturedFresh /\ GuaranteeFresh

ExactPublicationRequiresTrustedGuarantee ==
  outcome = "CompleteExact" => guaranteeTrusted

ExactPublicationRequiresIndependentDomain ==
  outcome = "CompleteExact" => guaranteeDomain # ProducerDomain

SelfConfirmationBlocksExact ==
  guaranteeActor = ProducerActor => outcome # "CompleteExact"

DependentGuaranteeBlocksExact ==
  guaranteeDomain = ProducerDomain => outcome # "CompleteExact"

ApproximateNeverPublishesExact ==
  adaptedStatus = "CompleteApproximate" => outcome # "CompleteExact"

IncompleteNeverPublishesExact ==
  adaptedStatus = "Incomplete" => outcome # "CompleteExact"

CacheContainsOnlyComplete ==
  /\ cacheClass = "Exact" => outcome = "CompleteExact"
  /\ cacheClass = "Approximate" => outcome = "CompleteApproximate"

NativeOwnershipBalanced ==
  (borrows = 0) <=> phase \in {"Registered", "Released"}

ReleaseIsTerminal ==
  released => phase = "Released" /\ borrows = 0

TerminalOutcomeIsClassified ==
  phase \in {"Terminal", "Released"} => outcome # "None"

=============================================================================
