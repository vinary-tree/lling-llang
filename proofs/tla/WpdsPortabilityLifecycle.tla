---------------------- MODULE WpdsPortabilityLifecycle ----------------------
EXTENDS Integers, Naturals, FiniteSets, Sequences, TLC

\* Bounded executable refinement of the generic Rocq contract.  External rule
\* keys are portable; dense identifiers remain a local, zero-based hot-path
\* representation.  All decoder and replay control lives in explicit state.

RuleKeys == {"rule-a", "rule-b"}
CanonicalKeys == <<"rule-a", "rule-b">>
DuplicateKeys == <<"rule-a", "rule-a">>
EncodedBytes == 3
MaxNodes == 2
MaxEdges == 2

IdentityFields == {"rules", "context", "query", "semantics", "codec"}
ExpectedIdentity == [field \in IdentityFields |-> 0]
QueryMismatch == [ExpectedIdentity EXCEPT !["query"] = 1]
ContextMismatch == [ExpectedIdentity EXCEPT !["context"] = 1]
RuleMismatch == [ExpectedIdentity EXCEPT !["rules"] = 1]
SemanticMismatch == [ExpectedIdentity EXCEPT !["semantics"] = 1]
CodecMismatch == [ExpectedIdentity EXCEPT !["codec"] = 1]
CandidateIdentities == {
  ExpectedIdentity, QueryMismatch, ContextMismatch, RuleMismatch,
  SemanticMismatch, CodecMismatch
}

CancelReasons == {"requested", "deadline", "budget", "source"}
TerminalPhases == {"published", "rejected", "released"}
NoKey == "input"
UnknownKey == "unknown"

VARIABLES phase, encodedKeys, denseMap, sealedMap,
          observedIdentity, checksumOk, canonicalOk, witnessClaimOk,
          cancellationReason, cancellationHistory,
          cursor, nodesUsed, edgesUsed, decodedNodes,
          outputPublished, handleLive

vars == <<phase, encodedKeys, denseMap, sealedMap,
          observedIdentity, checksumOk, canonicalOk, witnessClaimOk,
          cancellationReason, cancellationHistory,
          cursor, nodesUsed, edgesUsed, decodedNodes,
          outputPublished, handleLive>>

MappedKeys(map) == {map[index].key : index \in DOMAIN map}
MappedDense(map) == {map[index].dense : index \in DOMAIN map}

IdentityMatches == observedIdentity = ExpectedIdentity
MapComplete == Len(denseMap) = Len(encodedKeys)
MapCanonical == encodedKeys = CanonicalKeys
KnownWitnessKeys ==
  \A index \in DOMAIN decodedNodes :
    decodedNodes[index].key = NoKey
    \/ decodedNodes[index].key \in MappedKeys(denseMap)
PremisesPrecede ==
  \A index \in DOMAIN decodedNodes :
    \A premise \in decodedNodes[index].premises :
      premise < decodedNodes[index].id
ExactAdmission ==
  IdentityMatches /\ checksumOk /\ canonicalOk /\ MapCanonical
  /\ MapComplete /\ witnessClaimOk /\ KnownWitnessKeys
  /\ PremisesPrecede /\ cancellationReason = "none"
  /\ cursor = EncodedBytes

Init ==
  /\ phase = "mapping"
  /\ encodedKeys \in {CanonicalKeys, DuplicateKeys}
  /\ denseMap = <<>>
  /\ sealedMap = <<>>
  /\ observedIdentity \in CandidateIdentities
  /\ checksumOk \in BOOLEAN
  /\ canonicalOk \in BOOLEAN
  /\ witnessClaimOk \in BOOLEAN
  /\ cancellationReason = "none"
  /\ cancellationHistory = <<>>
  /\ cursor = 0
  /\ nodesUsed = 0
  /\ edgesUsed = 0
  /\ decodedNodes = <<>>
  /\ outputPublished = FALSE
  /\ handleLive = TRUE

