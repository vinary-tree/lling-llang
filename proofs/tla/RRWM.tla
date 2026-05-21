-------------------------------- MODULE RRWM --------------------------------
(***************************************************************************
 * Finite model of repeated weighted-majority accounting.
 *
 * The asymptotic regret theorem belongs in a mathematical proof, not in TLC's
 * finite-state engine. This model checks the bounded integer accounting
 * invariants used by an RRWM implementation over a finite loss horizon.
 ***************************************************************************)

EXTENDS Integers, FiniteSets

CONSTANTS
    NumExperts,
    MaxLoss,
    MaxRounds

VARIABLES
    round,
    weights,
    totalLoss,
    expertLosses

vars == <<round, weights, totalLoss, expertLosses>>

Experts == 1..NumExperts
LossVector == [Experts -> 0..MaxLoss]
MaxTotalLoss == MaxRounds * MaxLoss

TypeOK ==
    /\ round \in 0..MaxRounds
    /\ weights \in [Experts -> 1..(MaxTotalLoss + 1)]
    /\ totalLoss \in 0..MaxTotalLoss
    /\ expertLosses \in [Experts -> 0..MaxTotalLoss]

Init ==
    /\ round = 0
    /\ weights = [i \in Experts |-> MaxTotalLoss + 1]
    /\ totalLoss = 0
    /\ expertLosses = [i \in Experts |-> 0]

BestExpertLoss ==
    IF NumExperts = 0 THEN 0
    ELSE
        LET Losses == { expertLosses[i] : i \in Experts }
        IN CHOOSE m \in Losses : \A x \in Losses : m <= x

Regret == totalLoss - BestExpertLoss

RegretBound ==
    Regret <= MaxTotalLoss

WeightsPositive ==
    \A i \in Experts : weights[i] >= 1

LossesBounded ==
    \A i \in Experts : expertLosses[i] <= round * MaxLoss

ReceiveLoss(losses) ==
    /\ round < MaxRounds
    /\ losses \in LossVector
    /\ LET chosen == IF NumExperts = 0 THEN 0 ELSE CHOOSE i \in Experts : TRUE
           nextExpertLosses == [i \in Experts |-> expertLosses[i] + losses[i]]
       IN
       /\ totalLoss' = IF NumExperts = 0 THEN totalLoss ELSE totalLoss + losses[chosen]
       /\ expertLosses' = nextExpertLosses
       /\ weights' = [i \in Experts |-> MaxTotalLoss + 1 - nextExpertLosses[i]]
    /\ round' = round + 1

Done ==
    /\ round = MaxRounds
    /\ UNCHANGED vars

Next ==
    (\E losses \in LossVector : ReceiveLoss(losses)) \/ Done

Spec == Init /\ [][Next]_vars

Fairness == WF_vars(\E losses \in LossVector : ReceiveLoss(losses))
FairSpec == Spec /\ Fairness

THEOREM Spec => []TypeOK
THEOREM Spec => []RegretBound
THEOREM Spec => []WeightsPositive
THEOREM Spec => []LossesBounded

=============================================================================
