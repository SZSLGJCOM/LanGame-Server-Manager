#pragma once
#include <windows.h>
#include <array>
#include <cstdint>
#include <filesystem>
#include <vector>
namespace dragon_save {
namespace fs = std::filesystem;
struct Handle {
    HANDLE value = INVALID_HANDLE_VALUE;
    Handle() = default;
    explicit Handle(HANDLE input) : value(input) {}
    Handle(const Handle&) = delete;
    Handle& operator=(const Handle&) = delete;
    Handle(Handle&& other) noexcept : value(other.value) { other.value = INVALID_HANDLE_VALUE; }
    ~Handle() { if (value != INVALID_HANDLE_VALUE) CloseHandle(value); }
};
struct Snapshot {
    std::array<unsigned char,32> digest{};
    std::uint64_t size{}, index{};
    DWORD volume{};
    std::vector<Handle> parents;
};
fs::path full_path(const fs::path& input);
bool equal_path(const fs::path& first, const fs::path& second) noexcept;
std::vector<Handle> pin_parents(const fs::path& file);
Snapshot snapshot(const fs::path& path);
bool matches(const Snapshot& expected, const fs::path& actual, bool same_identity) noexcept;
}
