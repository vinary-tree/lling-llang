(** * StatusMapping — the lling-llang ABI status contract

    Two status alphabets meet at the lling-llang ABI: the crate's own
    [LlingStatus] (returned by every `lling_*` C entry point) and the family
    interop [VtStatus] (returned by the vtable callbacks of a re-exported
    resource to its downstream consumer). This file is the formal model of how a
    binding-layer error is classified into each — obligation #20, the formal
    home of invariants LLING-STAT-1..3 (registry: proofs/doc/abi-invariants.tsv).

    It mirrors two Rust functions:
      - `map_error` (src/ffi.rs) : BindingError -> LlingStatus, the C-ABI
        classification;
      - `expansion_error_status` (src/bindings.rs) : BindingError -> VtStatus,
        the re-export classification a composed resource reports downstream.

    **The LLING-B8 arbitration (LLING-STAT-3).** A label outside the Unicode
    scalar range cannot be held by this char-based binding specialization; it is
    therefore classified as a *representation limit*
    ([BindingError.RepresentationLimit]) — a limit of THIS binding, which a
    u64-label specialization could hold — and NOT as provider misbehavior. The
    finding LLING-B8 was that the two ABI surfaces disagreed: the import path
    already reported `LimitExceeded`, but the composition re-export coarsened it
    to `ProviderError`. This model certifies the harmonized contract: a
    representation limit reads as "limit exceeded" on BOTH surfaces, and is
    distinct from a provider fault on both. The Rust code was harmonized to
    match this model (`expand_state` classifies a non-scalar label as
    `RepresentationLimit`, and `expansion_error_status` forwards it to
    `VtStatus::LimitExceeded`).

    Registry: proofs/doc/abi-invariants.tsv, LLING-STAT-1..3.
*)

(** The interop status alphabet (vinary-tree-interop::VtStatus, discriminants
    0..8). *)
Inductive VtStatus : Type :=
  | VOk
  | VEnd
  | VInvalidArgument
  | VNullPointer
  | VUnsupported
  | VIoError
  | VClosed
  | VLimitExceeded
  | VProviderError.

(** The lling-llang status alphabet (LlingStatus, discriminants 0..7). *)
Inductive LlingStatus : Type :=
  | LOk
  | LInvalidArgument
  | LNullPointer
  | LPanic
  | LIncompatibleResource
  | LProviderError
  | LLimitExceeded
  | LClosed.

(** The binding-layer error type (src/bindings.rs::BindingError). The [Provider]
    payload carries the foreign status that failed. *)
Inductive BindingError : Type :=
  | NullResource
  | IncompatibleResourceAbi
  | MissingWfstInterface
  | IncompatibleWfstInterface
  | UnitDomainMismatch
  | WeightDomainMismatch
  | Provider (s : VtStatus)
  | InvalidProviderOutput
  | RepresentationLimit.

(** ** The two classification functions *)

(** `map_error` — the C-ABI classification (src/ffi.rs). *)
Definition map_error (e : BindingError) : LlingStatus :=
  match e with
  | Provider _ => LProviderError
  | InvalidProviderOutput => LProviderError
  | RepresentationLimit => LLimitExceeded
  | NullResource => LNullPointer
  | IncompatibleResourceAbi => LIncompatibleResource
  | MissingWfstInterface => LIncompatibleResource
  | IncompatibleWfstInterface => LIncompatibleResource
  | UnitDomainMismatch => LIncompatibleResource
  | WeightDomainMismatch => LIncompatibleResource
  end.

(** `expansion_error_status` — the re-export classification (src/bindings.rs). A
    representation limit is preserved as `LimitExceeded`; every other expansion
    error is a generic provider error to the downstream consumer. *)
Definition expansion_error_status (e : BindingError) : VtStatus :=
  match e with
  | RepresentationLimit => VLimitExceeded
  | _ => VProviderError
  end.

(** ** LLING-STAT-1: no error is silently swallowed into success *)

Theorem map_error_never_ok : forall e, map_error e <> LOk.
Proof. destruct e; discriminate. Qed.

Theorem expansion_error_never_ok : forall e, expansion_error_status e <> VOk.
Proof. destruct e; discriminate. Qed.

(** ** LLING-STAT-2: the C-ABI classification is exactly as documented *)

Theorem provider_fault_is_provider_error :
  forall s, map_error (Provider s) = LProviderError.
Proof. reflexivity. Qed.

Theorem invalid_output_is_provider_error :
  map_error InvalidProviderOutput = LProviderError.
Proof. reflexivity. Qed.

Theorem representation_limit_is_limit_exceeded :
  map_error RepresentationLimit = LLimitExceeded.
Proof. reflexivity. Qed.

Theorem null_resource_is_null_pointer :
  map_error NullResource = LNullPointer.
Proof. reflexivity. Qed.

Theorem incompatibilities_are_incompatible_resource :
  map_error IncompatibleResourceAbi = LIncompatibleResource
  /\ map_error MissingWfstInterface = LIncompatibleResource
  /\ map_error IncompatibleWfstInterface = LIncompatibleResource
  /\ map_error UnitDomainMismatch = LIncompatibleResource
  /\ map_error WeightDomainMismatch = LIncompatibleResource.
Proof. repeat split; reflexivity. Qed.

(** ** LLING-STAT-3: the representation-limit classification is consistent
    across both ABI surfaces (the LLING-B8 arbitration) *)

(** A representation limit reads as "limit exceeded" on the direct C ABI AND on
    the re-export vtable — the same input (e.g. a non-scalar label) is
    classified the same way regardless of which surface it crosses. *)
Theorem representation_limit_consistent_across_surfaces :
  map_error RepresentationLimit = LLimitExceeded
  /\ expansion_error_status RepresentationLimit = VLimitExceeded.
Proof. split; reflexivity. Qed.

(** ...and it is distinct from a provider fault on both surfaces: a limit is
    never coarsened into a generic provider error (the exact regression that
    was LLING-B8 on the composition path). *)
Theorem representation_limit_distinct_from_provider_fault :
  map_error RepresentationLimit <> map_error InvalidProviderOutput
  /\ expansion_error_status RepresentationLimit
       <> expansion_error_status InvalidProviderOutput.
Proof. split; discriminate. Qed.

(** ** The C boundary: success, forwarded failure, and caught panic *)

(** The `boundary()` wrapper (src/ffi.rs): a successful operation yields Ok, a
    returned error status is forwarded verbatim, and an unwound panic yields
    Panic. *)
Inductive boundary_outcome : Type :=
  | Success
  | Failure (s : LlingStatus)
  | Panicked.

Definition boundary_status (o : boundary_outcome) : LlingStatus :=
  match o with
  | Success => LOk
  | Failure s => s
  | Panicked => LPanic
  end.

Theorem boundary_success_is_ok : boundary_status Success = LOk.
Proof. reflexivity. Qed.

Theorem boundary_panic_is_panic : boundary_status Panicked = LPanic.
Proof. reflexivity. Qed.

(** A failure carrying a mapped binding error never reports Ok: composing the
    boundary with [map_error] cannot swallow an error into success. *)
Theorem boundary_mapped_failure_never_ok :
  forall e, boundary_status (Failure (map_error e)) <> LOk.
Proof. intro e; simpl; apply map_error_never_ok. Qed.

(** A caught panic is never confused with success or with any ordinary error
    status the operation could have returned: [LPanic] is a distinct code. *)
Theorem panic_is_not_success : boundary_status Panicked <> boundary_status Success.
Proof. discriminate. Qed.
