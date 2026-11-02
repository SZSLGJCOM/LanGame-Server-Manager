#define WIN32_LEAN_AND_MEAN
#include <Windows.h>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
std::filesystem::path ExecutableDirectory() {
    wchar_t path[32768]{};
    const auto size = GetModuleFileNameW(nullptr, path, static_cast<DWORD>(std::size(path)));
    if (!size || size >= std::size(path)) throw std::runtime_error("fixture executable path is unavailable");
    return std::filesystem::path(path).parent_path();
}

std::filesystem::path SelectedCache(const std::filesystem::path& bin) {
    wchar_t directory[1024]{};
    const auto size = GetEnvironmentVariableW(L"LGSM_ASA_PROXY_CACHE_DIRECTORY", directory, 1024);
    return size ? bin / L"ArkApi/Cache" / directory : bin / L"ArkApi/Cache";
}

std::vector<std::filesystem::path> PreparedFiles(const std::filesystem::path& bin) {
    const auto cache = SelectedCache(bin);
    return {bin / L"config.json", bin / L"ArkApi/Cache/cached_key.cache",
        cache / L"cached_offsets.cache", cache / L"cached_bitfields.cache"};
}

bool AccessDeniedByPin(const std::filesystem::path& path, DWORD access, bool directory) {
    const auto file = CreateFileW(path.c_str(), access, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        nullptr, OPEN_EXISTING, directory ? FILE_FLAG_BACKUP_SEMANTICS : 0, nullptr);
    if (file != INVALID_HANDLE_VALUE) { CloseHandle(file); return false; }
    return GetLastError() == ERROR_SHARING_VIOLATION;
}

bool PreparedInputsStayReadableAndStable() {
    const auto bin = ExecutableDirectory();
    for (const auto& path : PreparedFiles(bin)) {
        // Upstream reopens these with the CRT after the proxy's validation.
        std::ifstream input(path, std::ios::binary);
        char byte{};
        if (!input.read(&byte, 1) || !AccessDeniedByPin(path, GENERIC_WRITE, false)
            || !AccessDeniedByPin(path, DELETE, false)) return false;
    }
    auto directory = SelectedCache(bin);
    while (directory != bin) {
        if (!AccessDeniedByPin(directory, DELETE, true) || !AccessDeniedByPin(directory, GENERIC_WRITE, true)) return false;
        directory = directory.parent_path();
    }
    return AccessDeniedByPin(bin, DELETE, true);
}
}

#if defined(LGSM_ASA_API_FIXTURE)
// Only this isolated fixture is loaded by the host; no game or framework runs.
#if LGSM_ASA_API_FIXTURE == 0
extern "C" __declspec(dllexport) void NotInitApi() {}
#else
extern "C" __declspec(dllexport) void InitApi() noexcept(false) {
    SetEnvironmentVariableW(L"LGSM_ASA_PROXY_ENTERED", L"1");
    if (!PreparedInputsStayReadableAndStable()) return;
    SetEnvironmentVariableW(L"LGSM_ASA_PROXY_PROTECTED", L"1");
#if LGSM_ASA_API_FIXTURE == 1
    throw std::runtime_error("isolated fixture initialization failure");
#else
    SetEnvironmentVariableW(L"LGSM_ASA_PROXY_COMPLETED", L"1");
#endif
}
#endif
#else
int wmain(int argc, wchar_t** argv) {
    if (argc != 4) return 1;
    SetEnvironmentVariableW(L"LGSM_ASA_PROXY_ENTERED", nullptr);
    SetEnvironmentVariableW(L"LGSM_ASA_PROXY_COMPLETED", nullptr);
    SetEnvironmentVariableW(L"LGSM_ASA_PROXY_PROTECTED", nullptr);
    SetEnvironmentVariableW(L"LGSM_ASA_PROXY_CACHE_DIRECTORY", std::wstring(argv[3]) == L"-" ? nullptr : argv[3]);
    wchar_t buffer[32768]{};
    DWORD count = GetModuleFileNameW(nullptr, buffer, static_cast<DWORD>(std::size(buffer)));
    if (!count || count >= std::size(buffer)) return 2;
    const auto proxy_path = std::filesystem::path(buffer).parent_path() / L"version.dll";
    const auto proxy = LoadLibraryW(proxy_path.c_str());
    if (!proxy) { std::fprintf(stderr, "Proxy failed to load: %lu\n", GetLastError()); return 3; }
    count = GetSystemDirectoryW(buffer, static_cast<UINT>(std::size(buffer)));
    if (!count || count >= std::size(buffer)) return 4;
    const auto system_path = std::filesystem::path(buffer) / L"version.dll";
    const auto system = LoadLibraryW(system_path.c_str());
    using Function = DWORD(WINAPI*)(DWORD, LPWSTR, DWORD);
    const auto forwarded = reinterpret_cast<Function>(GetProcAddress(proxy, "VerLanguageNameW"));
    const auto original = reinterpret_cast<Function>(GetProcAddress(system, "VerLanguageNameW"));
    if (!forwarded || !original) return 5;
    wchar_t actual[256]{}, expected[256]{};
    const auto language = MAKELANGID(LANG_ENGLISH, SUBLANG_ENGLISH_US);
    const auto length = forwarded(language, actual, static_cast<DWORD>(std::size(actual)));
    if (!length || length != original(language, expected, static_cast<DWORD>(std::size(expected)))
        || std::wstring(actual) != expected) return 6;
    const bool entered = GetEnvironmentVariableW(L"LGSM_ASA_PROXY_ENTERED", buffer, 32768) != 0;
    const bool completed = GetEnvironmentVariableW(L"LGSM_ASA_PROXY_COMPLETED", buffer, 32768) != 0;
    const bool protected_inputs = GetEnvironmentVariableW(L"LGSM_ASA_PROXY_PROTECTED", buffer, 32768) != 0;
    if (entered != (std::wstring(argv[1]) == L"1") || completed != (std::wstring(argv[2]) == L"1") || protected_inputs != entered) {
        std::fprintf(stderr, "Unexpected optional API state: entered=%d completed=%d protected=%d\n", entered, completed, protected_inputs);
        return 7;
    }
    for (const auto& path : PreparedFiles(ExecutableDirectory())) {
        if (!std::filesystem::exists(path)) continue;
        const auto file = CreateFileW(path.c_str(), GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            nullptr, OPEN_EXISTING, 0, nullptr);
        if (file == INVALID_HANDLE_VALUE) { std::fprintf(stderr, "Preparation lock was not released: %lu\n", GetLastError()); return 8; }
        CloseHandle(file);
    }
    std::printf("PASS system Version forwarding; optional API entered=%d completed=%d; preparation locks released\n", entered, completed);
    // The API may retain callbacks. End this host instead of unloading it.
    return 0;
}
#endif
