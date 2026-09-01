---------------------- MODULE NeutralFoundationLifecycle ----------------------
EXTENDS Naturals, Sequences, TLC

(***************************************************************************
Provider-neutral lifecycle shared by the canonical wire, analysis graph,
runtime, requirements, assurance, and documentation foundation contracts.
The model deliberately keeps epistemic axes independent and treats every
resource or validation failure as rejection rather than semantic success.
***************************************************************************)

CONSTANT OutputCap, Scenarios

VARIABLES phase, state

vars == <<phase, state>>

Phases == {
  "Initial", "Canonical", "Graph", "Runtime", "Requirements",
  "Assurance", "Documentation", "Terminal"
}

CanonicalOutcomes == {"Undecided", "Success", "Malformed", "Budget", "NumberRejected"}
Precisions == {"Undecided", "Exact", "Approximate"}
Completions == {"Undecided", "Complete", "Incomplete"}
Authorities == {
  "Undecided", "TheoremProof", "BoundedModelCheck", "Statistics",
  "Empirical", "Assumption", "Unsupported", "OutOfScope"
}
Obligations == {"Undecided", "Theorem", "Bounded", "Statistical", "Empirical"}
SpillLocations == {
  "Undecided", "None", "RepositoryBacked", "TemporaryMemoryFilesystem"
}
TerminalOutcomes == {"Undecided", "Released", "Rejected"}

CanonicalOutcomeFor(scenario) ==
  IF scenario = "Malformed" THEN "Malformed"
  ELSE IF scenario = "Budget" THEN "Budget"
  ELSE "Success"

StrengthBeforeFor(scenario) == IF scenario = "Projection" THEN 1 ELSE 3
PatchBaseMatchesFor(scenario) == scenario # "StalePatch"
PrecisionFor(scenario) == IF scenario = "Approximate" THEN "Approximate" ELSE "Exact"
CompletionFor(scenario) == IF scenario = "Incomplete" THEN "Incomplete" ELSE "Complete"
LocksMatchFor(scenario) == scenario # "StaleLocks"
OutputBytesFor(scenario) == IF scenario = "Spill" THEN 2 ELSE 0
CheckpointMatchesFor(scenario) == scenario # "StaleCheckpoint"
TombstonedFor(scenario) == scenario = "Tombstone"
AuthorityFor(scenario) == IF scenario = "StatisticsTheorem" THEN "Statistics" ELSE "TheoremProof"
ObligationFor(scenario) == "Theorem"
EvidenceFreshFor(scenario) == scenario # "StaleEvidence"
NegativeControlFor(scenario) == scenario # "MissingNegative"
AttestationMatchesFor(scenario) == scenario # "BadAttestation"
ManifestMatchesFor(scenario) == scenario # "StaleManifest"

Discharges(authority, obligation) ==
  \/ /\ authority = "TheoremProof" /\ obligation = "Theorem"
  \/ /\ authority = "BoundedModelCheck" /\ obligation = "Bounded"
  \/ /\ authority = "Statistics" /\ obligation = "Statistical"
  \/ /\ authority = "Empirical" /\ obligation = "Empirical"

InitialState == [
  scenario |-> "Undecided",
  profile |-> "Undecided",
  canonicalOutcome |-> "Undecided",
  schemaDomain |-> "Undecided",
  contentDomain |-> "Undecided",
  graphStrengthBefore |-> 0,
  graphStrengthAfter |-> 0,
  patchBaseMatches |-> FALSE,
  patchCommitted |-> FALSE,
  precision |-> "Undecided",
  completion |-> "Undecided",
  locksMatch |-> FALSE,
  cached |-> FALSE,
  runtimeReleaseEligible |-> FALSE,
  outputBytes |-> 0,
  spillLocation |-> "Undecided",
  checkpointMatches |-> FALSE,
  resumed |-> FALSE,
  tombstoned |-> FALSE,
  requirementActive |-> FALSE,
  unclassifiedRetained |-> FALSE,
  authority |-> "Undecided",
  obligation |-> "Undecided",
  evidenceFresh |-> FALSE,
  negativeControl |-> FALSE,
  attestationMatches |-> FALSE,
  assuranceVerified |-> FALSE,
  manifestMatches |-> FALSE,
  lintMode |-> "Undecided",
  documentationMutated |-> FALSE,
  lintPassed |-> FALSE,
  nativeFrames |-> 1,
  outcome |-> "Undecided"
]

