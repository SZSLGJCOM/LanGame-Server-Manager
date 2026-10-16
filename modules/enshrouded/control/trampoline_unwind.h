#pragma once
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdint>
namespace enshrouded_control {
enum class TrampolineKind { UpdatePlayers, LocalCopy };
// Registration and the MinHook code it describes must live until process exit.
struct TrampolineUnwind { RUNTIME_FUNCTION function{}; void* metadata{}; std::uintptr_t base{}; bool registered{}; };
bool RegisterTrampolineUnwind(TrampolineUnwind& registration, void* trampoline,
    void* original, TrampolineKind kind) noexcept;
} // namespace enshrouded_control
