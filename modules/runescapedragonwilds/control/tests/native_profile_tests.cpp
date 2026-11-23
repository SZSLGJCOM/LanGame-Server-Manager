// Compile the production admission and byte checks unchanged. This executable
// never constructs Image, dispatches a request, or executes mapped game code.
#include "../bridge.cpp"
#include <iostream>

namespace {
constexpr std::string_view LEGACY_HASH = "210adebb1d4971c3a85b5210783c7b861eea9e153bf884a460840f43e96321ba";
constexpr std::string_view PREVIOUS_HASH = "f7840d934fccb6f3ca0b01e781abf4463f3718ebb29418985cd9281bbd3a8fb2";
constexpr std::string_view BUILD_26574459_HASH = "c0fe936373671887173ddf99d7665e9308a6a7561b414d738aaf0c71b64839f9";
constexpr std::string_view CURRENT_HASH = "5bb9c59d35ac2669b632f3368d64b116173e5fb68f969ca8b6eda2b84f788cf9";

struct ReadOnlyImage {
    Handle file, mapping;
    std::string shipping_hash;
    void* view{};
    explicit ReadOnlyImage(const fs::path& path)
        : file(open_file(path, GENERIC_READ, OPEN_EXISTING, FILE_SHARE_READ)) {
        shipping_hash = hash_file(file.value);
        require(shipping_hash == CURRENT_HASH || shipping_hash == BUILD_26574459_HASH
            || shipping_hash == PREVIOUS_HASH || shipping_hash == LEGACY_HASH,
            "fixture_is_not_a_reviewed_build");
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
            "da1761a644a9d9b19da0c2da86ad98c7c4ce8ff265f710bf1897e3b007d2dcbe",
            "210adebb1d4971c3a85b5210783c7b861eea9e153bf884a460840f43e96321ba",
            "f7840d934fccb6f3ca0b01e781abf4463f3718ebb29418985cd9281bbd3a8fb2",
            "c0fe936373671887173ddf99d7665e9308a6a7561b414d738aaf0c71b64839f9"}) {
        require(native_profile(hash) != nullptr, "previous_profile_removed");
    }
}

