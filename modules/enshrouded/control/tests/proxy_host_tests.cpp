#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include "../control_files.h"
#include <string>
#include <cstdio>
using namespace enshrouded_control;
namespace {
bool LoadStop(const std::wstring& path) {
    const auto module = LoadLibraryW(path.c_str()); if (!module) return false;
    using Stop = BOOL(WINAPI*)();
    const auto stop = reinterpret_cast<Stop>(GetProcAddress(module, "EnshroudedControlStop"));
    const bool ok = stop && GetProcAddress(module, "MiniDumpWriteDump") && stop();
    // Negative hosts have no hooks/PIN. The worker is joined before unloading.
    if (!ok) return false;
    return FreeLibrary(module) != 0;
}
}
int wmain(int argc, wchar_t** argv) {
    if (argc != 2) return 1;
    const std::wstring build = argv[1], root = build + L"\\proxy-negative-fixture";
    if (!CreateDirectoryW(root.c_str(), nullptr)) return 2;
    const auto payload = root + L"\\dbghelp.dll", control = root + L"\\.langame-enshrouded-control";
    if (!CopyFileW((build + L"\\dbghelp.dll").c_str(), payload.c_str(), TRUE)) return 3;
    if (!LoadStop(payload) || GetFileAttributesW(control.c_str()) != INVALID_FILE_ATTRIBUTES) return 4;
    if (!CreateDirectoryW(control.c_str(), nullptr)
        || !WriteAtomicFile(control + L"\\enable", "LGSM_ENSHROUDED_CONTROL_1\n") || !LoadStop(payload)) return 5;
    std::string status;
    if (!ReadBoundedFile(control + L"\\status.json", 4096, status)
        || status.find("unsupported_executable") == std::string::npos
        || GetFileAttributesW((control + L"\\players.json").c_str()) != INVALID_FILE_ATTRIBUTES) return 6;
    if (!DeleteFileW((control + L"\\status.json").c_str()) || !DeleteFileW((control + L"\\enable").c_str())
        || !RemoveDirectoryW(control.c_str()) || !DeleteFileW(payload.c_str()) || !RemoveDirectoryW(root.c_str())) return 7;
    std::puts("PASS proxy host: no opt-in creates no files; exact enable with wrong EXE fails closed; worker joined before unload");
    return 0;
}
