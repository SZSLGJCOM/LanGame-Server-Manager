#pragma once
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <cstdint>
#include <string>
#include "control_protocol.h"
namespace enshrouded_control {
struct NativeProfile {
    std::uintptr_t imageBase{};
    void* updatePlayers{};
    void* localCopy{};
    void* accountAction{};
    std::uintptr_t callerA{}, callerB{};
    const char* exeSha256{};
    void* saveWorld{};
};
bool ReadCurrentMemory(void*, std::uintptr_t address, void* out, std::size_t bytes) noexcept;
std::wstring ModuleFile(HMODULE module);
bool ValidateNativeProfile(NativeProfile& profile);
// The runtime wrapper binds these to its current executable and loaded image.
// An inert PE image also exercises the exact identity/entry checks without running game code.
bool ValidateNativeProfileImage(const wchar_t* path, std::uintptr_t imageBase, NativeProfile& profile);
bool CreateProcessIdentity(ProcessIdentity& identity) noexcept;
} // namespace enshrouded_control