void reviewed_native_contract(const ReadOnlyImage& mapped) {
    const auto* profile = native_profile(mapped.shipping_hash);
    require(profile != nullptr, mapped.shipping_hash == CURRENT_HASH
        ? "build_26670300_rejected_by_native_bridge" : mapped.shipping_hash == BUILD_26574459_HASH
        ? "build_26574459_rejected_by_native_bridge" : mapped.shipping_hash == PREVIOUS_HASH
        ? "build_26419371_rejected_by_native_bridge" : "build_26161880_rejected_by_native_bridge");
    const auto image = mapped.base();
    for (const auto* code : {&profile->factory, &profile->ban, &profile->unban,
                             &profile->known_players, &profile->save_bans}) {
        MEMORY_BASIC_INFORMATION region{};
        require(VirtualQuery(reinterpret_cast<void*>(image + code->rva), &region, sizeof(region)) != 0
            && !(region.Protect & (PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY)),
            "fixture_code_must_not_be_executable");
        prologue(image, code->rva, code->prologue);
    }

    if (mapped.shipping_hash == LEGACY_HASH) {
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

    } else if (mapped.shipping_hash == PREVIOUS_HASH) {
        // Build 26419371: independently located wrappers, reflected class
        // registrations and constructors; no shared RVA delta is assumed.
        call_target(image, 0x5b5d47c, profile->factory.rva);
        call_target(image, 0x5dbcbaa, profile->ban.rva);
        call_target(image, 0x5dbcf82, profile->unban.rva);
        call_target(image, 0x5d72686, profile->save_bans.rva);
        call_target(image, 0x5d712d1, profile->save_bans.rva);
        call_target(image, 0x5d71ca8, profile->known_players.rva);

        prologue(image, 0x557f557, "488b05");
        prologue(image, 0x569be17, "488b05");
        prologue(image, 0x569e639, "488d05");
        prologue(image, 0x5d584a7, "488d05");
        require(relative_target(image, 0x557f557, 3, 7) == profile->settings_class, "settings_class_binding");
        require(relative_target(image, 0x569be17, 3, 7) == profile->matchmaker_class, "matchmaker_class_binding");
        require(relative_target(image, 0x569e639, 3, 7) == profile->matchmaker_vtable, "matchmaker_vtable_binding");
        require(relative_target(image, 0x5d584a7, 3, 7) == profile->state_vtable, "state_vtable_binding");
        prologue(image, 0x557f5de, "c744242008010000"); // Settings size 0x108.
        prologue(image, 0x569bea5, "c744242030070000"); // Matchmaker size 0x730.
        prologue(image, 0x5dc395e, "b9e0030000"); // 0x10 controller + 0x3d0 state.
        call_target(image, 0x5dc3997, 0x5d584a0);

        // The initializer obtains the same Outer GameInstance supplied to the
        // state's constructor, then publishes it at Matchmaker + 0x6c8.
        call_target(image, 0x5dc3842, 0x3f10970);
        prologue(image, 0x3f10976, "488b5920");
        prologue(image, 0x5dc39cd, "498d86c8060000");
        prologue(image, 0x5d584a3, "48895108");
        prologue(image, 0x38c1220, "488b41304885c07408488b80c0020000");
        prologue(image, 0x5d7223a, "4883bb8803000000");
        prologue(image, 0x5d72248, "4883bb9803000000");
        require(read<uint32_t>(image + 0x952e410 + 0x32) == 0x228, "world_owner_property_offset");
        require(read<uint32_t>(image + 0x9be3d70 + 0x32) == 0xd0, "known_players_property_offset");

        prologue(image, profile->known_players.rva + 0x18, "486381d8000000");
        prologue(image, profile->known_players.rva + 0x22, "4c6bf858");
        prologue(image, profile->known_players.rva + 0x13a, "807f4100");
        call_target(image, profile->known_players.rva + 0x18d, 0x15db640);
        prologue(image, 0x5d7195c, "4c8dbf98020000"); // Persist the native ban map.

        // Factory RCX=KnownPlayer, RDX=sret; copy the identity and FString,
        // return one owned SharedUser, and release it exactly once in each API.
        prologue(image, 0x5b5d474, "488d55d7488d4de7");
        prologue(image, profile->factory.rva + 0x1a, "488bf24c8bf1");
        call_target(image, 0x59ab335, 0x38c0f00);
        prologue(image, 0x59ab3eb, "4d8d4630488d542440");
        call_target(image, 0x59ab3f4, 0x5981280);
        prologue(image, 0x59ab3f9, "488d471048890648897e08");
        call_target(image, profile->ban.rva + 0x4c6, 0x12be520);
        prologue(image, 0x12be539, "8bc7f00fc1430883f801");
        prologue(image, profile->unban.rva + 0x22e, "498b5e084885db7429");
    } else if (mapped.shipping_hash == BUILD_26574459_HASH) {
        // Build 26574459: independently reviewed wrappers, class registrations
        // and constructors. Preserve the earlier images' separate bindings.
        call_target(image, 0x5b5e7ec, profile->factory.rva);
        call_target(image, 0x5dbdf8a, profile->ban.rva);
        call_target(image, 0x5dbe362, profile->unban.rva);
        call_target(image, 0x5d73ab6, profile->save_bans.rva);
        call_target(image, 0x5d72701, profile->save_bans.rva);
        call_target(image, 0x5d730d8, profile->known_players.rva);

        prologue(image, 0x55808e7, "488b05");
        prologue(image, 0x569d247, "488b05");
        prologue(image, 0x569fa69, "488d05");
        prologue(image, 0x5d598d7, "488d05");
        require(relative_target(image, 0x55808e7, 3, 7) == profile->settings_class, "settings_class_binding");
        require(relative_target(image, 0x569d247, 3, 7) == profile->matchmaker_class, "matchmaker_class_binding");
        require(relative_target(image, 0x569fa69, 3, 7) == profile->matchmaker_vtable, "matchmaker_vtable_binding");
        require(relative_target(image, 0x5d598d7, 3, 7) == profile->state_vtable, "state_vtable_binding");
        prologue(image, 0x558096e, "c744242008010000"); // Settings size 0x108.
        prologue(image, 0x569d2d5, "c744242030070000"); // Matchmaker size 0x730.
        prologue(image, 0x5dc4d3e, "b9e0030000"); // 0x10 controller + 0x3d0 state.
        call_target(image, 0x5dc4d77, 0x5d598d0);

        call_target(image, 0x5dc4c22, 0x3f11870); // Initializer obtains the Outer GameInstance.
        prologue(image, 0x3f11876, "488b5920");
        prologue(image, 0x5dc4dad, "498d86c8060000"); // Publish state at Matchmaker + 0x6c8.
        prologue(image, 0x5d598d3, "48895108"); // State owner.
        prologue(image, 0x38c1f30, "488b41304885c07408488b80c0020000");
        prologue(image, 0x5d7366a, "4883bb8803000000");
        prologue(image, 0x5d73678, "4883bb9803000000");
        require(read<uint32_t>(image + 0x95329a0 + 0x32) == 0x228, "world_owner_property_offset");
        require(read<uint32_t>(image + 0x9be7bf0 + 0x32) == 0xd0, "known_players_property_offset");

        prologue(image, profile->known_players.rva + 0x18, "486381d8000000");
        prologue(image, profile->known_players.rva + 0x22, "4c6bf858");
        prologue(image, profile->known_players.rva + 0x13a, "807f4100");
        call_target(image, profile->known_players.rva + 0x18d, 0x15dc370);
        prologue(image, 0x32e3bb0, "c6414101c3"); // Reflected bIsBanned setter uses +0x41.
        prologue(image, 0x5d72d8c, "4c8dbf98020000"); // Persist the native ban map.

        // Factory arguments and owned identity/name copies retain the same ABI.
        prologue(image, 0x5b5e7e4, "488d55d7488d4de7");
        prologue(image, profile->factory.rva + 0x1a, "488bf24c8bf1");
        call_target(image, 0x59ac6a5, 0x38c1c10);
        prologue(image, 0x59ac75b, "4d8d4630488d542440");
        call_target(image, 0x59ac764, 0x5982640);
        prologue(image, 0x59ac769, "488d471048890648897e08");
        call_target(image, profile->ban.rva + 0x4c6, 0x12bf420);
        prologue(image, 0x12bf439, "8bc7f00fc1430883f801");
        prologue(image, profile->unban.rva + 0x22e, "498b5e084885db7429");

        // SaveBans' indirect variant tables also moved. Their copy/release
        // operands and leaf implementations were reviewed separately.
        prologue(image, 0x5d72f88, "42ff94c328f13009");
        prologue(image, 0x5d72fb6, "ff94c328f13009");
        prologue(image, 0x5d73068, "ff94c728f13009");
        prologue(image, 0x5d72ff6, "ff94c770a20309");
        prologue(image, 0x5d730af, "ff94c770a20309");
        prologue(image, 0x38b1a40, "488b02488901488b4208488941084885c07404f0ff4008c3");
        prologue(image, 0x14610e0, "8b028901c3");
        prologue(image, 0x123fb30, "c20000");
    } else {
        // Build 26670300 (1.0.2.2): independently reviewed wrappers, class
        // registrations, constructors and variant tables.
        call_target(image, 0x5b5ccbc, profile->factory.rva);
        call_target(image, 0x5dbc4da, profile->ban.rva);
        call_target(image, 0x5dbc8b2, profile->unban.rva);
        call_target(image, 0x5d71fb6, profile->save_bans.rva);
        call_target(image, 0x5d70c01, profile->save_bans.rva);
        call_target(image, 0x5d715d8, profile->known_players.rva);

        prologue(image, 0x557e867, "488b05");
        prologue(image, 0x569af57, "488b05");
        prologue(image, 0x569d779, "488d05");
        prologue(image, 0x5d57dd7, "488d05");
        require(relative_target(image, 0x557e867, 3, 7) == profile->settings_class, "settings_class_binding");
        require(relative_target(image, 0x569af57, 3, 7) == profile->matchmaker_class, "matchmaker_class_binding");
        require(relative_target(image, 0x569d779, 3, 7) == profile->matchmaker_vtable, "matchmaker_vtable_binding");
        require(relative_target(image, 0x5d57dd7, 3, 7) == profile->state_vtable, "state_vtable_binding");
        prologue(image, 0x557e8ee, "c744242008010000"); // Settings size 0x108.
        prologue(image, 0x569afe5, "c744242030070000"); // Matchmaker size 0x730.
        prologue(image, 0x5dc328e, "b9e0030000"); // 0x10 controller + 0x3d0 state.
        call_target(image, 0x5dc32c7, 0x5d57dd0);

        call_target(image, 0x5dc3172, 0x3f10600); // Initializer obtains the Outer GameInstance.
        prologue(image, 0x3f10606, "488b5920");
        prologue(image, 0x5dc32fd, "498d86c8060000"); // Publish state at Matchmaker + 0x6c8.
        prologue(image, 0x5d57dd3, "48895108"); // State owner.
        prologue(image, 0x38c1080, "488b41304885c07408488b80c0020000");
        prologue(image, 0x5d71b6a, "4883bb8803000000");
        prologue(image, 0x5d71b78, "4883bb9803000000");
        require(read<uint32_t>(image + 0x952ca70 + 0x32) == 0x228, "world_owner_property_offset");
        require(read<uint32_t>(image + 0x9be20f0 + 0x32) == 0xd0, "known_players_property_offset");

        prologue(image, profile->known_players.rva + 0x18, "486381d8000000");
        prologue(image, profile->known_players.rva + 0x22, "4c6bf858");
        prologue(image, profile->known_players.rva + 0x13a, "807f4100");
        call_target(image, profile->known_players.rva + 0x18d, 0x15dbaf0);
        prologue(image, 0x32e3100, "c6414101c3"); // Reflected bIsBanned setter uses +0x41.
        prologue(image, 0x5d7128c, "4c8dbf98020000"); // Persist the native ban map.

        // Factory arguments and owned identity/name copies retain the same ABI.
        prologue(image, 0x5b5ccb4, "488d55d7488d4de7");
        prologue(image, profile->factory.rva + 0x1a, "488bf24c8bf1");
        call_target(image, 0x59aa4a5, 0x38c0d60);
        prologue(image, 0x59aa55b, "4d8d4630488d542440");
        call_target(image, 0x59aa564, 0x5980440);
        prologue(image, 0x59aa569, "488d471048890648897e08");
        call_target(image, profile->ban.rva + 0x4c6, 0x12bebf0);
        prologue(image, 0x12bec09, "8bc7f00fc1430883f801");
        prologue(image, profile->unban.rva + 0x22e, "498b5e084885db7429");

        // SaveBans' indirect variant tables moved; copy/release leaf contracts verified.
        prologue(image, 0x5d71488, "42ff94c378b63009");
        prologue(image, 0x5d714b6, "ff94c378b63009");
        prologue(image, 0x5d71568, "ff94c778b63009");
        prologue(image, 0x5d714f6, "ff94c7707a0309");
        prologue(image, 0x5d715af, "ff94c7707a0309");
        prologue(image, 0x38b0b90, "488b02488901488b4208488941084885c07404f0ff4008c3");
        prologue(image, 0x1460860, "8b028901c3");
        prologue(image, 0x123f330, "c20000");
    }

    // The production byte guard must reject a changed instruction, even after
    // selection of a known profile (the mapped image remains read-only).
    for (const auto* code : {&profile->factory, &profile->ban, &profile->unban,
                             &profile->known_players, &profile->save_bans}) {
        std::array<unsigned char, 32> changed{};
        std::memcpy(changed.data(), reinterpret_cast<const void*>(image + code->rva), changed.size());
        changed[0] ^= 1;
        bool rejected = false;
        try { prologue(reinterpret_cast<uintptr_t>(changed.data()), 0, code->prologue); }
        catch (const std::runtime_error& error) { rejected = std::string_view(error.what()) == "native_code"; }
        require(rejected, "changed_native_code_admitted");
    }
}
} // namespace

int wmain(int argc, wchar_t** argv) {
    try {
        require(argc == 2, "usage_native_profile_tests_shipping_exe_path");
        const ReadOnlyImage mapped(argv[1]);
        reject_unknown_images();
        reviewed_native_contract(mapped);
        const auto build = mapped.shipping_hash == CURRENT_HASH ? "26670300"
            : mapped.shipping_hash == BUILD_26574459_HASH ? "26574459"
            : mapped.shipping_hash == PREVIOUS_HASH ? "26419371" : "26161880";
        std::cout << "PASS: build " << build
            << " admission, PE call/layout bindings, unknown and changed-code rejection; no game code executed.\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "FAIL: " << error.what() << '\n';
        return 1;
    }
}
