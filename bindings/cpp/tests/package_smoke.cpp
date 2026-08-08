#include <lling_llang.hpp>

int main() {
    using namespace vinary_tree::lling_llang;
    builder value;
    const auto first = value.add_state();
    const auto second = value.add_state();
    value.start(first).final_state(second).arc(first, U'a', U'b', second);
    auto graph = value.build();
    auto resource = graph.retained_resource();
    return resource.get().context == nullptr ? 1 : 0;
}
