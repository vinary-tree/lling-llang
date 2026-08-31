#ifndef LLING_LLANG_HPP
#define LLING_LLANG_HPP

#include "lling_llang.h"
#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <stdexcept>
#include <string>
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
        std::size_t written = 0;
        std::size_t required = 0;
        check(operation(value_, nullptr, 0, &written, &required));
        for (unsigned attempt = 0; attempt < 3; ++attempt) {
            std::vector<std::uint8_t> bytes(required);
            written = 0;
            std::size_t next_required = 0;
            check(operation(
                value_, bytes.empty() ? nullptr : bytes.data(), bytes.size(),
                &written, &next_required));
            if (written == next_required && next_required <= bytes.size()) {
                bytes.resize(written);
                return bytes;
            }
            required = next_required;
        }
        throw error(
            LLING_STATUS_PROVIDER_ERROR,
            "lattice byte length did not stabilize within three reads");
    }

    LlingLatticeValue* value_ = nullptr;
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
