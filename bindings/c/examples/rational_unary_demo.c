/* C11 consumer of every budgeted WFST entry point added in API revisions 8
 * and 9. Inputs are released before the independently owned outputs are read. */
#include <lling_llang.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static void require(int condition, const char* description) {
    if (!condition) {
        fprintf(stderr, "%s\n", description);
        exit(EXIT_FAILURE);
    }
}

static void require_ok(LlingLlangStatus status, const char* description) {
    if (status != LLING_STATUS_OK) {
        fprintf(stderr, "%s (%u): %s\n", description, (unsigned)status,
                lling_last_error_message());
        exit(EXIT_FAILURE);
    }
}

static LlingBudgetV2 budget(void) {
    LlingBudgetV2 value = {0};
    value.header.struct_size = (uint32_t)sizeof(value);
    value.header.abi_version = LLING_ABI_V2;
    return value;
}

static VtResource fixture(uint32_t unit_domain) {
    LlingWfstBuilder* builder = NULL;
    LlingWfst* graph = NULL;
    VtResource resource = {NULL, NULL};
    uint32_t start = UINT32_MAX;
    uint32_t final_state = UINT32_MAX;
    require_ok(lling_wfst_builder_new_for_domains(
                   unit_domain, VT_WEIGHT_DOMAIN_TROPICAL_F64, &builder),
               "builder domain");
    require_ok(lling_wfst_builder_add_state(builder, &start), "start state");
    require_ok(lling_wfst_builder_add_state(builder, &final_state), "final state");
    require_ok(lling_wfst_builder_set_start(builder, start), "set start");
    require_ok(lling_wfst_builder_set_final(builder, final_state, 0.0),
               "set final");
    require_ok(lling_wfst_builder_add_arc(builder, start, 'a', 1, 'x', 1,
                                          final_state, 0.5),
               "add arc");
    require_ok(lling_wfst_builder_build(builder, &graph), "build graph");
    lling_wfst_builder_free(builder);
    require_ok(lling_wfst_resource(graph, &resource), "export graph");
    lling_wfst_free(graph);
    return resource;
}

static void inspect(LlingWfst* graph, size_t expected_states,
                    size_t expected_start_arcs, uint8_t expected_start_final) {
    VtResource resource = {NULL, NULL};
    const void* interface = NULL;
    require_ok(lling_wfst_resource(graph, &resource), "export result");
    require(resource.vtable->query_interface(
                resource.context, &VT_WFST_INTERFACE_ID,
                VT_WFST_INTERFACE_VERSION, &interface) == VT_STATUS_OK &&
                interface != NULL,
            "discover scalar WFST");
    const VtWfstVTable* table = (const VtWfstVTable*)interface;
    require(table->unit_domain == VT_UNIT_DOMAIN_UNICODE_SCALAR &&
                table->weight_domain == VT_WEIGHT_DOMAIN_TROPICAL_F64,
            "preserve scalar domains");
    size_t states = 0;
    uint8_t known = 0;
    require(table->num_states(resource.context, &states, &known) == VT_STATUS_OK &&
                (!known || states == expected_states),
            "result state count");
    /* A lazy source advertises an upper bound internally but deliberately
     * reports an unknown exact cardinality over the family ABI. Its compact
     * state IDs must still validate across the reserved finite range. */
    for (size_t index = 0; index < expected_states; ++index) {
        uint8_t valid_state = 0;
        uint8_t final_state = 0;
        double weight = 0.0;
        require(table->state_info(resource.context, (uint64_t)index,
                                  &valid_state, &final_state, &weight) ==
                    VT_STATUS_OK &&
                    valid_state == 1,
                "reserved state is valid");
    }
    uint64_t start = UINT64_MAX;
    require(table->start(resource.context, &start) == VT_STATUS_OK &&
                start != UINT64_MAX,
            "result start");
    uint8_t valid = 0;
    uint8_t is_final = 0;
    double final_weight = 0.0;
    require(table->state_info(resource.context, start, &valid, &is_final,
                              &final_weight) == VT_STATUS_OK &&
                valid == 1 && is_final == expected_start_final,
            "result start finality");
    VtWfstArc arcs[4] = {0};
    size_t written = 0;
    size_t total = 0;
    require(table->state_arcs(resource.context, start, 0, arcs, 4, &written,
                              &total) == VT_STATUS_OK &&
                written == expected_start_arcs && total == expected_start_arcs,
            "result start arcs");
    lling_resource_release(resource);
}

