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

struct boolean_semiring {
    std::atomic<std::size_t> references{1};
    std::atomic<std::size_t> live_tokens{0};
};

std::atomic<std::size_t> live_semirings{0};
constexpr std::uint64_t boolean_token_tag = UINT64_C(0xb0015e11);

const VtResourceVTable& boolean_resource_table();
const VtSemiringVTable& boolean_semiring_table();
const VtSemiringDivisionVTable& boolean_division_table();
const VtSemiringStarVTable& boolean_star_table();
const VtSemiringNumericVTable& boolean_numeric_table();
const VtSemiringPropertiesVTable& boolean_properties_table();

VtResource make_boolean_semiring() {
    ++live_semirings;
    return VtResource{new boolean_semiring, &boolean_resource_table()};
}

boolean_semiring* boolean_state(void* context) {
    return static_cast<boolean_semiring*>(context);
}

void boolean_retain(void* context) {
    boolean_state(context)->references.fetch_add(1, std::memory_order_relaxed);
}

void boolean_release(void* context) {
    auto* state = boolean_state(context);
    if (state->references.fetch_sub(1, std::memory_order_acq_rel) == 1) {
        delete state;
        --live_semirings;
    }
}

VtStatus boolean_query_interface(
    void*, const VtInterfaceId* interface_id, std::uint32_t minimum_version,
    const void** output) {
    if (interface_id == nullptr || output == nullptr) {
        return VT_STATUS_NULL_POINTER;
    }
    *output = nullptr;
    const auto matches = [interface_id](const VtInterfaceId& expected) {
        return std::memcmp(interface_id->bytes, expected.bytes, 16) == 0;
    };
    if (matches(VT_SEMIRING_INTERFACE_ID) &&
        minimum_version <= VT_SEMIRING_INTERFACE_VERSION) {
        *output = &boolean_semiring_table();
    } else if (matches(VT_SEMIRING_DIVISION_INTERFACE_ID) &&
               minimum_version <= VT_SEMIRING_DIVISION_INTERFACE_VERSION) {
        *output = &boolean_division_table();
    } else if (matches(VT_SEMIRING_STAR_INTERFACE_ID) &&
               minimum_version <= VT_SEMIRING_STAR_INTERFACE_VERSION) {
        *output = &boolean_star_table();
    } else if (matches(VT_SEMIRING_NUMERIC_INTERFACE_ID) &&
               minimum_version <= VT_SEMIRING_NUMERIC_INTERFACE_VERSION) {
        *output = &boolean_numeric_table();
    } else if (matches(VT_SEMIRING_PROPERTIES_INTERFACE_ID) &&
               minimum_version <= VT_SEMIRING_PROPERTIES_INTERFACE_VERSION) {
        *output = &boolean_properties_table();
    } else {
        return VT_STATUS_UNSUPPORTED;
    }
    return VT_STATUS_OK;
}

bool decode_boolean(const VtSemiringValue* value, bool& output) {
    if (value == nullptr || value->word1 != boolean_token_tag ||
        value->word0 > 1) {
        return false;
    }
    output = value->word0 != 0;
    return true;
}

VtStatus write_boolean(
    void* context, VtSemiringValue* output, bool value) {
    if (context == nullptr || output == nullptr) {
        return VT_STATUS_NULL_POINTER;
    }
    *output = VtSemiringValue{value ? UINT64_C(1) : UINT64_C(0),
                             boolean_token_tag};
    boolean_state(context)->live_tokens.fetch_add(1, std::memory_order_relaxed);
    return VT_STATUS_OK;
}

VtStatus boolean_zero(void* context, VtSemiringValue* output) {
    return write_boolean(context, output, false);
}

VtStatus boolean_one(void* context, VtSemiringValue* output) {
    return write_boolean(context, output, true);
}

VtStatus boolean_clone(
    void* context, const VtSemiringValue* value, VtSemiringValue* output) {
    bool decoded = false;
    return decode_boolean(value, decoded)
        ? write_boolean(context, output, decoded)
        : VT_STATUS_INVALID_ARGUMENT;
}

VtStatus boolean_release_values(
    void* context, VtSemiringValue* values, std::size_t count) {
    if (context == nullptr || (count != 0 && values == nullptr)) {
        return VT_STATUS_NULL_POINTER;
    }
    for (std::size_t index = 0; index < count; ++index) {
        bool decoded = false;
        if (!decode_boolean(&values[index], decoded)) {
            return VT_STATUS_INVALID_ARGUMENT;
        }
    }
    for (std::size_t index = 0; index < count; ++index) {
        values[index] = {};
    }
    const auto previous = boolean_state(context)->live_tokens.fetch_sub(
        count, std::memory_order_acq_rel);
    return previous >= count ? VT_STATUS_OK : VT_STATUS_PROVIDER_ERROR;
}

VtStatus boolean_binary(
    void* context, const VtSemiringValue* left,
    const VtSemiringValue* right, VtSemiringValue* output,
    bool additive) {
    bool left_value = false;
    bool right_value = false;
    if (!decode_boolean(left, left_value) ||
        !decode_boolean(right, right_value)) {
        return VT_STATUS_INVALID_ARGUMENT;
    }
    return write_boolean(
        context, output,
        additive ? left_value || right_value : left_value && right_value);
}

