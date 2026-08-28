-------------------------- MODULE OptimizerLifecycle -------------------------
(***************************************************************************)
(* Bounded concurrent optimizer-plan lifecycle.  Dependencies are directed  *)
(* from prerequisite to dependent and are rank-ordered by the natural node  *)
(* identifiers in the model configuration.  Workers may finish ready nodes  *)
(* in different orders, but provenance is committed only in canonical node  *)
(* order.  Precision and completeness can degrade but cannot self-promote.  *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS Nodes, MaxBudget

(** The finite TLC instance uses the canonical three-node chain with its
    transitive edge.  The unbounded rank theorem is in PlanDag.v. *)
Dependencies == {<<1, 2>>, <<1, 3>>, <<2, 3>>}

VARIABLES phase, running, finished, completionOrder, provenance,
          nextSequence, precision, completeness, everApproximate,
          everIncomplete, everTerminal, witness, exactConfirmed, spent

vars == <<phase, running, finished, completionOrder, provenance,
          nextSequence, precision, completeness, everApproximate,
          everIncomplete, everTerminal, witness, exactConfirmed, spent>>

Phases == {"Validated", "Running", "Completed", "Cancelled",
           "BudgetExceeded", "Failed", "Published"}
PrecisionKinds == {"Exact", "Approximate"}
CompletenessKinds == {"Complete", "Incomplete"}
TerminalPhases == {"Cancelled", "BudgetExceeded", "Failed", "Published"}

DependenciesOf(node) ==
  {dependency \in Nodes : <<dependency, node>> \in Dependencies}

Ready(node) ==
  /\ node \in Nodes \ (finished \cup running)
  /\ DependenciesOf(node) \subseteq finished

Canonical == [index \in 1..Cardinality(Nodes) |-> index]

TypeOK ==
  /\ phase \in Phases
  /\ running \subseteq Nodes
  /\ finished \subseteq Nodes
  /\ completionOrder \in Seq(Nodes)
  /\ provenance \in Seq(Nodes)
  /\ nextSequence \in 1..(Cardinality(Nodes) + 1)
  /\ precision \in PrecisionKinds
  /\ completeness \in CompletenessKinds
  /\ everApproximate \in BOOLEAN
  /\ everIncomplete \in BOOLEAN
  /\ everTerminal \in BOOLEAN
  /\ witness \in BOOLEAN
  /\ exactConfirmed \in BOOLEAN
  /\ spent \in Nat

Init ==
  /\ phase = "Validated"
  /\ running = {}
  /\ finished = {}
  /\ completionOrder = <<>>
  /\ provenance = <<>>
  /\ nextSequence = 1
  /\ precision = "Exact"
  /\ completeness = "Complete"
  /\ everApproximate = FALSE
  /\ everIncomplete = FALSE
  /\ everTerminal = FALSE
  /\ witness = FALSE
  /\ exactConfirmed = FALSE
  /\ spent = 0

Start(node) ==
  /\ phase \in {"Validated", "Running"}
  /\ Ready(node)
  /\ spent < MaxBudget
  /\ phase' = "Running"
  /\ running' = running \cup {node}
  /\ UNCHANGED <<finished, completionOrder, provenance, nextSequence,
                  precision, completeness, everApproximate, everIncomplete,
                  everTerminal, witness, exactConfirmed, spent>>

Finish(node) ==
  /\ phase = "Running"
  /\ node \in running
  /\ spent < MaxBudget
  /\ LET nextPrecision ==
       IF precision = "Exact"
       THEN {"Exact", "Approximate"}
       ELSE {"Approximate"}
     IN
     LET nextCompleteness ==
       IF completeness = "Complete"
       THEN {"Complete", "Incomplete"}
       ELSE {"Incomplete"}
     IN
       /\ precision' \in nextPrecision
       /\ completeness' \in nextCompleteness
  /\ everApproximate' =
       (everApproximate \/ (precision' = "Approximate"))
  /\ everIncomplete' =
       (everIncomplete \/ (completeness' = "Incomplete"))
  /\ running' = running \ {node}
  /\ finished' = finished \cup {node}
  /\ completionOrder' = Append(completionOrder, node)
  /\ spent' = spent + 1
  /\ UNCHANGED <<phase, provenance, nextSequence, everTerminal, witness,
                  exactConfirmed>>

(** Work can consume a bounded unit without completing a node.  This exposes
    the BudgetExceeded outcome in the same finite model as successful plans. *)
ConsumeBudget ==
  /\ phase = "Running"
  /\ spent < MaxBudget
  /\ spent' = spent + 1
  /\ UNCHANGED <<phase, running, finished, completionOrder, provenance,
                  nextSequence, precision, completeness, everApproximate,
                  everIncomplete, everTerminal, witness, exactConfirmed>>

(** Completion order is nondeterministic; commit order is deterministic. *)
CommitProvenance ==
  /\ phase = "Running"
  /\ nextSequence \in finished
  /\ provenance' = Append(provenance, nextSequence)
  /\ nextSequence' = nextSequence + 1
  /\ UNCHANGED <<phase, running, finished, completionOrder, precision,
                  completeness, everApproximate, everIncomplete, everTerminal,
                  witness, exactConfirmed, spent>>

CompletePlan ==
  /\ phase = "Running"
  /\ finished = Nodes
  /\ running = {}
  /\ Len(provenance) = Cardinality(Nodes)
  /\ phase' = "Completed"
  /\ witness' = TRUE
  /\ UNCHANGED <<running, finished, completionOrder, provenance, nextSequence,
                  precision, completeness, everApproximate, everIncomplete,
                  everTerminal, exactConfirmed, spent>>

ConfirmExact ==
  /\ phase = "Completed"
  /\ precision = "Exact"
  /\ exactConfirmed' = TRUE
  /\ UNCHANGED <<phase, running, finished, completionOrder, provenance,
                  nextSequence, precision, completeness, everApproximate,
                  everIncomplete, everTerminal, witness, spent>>

Publish ==
  /\ phase = "Completed"
  /\ witness
  /\ (precision # "Exact" \/ exactConfirmed)
  /\ phase' = "Published"
  /\ UNCHANGED <<running, finished, completionOrder, provenance, nextSequence,
                  precision, completeness, everApproximate, everIncomplete,
                  everTerminal, witness, exactConfirmed, spent>>

Cancel ==
  /\ phase \in {"Validated", "Running", "Completed"}
  /\ phase' = "Cancelled"
  /\ everTerminal' = TRUE
  /\ UNCHANGED <<running, finished, completionOrder, provenance, nextSequence,
                  precision, completeness, everApproximate, everIncomplete,
                  witness, exactConfirmed, spent>>

ExhaustBudget ==
  /\ phase = "Running"
  /\ spent = MaxBudget
  /\ finished # Nodes
  /\ phase' = "BudgetExceeded"
  /\ everTerminal' = TRUE
  /\ UNCHANGED <<running, finished, completionOrder, provenance, nextSequence,
                  precision, completeness, everApproximate, everIncomplete,
                  witness, exactConfirmed, spent>>

Fail ==
  /\ phase \in {"Validated", "Running", "Completed"}
  /\ phase' = "Failed"
  /\ everTerminal' = TRUE
  /\ UNCHANGED <<running, finished, completionOrder, provenance, nextSequence,
                  precision, completeness, everApproximate, everIncomplete,
                  witness, exactConfirmed, spent>>

Next ==
  \/ \E node \in Nodes : Start(node)
  \/ \E node \in Nodes : Finish(node)
  \/ ConsumeBudget
  \/ CommitProvenance
  \/ CompletePlan
  \/ ConfirmExact
  \/ Publish
  \/ Cancel
  \/ ExhaustBudget
  \/ Fail

Spec == Init /\ [][Next]_vars

(***************************************************************************)
(* Safety invariants                                                        *)
(***************************************************************************)

PlanIsRankedDag ==
  \A edge \in Dependencies :
    /\ edge \in Nodes \X Nodes
    /\ edge[1] < edge[2]

RunningDependenciesFinished ==
  \A node \in running : DependenciesOf(node) \subseteq finished

FinishedDependenciesFinished ==
  \A node \in finished : DependenciesOf(node) \subseteq finished

NoNodeRunningAndFinished == running \cap finished = {}

PrecisionNeverPromotes == ~(everApproximate /\ precision = "Exact")
CompletenessNeverPromotes == ~(everIncomplete /\ completeness = "Complete")

ProvenanceIsCanonicalPrefix ==
  provenance = SubSeq(Canonical, 1, Len(provenance))

NextSequenceMatchesProvenance == nextSequence = Len(provenance) + 1

TerminalHistoryIsExact ==
  everTerminal = (phase \in {"Cancelled", "BudgetExceeded", "Failed"})

TerminalOutcomeNeverPublishes == everTerminal => phase # "Published"

PublishedHasWitness == phase = "Published" => witness

ExactPublicationWasConfirmed ==
  phase = "Published" /\ precision = "Exact" => exactConfirmed

PublishedPlanIsComplete == phase = "Published" => finished = Nodes

=============================================================================
