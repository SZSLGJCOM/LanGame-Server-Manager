// Exercise the production native ownership/readiness gates without a game or save.
#include "../bridge.cpp"
#include <cassert>
#include <functional>
#include <iostream>
namespace {
unsigned calls = 0;
void* received_system = nullptr;
bool received_force = false;
void save_boundary(void* system, bool force) {
    ++calls; received_system = system; received_force = force;
}
template<class T> void put(uintptr_t address, T value) {
    std::memcpy(reinterpret_cast<void*>(address), &value, sizeof(value));
}
void rejects(const std::function<void()>& action, std::string_view expected) {
    bool rejected = false;
    try { action(); } catch (const std::runtime_error& error) { rejected = error.what() == expected; }
    assert(rejected);
}
}
void check_profile(const GameProfile& profile) {
    assert(select_game_profile(profile.image_hash));
    calls = 0;
    // Zero-filled private memory represents only documented object fields. No
    // game function pointer, DLL loader or persistence routine is executed.
    // The class singleton is the highest image address touched by this fixture.
    const auto image_size = profile.entity_class + sizeof(uintptr_t);
    auto allocated = VirtualAlloc(nullptr, image_size, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    assert(allocated);
    auto image = reinterpret_cast<uintptr_t>(allocated);
    std::array<unsigned char, 0x800> system_bytes{};
    std::array<unsigned char, 0x200> instance_bytes{}, world_bytes{}, mode_bytes{}, state_bytes{};
    auto system = reinterpret_cast<uintptr_t>(system_bytes.data());
    auto instance = reinterpret_cast<uintptr_t>(instance_bytes.data());
    auto world = reinterpret_cast<uintptr_t>(world_bytes.data());
    auto mode = reinterpret_cast<uintptr_t>(mode_bytes.data());
    auto state = reinterpret_cast<uintptr_t>(state_bytes.data());
    put(image + profile.entity_class, image + 0x100);
    put(system, image + profile.entity_vtable); put(system + 0x10, image + 0x100);
    put(system + 0x20, instance); put<uint8_t>(system + 0x30, 1);
    put(world + 0x180, instance); put(world + 0x118, mode); put(world + 0x120, state);
    put<uint8_t>(mode + 0xf0, 3); put<uint8_t>(mode + 0x5c, 0x80); put<uint8_t>(state + 0x5c, 0x80);
    auto validate = [&] { entity_contract(image, system, instance, world); };
    validate();
    native_save(reinterpret_cast<uintptr_t>(&save_boundary) - SAVE_RVA, system);
    assert(calls == 1 && received_system == reinterpret_cast<void*>(system) && received_force);
    for (auto offset : {0x30, 0x798, 0x79a, 0x79b}) {
        auto previous = read<uint8_t>(system + offset);
        put<uint8_t>(system + offset, previous == 0 ? 1 : 0);
        rejects(validate, "entity_system_not_ready"); put<uint8_t>(system + offset, previous);
    }
    put<uint8_t>(mode + 0xf0, 1); rejects(validate, "world_authority"); put<uint8_t>(mode + 0xf0, 3);
    for (auto actor : {mode, state}) {
        for (uint8_t lifecycle : std::array<uint8_t, 3>{0x00, 0x40, 0xc0}) {
            put<uint8_t>(actor + 0x5c, lifecycle); rejects(validate, "world_not_begun_play");
        }
        put<uint8_t>(actor + 0x5c, 0x80);
    }
    put(system + 0x20, world); rejects(validate, "entity_system_instance"); put(system + 0x20, instance);
    put(world + 0x180, world); rejects(validate, "world_instance"); put(world + 0x180, instance);
    put(system + 0x10, world); rejects(validate, "entity_system_class"); put(system + 0x10, image + 0x100);
    put(system, world); rejects(validate, "entity_system_class"); put(system, image + profile.entity_vtable);
    for (uint32_t flag : {0x10u, 0x8000u, 0x10000u, 0x40000000u}) {
        put(system + 8, flag); rejects(validate, "object_unavailable"); put<uint32_t>(system + 8, 0);
    }
    validate();
    const std::string frame = "schema=1\nnonce=1234567890abcdef1234567890abcdef\nop=save\nrequested_at=123\nproxy_address=456\ninstance_address=789\nworld_address=321\n";
    auto parsed = parse(frame); assert(parsed.world == 321 && parsed.operation == "save");
    rejects([&] { (void)parse(frame + "extra=1\n"); }, "request_format");
    rejects([&] { (void)parse(frame.substr(0, frame.size() - 1)); }, "request_format");
    assert(VirtualFree(allocated, 0, MEM_RELEASE));
    std::cout << "SCUM native ownership, authority, lifecycle, loading, busy, shutdown and frame guards passed\n";
}

int main() {
    for (const auto& profile : PROFILES) {
        check_profile(profile);
        assert(&game_profile() == &profile);
        assert(!select_game_profile(std::string(64, '0')));
        rejects([] { (void)game_profile(); }, "game_profile_unavailable");
    }
}
