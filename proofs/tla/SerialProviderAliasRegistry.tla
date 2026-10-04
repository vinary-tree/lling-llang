------------------ MODULE SerialProviderAliasRegistry -------------------
(**************************************************************************)
(* Context-identity gate sharing during independent scalar-WFST captures. *)
(* A capture owns a retained opaque context throughout gate lookup; the   *)
(* shared gate owns one additional retain until its final Arc dies. Thus  *)
(* a numeric address cannot denote a new provider while an upgraded gate *)
(* for its old occupant exists. Snapshot aliases reuse the same key and   *)
(* must advertise the same PARALLEL_REENTRANT capability. Different keys  *)
(* remain independent. A provisional gate serializes pre-flag interface  *)
(* discovery on one context without blocking independent contexts. The   *)
(* turnstile's waiting protocol is separately checked by                *)
(* SerialProviderTurnstile.tla.                                          *)
(**************************************************************************)
EXTENDS FiniteSets, Naturals

CONSTANTS Captures, Lookups, Contexts, NONE

VARIABLES phase, context, mode, generation, epoch, owners, policy,
          callback, rejected, lookupContext, lookupGeneration, pinned,
          bootstrapOwner
vars == <<phase, context, mode, generation, epoch, owners, policy,
          callback, rejected, lookupContext, lookupGeneration, pinned,
          bootstrapOwner>>

Phases == {"idle", "bootstrap", "live", "snapshotBootstrap",
           "captured", "rejected"}
Modes == {"serial", "parallel"}
Live == {"live", "captured"}
Holders(c) == {l \in Lookups : lookupContext[l] = c}

TypeOK ==
  /\ phase \in [Captures -> Phases]
  /\ context \in [Captures -> Contexts \cup {NONE}]
  /\ mode \in [Captures -> Modes \cup {NONE}]
  /\ generation \in [Captures -> {0, 1}]
  /\ epoch \in [Contexts -> {0, 1}]
  /\ owners \in [Contexts -> SUBSET Captures]
  /\ policy \in [Contexts -> Modes \cup {NONE, "unknown"}]
  /\ callback \subseteq Captures
  /\ rejected \subseteq Captures
  /\ lookupContext \in [Lookups -> Contexts \cup {NONE}]
  /\ lookupGeneration \in [Lookups -> {0, 1}]
  /\ pinned \in [Contexts -> BOOLEAN]
  /\ bootstrapOwner \in [Contexts -> Captures \cup {NONE}]

Init ==
  /\ phase = [t \in Captures |-> "idle"]
  /\ context = [t \in Captures |-> NONE]
  /\ mode = [t \in Captures |-> NONE]
  /\ generation = [t \in Captures |-> 0]
  /\ epoch = [c \in Contexts |-> 0]
  /\ owners = [c \in Contexts |-> {}]
  /\ policy = [c \in Contexts |-> NONE]
  /\ callback = {}
  /\ rejected = {}
  /\ lookupContext = [l \in Lookups |-> NONE]
  /\ lookupGeneration = [l \in Lookups |-> 0]
  /\ pinned = [c \in Contexts |-> FALSE]
  /\ bootstrapOwner = [c \in Contexts |-> NONE]

(* A provisional context gate exists before querying the interface flags. *)
(* Its one retained context pins the address and serializes base discovery. *)
(* Registry insertion and the initial pin are abstracted as one step: the  *)
(* caller's required owned retain keeps the context live in that interval. *)
BeginBootstrap(t, c) ==
  /\ phase[t] = "idle"
  /\ bootstrapOwner[c] = NONE
  /\ (policy[c] = "parallel" \/
       \A s \in callback : context[s] # c)
  /\ phase' = [phase EXCEPT ![t] = "bootstrap"]
  /\ owners' = [owners EXCEPT ![c] = @ \cup {t}]
  /\ policy' = [policy EXCEPT ![c] =
       IF policy[c] = NONE THEN "unknown" ELSE @]
  /\ pinned' = [pinned EXCEPT ![c] = TRUE]
  /\ bootstrapOwner' = [bootstrapOwner EXCEPT ![c] = t]
  /\ context' = [context EXCEPT ![t] = c]
  /\ generation' = [generation EXCEPT ![t] = epoch[c]]
  /\ UNCHANGED <<mode, epoch, callback, rejected,
                  lookupContext, lookupGeneration>>

FinishBootstrap(t, m) ==
  /\ phase[t] = "bootstrap"
  /\ LET c == context[t] IN
       /\ bootstrapOwner[c] = t
       /\ bootstrapOwner' = [bootstrapOwner EXCEPT ![c] = NONE]
       /\ IF policy[c] \in Modes /\ policy[c] # m
            THEN /\ phase' = [phase EXCEPT ![t] = "rejected"]
                 /\ rejected' = rejected \cup {t}
                 /\ owners' = [owners EXCEPT ![c] = @ \ {t}]
                 /\ policy' = [policy EXCEPT ![c] =
                      IF owners[c] = {t} /\ Holders(c) = {} THEN NONE ELSE @]
                 /\ pinned' = [pinned EXCEPT ![c] =
                      IF owners[c] = {t} /\ Holders(c) = {} THEN FALSE ELSE @]
            ELSE /\ phase' = [phase EXCEPT ![t] = "live"]
                 /\ policy' = [policy EXCEPT ![c] = m]
                 /\ UNCHANGED <<owners, rejected, pinned>>
  /\ mode' = [mode EXCEPT ![t] = m]
  /\ UNCHANGED <<context, generation, epoch, callback,
                  lookupContext, lookupGeneration>>