TypeOK ==
  /\ phase \in Phases
  /\ state.scenario \in Scenarios \cup {"Undecided"}
  /\ state.profile \in {"Undecided", "vinary.canonical-json/v1"}
  /\ state.canonicalOutcome \in CanonicalOutcomes
  /\ state.schemaDomain \in {"Undecided", "WireSchema"}
  /\ state.contentDomain \in {"Undecided", "CanonicalContent"}
  /\ state.graphStrengthBefore \in 0..3
  /\ state.graphStrengthAfter \in 0..3
  /\ state.patchBaseMatches \in BOOLEAN
  /\ state.patchCommitted \in BOOLEAN
  /\ state.precision \in Precisions
  /\ state.completion \in Completions
  /\ state.locksMatch \in BOOLEAN
  /\ state.cached \in BOOLEAN
  /\ state.runtimeReleaseEligible \in BOOLEAN
  /\ state.outputBytes \in {0, 2}
  /\ state.spillLocation \in SpillLocations
  /\ state.checkpointMatches \in BOOLEAN
  /\ state.resumed \in BOOLEAN
  /\ state.tombstoned \in BOOLEAN
  /\ state.requirementActive \in BOOLEAN
  /\ state.unclassifiedRetained \in BOOLEAN
  /\ state.authority \in Authorities
  /\ state.obligation \in Obligations
  /\ state.evidenceFresh \in BOOLEAN
  /\ state.negativeControl \in BOOLEAN
  /\ state.attestationMatches \in BOOLEAN
  /\ state.assuranceVerified \in BOOLEAN
  /\ state.manifestMatches \in BOOLEAN
  /\ state.lintMode \in {"Undecided", "CheckOnly"}
  /\ state.documentationMutated \in BOOLEAN
  /\ state.lintPassed \in BOOLEAN
  /\ state.nativeFrames = 1
  /\ state.outcome \in TerminalOutcomes

Init ==
  /\ phase = "Initial"
  /\ \E selected \in Scenarios:
      state = [InitialState EXCEPT !.scenario = selected]

Canonicalize ==
  /\ phase = "Initial"
  /\ phase' = "Canonical"
  /\ state' = [state EXCEPT
      !.profile = "vinary.canonical-json/v1",
      !.canonicalOutcome = CanonicalOutcomeFor(state.scenario),
      !.schemaDomain = "WireSchema",
      !.contentDomain = "CanonicalContent"]

BuildGraph ==
  /\ phase = "Canonical"
  /\ phase' = "Graph"
  /\ state' = [state EXCEPT
      !.graphStrengthBefore = StrengthBeforeFor(state.scenario),
      !.graphStrengthAfter = StrengthBeforeFor(state.scenario),
      !.patchBaseMatches = PatchBaseMatchesFor(state.scenario),
      !.patchCommitted = PatchBaseMatchesFor(state.scenario)]

RunRuntime ==
  /\ phase = "Graph"
  /\ phase' = "Runtime"
  /\ state' = [state EXCEPT
      !.precision = PrecisionFor(state.scenario),
      !.completion = CompletionFor(state.scenario),
      !.locksMatch = LocksMatchFor(state.scenario),
      !.cached = (CompletionFor(state.scenario) = "Complete"),
      !.runtimeReleaseEligible =
          (PrecisionFor(state.scenario) = "Exact" /\
           CompletionFor(state.scenario) = "Complete" /\
           LocksMatchFor(state.scenario)),
      !.outputBytes = OutputBytesFor(state.scenario),
      !.spillLocation =
          IF OutputBytesFor(state.scenario) > OutputCap
          THEN "RepositoryBacked" ELSE "None",
      !.checkpointMatches = CheckpointMatchesFor(state.scenario),
      !.resumed = CheckpointMatchesFor(state.scenario)]

RecordRequirements ==
  /\ phase = "Runtime"
  /\ phase' = "Requirements"
  /\ state' = [state EXCEPT
      !.tombstoned = TombstonedFor(state.scenario),
      !.requirementActive = ~TombstonedFor(state.scenario),
      !.unclassifiedRetained = TRUE]

