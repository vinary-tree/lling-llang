#ifndef LLING_LLANG_HPP
#define LLING_LLANG_HPP

#include "lling_llang.h"
#include <cstdint>
#include <stdexcept>
#include <utility>

namespace vinary_tree::lling_llang {

class error final : public std::runtime_error {
public:
    explicit error(LlingStatus status)
        : std::runtime_error(lling_last_error_message()), status_(status) {}
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
