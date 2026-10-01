// Adapted from ArkServerApi/AsaApiLoader, commit 4e04dc8982e1473920278f199638b29dc511d539.
// Copyright (c) 2023 Game Servers Hub. MIT; see THIRD_PARTY_NOTICES.txt.
#define WIN32_LEAN_AND_MEAN
#include <Windows.h>
#include <filesystem>
#include <string>
#include <vector>

extern "C" UINT_PTR mProcs[17]{};

namespace {
constexpr const char* Imports[] = {
    "GetFileVersionInfoA", "GetFileVersionInfoByHandle", "GetFileVersionInfoExA", "GetFileVersionInfoExW",
    "GetFileVersionInfoSizeA", "GetFileVersionInfoSizeExA", "GetFileVersionInfoSizeExW", "GetFileVersionInfoSizeW",
    "GetFileVersionInfoW", "VerFindFileA", "VerFindFileW", "VerInstallFileA", "VerInstallFileW", "VerLanguageNameA",
    "VerLanguageNameW", "VerQueryValueA", "VerQueryValueW"
};

bool Initialize() {
    // The stock server may live in a long private runtime path. Reject truncation
    // instead of resolving the API relative to an incomplete MAX_PATH buffer.
    std::vector<wchar_t> buffer(32768);
    DWORD count = GetSystemDirectoryW(buffer.data(), static_cast<UINT>(buffer.size()));
    if (!count || count >= buffer.size()) return false;
    const auto system_dll = std::filesystem::path(std::wstring(buffer.data(), count)) / L"version.dll";
    HMODULE system = LoadLibraryW(system_dll.c_str());
    if (!system) return false;
    for (std::size_t i = 0; i < std::size(Imports); ++i) {
        mProcs[i] = reinterpret_cast<UINT_PTR>(GetProcAddress(system, Imports[i]));
        if (!mProcs[i]) return false;
    }
    count = GetModuleFileNameW(nullptr, buffer.data(), static_cast<DWORD>(buffer.size()));
    if (!count || count >= buffer.size()) return false;
    const auto api_path = std::filesystem::path(std::wstring(buffer.data(), count)).parent_path()
        / L"ArkApi" / L"AsaApi.dll";
    HMODULE api = LoadLibraryW(api_path.c_str());
    if (!api) return false;
    const auto initialize = reinterpret_cast<void (*)()>(GetProcAddress(api, "InitApi"));
    if (!initialize) return false;
    initialize();
    return true;
}
}

BOOL WINAPI DllMain(HINSTANCE module, DWORD reason, LPVOID) {
    if (reason != DLL_PROCESS_ATTACH) return TRUE;
    DisableThreadLibraryCalls(module);
    try { return Initialize() ? TRUE : FALSE; }
    catch (...) { return FALSE; }
}
