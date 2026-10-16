#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdint>
#include <cstdio>
#include "../identity_cache.h"
#include "../vendor/minhook/include/MinHook.h"
extern "C" {
void ControlCopyObserver(), LocalCopyProbeTarget();
void LocalCopyBody(), LocalCopyBeforeOriginal(), LocalCopyAfterOriginal();
void LocalCopyFlagsPushed(), LocalCopyFlagsPopped(), LocalCopyRestoreFlagsPushed();
void LocalCopyRestoreFlagsPopped(), LocalCopyEpilogueLea(), LocalCopyEpiloguePop(), LocalCopyEpilogueReturn();
std::uint64_t CallLocalCopyWithCarry(void*, void*, std::uint32_t);
void* g_copyOriginal{};
enshrouded_control::NativeLookupContext g_localContext{};
std::uint64_t g_localResult{}, g_localCallbacks{}, g_localCarry{};
}
int main() {
    std::uint64_t output[2]{};
    auto manager = reinterpret_cast<void*>(0x12345678ull);
    g_copyOriginal = reinterpret_cast<void*>(&LocalCopyProbeTarget);
    const auto success = CallLocalCopyWithCarry(output, manager, 64);
    if (success != 0x3ff4000000000000ull || g_localResult != success || g_localCarry != 1
        || g_localCallbacks != 1 || output[0] != 64 || output[1] != 0x12345678
        || g_localContext.output != reinterpret_cast<std::uintptr_t>(output)
        || g_localContext.manager != 0x12345678 || g_localContext.requestedHandle != 64
        || !g_localContext.caller) return 1;
    if (CallLocalCopyWithCarry(output, manager, UINT32_MAX) != 0x3ff400000000001full
        || (g_localResult & 0xff) != 0x1f || g_localCarry != 1 || g_localCallbacks != 2) return 2;
    using FloatingReturn = double(*)(void*, void*, std::uint32_t);
    if (reinterpret_cast<FloatingReturn>(&ControlCopyObserver)(output, manager, 64) != 1.25) return 3;
    using Target = std::uint64_t(*)(void*, void*, std::uint32_t);
    Target volatile target = reinterpret_cast<Target>(&LocalCopyProbeTarget);
    if (MH_Initialize() != MH_OK || MH_CreateHook(reinterpret_cast<void*>(target),
        reinterpret_cast<void*>(&ControlCopyObserver), &g_copyOriginal) != MH_OK
        || MH_EnableHook(reinterpret_cast<void*>(target)) != MH_OK) return 4;
    const auto previousCalls = g_localCallbacks;
    if (target(output, manager, 128) != success || g_localCallbacks != previousCalls + 1
        || output[0] != 128 || g_localContext.requestedHandle != 128) return 5;
    if (MH_DisableHook(reinterpret_cast<void*>(target)) != MH_OK
        || MH_RemoveHook(reinterpret_cast<void*>(target)) != MH_OK || MH_Uninitialize() != MH_OK) return 6;
    alignas(16) std::uint64_t stack[96]{};
    const auto entryRsp = reinterpret_cast<DWORD64>(&stack[65]);
    const auto frameRbp = entryRsp - 8, localsRsp = frameRbp - 0xe0;
    constexpr DWORD64 callerRbp = 0x22334455, callerRip = 0x12345678;
    *reinterpret_cast<DWORD64*>(entryRsp) = callerRip;
    *reinterpret_cast<DWORD64*>(frameRbp) = callerRbp;
    struct Point { void(*rip)(); DWORD64 rsp, rbp; };
    const Point points[] = {
        {LocalCopyBody, localsRsp, frameRbp}, {LocalCopyBeforeOriginal, localsRsp, frameRbp},
        {LocalCopyAfterOriginal, localsRsp, frameRbp}, {LocalCopyFlagsPushed, localsRsp - 8, frameRbp},
        {LocalCopyFlagsPopped, localsRsp, frameRbp}, {LocalCopyRestoreFlagsPushed, localsRsp - 8, frameRbp},
        {LocalCopyRestoreFlagsPopped, localsRsp, frameRbp}, {LocalCopyEpilogueLea, localsRsp, frameRbp},
        {LocalCopyEpiloguePop, frameRbp, frameRbp}, {LocalCopyEpilogueReturn, entryRsp, callerRbp},
    };
    for (const auto& point : points) {
        CONTEXT context{}; context.ContextFlags = CONTEXT_FULL;
        context.Rip = reinterpret_cast<DWORD64>(point.rip); context.Rsp = point.rsp; context.Rbp = point.rbp;
        DWORD64 imageBase{}, establisher{}; void* handlerData{};
        auto function = RtlLookupFunctionEntry(context.Rip, &imageBase, nullptr);
        if (!function) return 7;
        RtlVirtualUnwind(UNW_FLAG_NHANDLER, imageBase, context.Rip, function, &context,
            &handlerData, &establisher, nullptr);
        if (context.Rip != callerRip || context.Rsp != entryRsp + 8 || context.Rbp != callerRbp) return 8;
    }
    std::puts("PASS local-copy observer: original 3 args, full RAX/AL success/failure, XMM0 return, CF, callback, MinHook, 10 unwind RIPs");
    return 0;
}
