#include "vinary_tree_interop.h"

#include <stdatomic.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

#if defined(_WIN32)
#define LLING_RAKU_API __declspec(dllexport)
#else
#define LLING_RAKU_API
#endif

typedef void (*LlingRakuDrop)(void *host_context);
typedef VtStatus (*LlingRakuStart)(void *host_context, uint64_t *out_state);
typedef VtStatus (*LlingRakuCount)(void *host_context, size_t *out_count,
                                  uint8_t *out_known);
typedef VtStatus (*LlingRakuStateInfo)(void *host_context, uint64_t state,
                                      uint8_t *out_valid,
                                      uint8_t *out_is_final,
                                      double *out_final_weight);
typedef VtStatus (*LlingRakuStateArcs)(void *host_context, uint64_t state,
                                      size_t start, VtWfstArc *out_arcs,
                                      size_t capacity, size_t *out_written,
                                      size_t *out_total);

typedef struct LlingRakuCallbacks {
    LlingRakuDrop drop;
    LlingRakuStart start;
    LlingRakuCount count;
    LlingRakuStateInfo state_info;
    LlingRakuStateArcs state_arcs;
} LlingRakuCallbacks;

static LlingRakuCallbacks CALLBACKS;
static atomic_bool CALLBACKS_CONFIGURED;
static atomic_flag CALLBACKS_LOCK = ATOMIC_FLAG_INIT;

typedef struct LlingRakuProvider {
    atomic_size_t references;
    void *host_context;
    LlingRakuDrop drop;
    LlingRakuStart start;
    LlingRakuCount count;
    LlingRakuStateInfo state_info;
    LlingRakuStateArcs state_arcs;
    VtWfstVTable wfst;
} LlingRakuProvider;

static const VtResourceVTable RESOURCE_VTABLE;

static void provider_retain(void *raw_context) {
    LlingRakuProvider *context = raw_context;
    (void)atomic_fetch_add_explicit(&context->references, 1,
                                    memory_order_relaxed);
}

static void provider_release(void *raw_context) {
    LlingRakuProvider *context = raw_context;
    if (atomic_fetch_sub_explicit(&context->references, 1,
                                  memory_order_acq_rel) == 1) {
        context->drop(context->host_context);
        free(context);
    }
}

