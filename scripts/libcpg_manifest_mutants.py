#!/usr/bin/env python3
"""Build one causal TLC mutant per libcpg manifest lifecycle property."""

from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass
from pathlib import Path

sys.dont_write_bytecode = True


@dataclass(frozen=True)
class Mutation:
    """One implementation-level defect and the property that must kill it."""

    target: str
    needle: str
    replacement: str
    temporal: bool = False


MUTATIONS = {
    "type-ok": Mutation(
        "TypeOK",
        '  /\\ nativeFrames = 1\n\nManifest ==',
        '  /\\ nativeFrames = 2\n\nManifest ==',
    ),
    "ownership-split": Mutation(
        "OwnershipIsSplit",
        'AdapterDimensions == {"fact-rule-lowering"}',
        'AdapterDimensions == {"fact-rule-lowering", "repository"}',
    ),
    "rename-identity": Mutation(
        "RenamePreservesDurableIdentity",
        "  /\\ manifestCompatible' = (scenario \\notin SemanticMismatchScenarios)",
        "  /\\ manifestCompatible' =\n       (scenario \\notin SemanticMismatchScenarios /\\ scenario # \"RepositoryRename\")",
    ),
    "semantic-invalidation": Mutation(
        "SemanticMismatchInvalidates",
        "  /\\ manifestCompatible' = (scenario \\notin SemanticMismatchScenarios)",
        "  /\\ manifestCompatible' =\n       (scenario \\notin SemanticMismatchScenarios \\/ scenario = \"ParserMismatch\")",
    ),
    "source-revision": Mutation(
        "SourceRevisionMismatchInvalidates",
        "  /\\ manifestCompatible' = (scenario \\notin SemanticMismatchScenarios)",
        "  /\\ manifestCompatible' =\n       (scenario \\notin SemanticMismatchScenarios \\/ scenario = \"SourceRevisionMismatch\")",
    ),
    "reactivation": Mutation(
        "TombstoneReactivationNeverAccepted",
        '  /\\ ~(oldFeatureState = "Tombstoned" /\\ newFeatureState = "Active")\n  /\\ featureSemanticSame',
        "  /\\ TRUE\n  /\\ TRUE",
    ),
    "tombstone-active": Mutation(
        "TombstonedFeaturesRemainInactive",
        'IF scenario \\in {"Tombstone", "Incomplete"} THEN {FactOne} ELSE Facts',
        'IF scenario = "Incomplete" THEN {FactOne} ELSE Facts',
    ),
    "historical-reuse": Mutation(
        "HistoricalFeatureIdNeverReused",
        "  /\\ featureSemanticSame' = (scenario # \"ReactivationAttempt\")",
        "  /\\ featureSemanticSame' = TRUE",
    ),
    "dense-roundtrip": Mutation(
        "DenseForwardReverseCorrespondence",
        "ELSE IF fact \\in nextActive THEN 1 ELSE NoDense",
        "ELSE IF fact \\in nextActive THEN 0 ELSE NoDense",
    ),
    "dense-orphan": Mutation(
        "DenseIndicesHaveNoOrphans",
        "ELSE IF FactTwo \\in nextActive THEN FactTwo ELSE NoFact",
        "ELSE NoFact",
    ),
    "inactive-dense": Mutation(
        "InactiveFactsHaveNoDenseIndex",
        "ELSE IF fact \\in nextActive THEN 1 ELSE NoDense",
        "ELSE 1",
    ),
    "cache-manifest": Mutation(
        "CacheReuseRequiresManifestCompatibility",
        'CanReuse ==\n  /\\ manifestCompatible\n  /\\ comparison = "Compatible"',
        "CanReuse ==\n  /\\ TRUE\n  /\\ TRUE",
    ),
    "cache-complete": Mutation(
        "CacheReuseRequiresCompleteExtraction",
        '  /\\ coverage = "Complete"\n  /\\ ~(oldFeatureState',
        "  /\\ TRUE\n  /\\ ~(oldFeatureState",
    ),
    "unknown-reuse": Mutation(
        'UnknownCompatibilityNeverReuses',
        'IF scenario = "UnknownCompatibility" THEN "Unknown"',
        'IF scenario = "UnknownCompatibility" THEN "Compatible"',
    ),
    "range-reuse": Mutation(
        "ExactSourceRangeRequiredForReuse",
        "  /\\ rangeValid\n  /\\ coverage =",
        "  /\\ TRUE\n  /\\ coverage =",
    ),
    "canonical-export": Mutation(
        "DeterministicExportIsCanonical",
        "THEN <<FactOne, FactTwo>> ELSE <<FactOne>>\n  /\\ phase' = \"Exported\"",
        "THEN <<FactTwo, FactOne>> ELSE <<FactOne>>\n  /\\ phase' = \"Exported\"",
    ),
    "insertion-order": Mutation(
        "InsertionPermutationDoesNotChangeExport",
        "THEN <<FactOne, FactTwo>> ELSE <<FactOne>>\n  /\\ phase' = \"Exported\"",
        "THEN <<FactTwo, FactOne>> ELSE <<FactOne>>\n  /\\ phase' = \"Exported\"",
    ),
    "incomplete-absence": Mutation(
        "IncompleteNeverEstablishesAbsence",
        '       (coverage = "Complete" /\\ FactTwo \\notin activeFacts)',
        "       (FactTwo \\notin activeFacts)",
    ),
    "incomplete-outcome": Mutation(
        "IncompleteNeverProducesAcceptedOutcome",
        'IF coverage = "Incomplete" THEN "Incomplete"',
        'IF coverage = "Incomplete" THEN "Accepted"',
    ),
    "lowering-orphan": Mutation(
        "EveryLoweredRuleHasSourceFact",
        'ELSE {<<FactOne, RuleOne>>}',
        'ELSE {<<FactOne, RuleOne>>, <<NoFact, RuleTwo>>}',
    ),
    "many-to-many": Mutation(
        "ManyToManyLoweringIsPreserved",
        "THEN {<<FactOne, RuleOne>>, <<FactOne, RuleTwo>>, <<FactTwo, RuleOne>>}",
        "THEN {<<FactOne, RuleOne>>, <<FactTwo, RuleOne>>}",
    ),
    "dependency-direction": Mutation(
        "CoreDependencyDirectionIsIndependent",
        '   <<"vinary-libcpg-adapter", "lling-llang">>}',
        '   <<"vinary-libcpg-adapter", "lling-llang">>,\n   <<"libcpg", "lling-llang">>}',
    ),
    "native-stack": Mutation(
        "NativeStackBoundIsInputIndependent",
        '  /\\ nativeFrames = 1\n\nManifest ==',
        '  /\\ nativeFrames = 2\n\nManifest ==',
    ),
    "linear-work": Mutation(
        "ExportWorkIsLinear",
        "  /\\ work' = work + 1",
        "  /\\ work' = work + 2",
    ),
    "terminal-outcome": Mutation(
        "TerminalOutcomeIsClassified",
        'ELSE IF CanReuse THEN "Accepted" ELSE "Rejected"',
        'ELSE IF CanReuse THEN "Accepted" ELSE "None"',
    ),
    "eventually-terminal": Mutation(
        "EventuallyTerminal",
        '  /\\ phase\' = "Manifested"',
        '  /\\ phase\' = "Declared"',
        temporal=True,
    ),
}


