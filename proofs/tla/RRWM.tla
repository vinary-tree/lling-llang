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
    expertLosses,
    lastChosen

vars == <<round, weights, totalLoss, expertLosses, lastChosen>>

Experts == 1..NumExperts
LossVector == [Experts -> 0..MaxLoss]
MaxTotalLoss == MaxRounds * MaxLoss

TypeOK ==
    /\ round \in 0..MaxRounds
    /\ weights \in [Experts -> 1..(MaxTotalLoss + 1)]
    /\ totalLoss \in 0..MaxTotalLoss
    /\ expertLosses \in [Experts -> 0..MaxTotalLoss]
    /\ lastChosen \in 0..NumExperts

Init ==
    /\ round = 0
    /\ weights = [i \in Experts |-> MaxTotalLoss + 1]
    /\ totalLoss = 0
    /\ expertLosses = [i \in Experts |-> 0]
    /\ lastChosen = 0

BestExpertLoss ==
    IF NumExperts = 0 THEN 0
    ELSE
        LET Losses == { expertLosses[i] : i \in Experts }
        IN CHOOSE m \in Losses : \A x \in Losses : m <= x

Regret == totalLoss - BestExpertLoss

RegretWithinAccountingHorizon ==
    Regret <= MaxTotalLoss

WeightsPositive ==
    \A i \in Experts : weights[i] >= 1

LossesBounded ==
    \A i \in Experts : expertLosses[i] <= round * MaxLoss

TotalLossBounded ==
    totalLoss <= round * MaxLoss

WeightsExact ==
    \A i \in Experts : weights[i] = MaxTotalLoss + 1 - expertLosses[i]

RoundAccounting ==
    /\ TotalLossBounded
    /\ LossesBounded
    /\ WeightsExact

ReceiveLoss(losses) ==
    /\ round < MaxRounds
    /\ losses \in LossVector
    /\ IF NumExperts = 0 THEN
           /\ totalLoss' = totalLoss
           /\ expertLosses' = expertLosses
           /\ weights' = weights
           /\ lastChosen' = 0
       ELSE
           \E chosen \in Experts :
             LET nextExpertLosses == [i \in Experts |-> expertLosses[i] + losses[i]]
             IN
             /\ totalLoss' = totalLoss + losses[chosen]
             /\ expertLosses' = nextExpertLosses
             /\ weights' = [i \in Experts |-> MaxTotalLoss + 1 - nextExpertLosses[i]]
             /\ lastChosen' = chosen
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
THEOREM Spec => []RegretWithinAccountingHorizon
THEOREM Spec => []WeightsPositive
THEOREM Spec => []LossesBounded
THEOREM Spec => []TotalLossBounded
THEOREM Spec => []WeightsExact
THEOREM Spec => []RoundAccounting

=============================================================================
