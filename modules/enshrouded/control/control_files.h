#pragma once
#include <string>
namespace enshrouded_control {
bool PlainDirectory(const std::wstring& path) noexcept;
bool ReadBoundedFile(const std::wstring& path, std::size_t maximum, std::string& bytes);
bool WriteAtomicFile(const std::wstring& path, const std::string& bytes);
} // namespace enshrouded_control
