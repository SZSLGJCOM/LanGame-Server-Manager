#include "native_profile.h"
#include <bcrypt.h>
#include <vector>
#include <cstring>
namespace enshrouded_control {
namespace {
struct Build {
    const char* sha256;
    DWORD timestamp;
    std::uintptr_t update, copy, action, callerA, callerB;
};
constexpr Build kBuilds[] = {
    {"4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba", 1792083984,
        0x7b33e0, 0x9f54d0, 0x79ba20, 0x7b3aaf, 0x7b489e},
    {"b627639ce4a3f8c940fe90afa1d36ed59db3ff9ced24e935890eb7c99b04d5bf", 1792148062,
        0x7b34c0, 0x9f5bc0, 0x79bb00, 0x7b3b8f, 0x7b497e},
};
constexpr unsigned char kUpdateBytes[] = {0x48,0x8b,0xc4,0x55,0x53,0x56,0x57,0x41,0x54,0x41,0x56,0x41,
    0x57,0x48,0x8d,0xa8,0x48,0xf6,0xff,0xff,0x48,0x81,0xec,0x80,0x0a,0x00,0x00,0x44,0x0f,0x29,0x40,0x88};
constexpr unsigned char kCopyBytes[] = {0x40,0x53,0x48,0x83,0xec,0x20,0x8b,0x42,0x3c,0x4c,0x8d,0x52,
    0x40,0x90,0xf0,0x41,0xff,0x04,0x82,0x4c,0x69,0xc8,0x68,0x26,0x00,0x00,0x41,0x8b,0xc0,0x83,0xe0,0x3f};
constexpr unsigned char kActionBytes[] = {0x48,0x89,0x5c,0x24,0x18,0x48,0x89,0x74,0x24,0x20,0x55,0x57,
    0x41,0x54,0x41,0x56,0x41,0x57,0x48,0x8d,0xac,0x24,0x00,0xfe,0xff,0xff,0x48,0x81,0xec,0x00,0x03,0x00};
const Build* IdentifyBuild(const wchar_t* path) {
    HANDLE file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL | FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
    if (file == INVALID_HANDLE_VALUE) return nullptr;
    BCRYPT_ALG_HANDLE algorithm{}; BCRYPT_HASH_HANDLE hash{}; const Build* match = nullptr;
    if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) >= 0) {
        DWORD objectBytes{}, returned{};
        if (BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH, reinterpret_cast<PUCHAR>(&objectBytes),
            sizeof objectBytes, &returned, 0) >= 0 && objectBytes < 1024 * 1024) {
            std::vector<unsigned char> object(objectBytes);
            if (BCryptCreateHash(algorithm, &hash, object.data(), objectBytes, nullptr, 0, 0) >= 0) {
                unsigned char bytes[64 * 1024]; DWORD count{}; bool ok = true;
                for (;;) {
                    if (!ReadFile(file, bytes, sizeof bytes, &count, nullptr)) { ok = false; break; }
                    if (!count) break;
                    if (BCryptHashData(hash, bytes, count, 0) < 0) { ok = false; break; }
                }
                unsigned char digest[32]{};
                if (ok && BCryptFinishHash(hash, digest, sizeof digest, 0) >= 0) {
                    constexpr char hex[] = "0123456789abcdef";
                    char text[65]{};
                    for (unsigned i = 0; i < sizeof digest; ++i) {
                        text[i * 2] = hex[digest[i] >> 4]; text[i * 2 + 1] = hex[digest[i] & 15];
                    }
                    for (const auto& build : kBuilds) {
                        if (!std::strcmp(text, build.sha256)) { match = &build; break; }
                    }
                }
            }
        }
    }
    if (hash) BCryptDestroyHash(hash);
    if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0);
    CloseHandle(file); return match;
}
bool EntryMatches(std::uintptr_t main, std::uintptr_t rva, const unsigned char* expected) noexcept {
    unsigned char bytes[32]{};
    return ReadCurrentMemory(nullptr, main + rva, bytes, sizeof bytes) && !std::memcmp(bytes, expected, sizeof bytes);
}
}
bool ReadCurrentMemory(void*, std::uintptr_t address, void* out, std::size_t bytes) noexcept {
    SIZE_T read{};
    return address && ReadProcessMemory(GetCurrentProcess(), reinterpret_cast<const void*>(address),
        out, bytes, &read) && read == bytes;
}
std::wstring ModuleFile(HMODULE module) {
    std::vector<wchar_t> path(32768);
    const auto count = GetModuleFileNameW(module, path.data(), static_cast<DWORD>(path.size()));
    return count && count < path.size() ? std::wstring(path.data(), count) : std::wstring();
}
bool ValidateNativeProfile(NativeProfile& profile) {
    const auto path = ModuleFile(nullptr);
    const auto build = path.empty() ? nullptr : IdentifyBuild(path.c_str());
    if (!build) return false;
    const auto main = reinterpret_cast<std::uintptr_t>(GetModuleHandleW(nullptr));
    IMAGE_DOS_HEADER dos{}; IMAGE_NT_HEADERS64 nt{};
    if (!ReadCurrentMemory(nullptr, main, &dos, sizeof dos) || dos.e_magic != IMAGE_DOS_SIGNATURE
        || dos.e_lfanew < 0 || dos.e_lfanew > 1024 * 1024
        || !ReadCurrentMemory(nullptr, main + static_cast<std::uintptr_t>(dos.e_lfanew), &nt, sizeof nt)
        || nt.Signature != IMAGE_NT_SIGNATURE || nt.FileHeader.Machine != IMAGE_FILE_MACHINE_AMD64
        || nt.OptionalHeader.SizeOfImage != 31256576 || nt.FileHeader.TimeDateStamp != build->timestamp
        || !EntryMatches(main, build->update, kUpdateBytes) || !EntryMatches(main, build->copy, kCopyBytes)
        || !EntryMatches(main, build->action, kActionBytes)) return false;
    profile = {main, reinterpret_cast<void*>(main + build->update), reinterpret_cast<void*>(main + build->copy),
        reinterpret_cast<void*>(main + build->action), build->callerA, build->callerB, build->sha256};
    return true;
}
bool CreateProcessIdentity(ProcessIdentity& identity) noexcept {
    FILETIME creation{}, exit{}, kernel{}, user{}; unsigned char random[16]{};
    if (!GetProcessTimes(GetCurrentProcess(), &creation, &exit, &kernel, &user)
        || BCryptGenRandom(nullptr, random, sizeof random, BCRYPT_USE_SYSTEM_PREFERRED_RNG) < 0) return false;
    identity.pid = GetCurrentProcessId();
    identity.creationTime = (static_cast<std::uint64_t>(creation.dwHighDateTime) << 32) | creation.dwLowDateTime;
    constexpr char hex[] = "0123456789abcdef";
    for (unsigned i = 0; i < sizeof random; ++i) {
        identity.runId[i * 2] = hex[random[i] >> 4]; identity.runId[i * 2 + 1] = hex[random[i] & 15];
    }
    identity.runId[32] = 0; return true;
}
} // namespace enshrouded_control