VtStatus boolean_plus(
    void* context, const VtSemiringValue* left,
    const VtSemiringValue* right, VtSemiringValue* output) {
    return boolean_binary(context, left, right, output, true);
}

VtStatus boolean_times(
    void* context, const VtSemiringValue* left,
    const VtSemiringValue* right, VtSemiringValue* output) {
    return boolean_binary(context, left, right, output, false);
}

VtStatus boolean_equal(
    void*, const VtSemiringValue* left, const VtSemiringValue* right,
    std::uint8_t* output) {
    if (output == nullptr) return VT_STATUS_NULL_POINTER;
    bool left_value = false;
    bool right_value = false;
    if (!decode_boolean(left, left_value) ||
        !decode_boolean(right, right_value)) {
        return VT_STATUS_INVALID_ARGUMENT;
    }
    *output = static_cast<std::uint8_t>(left_value == right_value);
    return VT_STATUS_OK;
}

VtStatus boolean_approx_equal(
    void* context, const VtSemiringValue* left,
    const VtSemiringValue* right, double, std::uint8_t* output) {
    return boolean_equal(context, left, right, output);
}

VtStatus boolean_natural_order(
    void*, const VtSemiringValue* left, const VtSemiringValue* right,
    std::int32_t* output) {
    if (output == nullptr) return VT_STATUS_NULL_POINTER;
    bool left_value = false;
    bool right_value = false;
    if (!decode_boolean(left, left_value) ||
        !decode_boolean(right, right_value)) {
        return VT_STATUS_INVALID_ARGUMENT;
    }
    *output = left_value == right_value
        ? VT_SEMIRING_ORDER_EQUAL
        : left_value ? VT_SEMIRING_ORDER_WORSE : VT_SEMIRING_ORDER_BETTER;
    return VT_STATUS_OK;
}

VtStatus boolean_stable_bytes(
    void*, const VtSemiringValue* value, std::uint8_t* output,
    std::size_t capacity, std::size_t* written, std::size_t* required) {
    bool decoded = false;
    if (!decode_boolean(value, decoded)) return VT_STATUS_INVALID_ARGUMENT;
    const char encoded = decoded ? '1' : '0';
    return copy_bytes(
        std::string_view(&encoded, 1), output, capacity, written, required);
}

VtStatus boolean_diagnostic(
    void*, const VtSemiringValue*, std::uint8_t* output,
    std::size_t capacity, std::size_t* written, std::size_t* required) {
    return copy_bytes(
        "C++ Boolean semiring", output, capacity, written, required);
}

VtStatus boolean_fold(
    void* context, const VtSemiringValue* values, std::size_t count,
    VtSemiringValue* output, bool additive) {
    if (count != 0 && values == nullptr) return VT_STATUS_NULL_POINTER;
    bool result = !additive;
    for (std::size_t index = 0; index < count; ++index) {
        bool value = false;
        if (!decode_boolean(&values[index], value)) {
            return VT_STATUS_INVALID_ARGUMENT;
        }
        result = additive ? result || value : result && value;
    }
    return write_boolean(context, output, result);
}

VtStatus boolean_plus_many(
    void* context, const VtSemiringValue* values, std::size_t count,
    VtSemiringValue* output) {
    return boolean_fold(context, values, count, output, true);
}

VtStatus boolean_times_many(
    void* context, const VtSemiringValue* values, std::size_t count,
    VtSemiringValue* output) {
    return boolean_fold(context, values, count, output, false);
}

VtStatus boolean_divide(
    void* context, const VtSemiringValue* dividend,
    const VtSemiringValue* divisor, VtSemiringValue* output) {
    bool dividend_value = false;
    bool divisor_value = false;
    if (!decode_boolean(dividend, dividend_value) ||
        !decode_boolean(divisor, divisor_value)) {
        return VT_STATUS_INVALID_ARGUMENT;
    }
    return divisor_value
        ? write_boolean(context, output, dividend_value)
        : VT_STATUS_END;
}

VtStatus boolean_star(
    void* context, const VtSemiringValue* value, VtSemiringValue* output) {
    bool decoded = false;
    return decode_boolean(value, decoded)
        ? write_boolean(context, output, true)
        : VT_STATUS_INVALID_ARGUMENT;
}

VtStatus boolean_numerical_value(
    void*, const VtSemiringValue* value, double* output) {
    if (output == nullptr) return VT_STATUS_NULL_POINTER;
    bool decoded = false;
    if (!decode_boolean(value, decoded)) return VT_STATUS_INVALID_ARGUMENT;
    *output = decoded ? 1.0 : 0.0;
    return VT_STATUS_OK;
}

VtStatus boolean_quantize(
    void*, const VtSemiringValue* value, double, std::int64_t* output) {
    if (output == nullptr) return VT_STATUS_NULL_POINTER;
    bool decoded = false;
    if (!decode_boolean(value, decoded)) return VT_STATUS_INVALID_ARGUMENT;
    *output = decoded ? 1 : 0;
    return VT_STATUS_OK;
}

