#pragma once
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <array>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

namespace scum_save {
namespace fs=std::filesystem;
struct Handle {
    HANDLE value{INVALID_HANDLE_VALUE};
    Handle()=default;
    explicit Handle(HANDLE input) noexcept : value(input) {}
    Handle(const Handle&)=delete; Handle& operator=(const Handle&)=delete;
    Handle(Handle&& other) noexcept : value(other.value) { other.value=INVALID_HANDLE_VALUE; }
    Handle& operator=(Handle&& other) noexcept {
        if (this!=&other) { if (value!=INVALID_HANDLE_VALUE && value) CloseHandle(value); value=other.value; other.value=INVALID_HANDLE_VALUE; }
        return *this;
    }
    ~Handle() { if (value!=INVALID_HANDLE_VALUE && value) CloseHandle(value); }
};
using Digest=std::array<unsigned char,32>;
bool memory(std::uintptr_t address,void* result,std::size_t count) noexcept;
template<class T> bool read(std::uintptr_t at,T& result) noexcept { return memory(at,&result,sizeof result); }
template<class T> T require_read(std::uintptr_t at) {
    T result{}; if (!read(at,result)) throw std::runtime_error("native_memory"); return result;
}
std::uintptr_t image_base() noexcept;
fs::path full_path(const fs::path& path);
fs::path executable_path();
bool path_equal(const fs::path& a,const fs::path& b) noexcept;
bool within_saved(const fs::path& path) noexcept;
fs::path storage_root();
std::vector<Handle> pin_parent(const fs::path& path);
bool buffer_hash(const void* data,std::size_t bytes,Digest& result) noexcept;
bool file_matches(const fs::path& path,const Digest& expected,std::uint64_t bytes,std::uint64_t deadline=UINT64_MAX) noexcept;
struct Hook {
    std::uint32_t rva{}; void* replacement{}; void** original{};
    std::string_view prefix; unsigned prefix_bytes{}; std::vector<unsigned char> unwind;
};
struct IatHook { std::uint32_t rva{}; const char* name{}; void* replacement{}; void** original{}; };
bool prepare_image() noexcept;
bool install_hooks(const std::vector<Hook>& hooks,const std::vector<IatHook>& imports) noexcept;
bool code_matches() noexcept;
#ifdef LGSM_SCUM_SAVE_TESTS
bool test_profile_copy(const fs::path& path,std::uintptr_t image) noexcept;
void test_saved_root(const fs::path& root);
void test_guarded_import(std::uint32_t rva,void* guard);
bool test_trampoline(const Hook& hook,const std::vector<std::pair<unsigned,unsigned>>& pushes,unsigned allocation_offset,unsigned allocation_bytes) noexcept;
#endif
}
