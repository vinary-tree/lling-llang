/* Stable project-owned C API for lling-llang scalar WFSTs. */
#ifndef LLING_LLANG_H
#define LLING_LLANG_H

#include <stddef.h>
#include <stdint.h>
#ifndef VT_INTEROP_HEADER
#define VT_INTEROP_HEADER "vinary_tree_interop.h"
#endif
#include VT_INTEROP_HEADER

#if defined(_WIN32) || defined(__CYGWIN__)
#  if defined(LLING_LLANG_BUILDING_DLL)
#    define LLING_API __declspec(dllexport)
#  elif defined(LLING_LLANG_USING_DLL)
#    define LLING_API __declspec(dllimport)
#  else
#    define LLING_API
#  endif
#elif defined(__GNUC__) || defined(__clang__)
#  define LLING_API __attribute__((visibility("default")))
#else
#  define LLING_API
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define LLING_ABI_VERSION 1u
#define LLING_API_REVISION 5u
#define LLING_ABI_V2 2u

#define LLING_DESCRIPTOR_SIGNATURE_KNOWN (UINT64_C(1) << 0)
#define LLING_DESCRIPTOR_SNAPSHOT_PRESENT (UINT64_C(1) << 1)
#define LLING_DESCRIPTOR_CONTEXT_PRESENT (UINT64_C(1) << 2)

#define LLING_BUDGET_STATES (UINT64_C(1) << 0)
#define LLING_BUDGET_ARCS (UINT64_C(1) << 1)
#define LLING_BUDGET_BYTES (UINT64_C(1) << 2)
#define LLING_BUDGET_WORK (UINT64_C(1) << 3)

typedef enum LlingStatus {
    LLING_STATUS_OK = 0,
    LLING_STATUS_INVALID_ARGUMENT = 1,
    LLING_STATUS_NULL_POINTER = 2,
    LLING_STATUS_PANIC = 3,
    LLING_STATUS_INCOMPATIBLE_RESOURCE = 4,
    LLING_STATUS_PROVIDER_ERROR = 5,
    LLING_STATUS_LIMIT_EXCEEDED = 6,
    LLING_STATUS_CLOSED = 7
} LlingStatus;

typedef struct LlingWfstBuilder LlingWfstBuilder;
typedef struct LlingWfst LlingWfst;
typedef struct LlingSemiring LlingSemiring;
typedef struct LlingSemiringWeight LlingSemiringWeight;
typedef struct LlingLatticeValue LlingLatticeValue;
typedef struct LlingCancellationV2 LlingCancellationV2;

typedef struct LlingAbiV2Header {
    uint32_t struct_size;
    uint32_t abi_version;
    uint64_t flags;
    uint64_t reserved;
} LlingAbiV2Header;

typedef struct LlingId128 {
    uint8_t bytes[16];
} LlingId128;

typedef struct LlingDigest256 {
    uint8_t bytes[32];
} LlingDigest256;

typedef struct LlingWfstDescriptorV2 {
    LlingAbiV2Header header;
    LlingId128 input_tape;
    LlingId128 output_tape;
    LlingId128 algebra;
    LlingId128 snapshot;
    LlingDigest256 context;
} LlingWfstDescriptorV2;

typedef struct LlingBudgetV2 {
    LlingAbiV2Header header;
    uint64_t max_states;
    uint64_t max_arcs;
    uint64_t max_bytes;
    uint64_t max_work;
    uint64_t reserved[2];
} LlingBudgetV2;

typedef enum LlingPrecisionV2 {
    LLING_PRECISION_EXACT_V2 = 1,
    LLING_PRECISION_APPROXIMATE_V2 = 2,
    LLING_PRECISION_UNKNOWN_V2 = 3
} LlingPrecisionV2;

typedef enum LlingCompletenessV2 {
    LLING_COMPLETENESS_COMPLETE_V2 = 1,
    LLING_COMPLETENESS_INCOMPLETE_V2 = 2
} LlingCompletenessV2;

typedef enum LlingApplicabilityV2 {
    LLING_APPLICABILITY_APPLICABLE_V2 = 1,
    LLING_APPLICABILITY_UNSUPPORTED_V2 = 2,
    LLING_APPLICABILITY_UNKNOWN_V2 = 3
} LlingApplicabilityV2;

