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
#    define LLING_LLANG_API __declspec(dllexport)
#  elif defined(LLING_LLANG_USING_DLL)
#    define LLING_LLANG_API __declspec(dllimport)
#  else
#    define LLING_LLANG_API
#  endif
#elif defined(__GNUC__) || defined(__clang__)
#  define LLING_LLANG_API __attribute__((visibility("default")))
#else
#  define LLING_LLANG_API
#endif

#ifdef __cplusplus
extern "C" {
#endif

#define LLING_ABI_VERSION 1u
#define LLING_LLANG_API_REVISION 12u
#define LLING_ABI_V2 2u

#define LLING_DESCRIPTOR_SIGNATURE_KNOWN (UINT64_C(1) << 0)
#define LLING_DESCRIPTOR_SNAPSHOT_PRESENT (UINT64_C(1) << 1)
#define LLING_DESCRIPTOR_CONTEXT_PRESENT (UINT64_C(1) << 2)

#define LLING_BUDGET_STATES (UINT64_C(1) << 0)
#define LLING_BUDGET_ARCS (UINT64_C(1) << 1)
#define LLING_BUDGET_BYTES (UINT64_C(1) << 2)
#define LLING_BUDGET_WORK (UINT64_C(1) << 3)

typedef enum LlingLlangStatus {
    LLING_STATUS_OK = 0,
    LLING_STATUS_INVALID_ARGUMENT = 1,
    LLING_STATUS_NULL_POINTER = 2,
    LLING_STATUS_PANIC = 3,
    LLING_STATUS_INCOMPATIBLE_RESOURCE = 4,
    LLING_STATUS_PROVIDER_ERROR = 5,
    LLING_STATUS_LIMIT_EXCEEDED = 6,
    LLING_STATUS_CLOSED = 7,
    LLING_STATUS_NON_CONVERGENT = 8,
    LLING_STATUS_UNSUPPORTED = 9
} LlingLlangStatus;

typedef struct LlingWfstBuilder LlingWfstBuilder;
typedef struct LlingWfst LlingWfst;
typedef struct LlingSemiring LlingSemiring;
typedef struct LlingSemiringWeight LlingSemiringWeight;
typedef struct LlingLatticeValue LlingLatticeValue;
typedef struct LlingCancellationV2 LlingCancellationV2;
typedef struct LlingPathCursor LlingPathCursor;
typedef struct LlingPath LlingPath;
typedef struct LlingGraphCursor LlingGraphCursor;
typedef struct LlingGraph LlingGraph;
typedef struct LlingGraphDistanceCursor LlingGraphDistanceCursor;
typedef struct LlingGraphDistances LlingGraphDistances;
typedef struct LlingRankedPathCursor LlingRankedPathCursor;
typedef struct LlingSamplePathCursor LlingSamplePathCursor;

/* Revision 8: each path walk owns a captured snapshot and explicit bounds. */
typedef struct LlingPathConfig {
    uint32_t struct_size;
    uint32_t version;
    uint64_t max_states;
    uint64_t max_arcs;
    uint64_t max_work;
    uint64_t work_per_call;
    uint64_t max_depth;
    uint64_t max_paths;
} LlingPathConfig;

typedef struct LlingPathStep {
    uint64_t from_state;
    VtWfstArc arc;
} LlingPathStep;

#define LLING_PATH_POLL_PATH 1u
#define LLING_PATH_POLL_PENDING 2u
#define LLING_PATH_POLL_EXHAUSTED 3u
#define LLING_PATH_POLL_TRUNCATED 4u
#define LLING_PATH_POLL_CANCELLED 5u

/* Revision 9: resumable capture of all reachable states, without num_states. */
typedef struct LlingGraphConfig {
    uint32_t struct_size;
    uint32_t version;
    uint64_t max_states;
    uint64_t max_arcs;
    uint64_t max_work;
    uint64_t work_per_call;
} LlingGraphConfig;

typedef struct LlingGraphArc {
    uint64_t target_local;
    VtWfstArc arc;
} LlingGraphArc;

#define LLING_GRAPH_POLL_PENDING 1u
#define LLING_GRAPH_POLL_COMPLETE 2u
#define LLING_GRAPH_POLL_CANCELLED 3u

/* Revision 10: exact, bounded and resumable graph-distance analysis. */
typedef struct LlingGraphDistanceConfig {
    uint32_t struct_size;
    uint32_t version;
    uint64_t max_work;
    uint64_t work_per_call;
} LlingGraphDistanceConfig;

#define LLING_DISTANCE_POLL_PENDING 1u
#define LLING_DISTANCE_POLL_COMPLETE 2u
#define LLING_DISTANCE_POLL_CANCELLED 3u

/* Revision 11: bounded best-first enumeration over a complete graph. */
typedef struct LlingRankedPathConfig {
    uint32_t struct_size;
    uint32_t version;
    uint64_t max_work;
    uint64_t work_per_call;
    uint64_t max_depth;
    uint64_t max_paths;
    uint64_t max_frontier;
} LlingRankedPathConfig;

#define LLING_RANKED_POLL_PATH 1u
#define LLING_RANKED_POLL_PENDING 2u
#define LLING_RANKED_POLL_EXHAUSTED 3u
#define LLING_RANKED_POLL_TRUNCATED 4u
#define LLING_RANKED_POLL_CANCELLED 5u

/* Revision 12: seeded, bounded accepting-path draws. */
typedef struct LlingSamplePathConfig {
    uint32_t struct_size;
    uint32_t version;
    uint64_t max_work;
    uint64_t work_per_call;
    uint64_t max_depth;
    uint64_t max_samples;
    uint32_t strategy;
    uint32_t reserved;
    uint64_t seed;
} LlingSamplePathConfig;

#define LLING_SAMPLE_UNIFORM 1u
#define LLING_SAMPLE_PROPORTIONAL 2u
#define LLING_SAMPLE_POLL_PATH 1u
#define LLING_SAMPLE_POLL_PENDING 2u
#define LLING_SAMPLE_POLL_EXHAUSTED 3u
#define LLING_SAMPLE_POLL_TRUNCATED 4u
#define LLING_SAMPLE_POLL_CANCELLED 5u

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
static_assert(sizeof(LlingPathConfig) == 56, "LlingPathConfig layout drift");
static_assert(sizeof(LlingPathStep) == 48, "LlingPathStep layout drift");
static_assert(sizeof(LlingGraphConfig) == 40, "LlingGraphConfig layout drift");
static_assert(sizeof(LlingGraphArc) == 48, "LlingGraphArc layout drift");
static_assert(sizeof(LlingGraphDistanceConfig) == 24, "LlingGraphDistanceConfig layout drift");
static_assert(sizeof(LlingRankedPathConfig) == 48, "LlingRankedPathConfig layout drift");
static_assert(sizeof(LlingSamplePathConfig) == 56, "LlingSamplePathConfig layout drift");
#elif defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(LlingAbiV2Header) == 24, "LlingAbiV2Header layout drift");
_Static_assert(sizeof(LlingId128) == 16, "LlingId128 layout drift");
_Static_assert(_Alignof(LlingId128) == 1, "LlingId128 alignment drift");
_Static_assert(sizeof(LlingDigest256) == 32, "LlingDigest256 layout drift");
_Static_assert(_Alignof(LlingDigest256) == 1, "LlingDigest256 alignment drift");
_Static_assert(sizeof(LlingWfstDescriptorV2) == 120, "LlingWfstDescriptorV2 layout drift");
_Static_assert(sizeof(LlingBudgetV2) == 72, "LlingBudgetV2 layout drift");
_Static_assert(sizeof(LlingOutcomeV2) == 96, "LlingOutcomeV2 layout drift");
_Static_assert(sizeof(LlingPathConfig) == 56, "LlingPathConfig layout drift");
_Static_assert(sizeof(LlingPathStep) == 48, "LlingPathStep layout drift");
_Static_assert(sizeof(LlingGraphConfig) == 40, "LlingGraphConfig layout drift");
_Static_assert(sizeof(LlingGraphArc) == 48, "LlingGraphArc layout drift");
_Static_assert(sizeof(LlingGraphDistanceConfig) == 24, "LlingGraphDistanceConfig layout drift");
_Static_assert(sizeof(LlingRankedPathConfig) == 48, "LlingRankedPathConfig layout drift");
_Static_assert(sizeof(LlingSamplePathConfig) == 56, "LlingSamplePathConfig layout drift");
#endif

LLING_LLANG_API uint32_t lling_abi_version(void);
LLING_LLANG_API uint32_t lling_llang_api_revision(void);
LLING_LLANG_API const char* lling_last_error_message(void);
/* Host-defined dynamic semiring consumer. The context retains resource. */
LLING_LLANG_API LlingLlangStatus lling_semiring_open(
    const VtResource* resource, LlingSemiring** out_semiring);
LLING_LLANG_API void lling_semiring_free(LlingSemiring* semiring);
LLING_LLANG_API void lling_semiring_weight_free(LlingSemiringWeight* weight);
LLING_LLANG_API LlingLlangStatus lling_semiring_properties(
    const LlingSemiring* semiring, uint64_t* out_properties);
LLING_LLANG_API LlingLlangStatus lling_semiring_zero(
    const LlingSemiring* semiring, LlingSemiringWeight** out_weight);
LLING_LLANG_API LlingLlangStatus lling_semiring_one(
    const LlingSemiring* semiring, LlingSemiringWeight** out_weight);
LLING_LLANG_API LlingLlangStatus lling_semiring_weight_clone(
    const LlingSemiringWeight* weight, LlingSemiringWeight** out_weight);
LLING_LLANG_API LlingLlangStatus lling_semiring_plus(
    const LlingSemiring* semiring, const LlingSemiringWeight* left,
    const LlingSemiringWeight* right, LlingSemiringWeight** out_weight);
LLING_LLANG_API LlingLlangStatus lling_semiring_times(
    const LlingSemiring* semiring, const LlingSemiringWeight* left,
    const LlingSemiringWeight* right, LlingSemiringWeight** out_weight);
LLING_LLANG_API LlingLlangStatus lling_semiring_equal(
    const LlingSemiring* semiring, const LlingSemiringWeight* left,
    const LlingSemiringWeight* right, uint8_t* out_equal);
LLING_LLANG_API LlingLlangStatus lling_semiring_approx_equal(
    const LlingSemiring* semiring, const LlingSemiringWeight* left,
    const LlingSemiringWeight* right, double epsilon, uint8_t* out_equal);
LLING_LLANG_API LlingLlangStatus lling_semiring_natural_order(
    const LlingSemiring* semiring, const LlingSemiringWeight* left,
    const LlingSemiringWeight* right, int32_t* out_order);
LLING_LLANG_API LlingLlangStatus lling_semiring_divide(
    const LlingSemiring* semiring, const LlingSemiringWeight* dividend,
    const LlingSemiringWeight* divisor, LlingSemiringWeight** out_weight,
    uint8_t* out_defined);
LLING_LLANG_API LlingLlangStatus lling_semiring_left_divide(
    const LlingSemiring* semiring, const LlingSemiringWeight* value,
    const LlingSemiringWeight* divisor, LlingSemiringWeight** out_weight,
    uint8_t* out_defined);
LLING_LLANG_API LlingLlangStatus lling_semiring_star(
    const LlingSemiring* semiring, const LlingSemiringWeight* value,
    LlingSemiringWeight** out_weight, uint8_t* out_defined);
LLING_LLANG_API LlingLlangStatus lling_semiring_numerical_value(
    const LlingSemiring* semiring, const LlingSemiringWeight* value,
    double* out_value);
LLING_LLANG_API LlingLlangStatus lling_semiring_quantize(
    const LlingSemiring* semiring, const LlingSemiringWeight* value,
    double epsilon, int64_t* out_value);
LLING_LLANG_API LlingLlangStatus lling_semiring_to_probability(
    const LlingSemiring* semiring, const LlingSemiringWeight* value,
    double* out_value);
LLING_LLANG_API LlingLlangStatus lling_semiring_closure_bound(
    const LlingSemiring* semiring, size_t* out_bound, uint8_t* out_known);
LLING_LLANG_API LlingLlangStatus lling_semiring_stable_bytes(
    const LlingSemiring* semiring, const LlingSemiringWeight* value,
    uint8_t* out_bytes, size_t capacity, size_t* out_written,
    size_t* out_required);
LLING_LLANG_API LlingLlangStatus lling_semiring_diagnostic(
    const LlingSemiring* semiring, const LlingSemiringWeight* value,
    uint8_t* out_bytes, size_t capacity, size_t* out_written,
    size_t* out_required);
LLING_LLANG_API LlingLlangStatus lling_semiring_plus_many(
    const LlingSemiring* semiring,
    const LlingSemiringWeight* const* weights, size_t count,
    LlingSemiringWeight** out_weight);
LLING_LLANG_API LlingLlangStatus lling_semiring_times_many(
    const LlingSemiring* semiring,
    const LlingSemiringWeight* const* weights, size_t count,
    LlingSemiringWeight** out_weight);
LLING_LLANG_API LlingLlangStatus lling_semiring_validate_laws(
    const LlingSemiring* semiring,
    const LlingSemiringWeight* const* weights, size_t count, double epsilon);
/* Validated, same-thread consumer for immutable vt.lattice.val.1 values. */
LLING_LLANG_API LlingLlangStatus lling_lattice_open(
    const VtResource* resource, LlingLatticeValue** out_value);
LLING_LLANG_API void lling_lattice_free(LlingLatticeValue* value);
LLING_LLANG_API LlingLlangStatus lling_lattice_domain_id(
    const LlingLatticeValue* value, VtInterfaceId* out_domain);
LLING_LLANG_API LlingLlangStatus lling_lattice_flags(
    const LlingLatticeValue* value, uint64_t* out_flags);
LLING_LLANG_API LlingLlangStatus lling_lattice_join(
    const LlingLatticeValue* left, const LlingLatticeValue* right,
    LlingLatticeValue** out_value);
LLING_LLANG_API LlingLlangStatus lling_lattice_meet(
    const LlingLatticeValue* left, const LlingLatticeValue* right,
    LlingLatticeValue** out_value);
LLING_LLANG_API LlingLlangStatus lling_lattice_equal(
    const LlingLatticeValue* left, const LlingLatticeValue* right,
    uint8_t* out_equal);
LLING_LLANG_API LlingLlangStatus lling_lattice_stable_bytes(
    const LlingLatticeValue* value, uint8_t* out_bytes, size_t capacity,
    size_t* out_written, size_t* out_required);
LLING_LLANG_API LlingLlangStatus lling_lattice_diagnostic(
    const LlingLatticeValue* value, uint8_t* out_bytes, size_t capacity,
    size_t* out_written, size_t* out_required);
LLING_LLANG_API LlingLlangStatus lling_lattice_join_many(
    const LlingLatticeValue* receiver,
    const LlingLatticeValue* const* others, size_t count,
    LlingLatticeValue** out_value);
LLING_LLANG_API LlingLlangStatus lling_lattice_meet_many(
    const LlingLatticeValue* receiver,
    const LlingLatticeValue* const* others, size_t count,
    LlingLatticeValue** out_value);
LLING_LLANG_API LlingLlangStatus lling_lattice_validate_laws(
    const LlingLatticeValue* const* values, size_t count);
LLING_LLANG_API LlingLlangStatus lling_wfst_builder_new(LlingWfstBuilder** out_builder);
/* Domain discriminants are VtUnitDomain and VtWeightDomain wire values. */
LLING_LLANG_API LlingLlangStatus lling_wfst_builder_new_for_domains(
    uint32_t unit_domain, uint32_t weight_domain,
    LlingWfstBuilder** out_builder);
LLING_LLANG_API void lling_wfst_builder_free(LlingWfstBuilder* builder);
LLING_LLANG_API LlingLlangStatus lling_wfst_builder_reserve_states(
    LlingWfstBuilder* builder, size_t additional);
LLING_LLANG_API LlingLlangStatus lling_wfst_builder_add_state(
    LlingWfstBuilder* builder, uint32_t* out_state);
LLING_LLANG_API LlingLlangStatus lling_wfst_builder_set_start(
    LlingWfstBuilder* builder, uint32_t state);
LLING_LLANG_API LlingLlangStatus lling_wfst_builder_set_final(
    LlingWfstBuilder* builder, uint32_t state, double weight);
LLING_LLANG_API LlingLlangStatus lling_wfst_builder_clear_final(
    LlingWfstBuilder* builder, uint32_t state);
LLING_LLANG_API LlingLlangStatus lling_wfst_builder_add_arc(
    LlingWfstBuilder* builder, uint32_t from,
    uint64_t input_label, uint8_t has_input,
    uint64_t output_label, uint8_t has_output,
    uint32_t to, double weight);
LLING_LLANG_API LlingLlangStatus lling_wfst_builder_build(
    LlingWfstBuilder* builder, LlingWfst** out_wfst);
LLING_LLANG_API void lling_wfst_free(LlingWfst* wfst);
LLING_LLANG_API LlingLlangStatus lling_wfst_import(
    VtResource resource, LlingWfst** out_wfst);
/* Pointer form for FFIs that cannot pass C aggregates by value. */
LLING_LLANG_API LlingLlangStatus lling_wfst_import_ref(
    const VtResource* resource, LlingWfst** out_wfst);
LLING_LLANG_API LlingLlangStatus lling_wfst_compose(
    VtResource first, VtResource second, LlingWfst** out_wfst);
/* Pointer form for FFIs that cannot pass C aggregates by value. */
LLING_LLANG_API LlingLlangStatus lling_wfst_compose_refs(
    const VtResource* first, const VtResource* second,
    LlingWfst** out_wfst);
/* On success, out_resource owns one retain. */
LLING_LLANG_API LlingLlangStatus lling_wfst_resource(
    const LlingWfst* wfst, VtResource* out_resource);
LLING_LLANG_API void lling_resource_release(VtResource resource);
LLING_LLANG_API LlingLlangStatus lling_abi_v2_validate_header(
    const LlingAbiV2Header* header, uint32_t required_size,
    uint64_t known_flags);
LLING_LLANG_API LlingLlangStatus lling_abi_v2_validate_descriptor(
    const LlingWfstDescriptorV2* descriptor,
    uint8_t* out_typed_evidence_allowed);
LLING_LLANG_API LlingLlangStatus lling_abi_v2_validate_budget(
    const LlingBudgetV2* budget);
LLING_LLANG_API LlingLlangStatus lling_abi_v2_validate_outcome(
    const LlingOutcomeV2* outcome, uint8_t resource_present,
    uint8_t evidence_present, uint8_t* out_authoritative_exact);
LLING_LLANG_API LlingLlangStatus lling_abi_v2_identity_matches(
    const LlingWfstDescriptorV2* expected,
    const LlingWfstDescriptorV2* observed, uint8_t* out_matches);
LLING_LLANG_API LlingLlangStatus lling_cancellation_v2_new(
    LlingCancellationV2** out_cancellation);
LLING_LLANG_API LlingLlangStatus lling_cancellation_v2_request(
    const LlingCancellationV2* cancellation, uint32_t reason);
LLING_LLANG_API LlingLlangStatus lling_cancellation_v2_reason(
    const LlingCancellationV2* cancellation, uint32_t* out_reason);
LLING_LLANG_API LlingLlangStatus lling_cancellation_v2_free(
    LlingCancellationV2** cancellation);
/* Revision 8 path traversal. Every cursor owns an immutable snapshot. */
LLING_LLANG_API LlingLlangStatus lling_path_cursor_open(
    const VtResource* resource, const LlingPathConfig* config,
    LlingPathCursor** out_cursor);
LLING_LLANG_API LlingLlangStatus lling_path_cursor_next(
    LlingPathCursor* cursor, const LlingCancellationV2* cancellation,
    uint32_t* out_poll, LlingPath** out_path);
LLING_LLANG_API void lling_path_cursor_free(LlingPathCursor* cursor);
LLING_LLANG_API LlingLlangStatus lling_path_info(
    const LlingPath* path, uint64_t* out_final_state, double* out_weight,
    size_t* out_step_count);
LLING_LLANG_API LlingLlangStatus lling_path_steps(
    const LlingPath* path, size_t offset, LlingPathStep* out_steps,
    size_t capacity, size_t* out_written, size_t* out_total);
LLING_LLANG_API void lling_path_free(LlingPath* path);
/* Revision 9 graph capture. A complete graph is independent of its snapshot. */
LLING_LLANG_API LlingLlangStatus lling_graph_cursor_open(
    const VtResource* resource, const LlingGraphConfig* config,
    LlingGraphCursor** out_cursor);
LLING_LLANG_API LlingLlangStatus lling_graph_cursor_next(
    LlingGraphCursor* cursor, const LlingCancellationV2* cancellation,
    uint32_t* out_poll);
LLING_LLANG_API LlingLlangStatus lling_graph_cursor_take(
    LlingGraphCursor* cursor, LlingGraph** out_graph);
LLING_LLANG_API void lling_graph_cursor_free(LlingGraphCursor* cursor);
LLING_LLANG_API LlingLlangStatus lling_graph_info(
    const LlingGraph* graph, uint32_t* out_unit_domain,
    uint32_t* out_weight_domain, uint64_t* out_start_raw,
    size_t* out_state_count, size_t* out_arc_count);
LLING_LLANG_API LlingLlangStatus lling_graph_state(
    const LlingGraph* graph, size_t local_id, uint64_t* out_raw_id,
    uint8_t* out_is_final, double* out_final_weight,
    size_t* out_arc_count);
LLING_LLANG_API LlingLlangStatus lling_graph_arcs(
    const LlingGraph* graph, size_t local_id, size_t offset,
    LlingGraphArc* out_arcs, size_t capacity,
    size_t* out_written, size_t* out_total);
LLING_LLANG_API void lling_graph_free(LlingGraph* graph);
/* A distance cursor retains the complete graph independently of its handle. */
LLING_LLANG_API LlingLlangStatus lling_graph_distance_open(
    const LlingGraph* graph, const LlingGraphDistanceConfig* config,
    LlingGraphDistanceCursor** out_cursor);
LLING_LLANG_API LlingLlangStatus lling_graph_distance_next(
    LlingGraphDistanceCursor* cursor, const LlingCancellationV2* cancellation,
    uint32_t* out_poll);
LLING_LLANG_API LlingLlangStatus lling_graph_distance_take(
    LlingGraphDistanceCursor* cursor, LlingGraphDistances** out_result);
LLING_LLANG_API void lling_graph_distance_cursor_free(LlingGraphDistanceCursor* cursor);
LLING_LLANG_API LlingLlangStatus lling_graph_distance_info(
    const LlingGraphDistances* result, double* out_total_weight,
    size_t* out_state_count);
LLING_LLANG_API LlingLlangStatus lling_graph_distance_page(
    const LlingGraphDistances* result, size_t offset,
    double* out_forward, double* out_backward, size_t capacity,
    size_t* out_written, size_t* out_total);
LLING_LLANG_API void lling_graph_distance_free(LlingGraphDistances* result);
LLING_LLANG_API LlingLlangStatus lling_graph_posterior_arcs(
    const LlingGraphDistances* result, size_t local_id, size_t offset,
    double* out_probabilities, size_t capacity,
    size_t* out_written, size_t* out_total);
LLING_LLANG_API LlingLlangStatus lling_graph_posterior_final(
    const LlingGraphDistances* result, size_t local_id,
    double* out_probability);
/* The ranked cursor retains its own graph lease. Each call has bounded work. */
LLING_LLANG_API LlingLlangStatus lling_ranked_path_cursor_open(
    const LlingGraph* graph, const LlingRankedPathConfig* config,
    LlingRankedPathCursor** out_cursor);
LLING_LLANG_API LlingLlangStatus lling_ranked_path_cursor_next(
    LlingRankedPathCursor* cursor, const LlingCancellationV2* cancellation,
    uint32_t* out_poll, LlingPath** out_path);
LLING_LLANG_API void lling_ranked_path_cursor_free(LlingRankedPathCursor* cursor);
/* Conditional draws use exact backward masses from the complete graph. */
LLING_LLANG_API LlingLlangStatus lling_sample_path_cursor_open(
    const LlingGraph* graph, const LlingSamplePathConfig* config,
    LlingSamplePathCursor** out_cursor);
LLING_LLANG_API LlingLlangStatus lling_sample_path_cursor_next(
    LlingSamplePathCursor* cursor, const LlingCancellationV2* cancellation,
    uint32_t* out_poll, LlingPath** out_path);
LLING_LLANG_API void lling_sample_path_cursor_free(LlingSamplePathCursor* cursor);

#ifdef __cplusplus
}
#endif

#endif /* LLING_LLANG_H */