EvaluateAssurance ==
  /\ phase = "Requirements"
  /\ phase' = "Assurance"
  /\ state' = [state EXCEPT
      !.authority = AuthorityFor(state.scenario),
      !.obligation = ObligationFor(state.scenario),
      !.evidenceFresh = EvidenceFreshFor(state.scenario),
      !.negativeControl = NegativeControlFor(state.scenario),
      !.attestationMatches = AttestationMatchesFor(state.scenario),
      !.assuranceVerified =
          (Discharges(AuthorityFor(state.scenario), ObligationFor(state.scenario)) /\
           EvidenceFreshFor(state.scenario) /\
           NegativeControlFor(state.scenario) /\
           AttestationMatchesFor(state.scenario))]

LintDocumentation ==
  /\ phase = "Assurance"
  /\ phase' = "Documentation"
  /\ state' = [state EXCEPT
      !.manifestMatches = ManifestMatchesFor(state.scenario),
      !.lintMode = "CheckOnly",
      !.documentationMutated = FALSE,
      !.lintPassed = ManifestMatchesFor(state.scenario)]

Finalize ==
  /\ phase = "Documentation"
  /\ LET releasable ==
        state.canonicalOutcome = "Success" /\
        state.patchCommitted /\
        state.runtimeReleaseEligible /\
        state.unclassifiedRetained /\
        state.lintPassed /\
        (state.obligation # "Theorem" \/ state.assuranceVerified)
     IN
      /\ phase' = "Terminal"
      /\ state' = [state EXCEPT
          !.outcome = IF releasable THEN "Released" ELSE "Rejected"]

Stop ==
  /\ phase = "Terminal"
  /\ UNCHANGED vars

Next ==
  \/ Canonicalize
  \/ BuildGraph
  \/ RunRuntime
  \/ RecordRequirements
  \/ EvaluateAssurance
  \/ LintDocumentation
  \/ Finalize
  \/ Stop

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

NamedProfileIsNotRfc8785 ==
  phase = "Initial" \/
  (state.profile = "vinary.canonical-json/v1" /\ state.profile # "RFC8785")

WireAndContentIdentityDomainsAreSeparate ==
  phase = "Initial" \/ state.schemaDomain # state.contentDomain

ProjectionNeverStrengthens ==
  phase \in {"Initial", "Canonical"} \/
  state.graphStrengthAfter <= state.graphStrengthBefore

PatchCommitRequiresMatchingBase ==
  ~state.patchCommitted \/ state.patchBaseMatches

IncompleteNeverEntersCache ==
  state.completion # "Incomplete" \/ ~state.cached

RuntimeReleaseRequiresExactCompleteLockedInputs ==
  ~state.runtimeReleaseEligible \/
  (state.precision = "Exact" /\ state.completion = "Complete" /\ state.locksMatch)

OverflowSpillsOnlyToRepositoryStorage ==
  state.outputBytes <= OutputCap \/
  state.spillLocation \in {"Undecided", "RepositoryBacked"}

ResumeRequiresCompatibleCheckpoint ==
  ~state.resumed \/ state.checkpointMatches

TombstonesAreNotActive ==
  ~state.tombstoned \/ ~state.requirementActive

SourceAccountingNeverDropsUnclassifiedText ==
  phase \in {"Initial", "Canonical", "Graph", "Runtime"} \/
  state.unclassifiedRetained

StatisticsNeverDischargeTheoremObligations ==
  ~(state.authority = "Statistics" /\
    state.obligation = "Theorem" /\ state.assuranceVerified)

StaleEvidenceCannotVerify ==
  ~state.assuranceVerified \/ state.evidenceFresh

VerifiedAssuranceRequiresNegativeControl ==
  ~state.assuranceVerified \/ state.negativeControl

VerifiedAssuranceRequiresRevisionAttestation ==
  ~state.assuranceVerified \/ state.attestationMatches

CheckOnlyLintNeverMutatesDocumentation ==
  state.lintMode # "CheckOnly" \/ ~state.documentationMutated

StaleManifestCannotPassLint ==
  ~state.lintPassed \/ state.manifestMatches

ReleaseRequiresEveryNeutralFoundationGate ==
  state.outcome # "Released" \/
  (state.canonicalOutcome = "Success" /\
   state.patchCommitted /\
   state.runtimeReleaseEligible /\
   state.unclassifiedRetained /\
   state.lintPassed /\
   (state.obligation # "Theorem" \/ state.assuranceVerified))

NativeStackBoundIsInputIndependent == state.nativeFrames = 1

EventuallyTerminal == <> (phase = "Terminal")

=============================================================================