typedef enum LlingTerminationV2 {
    LLING_TERMINATION_SUCCEEDED_V2 = 1,
    LLING_TERMINATION_CANCELLED_V2 = 2,
    LLING_TERMINATION_BUDGET_EXHAUSTED_V2 = 3,
    LLING_TERMINATION_FAILED_V2 = 4
} LlingTerminationV2;

typedef enum LlingEvidenceStateV2 {
    LLING_EVIDENCE_NONE_V2 = 0,
    LLING_EVIDENCE_CANDIDATE_V2 = 1,
    LLING_EVIDENCE_VERIFIED_V2 = 2,
    LLING_EVIDENCE_STALE_V2 = 3,
    LLING_EVIDENCE_INVALID_V2 = 4
} LlingEvidenceStateV2;

typedef enum LlingCancellationReasonV2 {
    LLING_CANCELLATION_REQUESTED_V2 = 1,
    LLING_CANCELLATION_DEADLINE_V2 = 2,
    LLING_CANCELLATION_BUDGET_V2 = 3,
    LLING_CANCELLATION_SOURCE_V2 = 4
} LlingCancellationReasonV2;

typedef struct LlingOutcomeV2 {
    LlingAbiV2Header header;
    uint32_t precision;
    uint32_t completeness;
    uint32_t applicability;
    uint32_t termination;
    uint32_t evidence;
    uint32_t reserved0;
    uint64_t states;
    uint64_t arcs;
    uint64_t bytes;
    uint64_t work;
    uint64_t limitations;
    uint64_t reserved1;
} LlingOutcomeV2;

#if defined(__cplusplus)
static_assert(sizeof(LlingAbiV2Header) == 24, "LlingAbiV2Header layout drift");
static_assert(sizeof(LlingId128) == 16, "LlingId128 layout drift");
static_assert(alignof(LlingId128) == 1, "LlingId128 alignment drift");
static_assert(sizeof(LlingDigest256) == 32, "LlingDigest256 layout drift");
static_assert(alignof(LlingDigest256) == 1, "LlingDigest256 alignment drift");
static_assert(sizeof(LlingWfstDescriptorV2) == 120, "LlingWfstDescriptorV2 layout drift");
static_assert(sizeof(LlingBudgetV2) == 72, "LlingBudgetV2 layout drift");
static_assert(sizeof(LlingOutcomeV2) == 96, "LlingOutcomeV2 layout drift");
#elif defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(LlingAbiV2Header) == 24, "LlingAbiV2Header layout drift");
_Static_assert(sizeof(LlingId128) == 16, "LlingId128 layout drift");
_Static_assert(_Alignof(LlingId128) == 1, "LlingId128 alignment drift");
_Static_assert(sizeof(LlingDigest256) == 32, "LlingDigest256 layout drift");
_Static_assert(_Alignof(LlingDigest256) == 1, "LlingDigest256 alignment drift");
_Static_assert(sizeof(LlingWfstDescriptorV2) == 120, "LlingWfstDescriptorV2 layout drift");
_Static_assert(sizeof(LlingBudgetV2) == 72, "LlingBudgetV2 layout drift");
_Static_assert(sizeof(LlingOutcomeV2) == 96, "LlingOutcomeV2 layout drift");
#endif

LLING_API uint32_t lling_abi_version(void);
LLING_API uint32_t lling_api_revision(void);
LLING_API const char* lling_last_error_message(void);
/* Host-defined dynamic semiring consumer. The context retains resource. */
LLING_API LlingStatus lling_semiring_open(
    const VtResource* resource, LlingSemiring** out_semiring);
LLING_API void lling_semiring_free(LlingSemiring* semiring);
LLING_API void lling_semiring_weight_free(LlingSemiringWeight* weight);
LLING_API LlingStatus lling_semiring_properties(
    const LlingSemiring* semiring, uint64_t* out_properties);
LLING_API LlingStatus lling_semiring_zero(
    const LlingSemiring* semiring, LlingSemiringWeight** out_weight);
LLING_API LlingStatus lling_semiring_one(
    const LlingSemiring* semiring, LlingSemiringWeight** out_weight);