int main(void) {
    require(lling_abi_version() == LLING_ABI_VERSION &&
                lling_llang_api_revision() >= 9,
            "ABI/API revision");
    VtResource first = fixture(VT_UNIT_DOMAIN_UNICODE_SCALAR);
    VtResource second = fixture(VT_UNIT_DOMAIN_UNICODE_SCALAR);
    VtResource byte_graph = fixture(VT_UNIT_DOMAIN_BYTE);
    LlingBudgetV2 unlimited = budget();
    LlingBudgetV2 exact_union = budget();
    exact_union.header.flags = LLING_BUDGET_STATES | LLING_BUDGET_ARCS |
                               LLING_BUDGET_WORK;
    exact_union.max_states = 9;
    exact_union.max_arcs = 6;
    exact_union.max_work = 15;

    LlingWfst* outputs[14] = {NULL};
    LlingWfst* unchanged = NULL;
    require_ok(lling_wfst_union(first, second, &exact_union, &outputs[0]),
               "exact-budget union");
    unchanged = outputs[0];
    exact_union.max_states = 8;
    require(lling_wfst_union(first, second, &exact_union, &unchanged) ==
                LLING_STATUS_LIMIT_EXCEEDED &&
                unchanged == outputs[0],
            "below-limit failure is atomic");
    exact_union.max_states = 9;
    require(lling_wfst_union(first, byte_graph, &exact_union, &unchanged) ==
                LLING_STATUS_INCOMPATIBLE_RESOURCE &&
                unchanged == outputs[0],
            "mismatched domain failure is atomic");
    LlingBudgetV2 invalid = budget();
    invalid.header.reserved = 1;
    require(lling_wfst_union(first, second, &invalid, &unchanged) ==
                LLING_STATUS_INVALID_ARGUMENT &&
                unchanged == outputs[0],
            "noncanonical budget failure is atomic");
    require(lling_wfst_union(first, second, &unlimited, NULL) ==
                LLING_STATUS_NULL_POINTER,
            "null output is rejected before capture");
    require(lling_wfst_union_refs(NULL, &second, &unlimited, &unchanged) ==
                LLING_STATUS_NULL_POINTER &&
                unchanged == outputs[0],
            "null pointer-form input is rejected");

    require_ok(lling_wfst_concat(first, second, &unlimited, &outputs[1]),
               "concat");
    require_ok(lling_wfst_closure(first, &unlimited, &outputs[2]),
               "closure");
    require_ok(lling_wfst_closure_plus(first, &unlimited, &outputs[3]),
               "closure plus");
    require_ok(lling_wfst_project_input(first, &unlimited, &outputs[4]),
               "project input");
    require_ok(lling_wfst_project_output(first, &unlimited, &outputs[5]),
               "project output");
    require_ok(lling_wfst_reverse(first, &unlimited, &outputs[6]), "reverse");
    require_ok(lling_wfst_union_refs(&first, &second, &unlimited, &outputs[7]),
               "union pointer form");
    require_ok(lling_wfst_concat_refs(&first, &second, &unlimited, &outputs[8]),
               "concat pointer form");
    require_ok(lling_wfst_closure_ref(&first, &unlimited, &outputs[9]),
               "closure pointer form");
    require_ok(lling_wfst_closure_plus_ref(&first, &unlimited, &outputs[10]),
               "closure-plus pointer form");
    require_ok(lling_wfst_project_input_ref(&first, &unlimited, &outputs[11]),
               "input-projection pointer form");
    require_ok(lling_wfst_project_output_ref(&first, &unlimited, &outputs[12]),
               "output-projection pointer form");
    require_ok(lling_wfst_reverse_ref(&first, &unlimited, &outputs[13]),
               "reversal pointer form");

    /* Each output must outlive both borrowed inputs and the unrelated byte
     * graph. Every inspection takes and releases another owned resource. */
    lling_resource_release(first);
    lling_resource_release(second);
    lling_resource_release(byte_graph);
    const size_t expected_states[14] = {5, 4, 3, 2, 2, 2, 3,
                                        5, 4, 3, 2, 2, 2, 3};
    const size_t expected_arcs[14] = {2, 1, 1, 1, 1, 1, 1,
                                      2, 1, 1, 1, 1, 1, 1};
    const uint8_t expected_final[14] = {0, 0, 1, 0, 0, 0, 0,
                                        0, 0, 1, 0, 0, 0, 0};
    for (size_t index = 0; index < 14; ++index) {
        require(outputs[index] != NULL, "result handle exists");
        inspect(outputs[index], expected_states[index], expected_arcs[index],
                expected_final[index]);
        lling_wfst_free(outputs[index]);
    }
    return EXIT_SUCCESS;
}
