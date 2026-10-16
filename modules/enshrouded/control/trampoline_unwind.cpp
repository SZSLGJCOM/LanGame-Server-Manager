#include "trampoline_unwind.h"
#include <cstring>
#include <algorithm>
namespace enshrouded_control {
namespace {
constexpr unsigned char kUpdatePrefix[] = {0x48,0x8b,0xc4,0x55,0x53};
constexpr unsigned char kCopyPrefix[] = {0x40,0x53,0x48,0x83,0xec,0x20};
// V2 has no epilogue descriptors here: FF25 transfers an existing frame to the
// native remainder, and must not be interpreted as a completed tail epilogue.
// Every host validates actual lookup/unwind behavior before enabling the hooks.
constexpr unsigned char kUpdateUnwind[] = {2,5,2,0, 5,0x30, 4,0x50};
constexpr unsigned char kCopyUnwind[] = {2,6,2,0, 6,0x32, 2,0x30};
void* AllocateNear(std::uintptr_t code) noexcept {
    SYSTEM_INFO info{}; GetSystemInfo(&info);
    const auto granularity = static_cast<std::uintptr_t>(info.dwAllocationGranularity);
    if (!granularity) return nullptr;
    const auto aligned = code - code % granularity;
    for (std::uintptr_t i = 1; i <= 256; ++i) {
        const auto distance = i * granularity;
        const std::uintptr_t hints[] = {aligned >= distance ? aligned - distance : 0,
            aligned <= UINTPTR_MAX - distance ? aligned + distance : 0};
        for (const auto hint : hints) {
            if (!hint) continue;
            if (auto page = VirtualAlloc(reinterpret_cast<void*>(hint), info.dwPageSize,
                MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE)) return page;
        }
    }
    return nullptr;
}
bool VerifyLookupAndUnwind(const TrampolineUnwind& registration, std::uintptr_t code, bool update) noexcept {
    alignas(16) DWORD64 stack[96]{};
    const auto entryRsp = reinterpret_cast<DWORD64>(&stack[65]);
    constexpr DWORD64 callerRip = 0x12345678, callerRbx = 0x34567890, callerRbp = 0x23456789;
    *reinterpret_cast<DWORD64*>(entryRsp) = callerRip;
    if (update) {
        *reinterpret_cast<DWORD64*>(entryRsp - 8) = callerRbp;
        *reinterpret_cast<DWORD64*>(entryRsp - 16) = callerRbx;
    } else *reinterpret_cast<DWORD64*>(entryRsp - 8) = callerRbx;
    const unsigned relocated = update ? 5 : 6;
    for (unsigned offset = 0; offset < relocated + 14; ++offset) {
        CONTEXT context{}; context.ContextFlags = CONTEXT_FULL;
        context.Rip = code + offset; context.Rsp = entryRsp;
        if (update) context.Rsp -= offset >= 5 ? 16 : offset >= 4 ? 8 : 0;
        else context.Rsp -= offset >= 6 ? 40 : offset >= 2 ? 8 : 0;
        context.Rbp = callerRbp; context.Rbx = callerRbx;
        DWORD64 imageBase{}, establisher{}; void* handlerData{};
        const auto function = RtlLookupFunctionEntry(context.Rip, &imageBase, nullptr);
        if (function != &registration.function || imageBase != registration.base) return false;
        RtlVirtualUnwind(UNW_FLAG_NHANDLER, imageBase, context.Rip, function, &context,
            &handlerData, &establisher, nullptr);
        if (context.Rip != callerRip || context.Rsp != entryRsp + 8
            || context.Rbp != callerRbp || context.Rbx != callerRbx) return false;
    }
    return true;
}
bool GuardedVerify(const TrampolineUnwind& registration, std::uintptr_t code, bool update) noexcept {
    __try { return VerifyLookupAndUnwind(registration, code, update); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
}
bool RegisterTrampolineUnwind(TrampolineUnwind& registration, void* trampoline,
    void* original, TrampolineKind kind) noexcept {
    if (registration.registered || !trampoline || !original) return false;
    const bool update = kind == TrampolineKind::UpdatePlayers;
    const auto count = update ? sizeof kUpdatePrefix : sizeof kCopyPrefix;
    const auto prefix = update ? kUpdatePrefix : kCopyPrefix;
    const auto unwind = update ? kUpdateUnwind : kCopyUnwind;
    unsigned char bytes[20]{}; SIZE_T read{};
    const auto code = reinterpret_cast<std::uintptr_t>(trampoline), entry = reinterpret_cast<std::uintptr_t>(original);
    if (entry > UINTPTR_MAX - count || !ReadProcessMemory(GetCurrentProcess(), trampoline, bytes, count + 14, &read)
        || read != count + 14 || std::memcmp(bytes, prefix, count)) return false;
    constexpr unsigned char absoluteJump[] = {0xff,0x25,0,0,0,0};
    std::uintptr_t destination{}; std::memcpy(&destination, bytes + count + 6, sizeof destination);
    if (std::memcmp(bytes + count, absoluteJump, sizeof absoluteJump) || destination != entry + count) return false;
    auto page = AllocateNear(code); if (!page) return false;
    const auto metadata = reinterpret_cast<std::uintptr_t>(page) + 16;
    const auto base = (std::min)(code, reinterpret_cast<std::uintptr_t>(page));
    const auto begin = code - base, unwindRva = metadata - base;
    if (begin > UINT32_MAX - (count + 14) || unwindRva > UINT32_MAX) {
        VirtualFree(page, 0, MEM_RELEASE); return false;
    }
    std::memcpy(reinterpret_cast<void*>(metadata), unwind, sizeof kUpdateUnwind);
    DWORD previous{};
    if (!VirtualProtect(page, 32, PAGE_READONLY, &previous)) { VirtualFree(page, 0, MEM_RELEASE); return false; }
    registration.function.BeginAddress = static_cast<DWORD>(begin);
    registration.function.EndAddress = static_cast<DWORD>(begin + count + 14);
    registration.function.UnwindData = static_cast<DWORD>(unwindRva);
    registration.metadata = page; registration.base = base;
    if (!RtlAddFunctionTable(&registration.function, 1, base)) {
        registration = {}; VirtualFree(page, 0, MEM_RELEASE); return false;
    }
    registration.registered = true;
    // A failed verification retains this pinned table/page until process exit,
    // while the caller leaves all hooks disabled and the native server intact.
    return GuardedVerify(registration, code, update);
}
} // namespace enshrouded_control
