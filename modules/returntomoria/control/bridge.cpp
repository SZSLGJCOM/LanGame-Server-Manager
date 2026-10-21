// Moria build 21872765. Use the native synchronous current-world save result.
#include <windows.h>
#include <cstdint>
#include <string_view>
namespace {
constexpr std::string_view GAME_HASH = "4513fb58ca31a3c7c7640ebeff9717ef68776b922986036f0c5b325dd28255fb";
constexpr std::string_view UE4SS_HASH = "91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958";
constexpr wchar_t GAME_NAME[] = L"MoriaServer-Win64-Shipping.exe";
constexpr wchar_t BRIDGE_NAME[] = L"LgsmMoriaSaveControl.dll";
constexpr uintptr_t SAVE_RVA = 0x25512a0;
constexpr std::string_view SAVE_PROLOGUE = "48895c241855488bec4883ec60488bd9e8ab0c000084c00f849d01000048";
void target_contract(uintptr_t, uintptr_t, uintptr_t, uintptr_t);
const char* native_save(uintptr_t, uintptr_t);
}
#define LGSM_NATIVE_SAVE_RESULT
#define LGSM_SAVE_EXPORT luaopen_lgsm_moria_save
#include "../../../apps/desktop/src-tauri/native/world_save_bridge.hpp"
namespace {
void alive(uintptr_t object, size_t size) {
    require(readable(object, size) && !(read<uint32_t>(object + 8) & 0x40018010), "object_unavailable");
}
void target_contract(uintptr_t image, uintptr_t manager, uintptr_t instance, uintptr_t world) {
    alive(manager, 0x88); alive(instance, 0x28); alive(world, 0x188);
    using StaticClass = void* (*)();
    prologue(image, 0x28007c0, "e91b690000");
    auto cls = reinterpret_cast<StaticClass>(image + 0x28007c0)();
    require(cls && read<uintptr_t>(manager + 0x10) == reinterpret_cast<uintptr_t>(cls)
        && read<uintptr_t>(manager + 0x20) == instance && read<uintptr_t>(world + 0x180) == instance,
        "save_manager_owner");
    require(read<uint8_t>(manager + 0x70) == 1 && read<uint8_t>(manager + 0x71) == 1
        && !read<uint8_t>(manager + 0x72) && !read<uint8_t>(manager + 0x73)
        && !read<uint8_t>(manager + 0x60) && !read<int32_t>(manager + 0x74), "save_manager_not_ready");
    using Weak = void* (*)(void*);
    auto state = reinterpret_cast<uintptr_t>(reinterpret_cast<Weak>(image + 0x2bf9ed0)(reinterpret_cast<void*>(manager + 0x48)));
    alive(state, 0x622);
    auto level = read<uintptr_t>(state + 0x20); alive(level, 0xc0);
    require(read<uintptr_t>(level + 0xb8) == world && read<uint8_t>(state + 0x240) == 1
        && read<uint8_t>(state + 0x621) == 1, "save_world_not_ready");
    prologue(image, 0x256f880, "405556574156488dac24d8fdffff4881ec280300");
}
const char* completion_status(bool returned_success, uint8_t state, int32_t profiles, int32_t error) {
    if (!returned_success) return "failed";
    // A populated server can still be syncing character profiles after its
    // synchronous world file write. Never call that unfinished operation complete.
    return state == 0 && profiles == 0 && error == 0 ? "confirmed" : "returned";
}
const char* native_save(uintptr_t image, uintptr_t manager) {
    using Save = bool (*)(void*);
    const auto result = reinterpret_cast<Save>(image + SAVE_RVA)(reinterpret_cast<void*>(manager));
    return completion_status(result, read<uint8_t>(manager + 0x60), read<int32_t>(manager + 0x74), read<int32_t>(manager + 0x64));
}
}
