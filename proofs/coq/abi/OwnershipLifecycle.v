(** * OwnershipLifecycle — opaque ABI v1 retain/release contracts

    The executable ABI uses opaque resources with one owned retain per handle.
    This mathematical model makes release partial at count zero, proves that a
    retain followed by release is neutral, and defines version-one
    compatibility only through public observations.  Pointer validity and C
    calling-convention preconditions remain implementation-refinement duties.
*)

From Stdlib Require Import Arith.Arith.

Definition ownership_count : Type := nat.

Definition new_resource : ownership_count := 1.
Definition retain (count : ownership_count) : ownership_count := S count.

Definition release (count : ownership_count) : option ownership_count :=
  match count with
  | 0 => None
  | S remaining => Some remaining
  end.

(** Moving a handle transfers the unique obligation without changing the
    resource's total retain count. *)
Definition transfer (count : ownership_count) : ownership_count := count.

Theorem new_resource_owns_exactly_one_retain : new_resource = 1.
Proof. reflexivity. Qed.

Theorem release_at_zero_is_rejected : release 0 = None.
Proof. reflexivity. Qed.

Theorem release_after_retain_is_neutral :
  forall count, release (retain count) = Some count.
Proof. reflexivity. Qed.

Theorem transfer_preserves_total_ownership :
  forall count, transfer count = count.
Proof. reflexivity. Qed.

Theorem clone_then_drop_preserves_live_count :
  forall count, release (retain count) = Some count.
Proof. apply release_after_retain_is_neutral. Qed.

Theorem final_release_reaches_zero : release new_resource = Some 0.
Proof. reflexivity. Qed.

Record public_observation : Type := {
  observed_abi_version : nat;
  observed_status : nat;
  observed_resource_identity : nat
}.

Record implementation_state : Type := {
  private_layout_token : nat;
  public_view : public_observation
}.

Definition opaque_v1_compatible
    (old new : implementation_state) : Prop :=
  public_view old = public_view new.

Definition v1_client : Type := public_observation -> nat.

Theorem opaque_v1_clients_cannot_observe_private_layout :
  forall old new (client : v1_client),
    opaque_v1_compatible old new ->
    client (public_view old) = client (public_view new).
Proof. intros old new client H; unfold opaque_v1_compatible in H; now rewrite H. Qed.

Theorem changing_only_private_layout_is_v1_compatible :
  forall old_token new_token observation,
    opaque_v1_compatible
      {| private_layout_token := old_token; public_view := observation |}
      {| private_layout_token := new_token; public_view := observation |}.
Proof. reflexivity. Qed.

Definition abi_v1 (observation : public_observation) : Prop :=
  observed_abi_version observation = 1.

Theorem compatible_state_preserves_abi_v1 :
  forall old new,
    opaque_v1_compatible old new ->
    abi_v1 (public_view old) ->
    abi_v1 (public_view new).
Proof.
  intros old new Hcompatible Hold.
  unfold opaque_v1_compatible in Hcompatible.
  unfold abi_v1 in *.
  now rewrite <- Hcompatible.
Qed.
