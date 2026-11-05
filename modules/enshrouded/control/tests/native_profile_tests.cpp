#include "../native_profile.h"
#include <cstdio>
#include <cstring>
#include <span>
#include <vector>

namespace {
struct Image {
    void* memory{};
    ~Image() { if (memory) VirtualFree(memory, 0, MEM_RELEASE); }
    bool Load(const wchar_t* path) {
        const auto file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr);
        if (file == INVALID_HANDLE_VALUE) return false;
        LARGE_INTEGER length{};
        const bool bounded = GetFileSizeEx(file, &length) && length.QuadPart > 0 && length.QuadPart <= 32 * 1024 * 1024;
        std::vector<unsigned char> bytes(bounded ? static_cast<std::size_t>(length.QuadPart) : 0);
        DWORD read{};
        const bool complete = bounded && ReadFile(file, bytes.data(), static_cast<DWORD>(bytes.size()), &read, nullptr)
            && read == bytes.size();
        CloseHandle(file);
        if (!complete || bytes.size() < sizeof(IMAGE_NT_HEADERS64)) return false;
        const auto dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(bytes.data());
        if (dos->e_magic != IMAGE_DOS_SIGNATURE || dos->e_lfanew < 0
            || static_cast<std::size_t>(dos->e_lfanew) > bytes.size() - sizeof(IMAGE_NT_HEADERS64)) return false;
        const auto nt = reinterpret_cast<const IMAGE_NT_HEADERS64*>(bytes.data() + dos->e_lfanew);
        const auto sectionOffset = static_cast<std::size_t>(dos->e_lfanew) + sizeof(DWORD)
            + sizeof(IMAGE_FILE_HEADER) + nt->FileHeader.SizeOfOptionalHeader;
        if (nt->Signature != IMAGE_NT_SIGNATURE || nt->OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC
            || !nt->OptionalHeader.SizeOfImage || nt->OptionalHeader.SizeOfImage > 64 * 1024 * 1024
            || nt->OptionalHeader.SizeOfHeaders > bytes.size()
            || nt->OptionalHeader.SizeOfHeaders > nt->OptionalHeader.SizeOfImage || sectionOffset > bytes.size()
            || nt->FileHeader.NumberOfSections > (bytes.size() - sectionOffset) / sizeof(IMAGE_SECTION_HEADER)) return false;
        memory = VirtualAlloc(nullptr, nt->OptionalHeader.SizeOfImage, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE);
        if (!memory) return false;
        std::memcpy(memory, bytes.data(), nt->OptionalHeader.SizeOfHeaders);
        const auto sections = reinterpret_cast<const IMAGE_SECTION_HEADER*>(bytes.data() + sectionOffset);
        for (unsigned i = 0; i < nt->FileHeader.NumberOfSections; ++i) {
            const auto& section = sections[i];
            if (section.VirtualAddress > nt->OptionalHeader.SizeOfImage
                || section.SizeOfRawData > nt->OptionalHeader.SizeOfImage - section.VirtualAddress
                || section.PointerToRawData > bytes.size()
                || section.SizeOfRawData > bytes.size() - section.PointerToRawData) return false;
            std::memcpy(static_cast<unsigned char*>(memory) + section.VirtualAddress,
                bytes.data() + section.PointerToRawData, section.SizeOfRawData);
        }
        return true;
    }
};
bool VerifyFile(const wchar_t* path) {
    using namespace enshrouded_control;
    Image image;
    if (!image.Load(path)) { std::puts("FAIL cannot map bounded PE sections"); return false; }
    const auto base = reinterpret_cast<std::uintptr_t>(image.memory);
    NativeProfile profile{};
    if (!ValidateNativeProfileImage(path, base, profile)) {
        std::puts("FAIL the real PE did not admit its native profile"); return false;
    }
    struct Expected {
        const char *sha, *build;
        std::uintptr_t save, commit, result, callerA, callerB;
        std::uintptr_t action;
        std::span<const std::uintptr_t> moderationWindows{};
    };
    static constexpr std::uintptr_t moderation26404334[] = {0x7a6e71, 0x7a70dd, 0x7a7110, 0x7a7472, 0x7a74b9};
    static constexpr std::uintptr_t moderation26430253[] = {0x7a6e71, 0x7a70dd, 0x7a7110, 0x7a7472, 0x7a74b9};
    constexpr Expected profiles[] = {
        {"4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba", "25967645",
            0x7ada80, 0x7b916c, 0x7b73de, 0x7b3aaf, 0x7b489e, 0x79ba20},
        {"b627639ce4a3f8c940fe90afa1d36ed59db3ff9ced24e935890eb7c99b04d5bf", "25988414",
            0, 0, 0, 0x7b3b8f, 0x7b497e, 0x79bb00},
        {"d7baa768eb00aa895d9f5a1cd8747ccf66858c7b542f50983092b2e3261588ee", "26083076",
            0x7afea0, 0x7bb58c, 0x7b97fe, 0x7b5ecf, 0x7b6cbe, 0x79de40},
        {"c5c9fe50c6398d2adedd98923c2d04bf9d9b4af64c1447ac167cdf8f874ff84f", "26404334",
            0x7b9390, 0x7c4afc, 0x7c2dae, 0x7bf45f, 0x7c02a3, 0x7a6e50, moderation26404334},
        {"f0d824be7fcd48ebfe2458ff5c28da26060c2074e3ec1fcac587e5a1d508aeb2", "26430253",
            0x7b9390, 0x7c4afc, 0x7c2dae, 0x7bf45f, 0x7c02a3, 0x7a6e50, moderation26430253},
    };
    const Expected* expected = nullptr;
    for (const auto& item : profiles) if (!std::strcmp(profile.exeSha256, item.sha)) expected = &item;
    if (!expected || profile.imageBase != base
        || profile.saveWorld != (expected->save ? reinterpret_cast<void*>(base + expected->save) : nullptr)
        || profile.accountAction != reinterpret_cast<void*>(base + expected->action)
        || profile.callerA != expected->callerA || profile.callerB != expected->callerB) return false;
    const auto dos = static_cast<IMAGE_DOS_HEADER*>(image.memory);
    auto nt = reinterpret_cast<IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    ++nt->OptionalHeader.SizeOfImage;
    NativeProfile rejected{};
    const bool wrongSizeRejected = !ValidateNativeProfileImage(path, base, rejected);
    --nt->OptionalHeader.SizeOfImage;
    if (!wrongSizeRejected) { std::puts("FAIL changed PE size was accepted"); return false; }
    for (auto entry : {profile.updatePlayers, profile.localCopy,
        reinterpret_cast<void*>(base + profile.callerA - 1), reinterpret_cast<void*>(base + profile.callerB - 1)}) {
        if (!entry) continue;
        auto byte = static_cast<unsigned char*>(entry);
        *byte ^= 1;
        const bool refused = !ValidateNativeProfileImage(path, base, rejected);
        *byte ^= 1;
        if (!refused) { std::puts("FAIL changed observation entry was accepted"); return false; }
    }
    const bool independentModeration = !expected->moderationWindows.empty();
    std::vector<std::uintptr_t> moderationSites{expected->action};
    moderationSites.insert(moderationSites.end(), expected->moderationWindows.begin(), expected->moderationWindows.end());
    for (auto rva : moderationSites) {
        auto byte = reinterpret_cast<unsigned char*>(base + rva);
        *byte ^= 1;
        const bool admitted = ValidateNativeProfileImage(path, base, rejected);
        *byte ^= 1;
        const bool refused = independentModeration
            ? admitted && !rejected.accountAction && rejected.saveWorld == profile.saveWorld
                && rejected.updatePlayers == profile.updatePlayers && rejected.localCopy == profile.localCopy
            : !admitted;
        if (!refused) { std::puts("FAIL changed moderation boundary was accepted or affected another capability"); return false; }
    }
    for (auto rva : {expected->save, expected->commit, expected->result}) {
        if (!rva) continue;
        auto byte = reinterpret_cast<unsigned char*>(base + rva);
        *byte ^= 1;
        const bool saveRefused = ValidateNativeProfileImage(path, base, rejected) && !rejected.saveWorld;
        *byte ^= 1;
        if (!saveRefused) { std::puts("FAIL changed save boundary was accepted"); return false; }
    }
    std::printf("PASS real PE native profile %s: identity, entry ABI, caller sites, changed PE/entry rejection; no game code executed\n",
        expected->build);
    return true;
}
}
int wmain(int argc, wchar_t** argv) {
    enshrouded_control::NativeProfile profile{};
    if (enshrouded_control::ValidateNativeProfile(profile)) {
        std::puts("FAIL unrecognized test host was admitted"); return 1;
    }
    if (argc == 1) {
        std::puts("PASS unknown executable rejected; pass an official executable path for inert PE validation"); return 0;
    }
    return argc == 2 && VerifyFile(argv[1]) ? 0 : 1;
}
