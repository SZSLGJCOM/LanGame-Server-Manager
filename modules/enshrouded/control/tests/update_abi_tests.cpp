#include <cstdint>
#include <cstdio>
#include "../vendor/minhook/include/MinHook.h"
extern "C" void ControlUpdateObserver();

extern "C" std::uint64_t ProbeEntryAlignment();
extern "C" std::uint64_t ProbeCarry();
extern "C" std::uint64_t CallObserverWithCarry();
extern "C" { void* g_updateOriginal{}; std::uint64_t g_observedRcx{}, g_callbackCount{}; }
__declspec(noinline) std::uint64_t IntegerTarget(std::uint64_t a, std::uint64_t b,
    std::uint64_t c, std::uint64_t d, std::uint64_t e, std::uint64_t f) {
    return a + b * 3 + c * 5 + d * 7 + e * 11 + f * 13;
}
__declspec(noinline) double FloatingTarget(double a, std::uint64_t b, float c,
    std::uint64_t d, double e, float f, std::uint64_t g, double h) {
    return a + static_cast<double>(b) * 3 + c * 5 + static_cast<double>(d) * 7
        + e * 11 + f * 13 + static_cast<double>(g) * 17 + h * 19;
}
int main() {
    g_updateOriginal = reinterpret_cast<void*>(&IntegerTarget);
    auto integerObserver = reinterpret_cast<decltype(&IntegerTarget)>(&ControlUpdateObserver);
    const auto integerResult = integerObserver(101, 2, 3, 4, 5, 6);
    if (integerResult != IntegerTarget(101, 2, 3, 4, 5, 6) || g_observedRcx != 101) return 1;
    g_updateOriginal = reinterpret_cast<void*>(&FloatingTarget);
    auto floatingObserver = reinterpret_cast<decltype(&FloatingTarget)>(&ControlUpdateObserver);
    if (floatingObserver(1.25, 2, 3.5f, 4, 5.25, 6.5f, 7, 8.25)
        != FloatingTarget(1.25, 2, 3.5f, 4, 5.25, 6.5f, 7, 8.25)) return 2;
    g_updateOriginal = reinterpret_cast<void*>(&ProbeEntryAlignment);
    if (reinterpret_cast<std::uint64_t(*)()>(&ControlUpdateObserver)() != 8) return 3;
    g_updateOriginal = reinterpret_cast<void*>(&ProbeCarry);
    if (CallObserverWithCarry() != 1) return 4;
    if (MH_Initialize() != MH_OK) return 5;
    decltype(&IntegerTarget) volatile integerTarget = &IntegerTarget;
    const auto expectedInteger = integerTarget(101, 2, 3, 4, 5, 6);
    if (MH_CreateHook(reinterpret_cast<void*>(integerTarget), reinterpret_cast<void*>(&ControlUpdateObserver),
        &g_updateOriginal) != MH_OK || MH_EnableHook(reinterpret_cast<void*>(integerTarget)) != MH_OK) return 6;
    g_callbackCount = 0;
    if (integerTarget(101, 2, 3, 4, 5, 6) != expectedInteger || g_observedRcx != 101 || g_callbackCount != 1) return 7;
    if (MH_DisableHook(reinterpret_cast<void*>(integerTarget)) != MH_OK
        || MH_RemoveHook(reinterpret_cast<void*>(integerTarget)) != MH_OK) return 8;
    decltype(&FloatingTarget) volatile floatingTarget = &FloatingTarget;
    const auto expectedFloating = floatingTarget(1.25, 2, 3.5f, 4, 5.25, 6.5f, 7, 8.25);
    if (MH_CreateHook(reinterpret_cast<void*>(floatingTarget), reinterpret_cast<void*>(&ControlUpdateObserver),
        &g_updateOriginal) != MH_OK || MH_EnableHook(reinterpret_cast<void*>(floatingTarget)) != MH_OK) return 9;
    g_callbackCount = 0;
    if (floatingTarget(1.25, 2, 3.5f, 4, 5.25, 6.5f, 7, 8.25) != expectedFloating || g_callbackCount != 1) return 10;
    if (MH_DisableHook(reinterpret_cast<void*>(floatingTarget)) != MH_OK
        || MH_RemoveHook(reinterpret_cast<void*>(floatingTarget)) != MH_OK || MH_Uninitialize() != MH_OK) return 11;
    std::puts("PASS entry observer: 6 integer args, 8 mixed FP/stack args, integer/FP returns, RSP alignment, carry flag");
    std::puts("PASS compiled MinHook: owned synthetic integer/FP targets, relocated original trampolines");
    return 0;
}
