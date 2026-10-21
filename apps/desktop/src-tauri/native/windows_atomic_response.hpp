#pragma once
#include <windows.h>
#include <algorithm>
#include <filesystem>
#include <vector>

namespace lgsm::control {
// Only for owned IPC response files. Callers retain their directory pins and
// flush a CREATE_NEW temporary handle opened with DELETE access and read sharing
// before calling. A new reader may open the destination before that handle closes.
// Windows 10 RS1+ POSIX replacement keeps an existing reader on the old object;
// later opens see the new object. A classic rename fallback reintroduces the race.
inline bool replace_response(HANDLE source, const std::filesystem::path& destination) noexcept {
    try {
        BY_HANDLE_FILE_INFORMATION source_info{};
        if (!destination.is_absolute() || destination.native().empty()
                || destination.native().size() >= 32768
                || !GetFileInformationByHandle(source, &source_info)
                || (source_info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT))
                || source_info.nNumberOfLinks != 1) {
            SetLastError(ERROR_INVALID_DATA);
            return false;
        }
        {
            // Close our inspection handle before replacement. The caller's
            // readers may stay open; no write/delete sharing is relaxed here.
            HANDLE existing = CreateFileW(destination.c_str(), FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT, nullptr);
            if (existing == INVALID_HANDLE_VALUE) {
                if (GetLastError() != ERROR_FILE_NOT_FOUND) return false;
            } else {
                BY_HANDLE_FILE_INFORMATION info{};
                const bool ordinary = GetFileInformationByHandle(existing, &info)
                    && !(info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT))
                    && info.nNumberOfLinks == 1;
                const bool closed = CloseHandle(existing) != FALSE;
                if (!ordinary || !closed) {
                    SetLastError(ERROR_INVALID_DATA);
                    return false;
                }
            }
        }
        const auto& name = destination.native();
        const auto size = sizeof(FILE_RENAME_INFO) + name.size() * sizeof(wchar_t);
        std::vector<unsigned char> storage(size);
        auto* rename = reinterpret_cast<FILE_RENAME_INFO*>(storage.data());
        rename->Flags = FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS;
        rename->RootDirectory = nullptr;
        rename->FileNameLength = static_cast<DWORD>(name.size() * sizeof(wchar_t));
        std::copy(name.begin(), name.end(), rename->FileName);
        return SetFileInformationByHandle(source, FileRenameInfoEx, rename,
            static_cast<DWORD>(size)) != FALSE;
    } catch (...) {
        SetLastError(ERROR_NOT_ENOUGH_MEMORY);
        return false;
    }
}
} // namespace lgsm::control
