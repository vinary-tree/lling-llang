---------------------- MODULE JuliaPathSearchLifecycle ----------------------
(***************************************************************************)
(* Finite safety abstraction of the Julia path-search cursor contract.      *)
(* Three complete accepting paths have nondecreasing native costs; the      *)
(* cursor retains a captured graph after both source and graph close.       *)
(* One emitted path takes two bounded work decisions, exposing Pending.     *)
(***************************************************************************)
EXTENDS Naturals, Sequences

CONSTANTS Mode, Cost1, Cost2, Cost3, Beam, Seed,
          MaxWork, WorkPerCall, MaxResults, BadCost

ASSUME /\ Mode \in {"Ranked", "Pruned", "Sample"}
       /\ Cost1 \in 1..3 /\ Cost2 \in 1..3 /\ Cost3 \in 1..3
       /\ Cost1 <= Cost2 /\ Cost2 <= Cost3
       /\ Beam \in 0..2 /\ Seed \in 0..2
       /\ MaxWork \in 1..6 /\ WorkPerCall \in 1..2
       /\ MaxResults \in 1..3 /\ BadCost \in BOOLEAN

VARIABLE st
vars == <<st>>
Statuses == {"None", "Exhausted", "Truncated", "WorkLimit",
            "Cancelled", "NumericError", "EarlyClose", "ReducerStop"}
CursorStates == {"None", "Open", "Closed"}
Cost(index) == IF index = 1 THEN Cost1 ELSE IF index = 2 THEN Cost2 ELSE Cost3
SampleIndex(count) == 1 + ((Seed + count) % 3)
NextIndex == IF Mode = "Sample" THEN SampleIndex(st.produced) ELSE st.nextIndex
PrunedOut == Mode = "Pruned" /\ st.nextIndex <= 3 /\
             Cost(st.nextIndex) > Cost1 + Beam
CanEmit == /\ st.cursor = "Open" /\ ~st.cancelled
           /\ st.produced < MaxResults
           /\ (Mode = "Sample" \/ st.nextIndex <= 3)
           /\ ~PrunedOut

Init == st = [
  sourceLive |-> FALSE, everAcquired |-> FALSE,
  captured |-> FALSE, graphLive |-> FALSE,
  cursor |-> "None", retained |-> FALSE,
  work |-> 0, slice |-> 0, stage |-> 0,
  produced |-> 0, nextIndex |-> 1,
  lastIndex |-> 0, lastCost |-> 0,
  trace |-> <<>>, cancelled |-> FALSE,
  cancelCount |-> 0, status |-> "None"
]

Acquire ==
  /\ ~st.everAcquired
  /\ st' = [st EXCEPT !.sourceLive = TRUE, !.everAcquired = TRUE]

Capture ==
  /\ st.sourceLive /\ ~st.captured
  /\ st' = [st EXCEPT !.captured = TRUE, !.graphLive = TRUE]

CloseSource ==
  /\ st.sourceLive
  /\ st' = [st EXCEPT !.sourceLive = FALSE]

Open ==
  /\ st.graphLive /\ st.cursor = "None" /\ st.status = "None"
  /\ ~BadCost
  /\ st' = [st EXCEPT !.cursor = "Open", !.retained = TRUE]

RejectNumeric ==
  /\ st.graphLive /\ st.cursor = "None" /\ st.status = "None"
  /\ BadCost
  /\ st' = [st EXCEPT !.status = "NumericError"]

CloseGraph ==
  /\ st.graphLive
  /\ st' = [st EXCEPT !.graphLive = FALSE]

WorkStep ==
  /\ CanEmit /\ st.stage < 2
  /\ st.work < MaxWork /\ st.slice < WorkPerCall
  /\ st' = [st EXCEPT !.work = st.work + 1,
       !.slice = st.slice + 1, !.stage = st.stage + 1]

Pending ==
  /\ CanEmit /\ st.stage < 2 /\ st.slice = WorkPerCall
  /\ st' = [st EXCEPT !.slice = 0]

Emit ==
  /\ CanEmit /\ st.stage = 2
  /\ st' = [st EXCEPT !.produced = st.produced + 1,
       !.nextIndex = IF Mode = "Sample" THEN st.nextIndex ELSE st.nextIndex + 1,
       !.lastIndex = NextIndex, !.lastCost = Cost(NextIndex),
       !.trace = Append(st.trace, NextIndex),
       !.stage = 0, !.slice = 0]