MapNext ==
  /\ phase = "mapping"
  /\ Len(denseMap) < Len(encodedKeys)
  /\ LET key == encodedKeys[Len(denseMap) + 1] IN
       IF key \in MappedKeys(denseMap)
       THEN /\ phase' = "rejected"
            /\ UNCHANGED <<denseMap, sealedMap>>
       ELSE /\ denseMap' = Append(denseMap,
                    [key |-> key, dense |-> Len(denseMap)])
            /\ UNCHANGED <<phase, sealedMap>>
  /\ UNCHANGED <<encodedKeys, observedIdentity, checksumOk, canonicalOk,
                  witnessClaimOk, cancellationReason, cancellationHistory,
                  cursor, nodesUsed, edgesUsed, decodedNodes,
                  outputPublished, handleLive>>

FinishMap ==
  /\ phase = "mapping"
  /\ MapComplete
  /\ IF MapCanonical /\ checksumOk /\ canonicalOk /\ IdentityMatches
        /\ cancellationReason = "none"
     THEN /\ phase' = "decoding"
          /\ sealedMap' = denseMap
     ELSE /\ phase' = "rejected"
          /\ sealedMap' = <<>>
  /\ UNCHANGED <<encodedKeys, denseMap, observedIdentity, checksumOk,
                  canonicalOk, witnessClaimOk, cancellationReason,
                  cancellationHistory, cursor, nodesUsed, edgesUsed,
                  decodedNodes, outputPublished, handleLive>>

RequestCancellation ==
  /\ phase \notin TerminalPhases
  /\ cancellationReason = "none"
  /\ \E reason \in CancelReasons :
       /\ cancellationReason' = reason
       /\ cancellationHistory' = Append(cancellationHistory, reason)
  /\ UNCHANGED <<phase, encodedKeys, denseMap, sealedMap,
                  observedIdentity, checksumOk, canonicalOk, witnessClaimOk,
                  cursor, nodesUsed, edgesUsed, decodedNodes,
                  outputPublished, handleLive>>

RejectCancellation ==
  /\ phase \in {"decoding", "ready", "resuming"}
  /\ cancellationReason \in CancelReasons
  /\ phase' = "rejected"
  /\ UNCHANGED <<encodedKeys, denseMap, sealedMap, observedIdentity,
                  checksumOk, canonicalOk, witnessClaimOk,
                  cancellationReason, cancellationHistory,
                  cursor, nodesUsed, edgesUsed, decodedNodes,
                  outputPublished, handleLive>>

DecodeRecord ==
  /\ phase = "decoding"
  /\ cancellationReason = "none"
  /\ cursor < EncodedBytes
  /\ \E key \in (RuleKeys \cup {NoKey, UnknownKey}) :
     \E premises \in SUBSET (0..(nodesUsed - 1)) :
       LET nextEdges == edgesUsed + Cardinality(premises) IN
       IF nodesUsed + 1 > MaxNodes \/ nextEdges > MaxEdges
       THEN /\ phase' = "rejected"
            /\ UNCHANGED <<cursor, nodesUsed, edgesUsed, decodedNodes>>
       ELSE /\ cursor' = cursor + 1
            /\ nodesUsed' = nodesUsed + 1
            /\ edgesUsed' = nextEdges
            /\ decodedNodes' = Append(decodedNodes,
                 [id |-> nodesUsed, key |-> key, premises |-> premises])
            /\ UNCHANGED phase
  /\ UNCHANGED <<encodedKeys, denseMap, sealedMap, observedIdentity,
                  checksumOk, canonicalOk, witnessClaimOk,
                  cancellationReason, cancellationHistory,
                  outputPublished, handleLive>>

SkipByte ==
  /\ phase = "decoding"
  /\ cancellationReason = "none"
  /\ cursor < EncodedBytes
  /\ cursor' = cursor + 1
  /\ UNCHANGED <<phase, encodedKeys, denseMap, sealedMap,
                  observedIdentity, checksumOk, canonicalOk, witnessClaimOk,
                  cancellationReason, cancellationHistory,
                  nodesUsed, edgesUsed, decodedNodes,
                  outputPublished, handleLive>>

FinishDecode ==
  /\ phase = "decoding"
  /\ cursor = EncodedBytes
  /\ IF witnessClaimOk /\ KnownWitnessKeys /\ PremisesPrecede
     THEN phase' = "ready"
     ELSE phase' = "rejected"
  /\ UNCHANGED <<encodedKeys, denseMap, sealedMap, observedIdentity,
                  checksumOk, canonicalOk, witnessClaimOk,
                  cancellationReason, cancellationHistory,
                  cursor, nodesUsed, edgesUsed, decodedNodes,
                  outputPublished, handleLive>>

