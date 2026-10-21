#include "native_unwind.h"
#include <algorithm>
#include <cstring>
namespace vrising_save {
namespace {
constexpr unsigned char trigger_prefix[] = {0x40,0x55,0x53,0x41,0x54};
constexpr unsigned char serialize_prefix[] = {0x48,0x89,0x4c,0x24,0x08};
constexpr unsigned char finalize_prefix[] = {0x40,0x53,0x48,0x83,0xec,0x20};
// Version 2 with no epilogue descriptor: the FF25 branch transfers this active
// frame to the native remainder; it does not complete a tail epilogue.
constexpr unsigned char trigger_unwind[] = {2,5,3,0,5,0xc0,3,0x30,2,0x50,0,0};
constexpr unsigned char serialize_unwind[] = {2,5,0,0};
constexpr unsigned char finalize_unwind[] = {2,6,2,0,6,0x32,2,0x30};
void* allocate_near(std::uintptr_t code) noexcept {
    SYSTEM_INFO info{}; GetSystemInfo(&info);
    const auto granularity = static_cast<std::uintptr_t>(info.dwAllocationGranularity);
    if (!granularity) return nullptr;
    const auto aligned = code - code % granularity;
    for (std::uintptr_t i = 1; i <= 256; ++i) {
        const auto distance = i * granularity;
        const std::uintptr_t hints[] = {aligned >= distance ? aligned - distance : 0,
            aligned <= UINTPTR_MAX - distance ? aligned + distance : 0};
        for (auto hint : hints) if (hint)
            if (auto page = VirtualAlloc(reinterpret_cast<void*>(hint), info.dwPageSize,
                MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE)) return page;
    }
    return nullptr;
}
bool verify(const UnwindRegistration& registration, std::uintptr_t code, HookKind kind, unsigned count) noexcept {
    alignas(16) DWORD64 stack[96]{};
    const auto entry = reinterpret_cast<DWORD64>(&stack[65]);
    constexpr DWORD64 rip = 0x12345678, rbp = 0x23456789, rbx = 0x34567890, r12 = 0x456789ab;
    *reinterpret_cast<DWORD64*>(entry) = rip;
    for (unsigned offset = 0; offset < count + 14; ++offset) {
        CONTEXT context{}; context.ContextFlags = CONTEXT_FULL;
        context.Rip = code + offset; context.Rsp = entry;
        context.Rbp = rbp; context.Rbx = rbx; context.R12 = r12;
        if (kind == HookKind::Trigger) {
            *reinterpret_cast<DWORD64*>(entry - 8) = rbp;
            *reinterpret_cast<DWORD64*>(entry - 16) = rbx;
            *reinterpret_cast<DWORD64*>(entry - 24) = r12;
            context.Rsp -= offset >= 5 ? 24 : offset >= 3 ? 16 : offset >= 2 ? 8 : 0;
        } else if (kind == HookKind::Finalize) {
            *reinterpret_cast<DWORD64*>(entry - 8) = rbx;
            context.Rsp -= offset >= 6 ? 40 : offset >= 2 ? 8 : 0;
        }
        DWORD64 base{}, establisher{}; void* data{};
        auto function = RtlLookupFunctionEntry(context.Rip, &base, nullptr);
        if (function != &registration.function || base != registration.base) return false;
        RtlVirtualUnwind(UNW_FLAG_NHANDLER, base, context.Rip, function, &context, &data, &establisher, nullptr);
        if (context.Rip != rip || context.Rsp != entry + 8 || context.Rbp != rbp || context.Rbx != rbx || context.R12 != r12)
            return false;
    }
    return true;
}
bool guarded_verify(const UnwindRegistration& registration, std::uintptr_t code, HookKind kind, unsigned count) noexcept {
    __try { return verify(registration, code, kind, count); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
}
bool register_unwind(UnwindRegistration& registration, void* trampoline, void* original, HookKind kind) noexcept {
    if (registration.registered || !trampoline || !original) return false;
    const auto prefix = kind == HookKind::Trigger ? trigger_prefix : kind == HookKind::Serialize ? serialize_prefix : finalize_prefix;
    const unsigned count = kind == HookKind::Finalize ? 6 : 5;
    const auto unwind = kind == HookKind::Trigger ? trigger_unwind : kind == HookKind::Serialize ? serialize_unwind : finalize_unwind;
    const unsigned size = kind == HookKind::Trigger ? sizeof trigger_unwind : kind == HookKind::Serialize ? sizeof serialize_unwind : sizeof finalize_unwind;
    unsigned char bytes[20]{}; SIZE_T read{};
    const auto code = reinterpret_cast<std::uintptr_t>(trampoline), entry = reinterpret_cast<std::uintptr_t>(original);
    if (!ReadProcessMemory(GetCurrentProcess(), trampoline, bytes, count + 14, &read) || read != count + 14 || std::memcmp(bytes, prefix, count)) return false;
    constexpr unsigned char jump[] = {0xff,0x25,0,0,0,0};
    std::uintptr_t destination{}; std::memcpy(&destination, bytes + count + 6, sizeof destination);
    if (std::memcmp(bytes + count, jump, sizeof jump) || destination != entry + count) return false;
    auto page = allocate_near(code); if (!page) return false;
    const auto metadata = reinterpret_cast<std::uintptr_t>(page) + 16;
    const auto base = (std::min)(code, reinterpret_cast<std::uintptr_t>(page));
    const auto begin = code - base, unwind_rva = metadata - base;
    if (begin > UINT32_MAX - (count + 14) || unwind_rva > UINT32_MAX) { VirtualFree(page, 0, MEM_RELEASE); return false; }
    std::memcpy(reinterpret_cast<void*>(metadata), unwind, size);
    DWORD previous{};
    if (!VirtualProtect(page, 32, PAGE_READONLY, &previous)) { VirtualFree(page, 0, MEM_RELEASE); return false; }
    registration.function.BeginAddress = static_cast<DWORD>(begin);
    registration.function.EndAddress = static_cast<DWORD>(begin + count + 14);
    registration.function.UnwindData = static_cast<DWORD>(unwind_rva);
    registration.metadata = page; registration.base = base;
    if (!RtlAddFunctionTable(&registration.function, 1, base)) { registration = {}; VirtualFree(page, 0, MEM_RELEASE); return false; }
    registration.registered = true;
    // Tables/code remain pinned until process exit, including a failed verify.
    return guarded_verify(registration, code, kind, count);
}
}
