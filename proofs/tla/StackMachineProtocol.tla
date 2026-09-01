----------------------- MODULE StackMachineProtocol -----------------------
(***************************************************************************)
(* Finite-state validation of the ordered tree-fold pushdown protocol.      *)
(*                                                                         *)
(* [focus] is the current source node.  [frames] is the heap-resident       *)
(* continuation stack.  Each frame stores its parent and the next child     *)
(* index.  [nativeDepth] models the host call depth: every action is one     *)
(* transition of a loop, so it must remain exactly one.                     *)
(*                                                                         *)
(* Three configurations cover a branching tree, a deep unary tree, and     *)
(* left-to-right short-circuit failure after a complete left subtree.       *)
(***************************************************************************)

EXTENDS Integers, Naturals, Sequences, FiniteSets

CONSTANTS Shape, ErrorNode

NoNode == 99

Nodes ==
  CASE Shape = "Balanced" -> 0..6
    [] Shape = "Deep" -> 0..5

Root == 0

Children(node) ==
  CASE Shape = "Balanced" ->
         CASE node = 0 -> <<1, 2>>
           [] node = 1 -> <<3, 4>>
           [] node = 2 -> <<5, 6>>
           [] OTHER -> <<>>
    [] Shape = "Deep" ->
         CASE node = 0 -> <<1>>
           [] node = 1 -> <<2>>
           [] node = 2 -> <<3>>
           [] node = 3 -> <<4>>
           [] node = 4 -> <<5>>
           [] OTHER -> <<>>

NodeDepth(node) ==
  CASE Shape = "Balanced" ->
         CASE node = 0 -> 1
           [] node \in {1, 2} -> 2
           [] OTHER -> 3
    [] Shape = "Deep" -> node + 1

MaxDepth ==
  CASE Shape = "Balanced" -> 3
    [] Shape = "Deep" -> 6

ExpectedOutput ==
  CASE /\ Shape = "Balanced" /\ ErrorNode = NoNode ->
         <<3, 4, 1, 5, 6, 2, 0>>
    [] /\ Shape = "Balanced" /\ ErrorNode = 5 -> <<3, 4, 1>>
    [] /\ Shape = "Deep" /\ ErrorNode = NoNode -> <<5, 4, 3, 2, 1, 0>>
    [] OTHER -> <<>>

VARIABLES phase, focus, frames, output, failed, nativeDepth

vars == <<phase, focus, frames, output, failed, nativeDepth>>

Phases == {"down", "up", "done"}

SeqToSet(sequence) == {sequence[index] : index \in 1..Len(sequence)}

IsPrefix(prefix, whole) ==
  /\ Len(prefix) <= Len(whole)
  /\ \A index \in 1..Len(prefix) : prefix[index] = whole[index]

FrameType ==
  [parent : Nodes, nextChild : 1..(MaxDepth + Cardinality(Nodes) + 1)]

TypeOK ==
  /\ phase \in Phases
  /\ focus \in Nodes
  /\ frames \in Seq(FrameType)
  /\ output \in Seq(Nodes)
  /\ failed \in BOOLEAN
  /\ nativeDepth \in Nat

Init ==
  /\ phase = "down"
  /\ focus = Root
  /\ frames = <<>>
  /\ output = <<>>
  /\ failed = FALSE
  /\ nativeDepth = 1

Fail ==
  /\ phase = "down"
  /\ focus = ErrorNode
  /\ ErrorNode \in Nodes
  /\ phase' = "done"
  /\ failed' = TRUE
  /\ UNCHANGED <<focus, frames, output, nativeDepth>>

DownLeaf ==
  /\ phase = "down"
  /\ focus # ErrorNode
  /\ Len(Children(focus)) = 0
  /\ phase' = "up"
  /\ output' = Append(output, focus)
  /\ UNCHANGED <<focus, frames, failed, nativeDepth>>

DownInternal ==
  /\ phase = "down"
  /\ focus # ErrorNode
  /\ Len(Children(focus)) > 0
  /\ frames' = Append(frames, [parent |-> focus, nextChild |-> 2])
  /\ focus' = Children(focus)[1]
  /\ phase' = "down"
  /\ UNCHANGED <<output, failed, nativeDepth>>

UpNextChild ==
  /\ phase = "up"
  /\ Len(frames) > 0
  /\ LET top == frames[Len(frames)] IN
       /\ top.nextChild <= Len(Children(top.parent))
       /\ focus' = Children(top.parent)[top.nextChild]
       /\ frames' =
            [frames EXCEPT ![Len(frames)].nextChild = @ + 1]
  /\ phase' = "down"
  /\ UNCHANGED <<output, failed, nativeDepth>>

UpCompleteParent ==
  /\ phase = "up"
  /\ Len(frames) > 0
  /\ LET top == frames[Len(frames)] IN
       /\ top.nextChild > Len(Children(top.parent))
       /\ focus' = top.parent
       /\ output' = Append(output, top.parent)
  /\ frames' = SubSeq(frames, 1, Len(frames) - 1)
  /\ phase' = "up"
  /\ UNCHANGED <<failed, nativeDepth>>

Finish ==
  /\ phase = "up"
  /\ Len(frames) = 0
  /\ phase' = "done"
  /\ UNCHANGED <<focus, frames, output, failed, nativeDepth>>

Advance ==
  Fail \/ DownLeaf \/ DownInternal \/ UpNextChild \/ UpCompleteParent \/ Finish

Done ==
  /\ phase = "done"
  /\ UNCHANGED vars

Next == Advance \/ Done

Spec == Init /\ [][Next]_vars
FairSpec == Spec /\ WF_vars(Advance)

NativeStackConstant == nativeDepth = 1

HeapStackBounded == Len(frames) < MaxDepth

FocusDepthAligned ==
  phase # "done" => Len(frames) = NodeDepth(focus) - 1

NoDuplicateCompletion == Cardinality(SeqToSet(output)) = Len(output)

PostorderPrefix == IsPrefix(output, ExpectedOutput)

TerminalResultExact ==
  phase = "done" =>
    /\ output = ExpectedOutput
    /\ failed = (ErrorNode \in Nodes)

EventuallyDone == <>(phase = "done")

THEOREM Spec => []TypeOK
THEOREM Spec => []NativeStackConstant
THEOREM Spec => []HeapStackBounded
THEOREM Spec => []FocusDepthAligned
THEOREM Spec => []NoDuplicateCompletion
THEOREM Spec => []PostorderPrefix
THEOREM Spec => []TerminalResultExact
THEOREM FairSpec => EventuallyDone

=============================================================================