(* A snapshot that aliases the live context repeats base discovery under   *)
(* the same per-context bootstrap gate. The additional snapshot retain may *)
(* advertise a different flag, but that is invalid provider output.       *)
BeginSnapshotBootstrap(t) ==
  /\ phase[t] = "live"
  /\ t \notin callback
  /\ LET c == context[t] IN
       /\ bootstrapOwner[c] = NONE
       /\ (policy[c] = "parallel" \/
            \A s \in callback : context[s] # c)
       /\ bootstrapOwner' = [bootstrapOwner EXCEPT ![c] = t]
  /\ phase' = [phase EXCEPT ![t] = "snapshotBootstrap"]
  /\ UNCHANGED <<context, mode, generation, epoch, owners, policy,
                  callback, rejected, lookupContext, lookupGeneration, pinned>>

FinishSnapshotBootstrap(t, snapshotMode) ==
  /\ phase[t] = "snapshotBootstrap"
  /\ LET c == context[t] IN
       /\ bootstrapOwner[c] = t
       /\ bootstrapOwner' = [bootstrapOwner EXCEPT ![c] = NONE]
       /\ IF snapshotMode # policy[c]
         THEN /\ phase' = [phase EXCEPT ![t] = "rejected"]
              /\ rejected' = rejected \cup {t}
              /\ owners' = [owners EXCEPT ![c] = @ \ {t}]
              /\ policy' = [policy EXCEPT ![c] =
                    IF owners[c] = {t} /\ Holders(c) = {} THEN NONE ELSE @]
              /\ pinned' = [pinned EXCEPT ![c] =
                    IF owners[c] = {t} /\ Holders(c) = {} THEN FALSE ELSE @]
         ELSE /\ phase' = [phase EXCEPT ![t] = "captured"]
              /\ UNCHANGED <<rejected, owners, policy, pinned>>
  /\ UNCHANGED <<context, mode, generation, epoch, callback,
                  lookupContext, lookupGeneration>>

(* A Weak upgrade may outlive the last capture. Its Arc holds the gate's *)
(* independent provider retain, preserving the old allocation identity. *)
BeginLookup(l, c) ==
  /\ lookupContext[l] = NONE
  /\ policy[c] # NONE
  /\ lookupContext' = [lookupContext EXCEPT ![l] = c]
  /\ lookupGeneration' = [lookupGeneration EXCEPT ![l] = epoch[c]]
  /\ UNCHANGED <<phase, context, mode, generation, epoch, owners,
                  policy, callback, rejected, pinned, bootstrapOwner>>

EndLookup(l) ==
  /\ lookupContext[l] # NONE
  /\ LET c == lookupContext[l] IN
       /\ policy' = [policy EXCEPT ![c] =
            IF owners[c] = {} /\ Holders(c) = {l} THEN NONE ELSE @]
       /\ pinned' = [pinned EXCEPT ![c] =
            IF owners[c] = {} /\ Holders(c) = {l} THEN FALSE ELSE @]
  /\ lookupContext' = [lookupContext EXCEPT ![l] = NONE]
  /\ UNCHANGED <<phase, context, mode, generation, epoch, owners,
                  callback, rejected, lookupGeneration, bootstrapOwner>>

Enter(t) ==
  /\ phase[t] \in Live
  /\ t \notin callback
  /\ (policy[context[t]] = "parallel" \/
       bootstrapOwner[context[t]] = NONE)
  /\ (policy[context[t]] = "parallel" \/
       \A s \in callback : context[s] # context[t])
  /\ callback' = callback \cup {t}
  /\ UNCHANGED <<phase, context, mode, generation, epoch, owners,
                  policy, rejected, lookupContext, lookupGeneration, pinned,
                  bootstrapOwner>>

Exit(t) ==
  /\ t \in callback
  /\ callback' = callback \ {t}
  /\ UNCHANGED <<phase, context, mode, generation, epoch, owners,
                  policy, rejected, lookupContext, lookupGeneration, pinned,
                  bootstrapOwner>>

Release(t) ==
  /\ phase[t] \in Live
  /\ t \notin callback
  /\ LET c == context[t] IN
       /\ owners' = [owners EXCEPT ![c] = @ \ {t}]
       /\ policy' = [policy EXCEPT ![c] =
            IF owners[c] = {t} /\ Holders(c) = {} THEN NONE ELSE @]
       /\ pinned' = [pinned EXCEPT ![c] =
            IF owners[c] = {t} /\ Holders(c) = {} THEN FALSE ELSE @]
  /\ phase' = [phase EXCEPT ![t] = "idle"]
  /\ context' = [context EXCEPT ![t] = NONE]
  /\ mode' = [mode EXCEPT ![t] = NONE]
  /\ UNCHANGED <<generation, epoch, callback, rejected,
                  lookupContext, lookupGeneration, bootstrapOwner>>

