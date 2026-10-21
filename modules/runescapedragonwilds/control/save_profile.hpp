#pragma once
#include "save_files.hpp"
#include <span>
#include <string_view>
namespace dragon_save {
struct Hook {
    std::uint32_t rva;
    unsigned bytes;
    std::string_view prefix;
    void* replacement;
    void** original;
    std::span<const unsigned char> unwind;
};
bool read_memory(std::uintptr_t address,void* output,std::size_t bytes) noexcept;
template<class T> bool read(std::uintptr_t address,T& output) noexcept { return read_memory(address,&output,sizeof output); }
bool prepare_profile(std::uintptr_t image,std::span<const Hook> hooks) noexcept;
bool profile_matches() noexcept;
#ifdef LGSM_DRAGON_SAVE_TESTS
bool test_trampoline(const Hook& hook,std::span<const std::pair<unsigned,unsigned>> pushes,
    unsigned allocation_after=0,unsigned allocation_bytes=0) noexcept;
#endif
}
