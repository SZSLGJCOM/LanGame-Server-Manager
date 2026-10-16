#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include "../control_files.h"
#include <string>
#include <cstdio>
using namespace enshrouded_control;
int wmain(int argc, wchar_t** argv) {
    if (argc != 2) return 1;
    const std::wstring root = std::wstring(argv[1]) + L"\\file-boundary-fixture";
    if (!CreateDirectoryW(root.c_str(), nullptr)) return 2;
    const auto value = root + L"\\value.txt", temporary = value + L".tmp";
    std::string bytes;
    if (!PlainDirectory(root) || !WriteAtomicFile(value, "exact")
        || !ReadBoundedFile(value, 5, bytes) || bytes != "exact"
        || ReadBoundedFile(value, 4, bytes) || !bytes.empty()) return 3;
    if (!WriteAtomicFile(value, "replacement") || !ReadBoundedFile(value, 20, bytes) || bytes != "replacement") return 4;
    HANDLE collision = CreateFileW(temporary.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (collision == INVALID_HANDLE_VALUE) return 5;
    CloseHandle(collision);
    if (WriteAtomicFile(value, "unexpected") || !ReadBoundedFile(value, 20, bytes) || bytes != "replacement") return 6;
    if (ReadBoundedFile(root, 64, bytes) || WriteAtomicFile(value, std::string(65537, 'x'))) return 7;
    if (!DeleteFileW(temporary.c_str()) || !DeleteFileW(value.c_str()) || !RemoveDirectoryW(root.c_str())) return 8;
    std::puts("PASS file boundary: exact size, replacement, bounded read/write, directory rejection, existing temporary fail-closed");
    return 0;
}