static VtStatus provider_query(void *raw_context,
                               const VtInterfaceId *interface_id,
                               uint32_t minimum_version,
                               const void **out_vtable) {
    if (raw_context == NULL || interface_id == NULL || out_vtable == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    LlingRakuProvider *context = raw_context;
    if (memcmp(interface_id, &VT_WFST_INTERFACE_ID,
               sizeof(*interface_id)) != 0 ||
        minimum_version > VT_WFST_INTERFACE_VERSION) {
        return VT_STATUS_UNSUPPORTED;
    }
    *out_vtable = &context->wfst;
    return VT_STATUS_OK;
}

static VtStatus provider_snapshot(void *raw_context,
                                  VtResource *out_snapshot) {
    if (raw_context == NULL || out_snapshot == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    provider_retain(raw_context);
    out_snapshot->context = raw_context;
    out_snapshot->vtable = &RESOURCE_VTABLE;
    return VT_STATUS_OK;
}

static VtStatus provider_start(void *raw_context, uint64_t *out_state) {
    if (raw_context == NULL || out_state == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    LlingRakuProvider *context = raw_context;
    return context->start(context->host_context, out_state);
}

static VtStatus provider_count(void *raw_context, size_t *out_count,
                               uint8_t *out_known) {
    if (raw_context == NULL || out_count == NULL || out_known == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    LlingRakuProvider *context = raw_context;
    return context->count(context->host_context, out_count, out_known);
}

static VtStatus provider_state_info(void *raw_context, uint64_t state,
                                    uint8_t *out_valid,
                                    uint8_t *out_is_final,
                                    double *out_final_weight) {
    if (raw_context == NULL || out_valid == NULL || out_is_final == NULL ||
        out_final_weight == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    LlingRakuProvider *context = raw_context;
    return context->state_info(context->host_context, state, out_valid,
                               out_is_final, out_final_weight);
}

static VtStatus provider_state_arcs(void *raw_context, uint64_t state,
                                    size_t start, VtWfstArc *out_arcs,
                                    size_t capacity, size_t *out_written,
                                    size_t *out_total) {
    if (raw_context == NULL || out_written == NULL || out_total == NULL ||
        (capacity != 0 && out_arcs == NULL)) {
        return VT_STATUS_NULL_POINTER;
    }
    LlingRakuProvider *context = raw_context;
    return context->state_arcs(context->host_context, state, start, out_arcs,
                               capacity, out_written, out_total);
}

static const VtResourceVTable RESOURCE_VTABLE = {
    sizeof(VtResourceVTable), VT_ABI_VERSION, 0,
    provider_retain, provider_release, provider_query,
};

LLING_RAKU_API VtStatus lling_raku_provider_configure(
    LlingRakuDrop drop, LlingRakuStart start, LlingRakuCount count,
    LlingRakuStateInfo state_info, LlingRakuStateArcs state_arcs) {
    if (drop == NULL || start == NULL || count == NULL ||
        state_info == NULL || state_arcs == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    while (atomic_flag_test_and_set_explicit(&CALLBACKS_LOCK,
                                              memory_order_acquire)) {
    }
    VtStatus status = VT_STATUS_OK;
    if (atomic_load_explicit(&CALLBACKS_CONFIGURED, memory_order_relaxed)) {
        if (CALLBACKS.drop != drop || CALLBACKS.start != start ||
            CALLBACKS.count != count || CALLBACKS.state_info != state_info ||
            CALLBACKS.state_arcs != state_arcs) {
            status = VT_STATUS_BATCH_IN_USE;
        }
    } else {
        CALLBACKS = (LlingRakuCallbacks){
            drop, start, count, state_info, state_arcs,
        };
        atomic_store_explicit(&CALLBACKS_CONFIGURED, true,
                              memory_order_release);
    }
    atomic_flag_clear_explicit(&CALLBACKS_LOCK, memory_order_release);
    return status;
}

LLING_RAKU_API VtStatus lling_raku_provider_create(
    uint32_t unit_domain, uint32_t weight_domain, uint64_t flags,
    void *host_context, VtResource *out_resource) {
    if (host_context == NULL || out_resource == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    if (!atomic_load_explicit(&CALLBACKS_CONFIGURED, memory_order_acquire)) {
        return VT_STATUS_PROVIDER_ERROR;
    }
    LlingRakuProvider *context = calloc(1, sizeof(*context));
    if (context == NULL) {
        return VT_STATUS_IO_ERROR;
    }
    atomic_init(&context->references, 1);
    context->host_context = host_context;
    context->drop = CALLBACKS.drop;
    context->start = CALLBACKS.start;
    context->count = CALLBACKS.count;
    context->state_info = CALLBACKS.state_info;
    context->state_arcs = CALLBACKS.state_arcs;
    context->wfst = (VtWfstVTable){
        sizeof(VtWfstVTable), VT_WFST_INTERFACE_VERSION,
        (VtUnitDomain)unit_domain, (VtWeightDomain)weight_domain, 0, flags,
        provider_snapshot, provider_start, provider_count,
        provider_state_info, provider_state_arcs,
    };
    out_resource->context = context;
    out_resource->vtable = &RESOURCE_VTABLE;
    return VT_STATUS_OK;
}

/* Dynamic-semiring bridge. Raku owns the generation-checked value arena; this
 * shim owns only the reference-counted resource and stable native vtables. */
typedef VtStatus (*LlingRakuSemiringIdentity)(void *, VtSemiringValue *);
typedef VtStatus (*LlingRakuSemiringClone)(void *, const VtSemiringValue *,
                                          VtSemiringValue *);
typedef VtStatus (*LlingRakuSemiringRelease)(void *, VtSemiringValue *, size_t);
typedef VtStatus (*LlingRakuSemiringBinary)(void *, const VtSemiringValue *,
                                           const VtSemiringValue *,
                                           VtSemiringValue *);
typedef VtStatus (*LlingRakuSemiringEqual)(void *, const VtSemiringValue *,
                                          const VtSemiringValue *, uint8_t *);
typedef VtStatus (*LlingRakuSemiringApprox)(void *, const VtSemiringValue *,
                                           const VtSemiringValue *, double,
                                           uint8_t *);
typedef VtStatus (*LlingRakuSemiringOrder)(void *, const VtSemiringValue *,
                                          const VtSemiringValue *, int32_t *);
typedef VtStatus (*LlingRakuSemiringBytes)(void *, const VtSemiringValue *,
                                          uint8_t *, size_t, size_t *, size_t *);
typedef VtStatus (*LlingRakuSemiringMany)(void *, const VtSemiringValue *,
                                         size_t, VtSemiringValue *);
typedef VtStatus (*LlingRakuSemiringUnary)(void *, const VtSemiringValue *,
                                          VtSemiringValue *);
typedef VtStatus (*LlingRakuSemiringNumerical)(void *, const VtSemiringValue *,
                                              double *);
typedef VtStatus (*LlingRakuSemiringQuantize)(void *, const VtSemiringValue *,
                                             double, int64_t *);
typedef VtStatus (*LlingRakuSemiringClosure)(void *, size_t *, uint8_t *);

typedef struct LlingRakuSemiringCallbacks {
    LlingRakuDrop drop;
    LlingRakuSemiringIdentity zero;
    LlingRakuSemiringIdentity one;
    LlingRakuSemiringClone clone_value;
    LlingRakuSemiringRelease release_values;
    LlingRakuSemiringBinary plus;
    LlingRakuSemiringBinary times;
    LlingRakuSemiringEqual equal;
    LlingRakuSemiringApprox approx_equal;
    LlingRakuSemiringOrder natural_order;
    LlingRakuSemiringBytes stable_bytes;
    LlingRakuSemiringBytes diagnostic;
    LlingRakuSemiringMany plus_many;
    LlingRakuSemiringMany times_many;
    LlingRakuSemiringBinary divide;
    LlingRakuSemiringBinary left_divide;
    LlingRakuSemiringUnary star;
    LlingRakuSemiringNumerical numerical_value;
    LlingRakuSemiringQuantize quantize;
    LlingRakuSemiringNumerical to_probability;
    LlingRakuSemiringClosure closure_bound;
} LlingRakuSemiringCallbacks;

static LlingRakuSemiringCallbacks SEMIRING_CALLBACKS;
static atomic_bool SEMIRING_CALLBACKS_CONFIGURED;
static atomic_uint SEMIRING_CALLBACK_GROUPS;
static atomic_flag SEMIRING_CALLBACKS_LOCK = ATOMIC_FLAG_INIT;

typedef struct LlingRakuSemiring {
    atomic_size_t references;
    void *host_context;
    LlingRakuSemiringCallbacks callbacks;
    VtSemiringVTable base;
    VtSemiringDivisionVTable division;
    VtSemiringStarVTable star;
    VtSemiringNumericVTable numeric;
    VtSemiringPropertiesVTable properties;
    uint8_t has_division;
    uint8_t has_star;
    uint8_t has_numeric;
} LlingRakuSemiring;

static const VtResourceVTable SEMIRING_RESOURCE_VTABLE;

static void semiring_retain(void *raw_context) {
    LlingRakuSemiring *context = raw_context;
    (void)atomic_fetch_add_explicit(&context->references, 1,
                                    memory_order_relaxed);
}

static void semiring_release(void *raw_context) {
    LlingRakuSemiring *context = raw_context;
    if (atomic_fetch_sub_explicit(&context->references, 1,
                                  memory_order_acq_rel) == 1) {
        context->callbacks.drop(context->host_context);
        free(context);
    }
}

static VtStatus semiring_query(void *raw_context,
                               const VtInterfaceId *interface_id,
                               uint32_t minimum_version,
                               const void **out_vtable) {
    if (raw_context == NULL || interface_id == NULL || out_vtable == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    LlingRakuSemiring *context = raw_context;
    if (memcmp(interface_id, &VT_SEMIRING_INTERFACE_ID,
               sizeof(*interface_id)) == 0 &&
        minimum_version <= VT_SEMIRING_INTERFACE_VERSION) {
        *out_vtable = &context->base;
    } else if (memcmp(interface_id, &VT_SEMIRING_DIVISION_INTERFACE_ID,
                      sizeof(*interface_id)) == 0 &&
               minimum_version <= VT_SEMIRING_DIVISION_INTERFACE_VERSION &&
               context->has_division != 0) {
        *out_vtable = &context->division;
    } else if (memcmp(interface_id, &VT_SEMIRING_STAR_INTERFACE_ID,
                      sizeof(*interface_id)) == 0 &&
               minimum_version <= VT_SEMIRING_STAR_INTERFACE_VERSION &&
               context->has_star != 0) {
        *out_vtable = &context->star;
    } else if (memcmp(interface_id, &VT_SEMIRING_NUMERIC_INTERFACE_ID,
                      sizeof(*interface_id)) == 0 &&
               minimum_version <= VT_SEMIRING_NUMERIC_INTERFACE_VERSION &&
               context->has_numeric != 0) {
        *out_vtable = &context->numeric;
    } else if (memcmp(interface_id, &VT_SEMIRING_PROPERTIES_INTERFACE_ID,
                      sizeof(*interface_id)) == 0 &&
               minimum_version <= VT_SEMIRING_PROPERTIES_INTERFACE_VERSION) {
        *out_vtable = &context->properties;
    } else {
        return VT_STATUS_UNSUPPORTED;
    }
    return VT_STATUS_OK;
}

#define SEMIRING_CONTEXT(raw) LlingRakuSemiring *context = (raw)
#define FORWARD_IDENTITY(name, field)                                           \
    static VtStatus name(void *raw, VtSemiringValue *out) {                    \
        SEMIRING_CONTEXT(raw);                                                  \
        return context->callbacks.field(context->host_context, out);            \
    }
#define FORWARD_BINARY(name, field)                                             \
    static VtStatus name(void *raw, const VtSemiringValue *left,                \
                         const VtSemiringValue *right, VtSemiringValue *out) {   \
        SEMIRING_CONTEXT(raw);                                                  \
        return context->callbacks.field(context->host_context, left, right, out);\
    }

FORWARD_IDENTITY(semiring_zero, zero)
FORWARD_IDENTITY(semiring_one, one)
FORWARD_BINARY(semiring_plus, plus)
FORWARD_BINARY(semiring_times, times)
FORWARD_BINARY(semiring_divide, divide)
FORWARD_BINARY(semiring_left_divide, left_divide)

static VtStatus semiring_clone_value(void *raw, const VtSemiringValue *value,
                                     VtSemiringValue *out) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.clone_value(context->host_context, value, out);
}
static VtStatus semiring_release_values(void *raw, VtSemiringValue *values,
                                        size_t count) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.release_values(context->host_context, values, count);
}
static VtStatus semiring_equal(void *raw, const VtSemiringValue *left,
                               const VtSemiringValue *right, uint8_t *out) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.equal(context->host_context, left, right, out);
}
static VtStatus semiring_approx_equal(void *raw, const VtSemiringValue *left,
                                      const VtSemiringValue *right,
                                      double epsilon, uint8_t *out) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.approx_equal(context->host_context, left, right,
                                           epsilon, out);
}
static VtStatus semiring_natural_order(void *raw,
                                       const VtSemiringValue *left,
                                       const VtSemiringValue *right,
                                       int32_t *out) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.natural_order(context->host_context, left, right, out);
}
static VtStatus semiring_stable_bytes(void *raw, const VtSemiringValue *value,
                                      uint8_t *out, size_t capacity,
                                      size_t *written, size_t *required) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.stable_bytes(context->host_context, value, out,
                                           capacity, written, required);
}
static VtStatus semiring_diagnostic(void *raw, const VtSemiringValue *value,
                                    uint8_t *out, size_t capacity,
                                    size_t *written, size_t *required) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.diagnostic(context->host_context, value, out,
                                         capacity, written, required);
}
static VtStatus semiring_plus_many(void *raw, const VtSemiringValue *values,
                                   size_t count, VtSemiringValue *out) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.plus_many(context->host_context, values, count, out);
}
static VtStatus semiring_times_many(void *raw, const VtSemiringValue *values,
                                    size_t count, VtSemiringValue *out) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.times_many(context->host_context, values, count, out);
}
static VtStatus semiring_star(void *raw, const VtSemiringValue *value,
                              VtSemiringValue *out) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.star(context->host_context, value, out);
}
static VtStatus semiring_numerical_value(void *raw,
                                         const VtSemiringValue *value,
                                         double *out) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.numerical_value(context->host_context, value, out);
}
static VtStatus semiring_quantize(void *raw, const VtSemiringValue *value,
                                  double epsilon, int64_t *out) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.quantize(context->host_context, value, epsilon, out);
}
static VtStatus semiring_to_probability(void *raw,
                                        const VtSemiringValue *value,
                                        double *out) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.to_probability(context->host_context, value, out);
}
static VtStatus semiring_closure_bound(void *raw, size_t *out,
                                       uint8_t *known) {
    SEMIRING_CONTEXT(raw);
    return context->callbacks.closure_bound(context->host_context, out, known);
}

