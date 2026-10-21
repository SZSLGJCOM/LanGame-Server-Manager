#pragma once
#include "native_profile.hpp"
#include <algorithm>

namespace scum_save::early_mod {
// Use the pinned loader's actual public CppUserModBase constructor and
// destructor. No STL object or virtual table is reproduced by another CRT.
inline constexpr std::uint32_t CTOR=0x2dd730,DTOR=0x2dd9b0,TABLE=0x10def10;
inline constexpr std::size_t OBJECT_BYTES=0xc0;
inline bool range(std::uintptr_t core,std::uint32_t rva,std::size_t bytes,std::string_view hex) noexcept {
    Digest actual{};
    if(!buffer_hash(reinterpret_cast<void*>(core+rva),bytes,actual) || hex.size()!=64)return false;
    constexpr char digits[]="0123456789abcdef";
    for(std::size_t i=0;i<actual.size();++i)if(hex[i*2]!=digits[actual[i]>>4] || hex[i*2+1]!=digits[actual[i]&15])return false;
    return true;
}
inline void verify(std::uintptr_t core) {
    if(!core || reinterpret_cast<std::uintptr_t>(GetProcAddress(reinterpret_cast<HMODULE>(core),"??0CppUserModBase@RC@@QEAA@XZ"))!=core+CTOR ||
        reinterpret_cast<std::uintptr_t>(GetProcAddress(reinterpret_cast<HMODULE>(core),"??1CppUserModBase@RC@@UEAA@XZ"))!=core+DTOR ||
        !range(core,CTOR,638,"1ebf1e96d7f78b70e28119a20587c630351e0aca2c8bc17947ae3c695523998d") ||
        !range(core,DTOR,250,"145ba255b93117cd3440feb003e467eafe324c4cbf221bbaa6a28d212e2a553c"))
        throw std::runtime_error("native_cpp_mod_abi");
    for(unsigned slot=0;slot<16;++slot)
        if(require_read<std::uintptr_t>(core+TABLE+slot*8)!=core+(slot ? 0x1fd7a0 : 0x2dd6a0))throw std::runtime_error("native_cpp_mod_vtable");
}
inline void* create(std::uintptr_t core) {
    verify(core);
    auto* page=static_cast<unsigned char*>(VirtualAlloc(nullptr,4096,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE));
    if(!page)throw std::runtime_error("native_cpp_mod_allocation");
    std::fill(page,page+4096,static_cast<unsigned char>(0xcc));
    try {
        const auto result=reinterpret_cast<void*(*)(void*)>(core+CTOR)(page);
        if(result!=page || require_read<std::uintptr_t>(reinterpret_cast<std::uintptr_t>(page))!=core+TABLE ||
            !std::all_of(page+OBJECT_BYTES,page+4096,[](unsigned char byte){return byte==0xcc;}))
            throw std::runtime_error("native_cpp_mod_object");
        return page;
    } catch(...) { VirtualFree(page,0,MEM_RELEASE);throw; }
}
inline void destroy(std::uintptr_t core,void* object) {
    verify(core);
    if(!object || require_read<std::uintptr_t>(reinterpret_cast<std::uintptr_t>(object))!=core+TABLE)
        throw std::runtime_error("native_cpp_mod_object");
    reinterpret_cast<void(*)(void*)>(core+DTOR)(object);
    if(!VirtualFree(object,0,MEM_RELEASE))throw std::runtime_error("native_cpp_mod_release");
}
}
