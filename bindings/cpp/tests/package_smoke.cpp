#include <lling_llang.hpp>

#include <algorithm>
#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string_view>
#include <utility>

namespace {

struct max_min_value {
    std::atomic<std::size_t> references{1};
    int value;
};

std::atomic<std::size_t> live_values{0};

const VtResourceVTable& resource_table();
const VtLatticeVTable& lattice_table();

VtResource make_value(int value) {
    ++live_values;
    return VtResource{new max_min_value{{1}, value}, &resource_table()};
}

void retain(void* context) {
    static_cast<max_min_value*>(context)->references.fetch_add(
        1, std::memory_order_relaxed);
}

void release(void* context) {
    auto* value = static_cast<max_min_value*>(context);
    if (value->references.fetch_sub(1, std::memory_order_acq_rel) == 1) {
        delete value;
        --live_values;
    }
}

VtStatus query_interface(
    void*, const VtInterfaceId* interface_id, std::uint32_t minimum_version,
    const void** out_vtable) {
    if (interface_id == nullptr || out_vtable == nullptr) {
        return VT_STATUS_NULL_POINTER;
    }
    *out_vtable = nullptr;
    if (minimum_version > VT_LATTICE_INTERFACE_VERSION ||
        std::memcmp(interface_id->bytes, VT_LATTICE_INTERFACE_ID.bytes, 16) != 0) {
        return VT_STATUS_UNSUPPORTED;
    }
    *out_vtable = &lattice_table();
    return VT_STATUS_OK;
}

VtStatus binary(
    void* context, const VtResource* other, VtResource* output, bool join_values) {
    if (context == nullptr || other == nullptr || output == nullptr) {
        return VT_STATUS_NULL_POINTER;
    }
    *output = {};
    if (other->vtable != &resource_table() || other->context == nullptr) {
        return VT_STATUS_INVALID_ARGUMENT;
    }
    const int left = static_cast<max_min_value*>(context)->value;
    const int right = static_cast<max_min_value*>(other->context)->value;
    *output = make_value(
        join_values ? std::max(left, right) : std::min(left, right));
    return VT_STATUS_OK;
}

VtStatus join(void* context, const VtResource* other, VtResource* output) {
    return binary(context, other, output, true);
}

VtStatus meet(void* context, const VtResource* other, VtResource* output) {
    return binary(context, other, output, false);
}

VtStatus equal(void* context, const VtResource* other, std::uint8_t* output) {
    if (context == nullptr || other == nullptr || output == nullptr) {
        return VT_STATUS_NULL_POINTER;
    }
    if (other->vtable != &resource_table() || other->context == nullptr) {
        return VT_STATUS_INVALID_ARGUMENT;
    }
    *output = static_cast<std::uint8_t>(
        static_cast<max_min_value*>(context)->value ==
        static_cast<max_min_value*>(other->context)->value);
    return VT_STATUS_OK;
}

VtStatus copy_bytes(
    std::string_view value, std::uint8_t* output, std::size_t capacity,
    std::size_t* written, std::size_t* required) {
    if (written == nullptr || required == nullptr ||
        (capacity != 0 && output == nullptr)) {
        return VT_STATUS_NULL_POINTER;
    }
    *required = value.size();
    *written = std::min(capacity, value.size());
    if (*written != 0) {
        std::memcpy(output, value.data(), *written);
    }
    return VT_STATUS_OK;
}

VtStatus stable_bytes(
    void* context, std::uint8_t* output, std::size_t capacity,
    std::size_t* written, std::size_t* required) {
    if (context == nullptr) {
        return VT_STATUS_NULL_POINTER;
    }
    const auto value = static_cast<max_min_value*>(context)->value;
    const char encoded = static_cast<char>('0' + value);
    return copy_bytes(
        std::string_view(&encoded, 1), output, capacity, written, required);
}

VtStatus diagnostic(
    void* context, std::uint8_t* output, std::size_t capacity,
    std::size_t* written, std::size_t* required) {
    if (context == nullptr) {
        return VT_STATUS_NULL_POINTER;
    }
    return copy_bytes("C++ max/min lattice", output, capacity, written, required);
}

VtStatus fold(
    void* context, const VtResource* others, std::size_t count,
    VtResource* output, bool join_values) {
    if (context == nullptr || output == nullptr ||
        (count != 0 && others == nullptr)) {
        return VT_STATUS_NULL_POINTER;
    }
    int result = static_cast<max_min_value*>(context)->value;
    for (std::size_t index = 0; index < count; ++index) {
        if (others[index].vtable != &resource_table() ||
            others[index].context == nullptr) {
            *output = {};
            return VT_STATUS_INVALID_ARGUMENT;
        }
        const int next =
            static_cast<max_min_value*>(others[index].context)->value;
        result = join_values ? std::max(result, next) : std::min(result, next);
    }
    *output = make_value(result);
    return VT_STATUS_OK;
}

VtStatus join_many(
    void* context, const VtResource* others, std::size_t count,
    VtResource* output) {
    return fold(context, others, count, output, true);
}

VtStatus meet_many(
    void* context, const VtResource* others, std::size_t count,
    VtResource* output) {
    return fold(context, others, count, output, false);
}

const VtResourceVTable& resource_table() {
    static const VtResourceVTable table{
        sizeof(VtResourceVTable), VT_ABI_VERSION, 0, retain, release,
        query_interface};
    return table;
}

const VtLatticeVTable& lattice_table() {
    static const VtLatticeVTable table{
        sizeof(VtLatticeVTable),
        VT_LATTICE_INTERFACE_VERSION,
        0,
        VT_LATTICE_FLAG_STABLE_BYTES | VT_LATTICE_FLAG_BATCH,
        {{'d', 'e', 'm', 'o', '.', 'm', 'a', 'x',
          'm', 'i', 'n', '.', 'v', '1', '.', '.'}},
        join,
        meet,
        equal,
        stable_bytes,
        diagnostic,
        join_many,
        meet_many};
    return table;
}

bool check_wfst() {
    using namespace vinary_tree::lling_llang;
    builder value;
    const auto first = value.add_state();
    const auto second = value.add_state();
    value.start(first).final_state(second).arc(first, U'a', U'b', second);
    auto graph = value.build();
    auto retained = graph.retained_resource();
    return retained.get().context != nullptr;
}

bool check_lattice() {
    using namespace vinary_tree::lling_llang;
    {
        resource two_host(make_value(2));
        resource seven_host(make_value(7));
        resource four_host(make_value(4));
        auto two = lattice_value::open(two_host.get());
        auto seven = lattice_value::open(seven_host.get());
        auto four = lattice_value::open(four_host.get());

        auto maximum = two.join(seven);
        auto minimum = two.meet(seven);
        const std::array<const lattice_value*, 2> join_operands{&seven, &four};
        const std::array<const lattice_value*, 2> meet_operands{&two, &four};
        auto folded_maximum = two.join_many(join_operands);
        auto folded_minimum = seven.meet_many(meet_operands);
        const std::array<const lattice_value*, 3> samples{&two, &seven, &four};
        lattice_value::validate_laws(samples);

        const auto encoded_maximum = maximum.stable_bytes();
        const auto encoded_minimum = minimum.stable_bytes();
        const auto domain = two.domain_id();
        const auto moved = std::move(folded_maximum);
        if (!maximum.equivalent(seven) || !minimum.equivalent(two) ||
            encoded_maximum != std::vector<std::uint8_t>{'7'} ||
            encoded_minimum != std::vector<std::uint8_t>{'2'} ||
            moved.stable_bytes() != std::vector<std::uint8_t>{'7'} ||
            folded_minimum.stable_bytes() != std::vector<std::uint8_t>{'2'} ||
            two.diagnostic() != "C++ max/min lattice" ||
            (two.flags() & VT_LATTICE_FLAG_BATCH) == 0 || domain[0] != 'd') {
            return false;
        }
    }
    return live_values.load() == 0;
}

} // namespace

int main() {
    return check_wfst() && check_lattice() ? 0 : 1;
}