def config_for(source: str, mutation: Mutation) -> str:
    """Retain constants/specification and select exactly the mutant target."""

    prefix = source.split("INVARIANT TypeOK", maxsplit=1)[0]
    directive = "PROPERTY" if mutation.temporal else "INVARIANT"
    return f"{prefix}{directive} {mutation.target}\n"


def main() -> None:
    if sys.argv[1:] == ["--list"]:
        for name, mutation in MUTATIONS.items():
            kind = "property" if mutation.temporal else "invariant"
            print(f"{name}\t{mutation.target}\t{kind}")
        return

    parser = argparse.ArgumentParser()
    parser.add_argument("name", choices=sorted(MUTATIONS))
    parser.add_argument("spec", type=Path)
    parser.add_argument("config", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    mutation = MUTATIONS[args.name]
    spec = args.spec.read_text(encoding="utf-8")
    if spec.count(mutation.needle) != 1:
        raise SystemExit(
            f"mutation {args.name} expected one injection site; "
            f"found {spec.count(mutation.needle)}"
        )
    mutated = spec.replace(mutation.needle, mutation.replacement, 1)
    config = config_for(args.config.read_text(encoding="utf-8"), mutation)

    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "LibcpgManifestLifecycle.tla").write_text(mutated, encoding="utf-8")
    (args.output / "LibcpgManifestLifecycle.cfg").write_text(config, encoding="utf-8")


if __name__ == "__main__":
    main()