static const VtResourceVTable SEMIRING_RESOURCE_VTABLE = {
    sizeof(VtResourceVTable), VT_ABI_VERSION, 0,
    semiring_retain, semiring_release, semiring_query,
};

static VtStatus semiring_configure_group(unsigned int bit) {
    unsigned int groups = atomic_load_explicit(&SEMIRING_CALLBACK_GROUPS,
                                                memory_order_relaxed);
    if ((groups & bit) != 0) {
        return VT_STATUS_BATCH_IN_USE;
    }
    groups |= bit;
    atomic_store_explicit(&SEMIRING_CALLBACK_GROUPS, groups,
                          memory_order_release);
    return VT_STATUS_OK;
}

static void semiring_publish_if_complete(void) {
    if (atomic_load_explicit(&SEMIRING_CALLBACK_GROUPS,
                             memory_order_relaxed) == UINT32_C(31)) {
        atomic_store_explicit(&SEMIRING_CALLBACKS_CONFIGURED, true,
                              memory_order_release);
    }
}

LLING_RAKU_API VtStatus lling_raku_semiring_configure_lifecycle(
    LlingRakuDrop drop, LlingRakuSemiringIdentity zero,
    LlingRakuSemiringIdentity one, LlingRakuSemiringClone clone_value,
    LlingRakuSemiringRelease release_values) {
    if (drop == NULL || zero == NULL || one == NULL || clone_value == NULL ||
        release_values == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    while (atomic_flag_test_and_set_explicit(&SEMIRING_CALLBACKS_LOCK,
                                              memory_order_acquire)) {
    }
    VtStatus status = semiring_configure_group(UINT32_C(1));
    if (status == VT_STATUS_OK) {
        SEMIRING_CALLBACKS.drop = drop;
        SEMIRING_CALLBACKS.zero = zero;
        SEMIRING_CALLBACKS.one = one;
        SEMIRING_CALLBACKS.clone_value = clone_value;
        SEMIRING_CALLBACKS.release_values = release_values;
        semiring_publish_if_complete();
    }
    atomic_flag_clear_explicit(&SEMIRING_CALLBACKS_LOCK, memory_order_release);
    return status;
}

LLING_RAKU_API VtStatus lling_raku_semiring_configure_algebra(
    LlingRakuSemiringBinary plus, LlingRakuSemiringBinary times,
    LlingRakuSemiringEqual equal, LlingRakuSemiringApprox approx_equal,
    LlingRakuSemiringOrder natural_order) {
    if (plus == NULL || times == NULL || equal == NULL || approx_equal == NULL ||
        natural_order == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    while (atomic_flag_test_and_set_explicit(&SEMIRING_CALLBACKS_LOCK,
                                              memory_order_acquire)) {
    }
    VtStatus status = semiring_configure_group(UINT32_C(2));
    if (status == VT_STATUS_OK) {
        SEMIRING_CALLBACKS.plus = plus;
        SEMIRING_CALLBACKS.times = times;
        SEMIRING_CALLBACKS.equal = equal;
        SEMIRING_CALLBACKS.approx_equal = approx_equal;
        SEMIRING_CALLBACKS.natural_order = natural_order;
        semiring_publish_if_complete();
    }
    atomic_flag_clear_explicit(&SEMIRING_CALLBACKS_LOCK, memory_order_release);
    return status;
}

LLING_RAKU_API VtStatus lling_raku_semiring_configure_buffers(
    LlingRakuSemiringBytes stable_bytes, LlingRakuSemiringBytes diagnostic,
    LlingRakuSemiringMany plus_many, LlingRakuSemiringMany times_many) {
    if (stable_bytes == NULL || diagnostic == NULL || plus_many == NULL ||
        times_many == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    while (atomic_flag_test_and_set_explicit(&SEMIRING_CALLBACKS_LOCK,
                                              memory_order_acquire)) {
    }
    VtStatus status = semiring_configure_group(UINT32_C(4));
    if (status == VT_STATUS_OK) {
        SEMIRING_CALLBACKS.stable_bytes = stable_bytes;
        SEMIRING_CALLBACKS.diagnostic = diagnostic;
        SEMIRING_CALLBACKS.plus_many = plus_many;
        SEMIRING_CALLBACKS.times_many = times_many;
        semiring_publish_if_complete();
    }
    atomic_flag_clear_explicit(&SEMIRING_CALLBACKS_LOCK, memory_order_release);
    return status;
}

LLING_RAKU_API VtStatus lling_raku_semiring_configure_optional(
    LlingRakuSemiringBinary divide, LlingRakuSemiringBinary left_divide,
    LlingRakuSemiringUnary star, LlingRakuSemiringNumerical numerical_value,
    LlingRakuSemiringNumerical to_probability) {
    if (divide == NULL || left_divide == NULL || star == NULL ||
        numerical_value == NULL || to_probability == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    while (atomic_flag_test_and_set_explicit(&SEMIRING_CALLBACKS_LOCK,
                                              memory_order_acquire)) {
    }
    VtStatus status = semiring_configure_group(UINT32_C(8));
    if (status == VT_STATUS_OK) {
        SEMIRING_CALLBACKS.divide = divide;
        SEMIRING_CALLBACKS.left_divide = left_divide;
        SEMIRING_CALLBACKS.star = star;
        SEMIRING_CALLBACKS.numerical_value = numerical_value;
        SEMIRING_CALLBACKS.to_probability = to_probability;
        semiring_publish_if_complete();
    }
    atomic_flag_clear_explicit(&SEMIRING_CALLBACKS_LOCK, memory_order_release);
    return status;
}

LLING_RAKU_API VtStatus lling_raku_semiring_configure_metadata(
    LlingRakuSemiringQuantize quantize, LlingRakuSemiringClosure closure_bound) {
    if (quantize == NULL || closure_bound == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    while (atomic_flag_test_and_set_explicit(&SEMIRING_CALLBACKS_LOCK,
                                              memory_order_acquire)) {
    }
    VtStatus status = semiring_configure_group(UINT32_C(16));
    if (status == VT_STATUS_OK) {
        SEMIRING_CALLBACKS.quantize = quantize;
        SEMIRING_CALLBACKS.closure_bound = closure_bound;
        semiring_publish_if_complete();
    }
    atomic_flag_clear_explicit(&SEMIRING_CALLBACKS_LOCK, memory_order_release);
    return status;
}

LLING_RAKU_API VtStatus lling_raku_semiring_create(
    uint64_t flags, const VtInterfaceId *domain_id, uint64_t properties,
    uint8_t has_division, uint8_t has_star, uint8_t has_numeric,
    void *host_context, VtResource *out_resource) {
    if (domain_id == NULL || host_context == NULL || out_resource == NULL) {
        return VT_STATUS_NULL_POINTER;
    }
    if (!atomic_load_explicit(&SEMIRING_CALLBACKS_CONFIGURED,
                              memory_order_acquire)) {
        return VT_STATUS_PROVIDER_ERROR;
    }
    LlingRakuSemiring *context = calloc(1, sizeof(*context));
    if (context == NULL) {
        return VT_STATUS_IO_ERROR;
    }
    atomic_init(&context->references, 1);
    context->host_context = host_context;
    context->callbacks = SEMIRING_CALLBACKS;
    context->has_division = has_division;
    context->has_star = has_star;
    context->has_numeric = has_numeric;
    context->base = (VtSemiringVTable){
        sizeof(VtSemiringVTable), VT_SEMIRING_INTERFACE_VERSION, 0, flags,
        *domain_id, semiring_zero, semiring_one, semiring_clone_value,
        semiring_release_values, semiring_plus, semiring_times, semiring_equal,
        semiring_approx_equal, semiring_natural_order, semiring_stable_bytes,
        semiring_diagnostic,
        (flags & VT_SEMIRING_FLAG_BATCH) != 0 ? semiring_plus_many : NULL,
        (flags & VT_SEMIRING_FLAG_BATCH) != 0 ? semiring_times_many : NULL,
    };
    context->division = (VtSemiringDivisionVTable){
        sizeof(VtSemiringDivisionVTable), VT_SEMIRING_DIVISION_INTERFACE_VERSION,
        0, semiring_divide, semiring_left_divide,
    };
    context->star = (VtSemiringStarVTable){
        sizeof(VtSemiringStarVTable), VT_SEMIRING_STAR_INTERFACE_VERSION, 0,
        semiring_star,
    };
    context->numeric = (VtSemiringNumericVTable){
        sizeof(VtSemiringNumericVTable), VT_SEMIRING_NUMERIC_INTERFACE_VERSION, 0,
        semiring_numerical_value, semiring_quantize, semiring_to_probability,
    };
    context->properties = (VtSemiringPropertiesVTable){
        sizeof(VtSemiringPropertiesVTable), VT_SEMIRING_PROPERTIES_INTERFACE_VERSION,
        0, properties, semiring_closure_bound,
    };
    out_resource->context = context;
    out_resource->vtable = &SEMIRING_RESOURCE_VTABLE;
    return VT_STATUS_OK;
}
