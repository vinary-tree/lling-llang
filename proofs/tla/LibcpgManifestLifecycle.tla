---------------------- MODULE LibcpgManifestLifecycle ----------------------
(***************************************************************************)
(* Finite refinement model for libcpg extraction manifests, durable facts,  *)
(* dense local indices, canonical export, cache invalidation, source ranges, *)
(* historical feature tombstones, and adapter-owned fact/rule lowering.      *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS Facts, Rules, FactOne, FactTwo, RuleOne, RuleTwo, NoFact, NoDense

ASSUME /\ Facts = {FactOne, FactTwo}
       /\ Rules = {RuleOne, RuleTwo}
       /\ FactOne # FactTwo
       /\ RuleOne # RuleTwo
       /\ NoFact \notin Facts
       /\ NoDense \notin {0, 1}

ScenarioKinds ==
  {"Valid", "RepositoryRename", "ParserMismatch", "GrammarMismatch",
   "ExtractorMismatch", "QueryMismatch", "FeatureRevisionMismatch",
   "SchemaMismatch", "SourceIdentityMismatch", "SourceRevisionMismatch",
   "ConfigurationMismatch", "Tombstone", "ReactivationAttempt",
   "Incomplete", "UnknownCompatibility", "ManyToMany",
   "InsertionPermutation", "OutOfBoundsRange", "DenseBijection"}

SemanticMismatchScenarios ==
  {"ParserMismatch", "GrammarMismatch", "ExtractorMismatch", "QueryMismatch",
   "FeatureRevisionMismatch", "SchemaMismatch", "SourceIdentityMismatch",
   "SourceRevisionMismatch", "ConfigurationMismatch"}

Phases == {"Declared", "Manifested", "Indexed", "Exported", "Terminal"}
Comparisons == {"Undecided", "Compatible", "Incompatible", "Unknown"}
FeatureStates == {"Active", "Tombstoned"}
Coverages == {"Undecided", "Complete", "Incomplete"}
Outcomes == {"None", "Accepted", "Rejected", "Incomplete"}
DenseIds == {0, 1}

LibcpgDimensions ==
  {"repository", "parser", "grammar", "extractor", "query", "feature",
   "schema", "source", "source-revision", "semantic-configuration"}
RuntimeDimensions ==
  {"executable", "host", "environment", "invocation", "resource-envelope"}
AdapterDimensions == {"fact-rule-lowering"}

CoreDependencies ==
  {<<"vinary-libcpg-adapter", "libcpg">>,
   <<"vinary-libcpg-adapter", "lling-llang">>}

VARIABLES scenario, phase, manifestCompatible, comparison, displayRenamed,
          oldFeatureState, newFeatureState, featureSemanticSame, rangeValid,
          coverage, activeFacts, denseCount, forward, reverse,
          remaining, initialRemaining, work, exported, lowering,
          cacheReuse, absenceClaim, outcome, nativeFrames

vars ==
  <<scenario, phase, manifestCompatible, comparison, displayRenamed,
    oldFeatureState, newFeatureState, featureSemanticSame, rangeValid,
    coverage, activeFacts, denseCount, forward, reverse,
    remaining, initialRemaining, work, exported, lowering,
    cacheReuse, absenceClaim, outcome, nativeFrames>>

Init ==
  /\ scenario \in ScenarioKinds
  /\ phase = "Declared"
  /\ manifestCompatible = FALSE
  /\ comparison = "Undecided"
  /\ displayRenamed = FALSE
  /\ oldFeatureState = "Active"
  /\ newFeatureState = "Active"
  /\ featureSemanticSame = TRUE
  /\ rangeValid = TRUE
  /\ coverage = "Undecided"
  /\ activeFacts = {}
  /\ denseCount = 0
  /\ forward = [fact \in Facts |-> NoDense]
  /\ reverse = [dense \in DenseIds |-> NoFact]
  /\ remaining = 0
  /\ initialRemaining = 0
  /\ work = 0
  /\ exported = <<>>
  /\ lowering = {}
  /\ cacheReuse = FALSE
  /\ absenceClaim = FALSE
  /\ outcome = "None"
  /\ nativeFrames = 1

Manifest ==
  /\ phase = "Declared"
  /\ manifestCompatible' = (scenario \notin SemanticMismatchScenarios)
  /\ comparison' =
       IF scenario = "UnknownCompatibility" THEN "Unknown"
       ELSE IF scenario \in SemanticMismatchScenarios THEN "Incompatible"
       ELSE "Compatible"
  /\ displayRenamed' = (scenario = "RepositoryRename")
  /\ oldFeatureState' =
       IF scenario = "ReactivationAttempt" THEN "Tombstoned" ELSE "Active"
  /\ newFeatureState' =
       IF scenario = "Tombstone" THEN "Tombstoned" ELSE "Active"
  /\ featureSemanticSame' = (scenario # "ReactivationAttempt")
  /\ rangeValid' = (scenario # "OutOfBoundsRange")
  /\ coverage' = IF scenario = "Incomplete" THEN "Incomplete" ELSE "Complete"
  /\ phase' = "Manifested"
  /\ UNCHANGED <<scenario, activeFacts, denseCount, forward, reverse,
                  remaining, initialRemaining, work, exported, lowering,
                  cacheReuse, absenceClaim, outcome, nativeFrames>>

Index ==
  /\ phase = "Manifested"
  /\ LET nextActive ==
            IF scenario \in {"Tombstone", "Incomplete"} THEN {FactOne} ELSE Facts
         nextCount == Cardinality(nextActive)
     IN /\ activeFacts' = nextActive
        /\ denseCount' = nextCount
        /\ forward' =
             [fact \in Facts |->
                IF fact = FactOne THEN 0
                ELSE IF fact \in nextActive THEN 1 ELSE NoDense]
        /\ reverse' =
             [dense \in DenseIds |->
                IF dense = 0 THEN FactOne
                ELSE IF FactTwo \in nextActive THEN FactTwo ELSE NoFact]
        /\ remaining' = Cardinality(Facts)
        /\ initialRemaining' = Cardinality(Facts)
  /\ phase' = "Indexed"
  /\ UNCHANGED <<scenario, manifestCompatible, comparison, displayRenamed,
                  oldFeatureState, newFeatureState, featureSemanticSame,
                  rangeValid, coverage, work, exported, lowering,
                  cacheReuse, absenceClaim, outcome, nativeFrames>>

ExportStep ==
  /\ phase = "Indexed"
  /\ remaining > 0
  /\ remaining' = remaining - 1
  /\ work' = work + 1
  /\ UNCHANGED <<scenario, phase, manifestCompatible, comparison,
                  displayRenamed, oldFeatureState, newFeatureState,
                  featureSemanticSame, rangeValid, coverage, activeFacts,
                  denseCount, forward, reverse, initialRemaining, exported,
                  lowering, cacheReuse, absenceClaim, outcome, nativeFrames>>

FinishExport ==
  /\ phase = "Indexed"
  /\ remaining = 0
  /\ exported' =
       IF FactTwo \in activeFacts THEN <<FactOne, FactTwo>> ELSE <<FactOne>>
  /\ phase' = "Exported"
  /\ UNCHANGED <<scenario, manifestCompatible, comparison, displayRenamed,
                  oldFeatureState, newFeatureState, featureSemanticSame,
                  rangeValid, coverage, activeFacts, denseCount, forward,
                  reverse, remaining, initialRemaining, work, lowering,
                  cacheReuse, absenceClaim, outcome, nativeFrames>>

CanReuse ==
  /\ manifestCompatible
  /\ comparison = "Compatible"
  /\ rangeValid
  /\ coverage = "Complete"
  /\ ~(oldFeatureState = "Tombstoned" /\ newFeatureState = "Active")
  /\ featureSemanticSame

LowerAndClassify ==
  /\ phase = "Exported"
  /\ lowering' =
       IF scenario = "ManyToMany"
       THEN {<<FactOne, RuleOne>>, <<FactOne, RuleTwo>>, <<FactTwo, RuleOne>>}
       ELSE {<<FactOne, RuleOne>>}
  /\ cacheReuse' = CanReuse
  /\ absenceClaim' =
       (coverage = "Complete" /\ FactTwo \notin activeFacts)
  /\ outcome' =
       IF coverage = "Incomplete" THEN "Incomplete"
       ELSE IF CanReuse THEN "Accepted" ELSE "Rejected"
  /\ phase' = "Terminal"
  /\ UNCHANGED <<scenario, manifestCompatible, comparison, displayRenamed,
                  oldFeatureState, newFeatureState, featureSemanticSame,
                  rangeValid, coverage, activeFacts, denseCount, forward,
                  reverse, remaining, initialRemaining, work, exported,
                  nativeFrames>>

Done == phase = "Terminal" /\ UNCHANGED vars

Next == Manifest \/ Index \/ ExportStep \/ FinishExport \/ LowerAndClassify \/ Done

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
  /\ scenario \in ScenarioKinds
  /\ phase \in Phases
  /\ manifestCompatible \in BOOLEAN
  /\ comparison \in Comparisons
  /\ displayRenamed \in BOOLEAN
  /\ oldFeatureState \in FeatureStates
  /\ newFeatureState \in FeatureStates
  /\ featureSemanticSame \in BOOLEAN
  /\ rangeValid \in BOOLEAN
  /\ coverage \in Coverages
  /\ activeFacts \subseteq Facts
  /\ denseCount \in 0..Cardinality(Facts)
  /\ forward \in [Facts -> DenseIds \cup {NoDense}]
  /\ reverse \in [DenseIds -> Facts \cup {NoFact}]
  /\ remaining \in Nat
  /\ initialRemaining \in Nat
  /\ work \in Nat
  /\ exported \in Seq(Facts)
  /\ lowering \subseteq (Facts \X Rules)
  /\ cacheReuse \in BOOLEAN
  /\ absenceClaim \in BOOLEAN
  /\ outcome \in Outcomes
  /\ nativeFrames \in 0..1

OwnershipIsSplit ==
  /\ LibcpgDimensions \cap RuntimeDimensions = {}
  /\ LibcpgDimensions \cap AdapterDimensions = {}
  /\ RuntimeDimensions \cap AdapterDimensions = {}

RenamePreservesDurableIdentity ==
  (scenario = "RepositoryRename" /\ phase # "Declared") =>
    (manifestCompatible /\ comparison = "Compatible" /\ displayRenamed)

SemanticMismatchInvalidates ==
  (scenario \in SemanticMismatchScenarios /\ phase # "Declared") =>
    (~manifestCompatible /\ comparison = "Incompatible")

SourceRevisionMismatchInvalidates ==
  (scenario = "SourceRevisionMismatch" /\ phase # "Declared") =>
    ~manifestCompatible

TombstoneReactivationNeverAccepted ==
  (oldFeatureState = "Tombstoned" /\ newFeatureState = "Active" /\
   phase = "Terminal") =>
    (~cacheReuse /\ outcome = "Rejected")

TombstonedFeaturesRemainInactive ==
  (scenario = "Tombstone" /\ phase \in {"Indexed", "Exported", "Terminal"}) =>
    FactTwo \notin activeFacts

HistoricalFeatureIdNeverReused ==
  (scenario = "ReactivationAttempt" /\ phase # "Declared") =>
    ~featureSemanticSame

DenseForwardReverseCorrespondence ==
  (phase \in {"Indexed", "Exported", "Terminal"}) =>
    \A fact \in activeFacts :
      /\ forward[fact] \in DenseIds
      /\ reverse[forward[fact]] = fact

DenseIndicesHaveNoOrphans ==
  (phase \in {"Indexed", "Exported", "Terminal"}) =>
    \A dense \in 0..(denseCount - 1) :
      /\ reverse[dense] \in activeFacts
      /\ forward[reverse[dense]] = dense

InactiveFactsHaveNoDenseIndex ==
  (phase \in {"Indexed", "Exported", "Terminal"}) =>
    \A fact \in Facts \ activeFacts : forward[fact] = NoDense

CacheReuseRequiresManifestCompatibility == cacheReuse => manifestCompatible

CacheReuseRequiresCompleteExtraction == cacheReuse => coverage = "Complete"

UnknownCompatibilityNeverReuses ==
  scenario = "UnknownCompatibility" => ~cacheReuse

ExactSourceRangeRequiredForReuse == cacheReuse => rangeValid

CanonicalExport(active) ==
  IF FactTwo \in active THEN <<FactOne, FactTwo>> ELSE <<FactOne>>

DeterministicExportIsCanonical ==
  phase \in {"Exported", "Terminal"} => exported = CanonicalExport(activeFacts)

InsertionPermutationDoesNotChangeExport ==
  (scenario = "InsertionPermutation" /\ phase \in {"Exported", "Terminal"}) =>
    exported = <<FactOne, FactTwo>>

IncompleteNeverEstablishesAbsence ==
  coverage = "Incomplete" => ~absenceClaim

IncompleteNeverProducesAcceptedOutcome ==
  (coverage = "Incomplete" /\ phase = "Terminal") => outcome = "Incomplete"

EveryLoweredRuleHasSourceFact ==
  phase = "Terminal" =>
    \A rule \in Rules :
      (\E pair \in lowering : pair[2] = rule) =>
        (\E pair \in lowering : pair[1] \in activeFacts /\ pair[2] = rule)

ManyToManyLoweringIsPreserved ==
  (scenario = "ManyToMany" /\ phase = "Terminal") =>
    /\ <<FactOne, RuleOne>> \in lowering
    /\ <<FactOne, RuleTwo>> \in lowering
    /\ <<FactTwo, RuleOne>> \in lowering

CoreDependencyDirectionIsIndependent ==
  /\ <<"libcpg", "lling-llang">> \notin CoreDependencies
  /\ <<"lling-llang", "libcpg">> \notin CoreDependencies
  /\ <<"vinary-libcpg-adapter", "libcpg">> \in CoreDependencies
  /\ <<"vinary-libcpg-adapter", "lling-llang">> \in CoreDependencies

NativeStackBoundIsInputIndependent == nativeFrames = 1

ExportWorkIsLinear ==
  /\ work + remaining = initialRemaining
  /\ (phase \in {"Indexed", "Exported", "Terminal"} =>
        initialRemaining = Cardinality(Facts))

TerminalOutcomeIsClassified ==
  phase = "Terminal" => outcome \in {"Accepted", "Rejected", "Incomplete"}

EventuallyTerminal == <> (phase = "Terminal")

=============================================================================
