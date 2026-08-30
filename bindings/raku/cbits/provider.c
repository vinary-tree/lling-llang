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
