/* Build two scalar WFSTs, compose them lazily, inspect the product through the
 * family resource ABI, and release every owned resource. */
#include <inttypes.h>
#include <lling_llang.h>
#include <stdio.h>
#include <stdlib.h>

static void require_ok(LlingLlangStatus status, const char* operation) {
    if (status != LLING_STATUS_OK) {
        fprintf(stderr, "%s failed (%u): %s\n", operation, (unsigned)status,
                lling_last_error_message());
        exit(EXIT_FAILURE);
    }
}

static VtResource single_arc_resource(uint32_t input, uint32_t output,
                                      double weight) {
    LlingWfstBuilder* builder = NULL;
    LlingWfst* wfst = NULL;
    VtResource resource = {NULL, NULL};
    uint32_t q0 = 0;
    uint32_t q1 = 0;

    require_ok(lling_wfst_builder_new(&builder), "builder_new");
    require_ok(lling_wfst_builder_reserve_states(builder, 2), "reserve_states");
    require_ok(lling_wfst_builder_add_state(builder, &q0), "add_state");
    require_ok(lling_wfst_builder_add_state(builder, &q1), "add_state");
    require_ok(lling_wfst_builder_set_start(builder, q0), "set_start");
    require_ok(lling_wfst_builder_set_final(builder, q1, 0.0), "set_final");
    require_ok(lling_wfst_builder_add_arc(builder, q0, input, 1, output, 1,
                                          q1, weight),
               "add_arc");
    require_ok(lling_wfst_builder_build(builder, &wfst), "build");
    lling_wfst_builder_free(builder);

    require_ok(lling_wfst_resource(wfst, &resource), "wfst_resource");
    lling_wfst_free(wfst);
    return resource;
}

static void exercise_typed_v2_contract(void) {
    LlingWfstDescriptorV2 descriptor = {0};
    descriptor.header.struct_size = (uint32_t)sizeof(descriptor);
    descriptor.header.abi_version = LLING_ABI_V2;
    uint8_t typed_evidence = UINT8_MAX;
    require_ok(lling_abi_v2_validate_descriptor(&descriptor, &typed_evidence),
               "validate_descriptor_v2");
    if (typed_evidence != 0) {
        fprintf(stderr, "opaque ABI-v1 input gained typed evidence\n");
        exit(EXIT_FAILURE);
    }

    LlingBudgetV2 budget = {0};
    budget.header.struct_size = (uint32_t)sizeof(budget);
    budget.header.abi_version = LLING_ABI_V2;
    require_ok(lling_abi_v2_validate_budget(&budget), "validate_budget_v2");

    LlingCancellationV2* cancellation = NULL;
    uint32_t reason = UINT32_MAX;
    require_ok(lling_cancellation_v2_new(&cancellation), "cancellation_new_v2");
    require_ok(lling_cancellation_v2_request(
                   cancellation, LLING_CANCELLATION_REQUESTED_V2),
               "cancellation_request_v2");
    require_ok(lling_cancellation_v2_reason(cancellation, &reason),
               "cancellation_reason_v2");
    if (reason != LLING_CANCELLATION_REQUESTED_V2) {
        fprintf(stderr, "cancellation reason was not sticky\n");
        exit(EXIT_FAILURE);
    }
    require_ok(lling_cancellation_v2_free(&cancellation),
               "cancellation_free_v2");
}

int main(void) {
    if (lling_abi_version() != LLING_ABI_VERSION ||
        lling_llang_api_revision() < LLING_LLANG_API_REVISION) {
        fprintf(stderr, "incompatible lling-llang binary\n");
        return EXIT_FAILURE;
    }
    exercise_typed_v2_contract();

    VtResource first = single_arc_resource((uint32_t)'a', (uint32_t)'x', 0.5);
    VtResource second = single_arc_resource((uint32_t)'x', (uint32_t)'z', 0.25);
    LlingWfst* composed = NULL;
    require_ok(lling_wfst_compose(first, second, &composed), "compose");
    lling_resource_release(first);
    lling_resource_release(second);

    VtResource product = {NULL, NULL};
    require_ok(lling_wfst_resource(composed, &product), "wfst_resource");
    const void* interface = NULL;
    if (product.vtable->query_interface(
            product.context, &VT_WFST_INTERFACE_ID,
            VT_WFST_INTERFACE_VERSION, &interface) != VT_STATUS_OK ||
        interface == NULL) {
        fprintf(stderr, "scalar-WFST interface unavailable\n");
        return EXIT_FAILURE;
    }
    const VtWfstVTable* table = (const VtWfstVTable*)interface;

    uint64_t state = 0;
    if (table->start(product.context, &state) != VT_STATUS_OK) {
        return EXIT_FAILURE;
    }
    VtWfstArc arcs[VT_RECOMMENDED_ARC_BATCH];
    size_t written = 0;
    size_t total = 0;
    if (table->state_arcs(product.context, state, 0, arcs,
                          VT_RECOMMENDED_ARC_BATCH, &written,
                          &total) != VT_STATUS_OK ||
        written != 1 || total != 1) {
        return EXIT_FAILURE;
    }
    printf("arc %" PRIu64 " -> %" PRIu64 " in=%" PRIu64
           " out=%" PRIu64 " w=%g\n",
           state, arcs[0].target_state, arcs[0].input_label,
           arcs[0].output_label, arcs[0].weight);

    lling_resource_release(product);
    lling_wfst_free(composed);
    return EXIT_SUCCESS;
}
