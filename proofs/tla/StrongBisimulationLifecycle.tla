-------------------- MODULE StrongBisimulationLifecycle --------------------
EXTENDS Integers, FiniteSets, Sequences, TLC

(*
Finite executable specification for a validated labelled transition system
(LTS), descending strong-bisimulation refinement, replayable certificates, and
separation reasons.  The unbounded semantic and resource proofs live in
proofs/coq/algorithms/StrongBisimulation.v; TLC independently exhausts all
small edge sets and initial colorings selected by the model configuration.
*)

CONSTANTS StateCount, ActionCount, EndpointMode

ASSUME StateCount > 0
ASSUME ActionCount > 0
ASSUME EndpointMode \in {"Valid", "InvalidSource", "InvalidTarget",
                          "InvalidLabel"}

States == 0..(StateCount - 1)
Actions == 0..(ActionCount - 1)
Colors == 0..1
Pairs == States \X States

CandidateEdges ==
  {[source |-> source, label |-> label, target |-> target] :
    source \in States, label \in Actions, target \in States}

MalformedEdge ==
  CASE EndpointMode = "InvalidSource" ->
         [source |-> StateCount, label |-> 0, target |-> 0]
    [] EndpointMode = "InvalidTarget" ->
         [source |-> 0, label |-> 0, target |-> StateCount]
    [] EndpointMode = "InvalidLabel" ->
         [source |-> 0, label |-> ActionCount, target |-> 0]
    [] OTHER ->
         [source |-> 0, label |-> 0, target |-> 0]

WithEndpointMode(base) ==
  IF EndpointMode = "Valid" THEN base ELSE base \cup {MalformedEdge}

EdgeValid(edge) ==
  /\ edge.source \in States
  /\ edge.label \in Actions
  /\ edge.target \in States

EdgesValid(es) == \A edge \in es : EdgeValid(edge)

VARIABLES rawEdges, colors, relation, history, phase, iterations

vars == <<rawEdges, colors, relation, history, phase, iterations>>

InitialRelation ==
  {pair \in Pairs : colors[pair[1]] = colors[pair[2]]}

Successors(state, action) ==
  {edge.target :
    edge \in {candidate \in rawEdges :
      /\ candidate.source = state
      /\ candidate.label = action}}

TransfersLeftToRight(candidate, left, right) ==
  \A action \in Actions :
    \A leftTarget \in Successors(left, action) :
      \E rightTarget \in Successors(right, action) :
        <<leftTarget, rightTarget>> \in candidate

Transfers(candidate, left, right) ==
  /\ TransfersLeftToRight(candidate, left, right)
  /\ TransfersLeftToRight(candidate, right, left)

RefineRelation(candidate) ==
  {pair \in candidate : Transfers(candidate, pair[1], pair[2])}

Stable(candidate) == RefineRelation(candidate) = candidate

StrongBisimulation(candidate) ==
  /\ candidate \subseteq InitialRelation
  /\ Stable(candidate)

OracleRelation ==
  UNION {candidate \in SUBSET InitialRelation :
    StrongBisimulation(candidate)}

CanonicalMatrix(candidate) ==
  [leftIndex \in 1..StateCount |->
    [rightIndex \in 1..StateCount |->
      <<leftIndex - 1, rightIndex - 1>> \in candidate]]

Init ==
  /\ rawEdges \in
       {WithEndpointMode(base) : base \in SUBSET CandidateEdges}
  /\ colors \in [States -> Colors]
  /\ relation = {}
  /\ history = <<>>
  /\ phase = "Raw"
  /\ iterations = 0

Validate ==
  /\ phase = "Raw"
  /\ IF EdgesValid(rawEdges)
        THEN /\ relation' = InitialRelation
             /\ phase' = "Refining"
        ELSE /\ relation' = {}
             /\ phase' = "Rejected"
  /\ UNCHANGED <<rawEdges, colors, history, iterations>>

Refine ==
  LET next == RefineRelation(relation) IN
  /\ phase = "Refining"
  /\ next # relation
  /\ relation' = next
  /\ history' = Append(history,
       [before |-> relation,
        after |-> next,
        removed |-> relation \ next])
  /\ iterations' = iterations + 1
  /\ UNCHANGED <<rawEdges, colors, phase>>

Accept ==
  /\ phase = "Refining"
  /\ Stable(relation)
  /\ phase' = "Accepted"
  /\ UNCHANGED <<rawEdges, colors, relation, history, iterations>>

Terminal ==
  /\ phase \in {"Accepted", "Rejected"}
  /\ UNCHANGED vars

Next == Validate \/ Refine \/ Accept \/ Terminal

Spec ==
  /\ Init
  /\ [][Next]_vars
  /\ WF_vars(Validate)
  /\ WF_vars(Refine)
  /\ WF_vars(Accept)

TypeOK ==
  /\ rawEdges \subseteq CandidateEdges \cup {MalformedEdge}
  /\ colors \in [States -> Colors]
  /\ relation \subseteq Pairs
  /\ history \in Seq([
       before : SUBSET Pairs,
       after : SUBSET Pairs,
       removed : SUBSET Pairs])
  /\ phase \in {"Raw", "Refining", "Accepted", "Rejected"}
  /\ iterations \in Nat

InvalidInputRejected ==
  ~EdgesValid(rawEdges) =>
    phase \notin {"Refining", "Accepted"}

IndexedEndpointsValid ==
  phase \in {"Refining", "Accepted"} => EdgesValid(rawEdges)

RelationRefinesColors ==
  phase \in {"Refining", "Accepted"} =>
    relation \subseteq InitialRelation

RelationIsReflexive ==
  phase \in {"Refining", "Accepted"} =>
    \A state \in States : <<state, state>> \in relation

RelationIsSymmetric ==
  phase \in {"Refining", "Accepted"} =>
    \A pair \in relation : <<pair[2], pair[1]>> \in relation

HistorySound ==
  /\ Len(history) = iterations
  /\ \A index \in DOMAIN history :
       LET entry == history[index] IN
       /\ entry.after = RefineRelation(entry.before)
       /\ entry.after \subseteq entry.before
       /\ entry.after # entry.before
       /\ entry.removed = entry.before \ entry.after

HistoryChains ==
  /\ Len(history) = 0 \/ history[1].before = InitialRelation
  /\ \A index \in 2..Len(history) :
       history[index - 1].after = history[index].before
  /\ Len(history) = 0 \/ relation = history[Len(history)].after

RefinementTerminates ==
  iterations <= Cardinality(Pairs)

AcceptedStable ==
  phase = "Accepted" => Stable(relation)

AcceptedMatchesOracle ==
  phase = "Accepted" => relation = OracleRelation

CanonicalOutputExact ==
  phase = "Accepted" =>
    CanonicalMatrix(relation) = CanonicalMatrix(OracleRelation)

InitiallySeparated(pair) == pair \notin InitialRelation

RemovedByTrace(pair) ==
  \E index \in DOMAIN history : pair \in history[index].removed

WitnessComplete ==
  phase = "Accepted" =>
    \A pair \in Pairs \ relation :
      InitiallySeparated(pair) \/ RemovedByTrace(pair)

WitnessSound ==
  \A index \in DOMAIN history :
    \A pair \in history[index].removed :
      ~Transfers(history[index].before, pair[1], pair[2])

EventuallyTerminal == <> (phase \in {"Accepted", "Rejected"})

=============================================================================
