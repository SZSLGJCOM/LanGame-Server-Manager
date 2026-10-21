#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include "../control_files.h"
#include "../../../../apps/desktop/src-tauri/native/windows_atomic_response.hpp"
#include <array>
#include <filesystem>
#include <string>
#include <cstdio>

static bool old_body(HANDLE reader, const std::string& expected) {
    std::string body(expected.size(), '\0');
    DWORD read{};
    return ReadFile(reader, body.data(), static_cast<DWORD>(body.size()), &read, nullptr)
        && read == body.size() && body == expected;
}

static bool readable_before_writer_close(const std::filesystem::path& path) {
    const auto temporary = path.native() + L".pending";
    HANDLE writer = CreateFileW(temporary.c_str(), GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ, nullptr, CREATE_NEW, FILE_FLAG_OPEN_REPARSE_POINT, nullptr);
    if (writer == INVALID_HANDLE_VALUE) return false;
    DWORD written{};
    const bool renamed = WriteFile(writer, "visible", 7, &written, nullptr) && written == 7
        && FlushFileBuffers(writer) && lgsm::control::replace_response(writer, path);
    std::string body;
    bool valid = renamed && FlushFileBuffers(writer)
        && enshrouded_control::ReadBoundedFile(path.native(), 64, body) && body == "visible";
    for (const DWORD access : {DWORD(GENERIC_WRITE), DWORD(DELETE)}) {
        HANDLE peer = CreateFileW(path.c_str(), access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
            OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT, nullptr);
        const DWORD error = GetLastError();
        valid = valid && peer == INVALID_HANDLE_VALUE && error == ERROR_SHARING_VIOLATION;
        if (peer != INVALID_HANDLE_VALUE) CloseHandle(peer);
    }
    if (!renamed) {
        FILE_DISPOSITION_INFO disposition{TRUE};
        SetFileInformationByHandle(writer, FileDispositionInfo, &disposition, sizeof(disposition));
    }
    return CloseHandle(writer) && valid;
}

int main() {
    std::array<wchar_t, MAX_PATH + 1> temporary{};
    const auto length = GetTempPathW(static_cast<DWORD>(temporary.size()), temporary.data());
    if (!length || length >= temporary.size()) return 1;
    const std::filesystem::path root = std::filesystem::path(temporary.data()) /
        (L"enshrouded-reply-" + std::to_wstring(GetCurrentProcessId()) + L"-" + std::to_wstring(GetTickCount64()));
    if (!std::filesystem::create_directory(root)) return 2;
    const auto path = root / L"response.txt";
    if (!enshrouded_control::WriteAtomicFile(path.native(), "pending")) return 3;
    HANDLE reader = CreateFileW(path.c_str(), GENERIC_READ,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
        OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT, nullptr);
    if (reader == INVALID_HANDLE_VALUE) return 4;
    const bool published = enshrouded_control::WriteAtomicFile(path.native(), "completed");
    std::string body;
    const bool valid = published && old_body(reader, "pending")
        && enshrouded_control::ReadBoundedFile(path.native(), 64, body) && body == "completed";
    if (!CloseHandle(reader) || !valid) return 5;
    const auto linked = root / L"second-link.txt";
    if (!CreateHardLinkW(linked.c_str(), path.c_str(), nullptr)) return 6;
    if (enshrouded_control::WriteAtomicFile(path.native(), "must-not-replace")) return 7;
    if (!enshrouded_control::ReadBoundedFile(path.native(), 64, body) || body != "completed") return 8;
    const auto leftover = path.native() + L".tmp";
    if (GetFileAttributesW(leftover.c_str()) != INVALID_FILE_ATTRIBUTES || GetLastError() != ERROR_FILE_NOT_FOUND) return 9;
    if (!DeleteFileW(linked.c_str()) || !enshrouded_control::WriteAtomicFile(path.native(), "recovered")) return 10;
    if (!enshrouded_control::ReadBoundedFile(path.native(), 64, body) || body != "recovered") return 11;
    if (!readable_before_writer_close(path)) return 12;
    if (!DeleteFileW(path.c_str()) || !RemoveDirectoryW(root.c_str())) return 13;
    std::puts("PASS Enshrouded replies: held reader, visible-before-close, writer exclusion and failed-publish recovery.");
    return 0;
}
