// Adapted from ArkServerApi/AsaApiLoader, commit 4e04dc8982e1473920278f199638b29dc511d539.
// Copyright (c) 2023 Game Servers Hub. MIT; see THIRD_PARTY_NOTICES.txt.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <Windows.h>
#include <bcrypt.h>
#include <algorithm>
#include <array>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <string>
#include <unordered_set>
#include <utility>
#include <vector>
#include <json.hpp>

#pragma comment(lib, "bcrypt.lib")

extern "C" UINT_PTR mProcs[17]{};

namespace {
constexpr const char* Imports[] = {
    "GetFileVersionInfoA", "GetFileVersionInfoByHandle", "GetFileVersionInfoExA", "GetFileVersionInfoExW",
    "GetFileVersionInfoSizeA", "GetFileVersionInfoSizeExA", "GetFileVersionInfoSizeExW", "GetFileVersionInfoSizeW",
    "GetFileVersionInfoW", "VerFindFileA", "VerFindFileW", "VerInstallFileA", "VerInstallFileW", "VerLanguageNameA",
    "VerLanguageNameW", "VerQueryValueA", "VerQueryValueW"
};

bool InitializeVersionProxy() {
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
    return true;
}

void ReportOptionalFailure(const char* reason, DWORD error = 0) noexcept {
    char message[256]{};
    const int length = error
        ? std::snprintf(message, sizeof(message),
            "LGSM_ARK_TOOLS: optional ASA API unavailable: %s (Win32 error %lu). System Version forwarding remains active.\r\n",
            reason, error)
        : std::snprintf(message, sizeof(message),
            "LGSM_ARK_TOOLS: optional ASA API unavailable: %s. System Version forwarding remains active.\r\n", reason);
    if (length <= 0 || static_cast<std::size_t>(length) >= sizeof(message)) return;
    OutputDebugStringA(message);
    DWORD written{};
    // Use inherited stderr when available, without creating a writable state
    // directory or making optional initialization fatal.
    WriteFile(GetStdHandle(STD_ERROR_HANDLE), message, static_cast<DWORD>(length), &written, nullptr);
}

struct PreparationFailure { const char* reason; DWORD error{}; };

class Handle {
public:
    explicit Handle(HANDLE handle) : value(handle) {}
    ~Handle() { if (value != INVALID_HANDLE_VALUE) CloseHandle(value); }
    Handle(Handle&& other) noexcept : value(std::exchange(other.value, INVALID_HANDLE_VALUE)) {}
    Handle(const Handle&) = delete;
    Handle& operator=(const Handle&) = delete;
    HANDLE value;
};

struct Input { HANDLE handle; std::uint64_t size; std::filesystem::path path; };

Input Pin(const std::filesystem::path& path, bool directory, std::uint64_t limit,
          std::vector<Handle>& pins, const char* reason) {
    Handle file(CreateFileW(path.c_str(), directory ? FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES : GENERIC_READ,
        FILE_SHARE_READ, nullptr, OPEN_EXISTING,
        FILE_FLAG_OPEN_REPARSE_POINT | (directory ? FILE_FLAG_BACKUP_SEMANTICS : FILE_FLAG_SEQUENTIAL_SCAN), nullptr));
    if (file.value == INVALID_HANDLE_VALUE) throw PreparationFailure{reason, GetLastError()};
    BY_HANDLE_FILE_INFORMATION info{};
    if (!GetFileInformationByHandle(file.value, &info)) throw PreparationFailure{reason, GetLastError()};
    const auto size = (static_cast<std::uint64_t>(info.nFileSizeHigh) << 32) | info.nFileSizeLow;
    if ((info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT)
        || !!(info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != directory || (!directory && size > limit))
        throw PreparationFailure{reason};
    const Input input{file.value, size, path};
    pins.push_back(std::move(file));
    return input;
}

void PinDirectories(const std::filesystem::path& path, std::vector<Handle>& pins) {
    auto current = path.root_path();
    Pin(current, true, 0, pins, "symbol directory is unavailable or unsafe");
    for (const auto& part : path.relative_path()) {
        current /= part;
        Pin(current, true, 0, pins, "symbol directory is unavailable or unsafe");
    }
}

class Reader {
public:
    explicit Reader(const std::filesystem::path& path) : file_(path, std::ios::binary) {}
    bool Read(void* destination, std::size_t size) {
        return static_cast<bool>(file_.read(static_cast<char*>(destination), static_cast<std::streamsize>(size)));
    }
private:
    // Reopen exactly as the upstream cache reader does, while the Win32 handle
    // pins the verified file. This also checks actual CRT path support.
    std::ifstream file_;
};

std::string ReadText(Input input, const char* reason) {
    std::string text(static_cast<std::size_t>(input.size), '\0');
    if (!Reader(input.path).Read(text.data(), text.size())) throw PreparationFailure{reason};
    return text;
}

std::string ExecutableHash(Input executable) {
    struct Hash {
        BCRYPT_ALG_HANDLE algorithm{};
        BCRYPT_HASH_HANDLE hash{};
        ~Hash() { if (hash) BCryptDestroyHash(hash); if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0); }
    } digest;
    if (BCryptOpenAlgorithmProvider(&digest.algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0
        || BCryptCreateHash(digest.algorithm, &digest.hash, nullptr, 0, nullptr, 0, 0) < 0)
        throw PreparationFailure{"executable SHA-256 initialization failed"};
    std::array<unsigned char, 65536> buffer{};
    auto remaining = executable.size;
    while (remaining) {
        DWORD read{};
        if (!ReadFile(executable.handle, buffer.data(), static_cast<DWORD>(std::min<std::uint64_t>(remaining, buffer.size())), &read, nullptr) || !read)
            throw PreparationFailure{"server executable could not be hashed", GetLastError()};
        if (BCryptHashData(digest.hash, buffer.data(), read, 0) < 0)
            throw PreparationFailure{"executable SHA-256 update failed"};
        remaining -= read;
    }
    std::array<unsigned char, 32> bytes{};
    if (BCryptFinishHash(digest.hash, bytes.data(), static_cast<ULONG>(bytes.size()), 0) < 0)
        throw PreparationFailure{"executable SHA-256 calculation failed"};
    constexpr char hex[] = "0123456789abcdef";
    std::string result;
    for (const auto byte : bytes) { result += hex[byte >> 4]; result += hex[byte & 15]; }
    return result;
}

bool NormalizeHash(std::string& hash) {
    const auto first = hash.find_first_not_of(" \t\r\n\v\f");
    if (first == std::string::npos) return false;
    hash = hash.substr(first, hash.find_last_not_of(" \t\r\n\v\f") - first + 1);
    if (hash.size() != 64) return false;
    for (auto& ch : hash) {
        if (ch >= 'A' && ch <= 'F') ch = static_cast<char>(ch - 'A' + 'a');
        if (!((ch >= '0' && ch <= '9') || (ch >= 'a' && ch <= 'f'))) return false;
    }
    return true;
}

bool IsGeneration(std::string& directory) {
    std::replace(directory.begin(), directory.end(), '\\', '/');
    constexpr auto prefix = "generations/";
    if (!directory.starts_with(prefix)) return false;
    const auto name = directory.substr(std::strlen(prefix));
    if (name.size() < 70 || name[64] != '-') return false;
    auto hash = name.substr(0, 64);
    if (!NormalizeHash(hash)) return false;
    std::size_t begin = 65;
    for (int index = 0; index < 3; ++index) {
        const auto end = index == 2 ? name.size() : name.find('-', begin);
        if (end == std::string::npos || end == begin) return false;
        for (auto i = begin; i < end; ++i) if (name[i] < '0' || name[i] > '9') return false;
        begin = end + 1;
    }
    return true;
}

std::string CacheDirectory(const std::string& text, const std::string& executable_hash) {
    auto hash = text;
    std::string directory;
    const auto metadata = nlohmann::json::parse(text, nullptr, false);
    if (metadata.is_object()) {
        try {
            if (metadata.value("version", 0U) != 1) throw PreparationFailure{"symbol metadata version is unsupported"};
            hash = metadata.value("executable_hash", std::string{});
            directory = metadata.value("cache_directory", std::string{});
            (void)metadata.value("last_modified", std::string{});
        } catch (const nlohmann::json::exception&) { throw PreparationFailure{"symbol metadata is invalid"}; }
        if (!directory.empty() && !IsGeneration(directory)) throw PreparationFailure{"symbol generation path is invalid"};
    }
    if (!NormalizeHash(hash)) throw PreparationFailure{"symbol metadata is invalid"};
    if (hash != executable_hash) throw PreparationFailure{"prepared symbols do not match the server executable; run Check and update"};
    return directory;
}

void ValidateMap(Input input, std::size_t value_size) {
    // This is the pinned AsaApi 2.11 Cache.h x64 serialization, also validated by
    // ark_tools_symbol_cache.rs before publication. Do not infer ABI compatibility.
    static_assert(sizeof(std::size_t) == 8);
    Reader reader(input.path);
    std::unordered_set<std::string> keys;
    std::uint64_t remaining = input.size;
    while (remaining) {
        std::uint64_t size{};
        if (remaining < 8 || !reader.Read(&size, 8)) throw PreparationFailure{"prepared symbol map is truncated"};
        remaining -= 8;
        if (!size || size > 1024 * 1024 || size + value_size > remaining || keys.size() == 5'000'000)
            throw PreparationFailure{"prepared symbol map has an invalid entry"};
        std::string key(static_cast<std::size_t>(size), '\0');
        std::array<char, 32> value{};
        if (!reader.Read(key.data(), key.size()) || !reader.Read(value.data(), value_size))
            throw PreparationFailure{"prepared symbol map is unreadable"};
        if (!keys.insert(std::move(key)).second) throw PreparationFailure{"prepared symbol map has duplicate keys"};
        remaining -= size + value_size;
    }
    if (keys.empty()) throw PreparationFailure{"prepared symbol map is empty"};
}

std::vector<Handle> PinPreparedSymbols(const std::filesystem::path& executable) {
    std::vector<Handle> pins;
    const auto bin = executable.parent_path();
    PinDirectories(bin, pins);
    const auto image = Pin(executable, false, UINT64_MAX, pins, "server executable is unavailable or unsafe");
    const auto config = nlohmann::json::parse(ReadText(Pin(bin / L"config.json", false, 16384, pins,
        "symbol configuration is unavailable or unsafe"), "symbol configuration is unreadable"), nullptr, false);
    try {
        const auto& enabled = config.at("settings").at("AutomaticCacheDownload").at("Enable");
        if (!enabled.is_boolean() || enabled.get<bool>()) throw PreparationFailure{"automatic symbol download must be disabled; run Check and update"};
    } catch (const nlohmann::json::exception&) { throw PreparationFailure{"symbol configuration is invalid"}; }
    const auto cache = bin / L"ArkApi" / L"Cache";
    Pin(bin / L"ArkApi", true, 0, pins, "symbol directory is unavailable or unsafe");
    Pin(cache, true, 0, pins, "symbol directory is unavailable or unsafe");
    const auto selection = ReadText(Pin(cache / L"cached_key.cache", false, 16384, pins,
        "prepared symbol metadata is missing or unsafe; run Check and update"), "symbol metadata is unreadable");
    const auto directory = CacheDirectory(selection, ExecutableHash(image));
    auto selected = cache;
    if (!directory.empty()) {
        selected /= L"generations";
        Pin(selected, true, 0, pins, "symbol generation is unavailable or unsafe");
        selected = cache / directory;
        Pin(selected, true, 0, pins, "symbol generation is unavailable or unsafe");
    }
    constexpr std::uint64_t map_limit = 512ULL * 1024 * 1024;
    const auto offsets = Pin(selected / L"cached_offsets.cache", false, map_limit, pins, "prepared offsets are missing or unsafe");
    const auto bitfields = Pin(selected / L"cached_bitfields.cache", false, map_limit, pins, "prepared bitfields are missing or unsafe");
    if (offsets.size + bitfields.size > 768ULL * 1024 * 1024) throw PreparationFailure{"prepared symbol maps are oversized"};
    ValidateMap(offsets, 8);
    ValidateMap(bitfields, 32);
    // Keep the configuration, selection, maps and every parent directory stable
    // while upstream reopens them. Missing/replaced files otherwise retry forever,
    // even with AutomaticCacheDownload disabled. No downloads occur in this proxy.
    return pins;
}

void InitializeOptionalApi() noexcept {
    try {
        std::vector<wchar_t> buffer(32768);
        const DWORD count = GetModuleFileNameW(nullptr, buffer.data(), static_cast<DWORD>(buffer.size()));
        if (!count || count >= buffer.size()) {
            ReportOptionalFailure("server executable path unavailable", count ? ERROR_INSUFFICIENT_BUFFER : GetLastError());
            return;
        }
        const auto executable = std::filesystem::path(std::wstring(buffer.data(), count));
        // Both ArkBaseApi::Init and Tools::GetCurrentDir use MAX_PATH buffers;
        // the latter also uses the ANSI path to open config.json.
        char ansi_path[MAX_PATH]{};
        const auto ansi_count = GetModuleFileNameA(nullptr, ansi_path, MAX_PATH);
        if (count >= MAX_PATH || !ansi_count || ansi_count >= MAX_PATH
            || _wcsicmp(std::filesystem::path(std::string(ansi_path, ansi_count)).c_str(), executable.c_str()) != 0) {
            ReportOptionalFailure("server executable path is unsupported by the optional API");
            return;
        }
        if (_wcsicmp(executable.filename().c_str(), L"ArkAscendedServer.exe") != 0) {
            ReportOptionalFailure("server executable name is unsupported");
            return;
        }
        const auto prepared = PinPreparedSymbols(executable);
        const auto api_path = executable.parent_path() / L"ArkApi" / L"AsaApi.dll";
        HMODULE api = LoadLibraryW(api_path.c_str());
        if (!api) { ReportOptionalFailure("AsaApi.dll load failed", GetLastError()); return; }
        const auto initialize = reinterpret_cast<void (*)()>(GetProcAddress(api, "InitApi"));
        if (!initialize) { ReportOptionalFailure("InitApi export missing", GetLastError()); return; }
        try { initialize(); }
        catch (...) { ReportOptionalFailure("InitApi threw a C++ exception"); }
        // An initializer can retain callbacks before throwing. Keep its module
        // loaded; this boundary does not recover aborts or access violations.
    } catch (const PreparationFailure& failure) {
        ReportOptionalFailure(failure.reason, failure.error);
    } catch (...) {
        ReportOptionalFailure("API preparation threw a C++ exception");
    }
}
}

BOOL WINAPI DllMain(HINSTANCE module, DWORD reason, LPVOID) {
    if (reason != DLL_PROCESS_ATTACH) return TRUE;
    DisableThreadLibraryCalls(module);
    try { if (!InitializeVersionProxy()) return FALSE; }
    catch (...) { return FALSE; }
    InitializeOptionalApi();
    return TRUE;
}
