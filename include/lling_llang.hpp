#ifndef LLING_LLANG_HPP
#define LLING_LLANG_HPP

#include "lling_llang.h"
#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <span>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace vinary_tree::lling_llang {

class error final : public std::runtime_error {
public:
    explicit error(LlingStatus status)
        : std::runtime_error(lling_last_error_message()), status_(status) {}
    error(LlingStatus status, std::string message)
        : std::runtime_error(std::move(message)), status_(status) {}
    [[nodiscard]] LlingStatus status() const noexcept { return status_; }
private:
    LlingStatus status_;
};

inline void check(LlingStatus status) {
    if (status != LLING_STATUS_OK) throw error(status);
}

namespace detail {

template<class Reader>
[[nodiscard]] std::vector<std::uint8_t> read_bounded_bytes(
    Reader&& reader, std::string_view subject) {
    std::size_t written = 0;
    std::size_t required = 0;
    check(reader(nullptr, 0, &written, &required));
    for (unsigned attempt = 0; attempt < 3; ++attempt) {
        std::vector<std::uint8_t> bytes(required);
        written = 0;
        std::size_t next_required = 0;
        check(reader(
            bytes.empty() ? nullptr : bytes.data(), bytes.size(), &written,
            &next_required));
        if (written == next_required && next_required <= bytes.size()) {
            bytes.resize(written);
            return bytes;
        }
        required = next_required;
    }
    throw error(
        LLING_STATUS_PROVIDER_ERROR,
        std::string(subject) + " length did not stabilize within three reads");
}

struct semiring_state final {
    explicit semiring_state(LlingSemiring* value) noexcept : value(value) {}
    semiring_state(const semiring_state&) = delete;
    semiring_state& operator=(const semiring_state&) = delete;
    ~semiring_state() { lling_semiring_free(value); }

    LlingSemiring* value;
};

} // namespace detail

class resource final {
public:
    explicit resource(VtResource value) noexcept : value_(value) {}
    resource(const resource&) = delete;
    resource& operator=(const resource&) = delete;
    resource(resource&& other) noexcept : value_(std::exchange(other.value_, {})) {}
    resource& operator=(resource&& other) noexcept {
        if (this != &other) {
            lling_resource_release(value_);
            value_ = std::exchange(other.value_, {});
        }
        return *this;
    }
    ~resource() { lling_resource_release(value_); }
    [[nodiscard]] VtResource get() const noexcept { return value_; }
private:
    VtResource value_{};
};

enum class natural_order : std::int32_t {
    better = VT_SEMIRING_ORDER_BETTER,
    equal = VT_SEMIRING_ORDER_EQUAL,
    worse = VT_SEMIRING_ORDER_WORSE,
    incomparable = VT_SEMIRING_ORDER_INCOMPARABLE,
};

class semiring_context;

class semiring_weight final {
public:
    semiring_weight(const semiring_weight&) = delete;
    semiring_weight& operator=(const semiring_weight&) = delete;
    semiring_weight(semiring_weight&& other) noexcept
        : state_(std::move(other.state_)),
          value_(std::exchange(other.value_, nullptr)) {}
    semiring_weight& operator=(semiring_weight&& other) noexcept {
        if (this != &other) {
            lling_semiring_weight_free(value_);
            state_ = std::move(other.state_);
            value_ = std::exchange(other.value_, nullptr);
        }
        return *this;
    }
    ~semiring_weight() { lling_semiring_weight_free(value_); }

    [[nodiscard]] semiring_weight clone() const {
        LlingSemiringWeight* result = nullptr;
        check(lling_semiring_weight_clone(value_, &result));
        return semiring_weight(state_, result);
    }

    [[nodiscard]] semiring_weight plus(const semiring_weight& other) const {
        return binary(other, lling_semiring_plus);
    }

    [[nodiscard]] semiring_weight times(const semiring_weight& other) const {
        return binary(other, lling_semiring_times);
    }

    [[nodiscard]] bool equivalent(const semiring_weight& other) const {
        ensure_same_context(other);
        std::uint8_t result = 0;
        check(lling_semiring_equal(
            state_->value, value_, other.value_, &result));
        return result != 0;
    }

    [[nodiscard]] bool approximately_equivalent(
        const semiring_weight& other, double epsilon) const {
        ensure_same_context(other);
        std::uint8_t result = 0;
        check(lling_semiring_approx_equal(
            state_->value, value_, other.value_, epsilon, &result));
        return result != 0;
    }

    [[nodiscard]] natural_order compare(const semiring_weight& other) const {
        ensure_same_context(other);
        std::int32_t result = VT_SEMIRING_ORDER_INCOMPARABLE;
        check(lling_semiring_natural_order(
            state_->value, value_, other.value_, &result));
        return static_cast<natural_order>(result);
    }

