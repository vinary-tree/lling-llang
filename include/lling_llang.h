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
#define LLING_API_REVISION 3u

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

#ifdef __cplusplus
}
#endif

#endif /* LLING_LLANG_H */
