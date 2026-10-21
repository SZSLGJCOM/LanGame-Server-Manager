#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include "control_files.h"
#include "../../../apps/desktop/src-tauri/native/windows_atomic_response.hpp"
namespace enshrouded_control {
bool PlainDirectory(const std::wstring& path) noexcept {
    const auto attributes = GetFileAttributesW(path.c_str());
    return attributes != INVALID_FILE_ATTRIBUTES && (attributes & FILE_ATTRIBUTE_DIRECTORY)
        && !(attributes & FILE_ATTRIBUTE_REPARSE_POINT);
}
bool ReadBoundedFile(const std::wstring& path, std::size_t maximum, std::string& bytes) {
    bytes.clear();
    HANDLE file = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        nullptr, OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT | FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE) return false;
    BY_HANDLE_FILE_INFORMATION info{}; LARGE_INTEGER length{};
    bool ok = GetFileInformationByHandle(file, &info)
        && !(info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY))
        && GetFileSizeEx(file, &length) && length.QuadPart >= 0
        && static_cast<unsigned long long>(length.QuadPart) <= maximum;
    if (ok) {
        bytes.resize(static_cast<std::size_t>(length.QuadPart)); DWORD read{};
        ok = ReadFile(file, bytes.data(), static_cast<DWORD>(bytes.size()), &read, nullptr) && read == bytes.size();
    }
    CloseHandle(file); if (!ok) bytes.clear(); return ok;
}
bool WriteAtomicFile(const std::wstring& path, const std::string& bytes) {
    if (bytes.size() > 64 * 1024) return false;
    const auto temporary = path + L".tmp";
    HANDLE file = CreateFileW(temporary.c_str(), GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ, nullptr, CREATE_NEW,
        FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_WRITE_THROUGH, nullptr);
    if (file == INVALID_HANDLE_VALUE) return false;
    BY_HANDLE_FILE_INFORMATION info{}; DWORD written{};
    const bool ok = GetFileInformationByHandle(file, &info)
        && !(info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT)
        && WriteFile(file, bytes.data(), static_cast<DWORD>(bytes.size()), &written, nullptr)
        && written == bytes.size() && FlushFileBuffers(file);
    const bool renamed = ok && lgsm::control::replace_response(file, std::filesystem::path(path));
    const bool published = renamed && FlushFileBuffers(file);
    if (!renamed) {
        // Delete only the CREATE_NEW object we still own. After a successful
        // rename this handle is the published reply, even if its flush fails.
        FILE_DISPOSITION_INFO disposition{TRUE};
        SetFileInformationByHandle(file, FileDispositionInfo, &disposition, sizeof(disposition));
    }
    const bool closed = CloseHandle(file) != FALSE;
    return published && closed;
}
} // namespace enshrouded_control