VtStatus boolean_to_probability(
    void* context, const VtSemiringValue* value, double* output) {
    return boolean_numerical_value(context, value, output);
}

VtStatus boolean_closure_bound(void*, std::size_t* output, std::uint8_t* known) {
    if (output == nullptr || known == nullptr) return VT_STATUS_NULL_POINTER;
    *output = 1;
    *known = 1;
    return VT_STATUS_OK;
}

const VtResourceVTable& boolean_resource_table() {
    static const VtResourceVTable table{
        sizeof(VtResourceVTable), VT_ABI_VERSION, 0, boolean_retain,
        boolean_release, boolean_query_interface};
    return table;
}

const VtSemiringVTable& boolean_semiring_table() {
    static const VtSemiringVTable table{
        sizeof(VtSemiringVTable),
        VT_SEMIRING_INTERFACE_VERSION,
        0,
        VT_SEMIRING_FLAG_STABLE_BYTES | VT_SEMIRING_FLAG_BATCH,
        {{'d', 'e', 'm', 'o', '.', 'b', 'o', 'o',
          'l', '.', 'v', '1', '.', '.', '.', '.'}},
        boolean_zero,
        boolean_one,
        boolean_clone,
        boolean_release_values,
        boolean_plus,
        boolean_times,
        boolean_equal,
        boolean_approx_equal,
        boolean_natural_order,
        boolean_stable_bytes,
        boolean_diagnostic,
        boolean_plus_many,
        boolean_times_many};
    return table;
}

const VtSemiringDivisionVTable& boolean_division_table() {
    static const VtSemiringDivisionVTable table{
        sizeof(VtSemiringDivisionVTable), VT_SEMIRING_DIVISION_INTERFACE_VERSION,
        0, boolean_divide, boolean_divide};
    return table;
}

const VtSemiringStarVTable& boolean_star_table() {
    static const VtSemiringStarVTable table{
        sizeof(VtSemiringStarVTable), VT_SEMIRING_STAR_INTERFACE_VERSION, 0,
        boolean_star};
    return table;
}

const VtSemiringNumericVTable& boolean_numeric_table() {
    static const VtSemiringNumericVTable table{
        sizeof(VtSemiringNumericVTable), VT_SEMIRING_NUMERIC_INTERFACE_VERSION,
        0, boolean_numerical_value, boolean_quantize, boolean_to_probability};
    return table;
}

const VtSemiringPropertiesVTable& boolean_properties_table() {
    static const VtSemiringPropertiesVTable table{
        sizeof(VtSemiringPropertiesVTable),
        VT_SEMIRING_PROPERTIES_INTERFACE_VERSION,
        0,
        VT_SEMIRING_PROPERTY_HASHABLE |
            VT_SEMIRING_PROPERTY_IDEMPOTENT_PLUS |
            VT_SEMIRING_PROPERTY_K_CLOSED |
            VT_SEMIRING_PROPERTY_ZERO_SUM_FREE |
            VT_SEMIRING_PROPERTY_COMMUTATIVE_TIMES |
            VT_SEMIRING_PROPERTY_TOTALLY_ORDERED |
            VT_SEMIRING_PROPERTY_NONNEGATIVE,
        boolean_closure_bound};
    return table;
}

bool check_wfst() {
    using namespace vinary_tree::lling_llang;
    cancellation stop;
    if (stop.reason() != 0) return 2;
    stop.request(LLING_CANCELLATION_REQUESTED_V2);
    if (stop.reason() != LLING_CANCELLATION_REQUESTED_V2) return 3;
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

bool check_semiring() {
    using namespace vinary_tree::lling_llang;
    {
        resource host(make_boolean_semiring());
        auto semiring = semiring_context::open(host.get());
        auto zero = semiring.zero();
        auto one = semiring.one();
        auto sum = zero.plus(one);
        auto product = one.times(one);
        auto copied = product.clone();
        auto quotient = one.divide(one);
        auto undefined = one.divide(zero);
        auto closure = zero.star();
        const std::array<const semiring_weight*, 4> samples{
            &zero, &one, &sum, &product};
        semiring.validate_laws(samples, 0.0);

        if (!sum.equivalent(one) || !product.equivalent(copied) ||
            !one.approximately_equivalent(product, 0.0) ||
            zero.compare(one) != natural_order::better ||
            !quotient.has_value() || undefined.has_value() ||
            !closure.has_value() || one.numerical_value() != 1.0 ||
            one.quantize(0.25) != 1 || one.to_probability() != 1.0 ||
            semiring.closure_bound() != std::optional<std::size_t>{1} ||
            one.stable_bytes() != std::vector<std::uint8_t>{'1'} ||
            !semiring.declares(VT_SEMIRING_PROPERTY_HASHABLE |
                               VT_SEMIRING_PROPERTY_IDEMPOTENT_PLUS)) {
            return false;
        }
    }
    return live_semirings.load() == 0;
}

} // namespace

int main() {
    return check_wfst() && check_lattice() && check_semiring() ? 0 : 1;
}