    [[nodiscard]] std::optional<semiring_weight> divide(
        const semiring_weight& divisor) const {
        return optional_binary(divisor, lling_semiring_divide);
    }

    [[nodiscard]] std::optional<semiring_weight> left_divide(
        const semiring_weight& divisor) const {
        return optional_binary(divisor, lling_semiring_left_divide);
    }

    [[nodiscard]] std::optional<semiring_weight> star() const {
        LlingSemiringWeight* result = nullptr;
        std::uint8_t defined = 0;
        check(lling_semiring_star(state_->value, value_, &result, &defined));
        if (defined == 0) return std::nullopt;
        return semiring_weight(state_, result);
    }

    [[nodiscard]] double numerical_value() const {
        double result = 0.0;
        check(lling_semiring_numerical_value(state_->value, value_, &result));
        return result;
    }

    [[nodiscard]] std::int64_t quantize(double epsilon) const {
        std::int64_t result = 0;
        check(lling_semiring_quantize(state_->value, value_, epsilon, &result));
        return result;
    }

    [[nodiscard]] double to_probability() const {
        double result = 0.0;
        check(lling_semiring_to_probability(state_->value, value_, &result));
        return result;
    }

    [[nodiscard]] std::vector<std::uint8_t> stable_bytes() const {
        return detail::read_bounded_bytes(
            [this](
                std::uint8_t* output, std::size_t capacity,
                std::size_t* written, std::size_t* required) {
                return lling_semiring_stable_bytes(
                    state_->value, value_, output, capacity, written, required);
            },
            "semiring stable-byte sequence");
    }

    [[nodiscard]] LlingSemiringWeight* get() const noexcept { return value_; }

private:
    friend class semiring_context;

    using binary_operation = LlingStatus (*)(
        const LlingSemiring*, const LlingSemiringWeight*,
        const LlingSemiringWeight*, LlingSemiringWeight**);
    using optional_binary_operation = LlingStatus (*)(
        const LlingSemiring*, const LlingSemiringWeight*,
        const LlingSemiringWeight*, LlingSemiringWeight**, std::uint8_t*);

    semiring_weight(
        std::shared_ptr<detail::semiring_state> state,
        LlingSemiringWeight* value) noexcept
        : state_(std::move(state)), value_(value) {}

    void ensure_same_context(const semiring_weight& other) const {
        if (state_.get() != other.state_.get()) {
            throw error(
                LLING_STATUS_INVALID_ARGUMENT,
                "semiring weights belong to different operation contexts");
        }
    }

    [[nodiscard]] semiring_weight binary(
        const semiring_weight& other, binary_operation operation) const {
        ensure_same_context(other);
        LlingSemiringWeight* result = nullptr;
        check(operation(state_->value, value_, other.value_, &result));
        return semiring_weight(state_, result);
    }

    [[nodiscard]] std::optional<semiring_weight> optional_binary(
        const semiring_weight& other,
        optional_binary_operation operation) const {
        ensure_same_context(other);
        LlingSemiringWeight* result = nullptr;
        std::uint8_t defined = 0;
        check(operation(
            state_->value, value_, other.value_, &result, &defined));
        if (defined == 0) return std::nullopt;
        return semiring_weight(state_, result);
    }

    std::shared_ptr<detail::semiring_state> state_;
    LlingSemiringWeight* value_ = nullptr;
};

class semiring_context final {
public:
    semiring_context(const semiring_context&) noexcept = default;
    semiring_context& operator=(const semiring_context&) noexcept = default;
    semiring_context(semiring_context&&) noexcept = default;
    semiring_context& operator=(semiring_context&&) noexcept = default;

    [[nodiscard]] static semiring_context open(VtResource resource) {
        LlingSemiring* result = nullptr;
        check(lling_semiring_open(&resource, &result));
        return semiring_context(
            std::make_shared<detail::semiring_state>(result));
    }

    [[nodiscard]] std::uint64_t properties() const {
        std::uint64_t result = 0;
        check(lling_semiring_properties(state_->value, &result));
        return result;
    }

    [[nodiscard]] bool declares(std::uint64_t properties) const {
        return (this->properties() & properties) == properties;
    }

    [[nodiscard]] semiring_weight zero() const {
        return identity(lling_semiring_zero);
    }

    [[nodiscard]] semiring_weight one() const {
        return identity(lling_semiring_one);
    }

    [[nodiscard]] std::optional<std::size_t> closure_bound() const {
        std::size_t result = 0;
        std::uint8_t known = 0;
        check(lling_semiring_closure_bound(state_->value, &result, &known));
        if (known == 0) return std::nullopt;
        return result;
    }