LLING_API LlingStatus lling_semiring_weight_clone(
    const LlingSemiringWeight* weight, LlingSemiringWeight** out_weight);
LLING_API LlingStatus lling_semiring_plus(
    const LlingSemiring* semiring, const LlingSemiringWeight* left,
    const LlingSemiringWeight* right, LlingSemiringWeight** out_weight);
LLING_API LlingStatus lling_semiring_times(
    const LlingSemiring* semiring, const LlingSemiringWeight* left,
    const LlingSemiringWeight* right, LlingSemiringWeight** out_weight);
LLING_API LlingStatus lling_semiring_equal(
    const LlingSemiring* semiring, const LlingSemiringWeight* left,
    const LlingSemiringWeight* right, uint8_t* out_equal);
LLING_API LlingStatus lling_semiring_approx_equal(
    const LlingSemiring* semiring, const LlingSemiringWeight* left,
    const LlingSemiringWeight* right, double epsilon, uint8_t* out_equal);
LLING_API LlingStatus lling_semiring_natural_order(
    const LlingSemiring* semiring, const LlingSemiringWeight* left,
    const LlingSemiringWeight* right, int32_t* out_order);
LLING_API LlingStatus lling_semiring_divide(
    const LlingSemiring* semiring, const LlingSemiringWeight* dividend,
    const LlingSemiringWeight* divisor, LlingSemiringWeight** out_weight,
    uint8_t* out_defined);
LLING_API LlingStatus lling_semiring_left_divide(
    const LlingSemiring* semiring, const LlingSemiringWeight* value,
    const LlingSemiringWeight* divisor, LlingSemiringWeight** out_weight,
    uint8_t* out_defined);
LLING_API LlingStatus lling_semiring_star(
    const LlingSemiring* semiring, const LlingSemiringWeight* value,
    LlingSemiringWeight** out_weight, uint8_t* out_defined);
LLING_API LlingStatus lling_semiring_numerical_value(
    const LlingSemiring* semiring, const LlingSemiringWeight* value,
    double* out_value);
LLING_API LlingStatus lling_semiring_quantize(
    const LlingSemiring* semiring, const LlingSemiringWeight* value,
    double epsilon, int64_t* out_value);
LLING_API LlingStatus lling_semiring_to_probability(
    const LlingSemiring* semiring, const LlingSemiringWeight* value,
    double* out_value);
LLING_API LlingStatus lling_semiring_closure_bound(
    const LlingSemiring* semiring, size_t* out_bound, uint8_t* out_known);
LLING_API LlingStatus lling_semiring_stable_bytes(
    const LlingSemiring* semiring, const LlingSemiringWeight* value,
    uint8_t* out_bytes, size_t capacity, size_t* out_written,
    size_t* out_required);
LLING_API LlingStatus lling_semiring_validate_laws(
    const LlingSemiring* semiring,
    const LlingSemiringWeight* const* weights, size_t count, double epsilon);
/* Validated, same-thread consumer for immutable vt.lattice.val.1 values. */
LLING_API LlingStatus lling_lattice_open(
    const VtResource* resource, LlingLatticeValue** out_value);
LLING_API void lling_lattice_free(LlingLatticeValue* value);
LLING_API LlingStatus lling_lattice_domain_id(
    const LlingLatticeValue* value, VtInterfaceId* out_domain);
LLING_API LlingStatus lling_lattice_flags(
    const LlingLatticeValue* value, uint64_t* out_flags);
LLING_API LlingStatus lling_lattice_join(
    const LlingLatticeValue* left, const LlingLatticeValue* right,
    LlingLatticeValue** out_value);
LLING_API LlingStatus lling_lattice_meet(
    const LlingLatticeValue* left, const LlingLatticeValue* right,
    LlingLatticeValue** out_value);
LLING_API LlingStatus lling_lattice_equal(
    const LlingLatticeValue* left, const LlingLatticeValue* right,
    uint8_t* out_equal);
LLING_API LlingStatus lling_lattice_stable_bytes(
    const LlingLatticeValue* value, uint8_t* out_bytes, size_t capacity,
    size_t* out_written, size_t* out_required);
