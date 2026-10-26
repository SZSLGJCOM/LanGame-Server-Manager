// Compile the production admission and byte checks unchanged. This executable
// never constructs Image, dispatches a request, or executes mapped game code.
#include "../bridge.cpp"
#include <iostream>

namespace {
constexpr std::string_view CURRENT_HASH = "210adebb1d4971c3a85b5210783c7b861eea9e153bf884a460840f43e96321ba";

struct ReadOnlyImage {
    Handle file, mapping;
    void* view{};
    explicit ReadOnlyImage(const fs::path& path)
        : file(open_file(path, GENERIC_READ, OPEN_EXISTING, FILE_SHARE_READ)) {
        require(hash_file(file.value) == CURRENT_HASH, "fixture_is_not_build_26161880");
        mapping.value = CreateFileMappingW(file.value, nullptr, PAGE_READONLY | SEC_IMAGE_NO_EXECUTE, 0, 0, nullptr);
        require(mapping.value != nullptr, "fixture_mapping");
        view = MapViewOfFile(mapping.value, FILE_MAP_READ, 0, 0, 0);
        require(view != nullptr, "fixture_view");
    }
    ~ReadOnlyImage() { if (view) UnmapViewOfFile(view); }
    uintptr_t base() const { return reinterpret_cast<uintptr_t>(view); }
};

uintptr_t relative_target(uintptr_t image, uintptr_t instruction, size_t displacement, size_t length) {
    return static_cast<uintptr_t>(static_cast<int64_t>(instruction + length)
        + read<int32_t>(image + instruction + displacement));
}

void call_target(uintptr_t image, uintptr_t instruction, uintptr_t expected) {
    require(read<uint8_t>(image + instruction) == 0xe8, "expected_native_call");
    require(relative_target(image, instruction, 1, 5) == expected, "wrong_native_call_target");
}

void reject_unknown_images() {
    for (const auto hash : {
            std::string_view("0000000000000000000000000000000000000000000000000000000000000000"),
            // Loader admission alone is insufficient native ABI evidence.
            std::string_view("ecaa5ebc6e0b08108856703a4aceed7b5cf0ea04d708b82dfc66ef74709a4269"),
            CURRENT_HASH.substr(1)}) {
        require(native_profile(hash) == nullptr, "unreviewed_image_admitted");
    }
    auto altered = std::string(CURRENT_HASH);
    altered[0] = '3';
    require(native_profile(altered) == nullptr, "altered_image_admitted");
    for (const auto hash : {
            "89ed65335a46db067a361d35ae4e6be80857a4c1f2453510600d632879dfbeed",
            "da1761a644a9d9b19da0c2da86ad98c7c4ce8ff265f710bf1897e3b007d2dcbe"}) {
        require(native_profile(hash) != nullptr, "previous_profile_removed");
    }
}

void reviewed_native_contract(const ReadOnlyImage& mapped) {
    const auto* profile = native_profile(CURRENT_HASH);
    require(profile != nullptr, "build_26161880_rejected_by_native_bridge");
    const auto image = mapped.base();
    for (const auto* code : {&profile->factory, &profile->ban, &profile->unban,
                             &profile->known_players, &profile->save_bans}) {
        MEMORY_BASIC_INFORMATION region{};
        require(VirtualQuery(reinterpret_cast<void*>(image + code->rva), &region, sizeof(region)) != 0
            && !(region.Protect & (PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY)),
            "fixture_code_must_not_be_executable");
        prologue(image, code->rva, code->prologue);
    }

    // Independently reviewed original call sites bind the profile to its actual
    // purpose, including functions with otherwise identical entry prefixes.
    call_target(image, 0x5b54e0c, profile->factory.rva);
    call_target(image, 0x5dbbbda, profile->ban.rva);
    call_target(image, 0x5dbbfb2, profile->unban.rva);
    call_target(image, 0x5d57376, profile->save_bans.rva);
    call_target(image, 0x5d549c1, profile->save_bans.rva);
    call_target(image, 0x5d55818, profile->known_players.rva);

    // StaticClass registrations and constructors, not a shared RVA increment.
    prologue(image, 0x557aec7, "488b05");
    prologue(image, 0x5696dd7, "488b05");
    prologue(image, 0x56995b9, "488d05");
    prologue(image, 0x5d358b7, "488d05");
    require(relative_target(image, 0x557aec7, 3, 7) == profile->settings_class, "settings_class_binding");
    require(relative_target(image, 0x5696dd7, 3, 7) == profile->matchmaker_class, "matchmaker_class_binding");
    require(relative_target(image, 0x56995b9, 3, 7) == profile->matchmaker_vtable, "matchmaker_vtable_binding");
    require(relative_target(image, 0x5d358b7, 3, 7) == profile->state_vtable, "state_vtable_binding");

    // Current object ownership, world and session preconditions used by dispatch.
    prologue(image, 0x3f0d306, "488b5920"); // subsystem UObject.Outer
    prologue(image, 0x5dc241d, "498d86c8060000"); // native shared state
    prologue(image, 0x5d358b3, "48895108"); // state owner
    prologue(image, 0x38bdca0, "488b41304885c07408488b80c0020000"); // GameInstance::GetWorld
    prologue(image, 0x5d56eea, "4883bb8803000000");
    prologue(image, 0x5d56ef8, "4883bb9803000000");
    require(read<uint32_t>(image + 0x951d700 + 0x32) == 0x228, "world_owner_property_offset");
    require(read<uint32_t>(image + 0x9bd1070 + 0x32) == 0xd0, "known_players_property_offset");

    // Known-player stride and ban flag, followed by the actual SaveConfig call.
    prologue(image, profile->known_players.rva + 0x18, "486381d8000000");
    prologue(image, profile->known_players.rva + 0x22, "4c6bf858");
    prologue(image, profile->known_players.rva + 0x13a, "807f4100");
    call_target(image, profile->known_players.rva + 0x18d, 0x15d8ad0);
    // The ban callee consumes one SharedUser through the reviewed release helper.
    call_target(image, profile->ban.rva + 0x506, 0x12bbbd0);
    prologue(image, profile->unban.rva + 0x22e, "498b5e084885db7429");

    // The production byte guard must reject a changed instruction, even after
    // selection of a known profile (the mapped image remains read-only).
    std::array<unsigned char, 32> changed{};
    std::memcpy(changed.data(), reinterpret_cast<const void*>(image + profile->factory.rva), changed.size());
    changed[0] ^= 1;
    bool rejected = false;
    try { prologue(reinterpret_cast<uintptr_t>(changed.data()), 0, profile->factory.prologue); }
    catch (const std::runtime_error& error) { rejected = std::string_view(error.what()) == "native_code"; }
    require(rejected, "changed_native_code_admitted");
}
} // namespace

int wmain(int argc, wchar_t** argv) {
    try {
        require(argc == 2, "usage_native_profile_tests_shipping_exe_path");
        const ReadOnlyImage mapped(argv[1]);
        reject_unknown_images();
        reviewed_native_contract(mapped);
        std::cout << "PASS: build 26161880 admission, PE call/layout bindings, unknown and changed-code rejection; no game code executed.\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "FAIL: " << error.what() << '\n';
        return 1;
    }
}