    void validate_laws(
        std::span<const semiring_weight* const> weights,
        double epsilon) const {
        std::vector<const LlingSemiringWeight*> handles;
        handles.reserve(weights.size());
        for (const semiring_weight* weight : weights) {
            if (weight == nullptr) {
                throw std::invalid_argument("semiring weight pointer is null");
            }
            if (weight->state_.get() != state_.get()) {
                throw error(
                    LLING_STATUS_INVALID_ARGUMENT,
                    "semiring weight belongs to a different operation context");
            }
            handles.push_back(weight->value_);
        }
        check(lling_semiring_validate_laws(
            state_->value, handles.data(), handles.size(), epsilon));
    }

private:
    using identity_operation = LlingStatus (*)(
        const LlingSemiring*, LlingSemiringWeight**);

    explicit semiring_context(
        std::shared_ptr<detail::semiring_state> state) noexcept
        : state_(std::move(state)) {}

    [[nodiscard]] semiring_weight identity(
        identity_operation operation) const {
        LlingSemiringWeight* result = nullptr;
        check(operation(state_->value, &result));
        return semiring_weight(state_, result);
    }

    std::shared_ptr<detail::semiring_state> state_;
};

class lattice_value final {
public:
    lattice_value(const lattice_value&) = delete;
    lattice_value& operator=(const lattice_value&) = delete;
    lattice_value(lattice_value&& other) noexcept
        : value_(std::exchange(other.value_, nullptr)) {}
    lattice_value& operator=(lattice_value&& other) noexcept {
        if (this != &other) {
            lling_lattice_free(value_);
            value_ = std::exchange(other.value_, nullptr);
        }
        return *this;
    }
    ~lattice_value() { lling_lattice_free(value_); }

    [[nodiscard]] static lattice_value open(VtResource resource) {
        LlingLatticeValue* result = nullptr;
        check(lling_lattice_open(&resource, &result));
        return lattice_value(result);
    }

    [[nodiscard]] LlingLatticeValue* get() const noexcept { return value_; }

    [[nodiscard]] std::array<std::uint8_t, 16> domain_id() const {
        VtInterfaceId result{};
        check(lling_lattice_domain_id(value_, &result));
        std::array<std::uint8_t, 16> bytes{};
        for (std::size_t index = 0; index < bytes.size(); ++index) {
            bytes[index] = result.bytes[index];
        }
        return bytes;
    }

    [[nodiscard]] std::uint64_t flags() const {
        std::uint64_t result = 0;
        check(lling_lattice_flags(value_, &result));
        return result;
    }

    [[nodiscard]] lattice_value join(const lattice_value& other) const {
        return binary(other, lling_lattice_join);
    }

    [[nodiscard]] lattice_value meet(const lattice_value& other) const {
        return binary(other, lling_lattice_meet);
    }

    [[nodiscard]] bool equivalent(const lattice_value& other) const {
        std::uint8_t result = 0;
        check(lling_lattice_equal(value_, other.value_, &result));
        return result != 0;
    }

    [[nodiscard]] std::vector<std::uint8_t> stable_bytes() const {
        return read_bytes(lling_lattice_stable_bytes);
    }

    [[nodiscard]] std::string diagnostic() const {
        const auto bytes = read_bytes(lling_lattice_diagnostic);
        return std::string(bytes.begin(), bytes.end());
    }

    [[nodiscard]] lattice_value join_many(
        std::span<const lattice_value* const> others) const {
        return fold(others, lling_lattice_join_many);
    }

    [[nodiscard]] lattice_value meet_many(
        std::span<const lattice_value* const> others) const {
        return fold(others, lling_lattice_meet_many);
    }

    static void validate_laws(std::span<const lattice_value* const> values) {
        const auto handles = checked_handles(values);
        check(lling_lattice_validate_laws(handles.data(), handles.size()));
    }

private:
    using binary_operation = LlingStatus (*)(
        const LlingLatticeValue*, const LlingLatticeValue*, LlingLatticeValue**);
    using fold_operation = LlingStatus (*)(
        const LlingLatticeValue*, const LlingLatticeValue* const*, std::size_t,
        LlingLatticeValue**);
    using byte_operation = LlingStatus (*)(
        const LlingLatticeValue*, std::uint8_t*, std::size_t, std::size_t*,
        std::size_t*);

    explicit lattice_value(LlingLatticeValue* value) noexcept : value_(value) {}

    [[nodiscard]] lattice_value binary(
        const lattice_value& other, binary_operation operation) const {
        LlingLatticeValue* result = nullptr;
        check(operation(value_, other.value_, &result));
        return lattice_value(result);
    }