BeginResume ==
  /\ phase = "ready"
  /\ IF ExactAdmission
     THEN phase' = "resuming"
     ELSE phase' = "rejected"
  /\ UNCHANGED <<encodedKeys, denseMap, sealedMap, observedIdentity,
                  checksumOk, canonicalOk, witnessClaimOk,
                  cancellationReason, cancellationHistory,
                  cursor, nodesUsed, edgesUsed, decodedNodes,
                  outputPublished, handleLive>>

Publish ==
  /\ phase = "resuming"
  /\ ExactAdmission
  /\ phase' = "published"
  /\ outputPublished' = TRUE
  /\ UNCHANGED <<encodedKeys, denseMap, sealedMap, observedIdentity,
                  checksumOk, canonicalOk, witnessClaimOk,
                  cancellationReason, cancellationHistory,
                  cursor, nodesUsed, edgesUsed, decodedNodes, handleLive>>

Release ==
  /\ phase \in {"published", "rejected"}
  /\ handleLive
  /\ phase' = "released"
  /\ handleLive' = FALSE
  /\ UNCHANGED <<encodedKeys, denseMap, sealedMap, observedIdentity,
                  checksumOk, canonicalOk, witnessClaimOk,
                  cancellationReason, cancellationHistory,
                  cursor, nodesUsed, edgesUsed, decodedNodes,
                  outputPublished>>

Done ==
  /\ phase \in TerminalPhases
  /\ UNCHANGED vars

Next == MapNext \/ FinishMap \/ RequestCancellation \/ RejectCancellation
        \/ DecodeRecord \/ SkipByte \/ FinishDecode \/ BeginResume
        \/ Publish \/ Release \/ Done

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in {"mapping", "decoding", "ready", "resuming"}
                \cup TerminalPhases
  /\ encodedKeys \in {CanonicalKeys, DuplicateKeys}
  /\ denseMap \in Seq([key : RuleKeys, dense : Nat])
  /\ sealedMap \in Seq([key : RuleKeys, dense : Nat])
  /\ observedIdentity \in CandidateIdentities
  /\ checksumOk \in BOOLEAN /\ canonicalOk \in BOOLEAN
  /\ witnessClaimOk \in BOOLEAN
  /\ cancellationReason \in CancelReasons \cup {"none"}
  /\ cancellationHistory \in Seq(CancelReasons)
  /\ cursor \in 0..EncodedBytes
  /\ nodesUsed \in 0..MaxNodes
  /\ edgesUsed \in 0..MaxEdges
  /\ decodedNodes \in Seq([
       id : 0..MaxNodes,
       key : RuleKeys \cup {NoKey, UnknownKey},
       premises : SUBSET (0..MaxNodes)])
  /\ outputPublished \in BOOLEAN /\ handleLive \in BOOLEAN

DenseMapBijection ==
  /\ \A left, right \in DOMAIN denseMap :
       denseMap[left].key = denseMap[right].key => left = right
  /\ \A left, right \in DOMAIN denseMap :
       denseMap[left].dense = denseMap[right].dense => left = right
  /\ \A index \in DOMAIN denseMap : denseMap[index].dense = index - 1

SealedMapStable == sealedMap = <<>> \/ denseMap = sealedMap
CancellationSticky == Len(cancellationHistory) <= 1
DecoderBounds ==
  /\ cursor <= EncodedBytes
  /\ nodesUsed = Len(decodedNodes)
  /\ nodesUsed <= MaxNodes
  /\ edgesUsed <= MaxEdges
PremiseOrder == PremisesPrecede
RejectedNeverPublishes == phase = "rejected" => ~outputPublished
PublicationRequiresExactAdmission == outputPublished => ExactAdmission
PublicationHasExactIdentity ==
  outputPublished => observedIdentity = ExpectedIdentity
PublicationWasNeverCancelled ==
  outputPublished => cancellationHistory = <<>>
ReleasedHandleIsDead == phase = "released" => ~handleLive
LiveOutsideRelease == phase # "released" => handleLive

=============================================================================