LLING_API LlingStatus lling_lattice_diagnostic(
    const LlingLatticeValue* value, uint8_t* out_bytes, size_t capacity,
    size_t* out_written, size_t* out_required);
LLING_API LlingStatus lling_lattice_join_many(
    const LlingLatticeValue* receiver,
    const LlingLatticeValue* const* others, size_t count,
    LlingLatticeValue** out_value);
LLING_API LlingStatus lling_lattice_meet_many(
    const LlingLatticeValue* receiver,
    const LlingLatticeValue* const* others, size_t count,
    LlingLatticeValue** out_value);
LLING_API LlingStatus lling_lattice_validate_laws(
    const LlingLatticeValue* const* values, size_t count);
LLING_API LlingStatus lling_wfst_builder_new(LlingWfstBuilder** out_builder);
LLING_API void lling_wfst_builder_free(LlingWfstBuilder* builder);
LLING_API LlingStatus lling_wfst_builder_reserve_states(
    LlingWfstBuilder* builder, size_t additional);
LLING_API LlingStatus lling_wfst_builder_add_state(
    LlingWfstBuilder* builder, uint32_t* out_state);
LLING_API LlingStatus lling_wfst_builder_set_start(
    LlingWfstBuilder* builder, uint32_t state);
LLING_API LlingStatus lling_wfst_builder_set_final(
    LlingWfstBuilder* builder, uint32_t state, double weight);
LLING_API LlingStatus lling_wfst_builder_clear_final(
    LlingWfstBuilder* builder, uint32_t state);
LLING_API LlingStatus lling_wfst_builder_add_arc(
    LlingWfstBuilder* builder, uint32_t from,
    uint64_t input_label, uint8_t has_input,
    uint64_t output_label, uint8_t has_output,
    uint32_t to, double weight);
LLING_API LlingStatus lling_wfst_builder_build(
    LlingWfstBuilder* builder, LlingWfst** out_wfst);
LLING_API void lling_wfst_free(LlingWfst* wfst);
LLING_API LlingStatus lling_wfst_import(
    VtResource resource, LlingWfst** out_wfst);
/* Pointer form for FFIs that cannot pass C aggregates by value. */
LLING_API LlingStatus lling_wfst_import_ref(
    const VtResource* resource, LlingWfst** out_wfst);
LLING_API LlingStatus lling_wfst_compose(
    VtResource first, VtResource second, LlingWfst** out_wfst);
/* Pointer form for FFIs that cannot pass C aggregates by value. */
LLING_API LlingStatus lling_wfst_compose_refs(
    const VtResource* first, const VtResource* second,
    LlingWfst** out_wfst);
/* On success, out_resource owns one retain. */
LLING_API LlingStatus lling_wfst_resource(
    const LlingWfst* wfst, VtResource* out_resource);
LLING_API void lling_resource_release(VtResource resource);
LLING_API LlingStatus lling_abi_v2_validate_header(
    const LlingAbiV2Header* header, uint32_t required_size,
    uint64_t known_flags);
LLING_API LlingStatus lling_abi_v2_validate_descriptor(
    const LlingWfstDescriptorV2* descriptor,
    uint8_t* out_typed_evidence_allowed);
LLING_API LlingStatus lling_abi_v2_validate_budget(
    const LlingBudgetV2* budget);
LLING_API LlingStatus lling_abi_v2_validate_outcome(
    const LlingOutcomeV2* outcome, uint8_t resource_present,
    uint8_t evidence_present, uint8_t* out_authoritative_exact);
LLING_API LlingStatus lling_abi_v2_identity_matches(
    const LlingWfstDescriptorV2* expected,
    const LlingWfstDescriptorV2* observed, uint8_t* out_matches);
LLING_API LlingStatus lling_cancellation_v2_new(
    LlingCancellationV2** out_cancellation);
LLING_API LlingStatus lling_cancellation_v2_request(
    const LlingCancellationV2* cancellation, uint32_t reason);
LLING_API LlingStatus lling_cancellation_v2_reason(
    const LlingCancellationV2* cancellation, uint32_t* out_reason);
LLING_API LlingStatus lling_cancellation_v2_free(
    LlingCancellationV2** cancellation);

#ifdef __cplusplus
}
#endif

#endif /* LLING_LLANG_H */
