#pragma once
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <cstdint>
#include <string>
#include "control_protocol.h"
namespace enshrouded_control {
inline constexpr char kExeSha256[] = "4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba";
struct NativeProfile { std::uintptr_t imageBase{}; void* updatePlayers{}; void* localCopy{}; void* accountAction{}; };
bool ReadCurrentMemory(void*, std::uintptr_t address, void* out, std::size_t bytes) noexcept;
std::wstring ModuleFile(HMODULE module);
bool ValidateNativeProfile(NativeProfile& profile);
bool CreateProcessIdentity(ProcessIdentity& identity) noexcept;
} // namespace enshrouded_control