Exhaust ==
  /\ st.cursor = "Open" /\ ~st.cancelled
  /\ Mode # "Sample"
  /\ (st.nextIndex > 3 \/ PrunedOut)
  /\ st' = [st EXCEPT !.cursor = "Closed", !.retained = FALSE,
       !.status = "Exhausted"]

Truncate ==
  /\ st.cursor = "Open" /\ ~st.cancelled
  /\ st.produced = MaxResults
  /\ (Mode = "Sample" \/ (st.nextIndex <= 3 /\ ~PrunedOut))
  /\ st' = [st EXCEPT !.cursor = "Closed", !.retained = FALSE,
       !.status = "Truncated"]

FailWork ==
  /\ CanEmit /\ st.stage < 2 /\ st.work = MaxWork
  /\ st' = [st EXCEPT !.cursor = "Closed", !.retained = FALSE,
       !.status = "WorkLimit"]

Cancel ==
  /\ st.cursor = "Open" /\ ~st.cancelled
  /\ st' = [st EXCEPT !.cancelled = TRUE,
       !.cancelCount = st.produced]

FinishCancel ==
  /\ st.cursor = "Open" /\ st.cancelled
  /\ st' = [st EXCEPT !.cursor = "Closed", !.retained = FALSE,
       !.status = "Cancelled"]

CloseCursor ==
  /\ st.cursor = "Open" /\ ~st.cancelled
  /\ st' = [st EXCEPT !.cursor = "Closed", !.retained = FALSE,
       !.status = "EarlyClose"]

ReducerExit ==
  /\ st.cursor = "Open" /\ ~st.cancelled
  /\ st' = [st EXCEPT !.cursor = "Closed", !.retained = FALSE,
       !.status = "ReducerStop"]

Next == Acquire \/ Capture \/ CloseSource \/ Open \/ RejectNumeric \/
        CloseGraph \/ WorkStep \/ Pending \/ Emit \/ Exhaust \/
        Truncate \/ FailWork \/ Cancel \/ FinishCancel \/
        CloseCursor \/ ReducerExit
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ st.sourceLive \in BOOLEAN /\ st.everAcquired \in BOOLEAN
  /\ st.captured \in BOOLEAN /\ st.graphLive \in BOOLEAN
  /\ st.cursor \in CursorStates /\ st.retained \in BOOLEAN
  /\ st.work \in 0..MaxWork /\ st.slice \in 0..WorkPerCall
  /\ st.stage \in 0..2 /\ st.produced \in 0..MaxResults
  /\ st.nextIndex \in 1..4 /\ st.lastIndex \in 0..3
  /\ st.lastCost \in 0..3 /\ st.trace \in Seq(1..3)
  /\ st.cancelled \in BOOLEAN /\ st.cancelCount \in 0..MaxResults
  /\ st.status \in Statuses

GraphPinned ==
  /\ st.graphLive => st.captured
  /\ st.cursor = "Open" => st.captured /\ st.retained
  /\ st.retained => st.cursor = "Open"

WorkBounded == st.work <= MaxWork /\ st.slice <= WorkPerCall
OutputBounded == st.produced <= MaxResults /\ Len(st.trace) = st.produced

RankedOrder == Mode \in {"Ranked", "Pruned"} =>
  /\ st.nextIndex = st.produced + 1
  /\ \A index \in 1..Len(st.trace) : st.trace[index] = index
  /\ st.produced > 0 => st.lastCost = Cost(st.lastIndex)

PruneSound == Mode = "Pruned" =>
  \A index \in 1..Len(st.trace) : Cost(st.trace[index]) <= Cost1 + Beam

SeedStable == Mode = "Sample" =>
  \A index \in 1..Len(st.trace) : st.trace[index] = SampleIndex(index - 1)

TerminalDistinct ==
  /\ st.status = "Exhausted" =>
       Mode # "Sample" /\ (st.nextIndex > 3 \/ PrunedOut)
  /\ st.status = "Truncated" =>
       st.produced = MaxResults /\
       (Mode = "Sample" \/ (st.nextIndex <= 3 /\ ~PrunedOut))
  /\ st.status = "WorkLimit" => st.work = MaxWork
  /\ st.status # "None" => st.cursor # "Open"

CancelSticky == st.cancelled => st.produced = st.cancelCount
NumericFailureNoOutput ==
  st.status = "NumericError" => BadCost /\ st.produced = 0 /\ ~st.retained
ReducerSettles == st.status = "ReducerStop" =>
  st.cursor = "Closed" /\ ~st.retained

=============================================================================
