// Invoke the fixed game's ordinary forced save once on its owner thread.
#include <windows.h>
#include <cstdint>
#include <string_view>
namespace {
constexpr std::string_view GAME_HASH = "4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad";
constexpr std::string_view UE4SS_HASH = "91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958";
constexpr wchar_t GAME_NAME[] = L"SCUMServer.exe";
constexpr wchar_t BRIDGE_NAME[] = L"LgsmScumSaveControl.dll";
constexpr uintptr_t SAVE_RVA = 0x1b94040;
constexpr std::string_view SAVE_PROLOGUE = "48895c2420574881ecb0000000488b05a45484054833c4488984249000000080";
void target_contract(uintptr_t, uintptr_t, uintptr_t, uintptr_t);
void native_save(uintptr_t, uintptr_t);
}
#define LGSM_SAVE_EXPORT luaopen_lgsm_scum_save
#include "../../../apps/desktop/src-tauri/native/world_save_bridge.hpp"
namespace {
void alive(uintptr_t object, size_t size) {
    require(readable(object, size) && !(read<uint32_t>(object + 8) & 0x40018010), "object_unavailable");
}
void entity_contract(uintptr_t image, uintptr_t system, uintptr_t instance, uintptr_t world) {
    alive(system, 0x79c);
    auto cls = read<uintptr_t>(image + 0x7447070);
    require(cls && read<uintptr_t>(system + 0x10) == cls
        && read<uintptr_t>(system) == image + 0x5e40c60, "entity_system_class");
    alive(instance, 0x28);
    require(read<uintptr_t>(system + 0x20) == instance, "entity_system_instance");
    alive(world, 0x188);
    // These offsets are the fixed image's generated UWorld property metadata.
    require(read<uintptr_t>(world + 0x180) == instance, "world_instance");
    auto game_mode = read<uintptr_t>(world + 0x118);
    auto game_state = read<uintptr_t>(world + 0x120);
    alive(game_mode, 0xf1); alive(game_state, 0x5d);
    require(read<uint8_t>(game_mode + 0xf0) == 3, "world_authority");
    // AActor::EndPlay (3ec3aa0) accepts 0x80 in this two-bit native state,
    // then immediately clears it. This is not GameState's replication flag.
    require((read<uint8_t>(game_mode + 0x5c) & 0xc0) == 0x80
        && (read<uint8_t>(game_state + 0x5c) & 0xc0) == 0x80, "world_not_begun_play");
    require(read<uint8_t>(system + 0x30) == 1
        && read<uint8_t>(system + 0x798) == 0
        && read<uint8_t>(system + 0x79a) == 0
        && read<uint8_t>(system + 0x79b) == 0, "entity_system_not_ready");
}
void target_contract(uintptr_t image, uintptr_t system, uintptr_t instance, uintptr_t world) {
    entity_contract(image, system, instance, world);
    prologue(image, 0x15501c0, "488b49204885c9740a488b0148ffa06001000033c0c3");
    require(read<uintptr_t>(image + 0x5e40c60 + 0x160) == image + 0x15501c0, "world_accessor");
    using GetWorld = void* (*)(void*);
    require(reinterpret_cast<uintptr_t>(reinterpret_cast<GetWorld>(image + 0x15501c0)(
        reinterpret_cast<void*>(system))) == world, "entity_system_world");
}
void native_save(uintptr_t image, uintptr_t system) {
    // The native void return acknowledges invocation, not asynchronous SQL or
    // disk completion. The shared transport records only a returned receipt.
    using Save = void (*)(void*, bool);
    reinterpret_cast<Save>(image + SAVE_RVA)(reinterpret_cast<void*>(system), true);
}
}
