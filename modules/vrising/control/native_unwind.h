#pragma once
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdint>
namespace vrising_save {
enum class HookKind { Trigger, Serialize, Finalize };
struct UnwindRegistration {
    RUNTIME_FUNCTION function{};
    void* metadata{};
    std::uintptr_t base{};
    bool registered{};
};
bool register_unwind(UnwindRegistration& registration, void* trampoline, void* original, HookKind kind) noexcept;
}
