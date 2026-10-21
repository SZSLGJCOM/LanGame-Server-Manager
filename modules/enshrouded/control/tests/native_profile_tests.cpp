#include "../native_profile.h"
#include <cstdio>
#include <cstring>
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
    if (!ValidateNativeProfileImage(path, base, profile) || !profile.saveWorld) {
        std::puts("FAIL the real PE did not admit its native save profile"); return false;
    }
    const bool current = !std::strcmp(profile.exeSha256,
        "d7baa768eb00aa895d9f5a1cd8747ccf66858c7b542f50983092b2e3261588ee");
    const bool baseline = !std::strcmp(profile.exeSha256,
        "4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba");
    if ((!current && !baseline) || profile.imageBase != base
        || reinterpret_cast<std::uintptr_t>(profile.saveWorld) - base != (current ? 0x7afea0u : 0x7ada80u)
        || profile.callerA != (current ? 0x7b5ecfu : 0x7b3aafu)
        || profile.callerB != (current ? 0x7b6cbeu : 0x7b489eu)) return false;
    const auto dos = static_cast<IMAGE_DOS_HEADER*>(image.memory);
    auto nt = reinterpret_cast<IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    ++nt->OptionalHeader.SizeOfImage;
    NativeProfile rejected{};
    const bool wrongSizeRejected = !ValidateNativeProfileImage(path, base, rejected);
    --nt->OptionalHeader.SizeOfImage;
    if (!wrongSizeRejected) { std::puts("FAIL changed PE size was accepted"); return false; }
    for (auto entry : {profile.updatePlayers, profile.localCopy, profile.accountAction}) {
        auto byte = static_cast<unsigned char*>(entry);
        *byte ^= 1;
        const bool refused = !ValidateNativeProfileImage(path, base, rejected);
        *byte ^= 1;
        if (!refused) { std::puts("FAIL changed moderation entry was accepted"); return false; }
    }
    const std::uintptr_t saveEntries[] = {
        reinterpret_cast<std::uintptr_t>(profile.saveWorld) - base,
        current ? 0x7bb58cu : 0x7b916cu,
        current ? 0x7b97feu : 0x7b73deu,
    };
    for (auto rva : saveEntries) {
        auto byte = reinterpret_cast<unsigned char*>(base + rva);
        *byte ^= 1;
        const bool saveRefused = ValidateNativeProfileImage(path, base, rejected) && !rejected.saveWorld;
        *byte ^= 1;
        if (!saveRefused) { std::puts("FAIL changed save boundary was accepted"); return false; }
    }
    std::printf("PASS real PE native profile %s: identity, entry ABI, caller sites, changed PE/entry rejection; no game code executed\n",
        current ? "26083076" : "25967645");
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