    [[nodiscard]] lattice_value fold(
        std::span<const lattice_value* const> others,
        fold_operation operation) const {
        const auto handles = checked_handles(others);
        LlingLatticeValue* result = nullptr;
        check(operation(value_, handles.data(), handles.size(), &result));
        return lattice_value(result);
    }

    [[nodiscard]] static std::vector<const LlingLatticeValue*> checked_handles(
        std::span<const lattice_value* const> values) {
        std::vector<const LlingLatticeValue*> handles;
        handles.reserve(values.size());
        for (const lattice_value* value : values) {
            if (value == nullptr) {
                throw std::invalid_argument("lattice value pointer is null");
            }
            handles.push_back(value->value_);
        }
        return handles;
    }

    [[nodiscard]] std::vector<std::uint8_t> read_bytes(
        byte_operation operation) const {
        return detail::read_bounded_bytes(
            [this, operation](
                std::uint8_t* output, std::size_t capacity,
                std::size_t* written, std::size_t* required) {
                return operation(
                    value_, output, capacity, written, required);
            },
            "lattice byte sequence");
    }

    LlingLatticeValue* value_ = nullptr;
};

class cancellation final {
public:
    cancellation() { check(lling_cancellation_v2_new(&value_)); }
    cancellation(const cancellation&) = delete;
    cancellation& operator=(const cancellation&) = delete;
    cancellation(cancellation&& other) noexcept
        : value_(std::exchange(other.value_, nullptr)) {}
    cancellation& operator=(cancellation&& other) noexcept {
        if (this != &other) {
            release();
            value_ = std::exchange(other.value_, nullptr);
        }
        return *this;
    }
    ~cancellation() { release(); }
    void request(LlingCancellationReasonV2 reason) const {
        check(lling_cancellation_v2_request(
            value_, static_cast<std::uint32_t>(reason)));
    }
    [[nodiscard]] std::uint32_t reason() const {
        std::uint32_t result = 0;
        check(lling_cancellation_v2_reason(value_, &result));
        return result;
    }
private:
    void release() noexcept {
        if (value_ != nullptr) {
            (void)lling_cancellation_v2_free(&value_);
        }
    }
    LlingCancellationV2* value_ = nullptr;
};

class wfst final {
public:
    explicit wfst(LlingWfst* value) noexcept : value_(value) {}
    wfst(const wfst&) = delete;
    wfst& operator=(const wfst&) = delete;
    wfst(wfst&& other) noexcept : value_(std::exchange(other.value_, nullptr)) {}
    wfst& operator=(wfst&& other) noexcept {
        if (this != &other) {
            lling_wfst_free(value_);
            value_ = std::exchange(other.value_, nullptr);
        }
        return *this;
    }
    ~wfst() { lling_wfst_free(value_); }
    [[nodiscard]] resource retained_resource() const {
        VtResource result{};
        check(lling_wfst_resource(value_, &result));
        return resource(result);
    }
    [[nodiscard]] static wfst import(VtResource value) {
        LlingWfst* result = nullptr;
        check(lling_wfst_import(value, &result));
        return wfst(result);
    }
    [[nodiscard]] static wfst compose(VtResource first, VtResource second) {
        LlingWfst* result = nullptr;
        check(lling_wfst_compose(first, second, &result));
        return wfst(result);
    }
private:
    LlingWfst* value_ = nullptr;
};

class builder final {
public:
    builder() { check(lling_wfst_builder_new(&value_)); }
    builder(const builder&) = delete;
    builder& operator=(const builder&) = delete;
    ~builder() { lling_wfst_builder_free(value_); }
    [[nodiscard]] std::uint32_t add_state() {
        std::uint32_t state = 0;
        check(lling_wfst_builder_add_state(value_, &state));
        return state;
    }
    builder& start(std::uint32_t state) { check(lling_wfst_builder_set_start(value_, state)); return *this; }
    builder& final_state(std::uint32_t state, double weight = 0.0) { check(lling_wfst_builder_set_final(value_, state, weight)); return *this; }
    builder& arc(std::uint32_t from, char32_t input, char32_t output, std::uint32_t to, double weight = 0.0) {
        check(lling_wfst_builder_add_arc(value_, from, input, 1, output, 1, to, weight));
        return *this;
    }
    builder& epsilon(std::uint32_t from, std::uint32_t to, double weight = 0.0) {
        check(lling_wfst_builder_add_arc(value_, from, 0, 0, 0, 0, to, weight));
        return *this;
    }
    [[nodiscard]] wfst build() {
        LlingWfst* result = nullptr;
        check(lling_wfst_builder_build(value_, &result));
        return wfst(result);
    }
private:
    LlingWfstBuilder* value_ = nullptr;
};

} // namespace vinary_tree::lling_llang

#endif /* LLING_LLANG_HPP */
