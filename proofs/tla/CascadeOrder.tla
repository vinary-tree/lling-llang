------------------------------ MODULE CascadeOrder ------------------------------
(***************************************************************************
 * ASR cascade ordering model.
 *
 * The model tracks the standard AM -> CD -> Lexicon -> LM order using finite
 * component identifiers and explicit alphabet functions so TLC can enumerate
 * the state space supplied by a config file.
 ***************************************************************************)

EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
    AcousticSymbols,
    PhoneSymbols,
    TriphoneSymbols,
    WordSymbols

VARIABLES
    composed,
    currentInput,
    currentOutput,
    cascadeOrder

vars == <<composed, currentInput, currentOutput, cascadeOrder>>

Components == {"AM", "CD", "Lexicon", "LM"}
AlphabetUniverse == AcousticSymbols \cup PhoneSymbols \cup TriphoneSymbols \cup WordSymbols

SeqToSet(seq) == { seq[i] : i \in 1..Len(seq) }

LastComponent ==
    cascadeOrder[Len(cascadeOrder)]

InputAlpha(c) ==
    CASE c = "AM" -> AcousticSymbols
      [] c = "CD" -> TriphoneSymbols
      [] c = "Lexicon" -> PhoneSymbols
      [] c = "LM" -> WordSymbols

OutputAlpha(c) ==
    CASE c = "AM" -> TriphoneSymbols
      [] c = "CD" -> PhoneSymbols
      [] c = "Lexicon" -> WordSymbols
      [] c = "LM" -> WordSymbols

AllowedNext(c1, c2) ==
    \/ /\ c1 = "AM" /\ c2 = "CD"
    \/ /\ c1 = "CD" /\ c2 = "Lexicon"
    \/ /\ c1 = "Lexicon" /\ c2 = "LM"

TypeOK ==
    /\ composed \subseteq (Components \X Components)
    /\ cascadeOrder \in Seq(Components)
    /\ currentInput \subseteq AlphabetUniverse
    /\ currentOutput \subseteq AlphabetUniverse

Init ==
    /\ composed = {}
    /\ cascadeOrder = <<"AM">>
    /\ currentInput = AcousticSymbols
    /\ currentOutput = TriphoneSymbols

CanCompose(c1, c2) ==
    /\ c1 \in Components
    /\ c2 \in Components
    /\ AllowedNext(c1, c2)
    /\ OutputAlpha(c1) = InputAlpha(c2)

Compose(c1, c2) ==
    /\ CanCompose(c1, c2)
    /\ <<c1, c2>> \notin composed
    /\ c2 \notin SeqToSet(cascadeOrder)
    /\ Len(cascadeOrder) < Cardinality(Components)
    /\ composed' = composed \cup {<<c1, c2>>}
    /\ cascadeOrder' = Append(cascadeOrder, c2)
    /\ currentInput' = currentInput
    /\ currentOutput' = OutputAlpha(c2)

BuildCascade ==
    /\ Len(cascadeOrder) > 0
    /\ \E next \in Components :
        /\ next \notin SeqToSet(cascadeOrder)
        /\ Compose(LastComponent, next)

CascadeComplete ==
    /\ Len(cascadeOrder) = 4
    /\ UNCHANGED vars

Next == BuildCascade \/ CascadeComplete

AlphabetsCompatible ==
    \A pair \in composed :
        OutputAlpha(pair[1]) = InputAlpha(pair[2])

StartsWithAcoustic ==
    cascadeOrder[1] = "AM" /\ currentInput = AcousticSymbols

EndsWithWords ==
    Len(cascadeOrder) = 4 => currentOutput = WordSymbols

NoRepetition ==
    \A i, j \in 1..Len(cascadeOrder) :
        i # j => cascadeOrder[i] # cascadeOrder[j]

CDAfterAM ==
    \A i \in 1..Len(cascadeOrder) :
        cascadeOrder[i] = "CD" =>
            \E j \in 1..(i-1) : cascadeOrder[j] = "AM"

LexiconAfterCD ==
    \A i \in 1..Len(cascadeOrder) :
        cascadeOrder[i] = "Lexicon" =>
            \E j \in 1..(i-1) : cascadeOrder[j] = "CD"

LMAfterLexicon ==
    \A i \in 1..Len(cascadeOrder) :
        cascadeOrder[i] = "LM" =>
            \E j \in 1..(i-1) : cascadeOrder[j] = "Lexicon"

OrderingConstraints ==
    /\ CDAfterAM
    /\ LexiconAfterCD
    /\ LMAfterLexicon

ValidCascade ==
    Len(cascadeOrder) = 4 =>
        /\ StartsWithAcoustic
        /\ EndsWithWords
        /\ AlphabetsCompatible
        /\ OrderingConstraints
        /\ NoRepetition

PrefixValid ==
    /\ StartsWithAcoustic
    /\ AlphabetsCompatible
    /\ OrderingConstraints
    /\ NoRepetition

EventuallyCascadeComplete ==
    <>(Len(cascadeOrder) = Cardinality(Components))

Spec == Init /\ [][Next]_vars

Fairness == WF_vars(BuildCascade)
FairSpec == Spec /\ Fairness

THEOREM Spec => []TypeOK
THEOREM Spec => []AlphabetsCompatible
THEOREM Spec => []OrderingConstraints
THEOREM Spec => []NoRepetition
THEOREM Spec => []ValidCascade
THEOREM Spec => []PrefixValid
THEOREM FairSpec => EventuallyCascadeComplete

=============================================================================