ResetRejected(t) ==
  /\ phase[t] = "rejected"
  /\ phase' = [phase EXCEPT ![t] = "idle"]
  /\ context' = [context EXCEPT ![t] = NONE]
  /\ mode' = [mode EXCEPT ![t] = NONE]
  /\ rejected' = rejected \ {t}
  /\ UNCHANGED <<generation, epoch, owners, policy, callback,
                  lookupContext, lookupGeneration, pinned, bootstrapOwner>>

(* Numeric address reuse is legal only after every gate and context owner *)
(* for the previous occupant has gone away. The next occupant has a fresh *)
(* epoch and starts without inherited policy or callback admission state. *)
ReuseAddress(c) ==
  /\ owners[c] = {}
  /\ Holders(c) = {}
  /\ policy[c] = NONE
  /\ ~pinned[c]
  /\ bootstrapOwner[c] = NONE
  /\ epoch[c] = 0
  /\ epoch' = [epoch EXCEPT ![c] = 1]
  /\ UNCHANGED <<phase, context, mode, generation, owners, policy,
                  callback, rejected, lookupContext, lookupGeneration, pinned,
                  bootstrapOwner>>

Next ==
  \/ \E t \in Captures : \E c \in Contexts : BeginBootstrap(t,c)
  \/ \E t \in Captures : \E m \in Modes : FinishBootstrap(t,m)
  \/ \E t \in Captures : BeginSnapshotBootstrap(t)
  \/ \E t \in Captures : \E m \in Modes : FinishSnapshotBootstrap(t,m)
  \/ \E t \in Captures : Enter(t) \/ Exit(t) \/ Release(t)
                         \/ ResetRejected(t)
  \/ \E l \in Lookups : \E c \in Contexts : BeginLookup(l,c)
  \/ \E l \in Lookups : EndLookup(l)
  \/ \E c \in Contexts : ReuseAddress(c)

Spec == Init /\ [][Next]_vars

NoOrphanPolicy ==
  \A c \in Contexts :
    /\ ((owners[c] = {} /\ Holders(c) = {}) <=> (policy[c] = NONE))
    /\ (pinned[c] <=> (policy[c] # NONE))
CurrentEpochOwnsGate ==
  \A t \in Captures :
    phase[t] \in Live \cup {"bootstrap", "snapshotBootstrap"} =>
    generation[t] = epoch[context[t]]
BootstrapDiscoverySerialized ==
  \A c \in Contexts : bootstrapOwner[c] # NONE =>
    (phase[bootstrapOwner[c]] \in {"bootstrap", "snapshotBootstrap"} /\
     context[bootstrapOwner[c]] = c)
BootstrapPinsIdentity ==
  \A t \in Captures :
    phase[t] \in {"bootstrap", "snapshotBootstrap"} =>
    (pinned[context[t]] /\ t \in owners[context[t]])
NoSerialDiscoveryCallbackOverlap ==
  \A c \in Contexts :
    (bootstrapOwner[c] # NONE /\ policy[c] # "parallel") =>
      \A t \in callback : context[t] # c
TemporaryUpgradePinsIdentity ==
  \A l \in Lookups : lookupContext[l] # NONE =>
    (pinned[lookupContext[l]] /\
     lookupGeneration[l] = epoch[lookupContext[l]])
SameContextSharesGate ==
  \A s, t \in Captures :
    (phase[s] \in Live /\ phase[t] \in Live /\
     context[s] = context[t]) => generation[s] = generation[t]
ModeStableForOwners ==
  \A t \in Captures : phase[t] \in Live =>
    (t \in owners[context[t]] /\ mode[t] = policy[context[t]])
RejectedOwnsNoGate ==
  \A t \in rejected :
    (phase[t] = "rejected" /\
     \A c \in Contexts : t \notin owners[c])
NoSerialCallbackOverlap ==
  \A s, t \in callback :
    (s # t /\ context[s] = context[t]) =>
      policy[context[s]] = "parallel"
DistinctContextsDistinctGates ==
  \A s, t \in Captures :
    (phase[s] \in Live /\ phase[t] \in Live /\
     context[s] # context[t]) =>
       <<context[s], generation[s]>> # <<context[t], generation[t]>>

(* Expected counterexample: an honest parallel context allows overlap. *)
NoParallelOverlap ==
  \A s, t \in callback : s = t
=============================================================================
