------------------------ MODULE JuliaPdaLifecycle -------------------------
(***************************************************************************)
(* Finite abstraction of the native PDA builder/session C ABI.            *)
(* Query publication is atomic; sessions retain the compiled machine;      *)
(* work and stack limits reject instead of reporting partial results.      *)
(***************************************************************************)
EXTENDS Naturals

CONSTANTS WorkCap, DepthCap, FrontierTotal, MaxEpoch
ASSUME /\ WorkCap \in 1..4 /\ DepthCap \in 1..3
       /\ FrontierTotal \in 1..3 /\ MaxEpoch \in 1..3

VARIABLE st
vars == <<st>>
BuilderStates == {"Open", "Built", "Closed"}
FrontierStates == {"None", "Ready", "Failed"}

Init == st = [builder |-> "Open", pdaLive |-> FALSE,
  everBuilt |-> FALSE, sessionLive |-> FALSE, retained |-> FALSE,
  depth |-> 1, work |-> 0, epoch |-> 0,
  frontier |-> "None", count |-> 0, page |-> 0]

Build ==
  /\ st.builder = "Open"
  /\ st' = [st EXCEPT !.builder = "Built", !.pdaLive = TRUE,
                  !.everBuilt = TRUE]

CloseBuilder ==
  /\ st.builder = "Open"
  /\ st' = [st EXCEPT !.builder = "Closed"]

OpenSession ==
  /\ st.pdaLive /\ ~st.sessionLive
  /\ st' = [st EXCEPT !.sessionLive = TRUE, !.retained = TRUE]

ClosePda ==
  /\ st.pdaLive
  /\ st' = [st EXCEPT !.pdaLive = FALSE]

CloseSession ==
  /\ st.sessionLive
  /\ st' = [st EXCEPT !.sessionLive = FALSE, !.retained = FALSE,
                  !.frontier = "None", !.count = 0, !.page = 0]

FrontierComplete ==
  /\ st.sessionLive /\ st.epoch < MaxEpoch
  /\ st.work + FrontierTotal <= WorkCap
  /\ st' = [st EXCEPT !.work = st.work + FrontierTotal,
                  !.epoch = st.epoch + 1, !.frontier = "Ready",
                  !.count = FrontierTotal, !.page = 0]

FrontierFailWork ==
  /\ st.sessionLive /\ st.epoch < MaxEpoch
  /\ st.work + FrontierTotal > WorkCap
  /\ st' = [st EXCEPT !.epoch = st.epoch + 1,
                  !.frontier = "Failed", !.count = 0, !.page = 0]

Page ==
  /\ st.sessionLive /\ st.frontier = "Ready"
  /\ st.page < st.count
  /\ st' = [st EXCEPT !.page = st.page + 1]

Advance ==
  /\ st.sessionLive /\ st.epoch < MaxEpoch
  /\ st.depth < DepthCap /\ st.work < WorkCap
  /\ st' = [st EXCEPT !.depth = st.depth + 1,
                  !.work = st.work + 1, !.epoch = st.epoch + 1,
                  !.frontier = "None", !.count = 0, !.page = 0]

AdvanceLimit ==
  /\ st.sessionLive /\ st.epoch < MaxEpoch
  /\ (st.depth = DepthCap \/ st.work = WorkCap)
  /\ st' = [st EXCEPT !.epoch = st.epoch + 1]

Idle == UNCHANGED st

Next == Build \/ CloseBuilder \/ OpenSession \/ ClosePda \/
        CloseSession \/ FrontierComplete \/ FrontierFailWork \/
        Page \/ Advance \/ AdvanceLimit \/ Idle
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ st.builder \in BuilderStates
  /\ st.pdaLive \in BOOLEAN /\ st.everBuilt \in BOOLEAN
  /\ st.sessionLive \in BOOLEAN /\ st.retained \in BOOLEAN
  /\ st.depth \in 1..DepthCap /\ st.work \in 0..WorkCap
  /\ st.epoch \in 0..MaxEpoch /\ st.frontier \in FrontierStates
  /\ st.count \in 0..FrontierTotal /\ st.page \in 0..FrontierTotal

SessionRetainsCompiledPda ==
  st.sessionLive => st.retained /\ st.everBuilt

NoPartialFrontier ==
  /\ st.frontier = "Ready" => st.count = FrontierTotal
  /\ st.frontier # "Ready" => st.count = 0 /\ st.page = 0

PagingBounded == st.page <= st.count
ResourcesBounded == st.work <= WorkCap /\ st.depth <= DepthCap
ClosedSessionHasNoBorrow == ~st.sessionLive => ~st.retained

=============================================================================
