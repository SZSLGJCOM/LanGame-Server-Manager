// Nightingale Steam build 22204011. Invoke the native periodic save queue once.
#include <windows.h>
#include <cstdint>
#include <string_view>
namespace {
constexpr std::string_view GAME_HASH = "8e06c60524066655aee66da2e06b52c702a951afc3c28b59a2923f9dd51bd7eb";
constexpr std::string_view UE4SS_HASH = "91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958";
constexpr wchar_t GAME_NAME[] = L"NWXServer-Win64-Shipping.exe";
constexpr wchar_t BRIDGE_NAME[] = L"LgsmNightingaleSaveControl.dll";
constexpr uintptr_t SAVE_RVA = 0x3f8d190;
constexpr std::string_view SAVE_PROLOGUE = "48895c240848896c24104889742418574883ec30488bf1e834a6ffff488bd848";
void target_contract(uintptr_t, uintptr_t, uintptr_t, uintptr_t);
void native_save(uintptr_t, uintptr_t);
}
#define LGSM_SAVE_EXPORT luaopen_lgsm_nightingale_save
#include "../../../apps/desktop/src-tauri/native/world_save_bridge.hpp"
namespace {
void target_contract(uintptr_t image, uintptr_t subsystem, uintptr_t instance, uintptr_t world) {
    const auto cls = read<uintptr_t>(image + 0x9f551e0);
    require(cls && readable(subsystem, 0x68) && read<uintptr_t>(subsystem + 0x10) == cls
        && !(read<uint32_t>(subsystem + 8) & 0x40018010), "subsystem_class");
    require(readable(instance, 0x28) && !(read<uint32_t>(instance + 8) & 0x40018010)
        && read<uintptr_t>(subsystem + 0x20) == instance, "subsystem_instance");
    using WeakWorld = void* (*)(void*);
    const auto native_world = reinterpret_cast<WeakWorld>(image + 0x1594190)(reinterpret_cast<void*>(subsystem + 0x60));
    require(reinterpret_cast<uintptr_t>(native_world) == world && readable(world, 0x28)
        && !(read<uint32_t>(world + 8) & 0x40018010), "subsystem_world");
}
void native_save(uintptr_t image, uintptr_t subsystem) {
    // The original timer closure receives the address of its captured subsystem,
    // not the UObject itself. It derives the filename and calls SaveWorldState
    // with async=true; the game owns disabled/load-state checks and task dispatch.
    auto captured = reinterpret_cast<void*>(subsystem);
    reinterpret_cast<void (*)(void**)>(image + SAVE_RVA)(&captured);
}
}
