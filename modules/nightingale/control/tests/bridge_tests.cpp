#include "../bridge.cpp"
#define main common_contract_checks
#include "../../../../apps/desktop/src-tauri/native/tests/world_save_bridge_tests.hpp"
#undef main

namespace {
unsigned periodic_calls{};
void* periodic_target{};
void periodic_capture(void** captured) {
    ++periodic_calls;
    periodic_target = *captured;
}
}
int main() {
    common_contract_checks();
    // The timer closure receives a pointer to its captured subsystem. A direct
    // UObject argument would reinterpret the object's first word as a pointer.
    const uintptr_t subsystem = 0x123456;
    native_save(reinterpret_cast<uintptr_t>(&periodic_capture) - SAVE_RVA, subsystem);
    assert(periodic_calls == 1 && periodic_target == reinterpret_cast<void*>(subsystem));
    std::puts("Native periodic request: one capture-based invocation; no storage confirmation.");
}
