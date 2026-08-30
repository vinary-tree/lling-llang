(** * Assumptions — audit selected optimizer and ABI theorems *)

Require Import LlingLlang.optimizer.TapeSignatures.
Require Import LlingLlang.optimizer.RewriteSemantics.
Require Import LlingLlang.optimizer.PlanDag.
Require Import LlingLlang.abi.OwnershipLifecycle.
Require Import LlingLlang.abi.TypedAbiV2.

Print Assumptions compose_morphism_associative.
Print Assumptions equal_inputs_do_not_imply_compatibility.
Print Assumptions publishable_exact_preserves_denotation.
Print Assumptions well_formed_plan_is_acyclic.
Print Assumptions commit_rejects_out_of_order.
Print Assumptions opaque_v1_clients_cannot_observe_private_layout.
Print Assumptions compose_signature_sound.
Print Assumptions header_extension_preserves_valid.
Print Assumptions authoritative_exact_requires_verified_evidence.
Print Assumptions cancellation_is_sticky.
