#!/usr/bin/env python3
"""Build one causally targeted mutant of the E9 neutral lifecycle model."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Mutation:
    """A single literal model defect and the obligation that must detect it."""

    target: str
    kind: str
    needle: str
    replacement: str


MUTATIONS = {
    "type-ok": Mutation(
        "TypeOK", "INVARIANT", "nativeFrames |-> 1,", "nativeFrames |-> 2,"
    ),
    "named-profile": Mutation(
        "NamedProfileIsNotRfc8785",
        "INVARIANT",
        '!.profile = "vinary.canonical-json/v1",',
        '!.profile = "RFC8785",',
    ),
    "identity-domains": Mutation(
        "WireAndContentIdentityDomainsAreSeparate",
        "INVARIANT",
        '!.contentDomain = "CanonicalContent"]',
        '!.contentDomain = "WireSchema"]',
    ),
    "projection-strength": Mutation(
        "ProjectionNeverStrengthens",
        "INVARIANT",
        "!.graphStrengthAfter = StrengthBeforeFor(state.scenario),",
        "!.graphStrengthAfter = 3,",
    ),
    "patch-base": Mutation(
        "PatchCommitRequiresMatchingBase",
        "INVARIANT",
        "!.patchCommitted = PatchBaseMatchesFor(state.scenario)]",
        "!.patchCommitted = TRUE]",
    ),
    "incomplete-cache": Mutation(
        "IncompleteNeverEntersCache",
        "INVARIANT",
        '!.cached = (CompletionFor(state.scenario) = "Complete"),',
        "!.cached = TRUE,",
    ),
    "release-locks": Mutation(
        "RuntimeReleaseRequiresExactCompleteLockedInputs",
        "INVARIANT",
        '''!.runtimeReleaseEligible =
          (PrecisionFor(state.scenario) = "Exact" /\\
           CompletionFor(state.scenario) = "Complete" /\\
           LocksMatchFor(state.scenario)),''',
        '''!.runtimeReleaseEligible =
          (PrecisionFor(state.scenario) = "Exact" /\\
           CompletionFor(state.scenario) = "Complete"),''',
    ),
    "repository-spill": Mutation(
        "OverflowSpillsOnlyToRepositoryStorage",
        "INVARIANT",
        'THEN "RepositoryBacked" ELSE "None",',
        'THEN "TemporaryMemoryFilesystem" ELSE "None",',
    ),
    "checkpoint-resume": Mutation(
        "ResumeRequiresCompatibleCheckpoint",
        "INVARIANT",
        "!.resumed = CheckpointMatchesFor(state.scenario)]",
        "!.resumed = TRUE]",
    ),
    "tombstone-active": Mutation(
        "TombstonesAreNotActive",
        "INVARIANT",
        "!.requirementActive = ~TombstonedFor(state.scenario),",
        "!.requirementActive = TRUE,",
    ),
    "source-accounting": Mutation(
        "SourceAccountingNeverDropsUnclassifiedText",
        "INVARIANT",
        "!.unclassifiedRetained = TRUE]",
        "!.unclassifiedRetained = FALSE]",
    ),
    "statistics-theorem": Mutation(
        "StatisticsNeverDischargeTheoremObligations",
        "INVARIANT",
        '''!.assuranceVerified =
          (Discharges(AuthorityFor(state.scenario), ObligationFor(state.scenario)) /\\
           EvidenceFreshFor(state.scenario) /\\
           NegativeControlFor(state.scenario) /\\
           AttestationMatchesFor(state.scenario))]''',
        '''!.assuranceVerified =
          (state.scenario = "StatisticsTheorem")]''',
    ),
    "stale-evidence": Mutation(
        "StaleEvidenceCannotVerify",
        "INVARIANT",
        '''!.assuranceVerified =
          (Discharges(AuthorityFor(state.scenario), ObligationFor(state.scenario)) /\\
           EvidenceFreshFor(state.scenario) /\\
           NegativeControlFor(state.scenario) /\\
           AttestationMatchesFor(state.scenario))]''',
        '''!.assuranceVerified =
          (state.scenario = "StaleEvidence")]''',
    ),
    "negative-control": Mutation(
        "VerifiedAssuranceRequiresNegativeControl",
        "INVARIANT",
        '''!.assuranceVerified =
          (Discharges(AuthorityFor(state.scenario), ObligationFor(state.scenario)) /\\
           EvidenceFreshFor(state.scenario) /\\
           NegativeControlFor(state.scenario) /\\
           AttestationMatchesFor(state.scenario))]''',
        '''!.assuranceVerified =
          (state.scenario = "MissingNegative")]''',
    ),
    "revision-attestation": Mutation(
        "VerifiedAssuranceRequiresRevisionAttestation",
        "INVARIANT",
        '''!.assuranceVerified =
          (Discharges(AuthorityFor(state.scenario), ObligationFor(state.scenario)) /\\
           EvidenceFreshFor(state.scenario) /\\
           NegativeControlFor(state.scenario) /\\
           AttestationMatchesFor(state.scenario))]''',
        '''!.assuranceVerified =
          (state.scenario = "BadAttestation")]''',
    ),
    "check-only-mutation": Mutation(
        "CheckOnlyLintNeverMutatesDocumentation",
        "INVARIANT",
        "!.documentationMutated = FALSE,",
        "!.documentationMutated = TRUE,",
    ),
    "stale-manifest": Mutation(
        "StaleManifestCannotPassLint",
        "INVARIANT",
        "!.lintPassed = ManifestMatchesFor(state.scenario)]",
        "!.lintPassed = TRUE]",
    ),
    "release-gates": Mutation(
        "ReleaseRequiresEveryNeutralFoundationGate",
        "INVARIANT",
        '''LET releasable ==
        state.canonicalOutcome = "Success" /\\
        state.patchCommitted /\\
        state.runtimeReleaseEligible /\\
        state.unclassifiedRetained /\\
        state.lintPassed /\\
        (state.obligation # "Theorem" \\/ state.assuranceVerified)
     IN''',
        "LET releasable == TRUE\n     IN",
    ),
    "native-stack": Mutation(
        "NativeStackBoundIsInputIndependent",
        "INVARIANT",
        "nativeFrames |-> 1,",
        "nativeFrames |-> 2,",
    ),
    "eventually-terminal": Mutation(
        "EventuallyTerminal",
        "PROPERTY",
        '''/\\ phase' = "Terminal"''',
        '''/\\ phase' = "Documentation"''',
    ),
}


def build(name: str, source_path: Path, config_path: Path, output: Path) -> None:
    """Materialize exactly one mutant and a config containing only its oracle."""
    mutation = MUTATIONS[name]
    source = source_path.read_text(encoding="utf-8")
    count = source.count(mutation.needle)
    if count != 1:
        raise SystemExit(
            f"ERROR: mutant {name} expected one source site, observed {count}"
        )
    mutated = source.replace(mutation.needle, mutation.replacement, 1)
    config = config_path.read_text(encoding="utf-8").splitlines()
    config = [
        line
        for line in config
        if not line.startswith("INVARIANT ") and not line.startswith("PROPERTY ")
    ]
    config.extend(["", f"{mutation.kind} {mutation.target}", ""])
    output.mkdir(parents=True, exist_ok=True)
    (output / source_path.name).write_text(mutated, encoding="utf-8")
    (output / config_path.name).write_text("\n".join(config), encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("name", choices=sorted(MUTATIONS))
    parser.add_argument("source", type=Path)
    parser.add_argument("config", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    build(args.name, args.source, args.config, args.output)


if __name__ == "__main__":
    main()
