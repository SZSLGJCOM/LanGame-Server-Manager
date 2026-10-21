#include "native_profile.hpp"
#include "native_profile_data.hpp"
#include "../../enshrouded/control/vendor/minhook/include/MinHook.h"
#include <bcrypt.h>
#include <algorithm>
#include <cstring>
#include <stdexcept>
#include <cstdio>

namespace scum_save {
namespace {
std::uintptr_t base{};
Handle image_file,mapping;
const unsigned char* mapped{};
std::size_t mapped_size{};
fs::path saved;
struct Pin { std::uint32_t rva{},bytes{}; };
struct Patch { std::uint32_t rva{};unsigned bytes{};std::array<unsigned char,32> value{}; };
std::vector<Patch> patches;
std::vector<Pin> pins;
struct Import { std::uint32_t rva{}; void* expected{}; };
std::vector<Import> imports;
struct Unwind { RUNTIME_FUNCTION function{}; void* page{}; std::uintptr_t base{}; };
std::vector<std::unique_ptr<Unwind>> tables;
#ifdef LGSM_SCUM_SAVE_TESTS
std::vector<Import> guarded_imports;
#endif
struct Hash {
    BCRYPT_ALG_HANDLE algorithm{}; BCRYPT_HASH_HANDLE handle{};
    Hash() {
        if (BCryptOpenAlgorithmProvider(&algorithm,BCRYPT_SHA256_ALGORITHM,nullptr,0)<0 ||
            BCryptCreateHash(algorithm,&handle,nullptr,0,nullptr,0,0)<0) throw std::runtime_error("native_hash");
    }
    ~Hash() { if(handle) BCryptDestroyHash(handle); if(algorithm) BCryptCloseAlgorithmProvider(algorithm,0); }
    bool add(const void* bytes,std::size_t count) noexcept {
        return count<=ULONG_MAX && BCryptHashData(handle,reinterpret_cast<PUCHAR>(const_cast<void*>(bytes)),static_cast<ULONG>(count),0)>=0;
    }
    bool finish(Digest& result) noexcept { return BCryptFinishHash(handle,result.data(),static_cast<ULONG>(result.size()),0)>=0; }
};
bool hash_file(HANDLE handle,Digest& result,std::uint64_t expected,std::uint64_t deadline=UINT64_MAX) {
    LARGE_INTEGER size{}; if (!GetFileSizeEx(handle,&size) || size.QuadPart<0 || static_cast<std::uint64_t>(size.QuadPart)!=expected) return false;
    Hash hash; std::array<unsigned char,65536> bytes{}; std::uint64_t total{};
    for (;;) {
        if(GetTickCount64()>deadline) return false;
        DWORD count{}; if (!ReadFile(handle,bytes.data(),static_cast<DWORD>(bytes.size()),&count,nullptr)) return false;
        if (!count) break; if (!hash.add(bytes.data(),count)) return false; total+=count;
    }
    return total==expected && hash.finish(result);
}
const unsigned char* file_rva(std::uint32_t rva,std::size_t count) noexcept {
    if (!mapped || mapped_size<sizeof(IMAGE_DOS_HEADER)) return nullptr;
    const auto* dos=reinterpret_cast<const IMAGE_DOS_HEADER*>(mapped);
    if (dos->e_magic!=IMAGE_DOS_SIGNATURE || dos->e_lfanew<0 || static_cast<std::size_t>(dos->e_lfanew)>mapped_size-sizeof(IMAGE_NT_HEADERS64)) return nullptr;
    const auto* nt=reinterpret_cast<const IMAGE_NT_HEADERS64*>(mapped+dos->e_lfanew);
    if (nt->Signature!=IMAGE_NT_SIGNATURE || nt->FileHeader.Machine!=IMAGE_FILE_MACHINE_AMD64 || nt->FileHeader.NumberOfSections>128) return nullptr;
    const auto* sections=IMAGE_FIRST_SECTION(nt);
    for (unsigned index=0;index<nt->FileHeader.NumberOfSections;++index) {
        const auto at=reinterpret_cast<const unsigned char*>(&sections[index])-mapped;
        if (at<0 || static_cast<std::size_t>(at)>mapped_size-sizeof(IMAGE_SECTION_HEADER)) return nullptr;
        const auto& section=sections[index]; if (rva<section.VirtualAddress) continue;
        const auto relative=static_cast<std::size_t>(rva-section.VirtualAddress);
        if (relative>section.SizeOfRawData || count>section.SizeOfRawData-relative) continue;
        const auto raw=static_cast<std::size_t>(section.PointerToRawData)+relative;
        if (raw<=mapped_size && count<=mapped_size-raw) return mapped+raw;
    }
    return nullptr;
}
bool decode(std::string_view value,unsigned char* output,std::size_t count) noexcept {
    if (value.size()!=count*2) return false;
    const auto digit=[](char c) { return c>='0' && c<='9' ? c-'0' : c>='a' && c<='f' ? c-'a'+10 : -1; };
    for (std::size_t index=0;index<count;++index) {
        const auto high=digit(value[index*2]),low=digit(value[index*2+1]); if (high<0 || low<0) return false;
        output[index]=static_cast<unsigned char>((high<<4)|low);
    }
    return true;
}
void* near_page(std::uintptr_t code) noexcept {
    SYSTEM_INFO info{}; GetSystemInfo(&info); const auto step=static_cast<std::uintptr_t>(info.dwAllocationGranularity);
    if (!step) return nullptr; const auto start=code-code%step;
    for (std::uintptr_t index=1;index<=512;++index) {
        const auto delta=index*step;
        for (const auto hint : {start>=delta ? start-delta : 0,start<=UINTPTR_MAX-delta ? start+delta : 0})
            if (hint) if (auto* page=VirtualAlloc(reinterpret_cast<void*>(hint),info.dwPageSize,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE)) return page;
    }
    return nullptr;
}
bool register_unwind(const Hook& hook,const unsigned char* prefix) {
    if (hook.unwind.empty()) return true;
    // The trampoline has no epilogue. Version 2 prevents the indirect jump
    // from being mistaken for an epilogue while native exceptions unwind it.
    if (hook.unwind.size()<4 || hook.unwind[0]!=2) return false;
    const auto code=reinterpret_cast<std::uintptr_t>(*hook.original); std::array<unsigned char,64> bytes{};
    if (!hook.prefix_bytes || hook.prefix_bytes>32 || hook.unwind.size()>64 || !memory(code,bytes.data(),hook.prefix_bytes+14) ||
        std::memcmp(bytes.data(),prefix,hook.prefix_bytes)) return false;
    constexpr unsigned char jump[]={0xff,0x25,0,0,0,0}; std::uintptr_t destination{};
    std::memcpy(&destination,bytes.data()+hook.prefix_bytes+6,sizeof destination);
    if (std::memcmp(bytes.data()+hook.prefix_bytes,jump,sizeof jump) || destination!=base+hook.rva+hook.prefix_bytes) return false;
    auto entry=std::make_unique<Unwind>(); entry->page=near_page(code); if (!entry->page) return false;
    entry->base=(std::min)(code,reinterpret_cast<std::uintptr_t>(entry->page)); const auto metadata=reinterpret_cast<std::uintptr_t>(entry->page)+16;
    if (code-entry->base>UINT32_MAX-64 || metadata-entry->base>UINT32_MAX) return false;
    std::memcpy(reinterpret_cast<void*>(metadata),hook.unwind.data(),hook.unwind.size()); DWORD previous{};
    if (!VirtualProtect(entry->page,128,PAGE_READONLY,&previous)) return false;
    entry->function={static_cast<DWORD>(code-entry->base),static_cast<DWORD>(code-entry->base+hook.prefix_bytes+14),static_cast<DWORD>(metadata-entry->base)};
    if (!RtlAddFunctionTable(&entry->function,1,entry->base)) return false;
    tables.push_back(std::move(entry)); return true;
}
}
bool memory(std::uintptr_t at,void* result,std::size_t count) noexcept {
    SIZE_T got{}; return at && count<=256ULL*1024*1024 && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(at),result,count,&got) && got==count;
}
std::uintptr_t image_base() noexcept { return base; }
fs::path executable_path() {
    std::vector<wchar_t> value(32768); const auto count=GetModuleFileNameW(nullptr,value.data(),static_cast<DWORD>(value.size()));
    if (!count || count>=value.size()) throw std::runtime_error("native_image_path"); return fs::path(std::wstring(value.data(),count));
}
fs::path full_path(const fs::path& value) {
    const auto& text=value.native(); if (text.empty() || text.size()>=32767 || text.find(L':',2)!=text.npos) throw std::runtime_error("native_path");
    for (auto c : text) if (c<32 || c==L'"') throw std::runtime_error("native_path");
    std::vector<wchar_t> output(32768); const auto count=GetFullPathNameW(text.c_str(),static_cast<DWORD>(output.size()),output.data(),nullptr);
    if (!count || count>=output.size()) throw std::runtime_error("native_path"); return fs::path(std::wstring(output.data(),count)).lexically_normal();
}
bool path_equal(const fs::path& a,const fs::path& b) noexcept { return CompareStringOrdinal(a.c_str(),-1,b.c_str(),-1,TRUE)==CSTR_EQUAL; }
bool within_saved(const fs::path& path) noexcept {
    const auto& text=path.native(); auto prefix=saved.native(); prefix+=L'\\';
    return prefix.size()>3 && text.size()>prefix.size() && text.size()<32768 &&
        CompareStringOrdinal(text.c_str(),static_cast<int>(prefix.size()),prefix.c_str(),static_cast<int>(prefix.size()),TRUE)==CSTR_EQUAL;
}
fs::path storage_root() {return saved;}
std::vector<Handle> pin_parent(const fs::path& path) {
    if (!within_saved(path)) throw std::runtime_error("native_save_scope"); std::vector<fs::path> ancestors;
    for (auto current=path.parent_path();!current.empty();) {
        ancestors.push_back(current); const auto parent=current.parent_path(); if(parent==current) break; current=parent;
        if (ancestors.size()>128) throw std::runtime_error("native_save_depth");
    }
    std::reverse(ancestors.begin(),ancestors.end()); std::vector<Handle> result;
    for (const auto& ancestor : ancestors) {
        Handle handle(CreateFileW(ancestor.c_str(),GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT|FILE_FLAG_BACKUP_SEMANTICS,nullptr)); FILE_ATTRIBUTE_TAG_INFO info{};
        if (handle.value==INVALID_HANDLE_VALUE || !GetFileInformationByHandleEx(handle.value,FileAttributeTagInfo,&info,sizeof info) ||
            (info.FileAttributes&FILE_ATTRIBUTE_REPARSE_POINT) || !(info.FileAttributes&FILE_ATTRIBUTE_DIRECTORY)) throw std::runtime_error("native_save_scope");
        result.push_back(std::move(handle));
    }
    return result;
}
bool buffer_hash(const void* input,std::size_t count,Digest& result) noexcept {
    try { Hash hash; std::array<unsigned char,65536> bytes{};
        auto at=reinterpret_cast<std::uintptr_t>(input);
        while(count) { const auto n=(std::min)(count,bytes.size()); if(!memory(at,bytes.data(),n) || !hash.add(bytes.data(),n)) return false; at+=n;count-=n; }
        return hash.finish(result);
    } catch (...) { return false; }
}
bool file_matches(const fs::path& path,const Digest& expected,std::uint64_t bytes,std::uint64_t deadline) noexcept {
    try {
        auto parent=pin_parent(path); Handle file(CreateFileW(path.c_str(),GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
        BY_HANDLE_FILE_INFORMATION info{}; Digest actual{};
        return file.value!=INVALID_HANDLE_VALUE && GetFileInformationByHandle(file.value,&info) && info.nNumberOfLinks==1 &&
            !(info.dwFileAttributes&(FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_DIRECTORY)) && hash_file(file.value,actual,bytes,deadline) && actual==expected;
    } catch (...) { return false; }
}
bool code_matches() noexcept {
    std::array<unsigned char,65536> actual{},expected{};
    for(const auto& pin:pins) {
        const auto* source=file_rva(pin.rva,pin.bytes);
        if(!source||!pin.bytes||pin.bytes>actual.size()||!memory(base+pin.rva,actual.data(),pin.bytes))return false;
        std::memcpy(expected.data(),source,pin.bytes);
        for(const auto& patch:patches) {
            const auto begin=(std::max)(pin.rva,patch.rva),end=(std::min)(pin.rva+pin.bytes,patch.rva+patch.bytes);
            if(begin<end)std::memcpy(expected.data()+begin-pin.rva,patch.value.data()+begin-patch.rva,end-begin);
        }
        if(std::memcmp(actual.data(),expected.data(),pin.bytes))return false;
    }
    for(const auto& imported:imports) {void* value{};if(!read(base+imported.rva,value)||value!=imported.expected)return false;}
    for(const auto& slot:NATIVE_SLOTS) {std::uintptr_t value{};if(!read(base+slot.rva,value)||value!=(slot.target?base+slot.target:0))return false;}
    return !pins.empty();
}
namespace {
bool prepare_at(const fs::path& path,std::uintptr_t image) noexcept {
    try {
        if (base || !path_equal(path.filename(),L"SCUMServer.exe")) return false;
        image_file.value=CreateFileW(path.c_str(),GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,nullptr);
        LARGE_INTEGER size{}; BY_HANDLE_FILE_INFORMATION info{}; Digest actual{},expected{};
        if (image_file.value==INVALID_HANDLE_VALUE || !GetFileSizeEx(image_file.value,&size) || size.QuadPart<=0 || size.QuadPart>320LL*1024*1024 ||
            !GetFileInformationByHandle(image_file.value,&info) || info.nNumberOfLinks!=1 || (info.dwFileAttributes&(FILE_ATTRIBUTE_DIRECTORY|FILE_ATTRIBUTE_REPARSE_POINT)) ||
            !decode("4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad",expected.data(),expected.size()) ||
            !hash_file(image_file.value,actual,static_cast<std::uint64_t>(size.QuadPart)) || actual!=expected) return false;
        mapping.value=CreateFileMappingW(image_file.value,nullptr,PAGE_READONLY,0,0,nullptr);
        mapped=static_cast<const unsigned char*>(MapViewOfFile(mapping.value,FILE_MAP_READ,0,0,0)); mapped_size=static_cast<std::size_t>(size.QuadPart);
        if (!mapped) return false; base=image;
        saved=full_path(path.parent_path().parent_path().parent_path()/L"Saved");
        for (const auto& range : NATIVE_RANGES) pins.push_back({range.rva,range.bytes}); return code_matches();
    } catch (...) { return false; }
}
}
bool prepare_image() noexcept {
    try {return prepare_at(executable_path(),reinterpret_cast<std::uintptr_t>(GetModuleHandleW(nullptr)));}
    catch(...) {return false;}
}
bool install_hooks(const std::vector<Hook>& hooks,const std::vector<IatHook>& imported) noexcept {
    try {
        HMODULE self{};
        if (!code_matches() || !GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_PIN,
            reinterpret_cast<LPCWSTR>(&install_hooks),&self) || MH_Initialize()!=MH_OK) return false;
        for (const auto& hook : hooks) {
            std::array<unsigned char,32> prefix{}; const auto* source=file_rva(hook.rva,hook.prefix_bytes);
            if (!hook.prefix_bytes || hook.prefix_bytes>prefix.size() || !source || !decode(hook.prefix,prefix.data(),hook.prefix_bytes) ||
                std::memcmp(source,prefix.data(),hook.prefix_bytes) || MH_CreateHook(reinterpret_cast<void*>(base+hook.rva),hook.replacement,hook.original)!=MH_OK ||
                !register_unwind(hook,prefix.data())) return false;
        }
        const auto kernel=GetModuleHandleW(L"kernel32.dll"); if(!kernel) return false;
        for (const auto& hook : imported) {
            auto canonical=reinterpret_cast<void*>(GetProcAddress(kernel,hook.name)); void* actual{};
#ifdef LGSM_SCUM_SAVE_TESTS
            // Components retain their preinstalled owned-copy OS write guard
            // below the real product observer. This branch is absent in DLLs.
            for(const auto& guard:guarded_imports)if(guard.rva==hook.rva)canonical=guard.expected;
#endif
            if (!canonical || !read(base+hook.rva,actual) || actual!=canonical) return false; *hook.original=actual;
        }
        for (const auto& hook : hooks) if (MH_QueueEnableHook(reinterpret_cast<void*>(base+hook.rva))!=MH_OK) return false;
        if (MH_ApplyQueued()!=MH_OK) return false;
        for(const auto& hook:hooks) {
            bool covered=false;
            for(const auto& pin:pins)if(hook.rva>=pin.rva&&hook.rva+hook.prefix_bytes<=pin.rva+pin.bytes)covered=true;
            if(!covered)return false;
            Patch patch{hook.rva,hook.prefix_bytes};if(!memory(base+patch.rva,patch.value.data(),patch.bytes))return false;
            patches.push_back(patch);
        }
        for (const auto& hook : imported) {
            auto* address=reinterpret_cast<void**>(base+hook.rva); DWORD previous{};
            if(!VirtualProtect(address,sizeof(void*),PAGE_READWRITE,&previous)) return false;
            const bool changed=InterlockedCompareExchangePointer(address,hook.replacement,*hook.original)==*hook.original;
            DWORD ignored{}; const bool restored=VirtualProtect(address,sizeof(void*),previous,&ignored)!=FALSE;
            if(!changed || !restored) return false; imports.push_back({hook.rva,hook.replacement});
        }
        return code_matches();
    } catch (...) { return false; }
}
#ifdef LGSM_SCUM_SAVE_TESTS
bool test_profile_copy(const fs::path& path,std::uintptr_t image) noexcept {return prepare_at(path,image);}
void test_saved_root(const fs::path& root) { saved=full_path(root); }
void test_guarded_import(std::uint32_t rva,void* guard) {guarded_imports.push_back({rva,guard});}
namespace {
unsigned __declspec(noinline) test_replacement() { return 99; }
}
bool test_trampoline(const Hook& input,const std::vector<std::pair<unsigned,unsigned>>& pushes,
    unsigned allocation_offset,unsigned allocation_bytes) noexcept {
    auto* page=static_cast<unsigned char*>(VirtualAlloc(nullptr,4096,MEM_RESERVE|MEM_COMMIT,PAGE_EXECUTE_READWRITE));
    if (!page) return false;
    const auto previous_base=base; const auto prior_count=tables.size(); bool ok{}; void* trampoline{};
    try {
        std::array<unsigned char,32> prefix{};
        if (!decode(input.prefix,prefix.data(),input.prefix_bytes) || input.prefix_bytes>32) throw std::runtime_error("test_prefix");
        std::memcpy(page,prefix.data(),input.prefix_bytes); auto offset=input.prefix_bytes;
        if (allocation_bytes) {
            // ADD RSP,imm32. The same concrete copied prologue is executed by
            // the real MinHook trampoline before this owned fixture epilogue.
            const unsigned char add[]={0x48,0x81,0xc4}; std::memcpy(page+offset,add,sizeof add); offset+=sizeof add;
            std::memcpy(page+offset,&allocation_bytes,sizeof allocation_bytes); offset+=sizeof allocation_bytes;
        }
        for (auto iterator=pushes.rbegin();iterator!=pushes.rend();++iterator) {
            const auto reg=iterator->second; if (reg>=8) page[offset++]=0x41; page[offset++]=static_cast<unsigned char>(0x58+(reg&7));
        }
        const unsigned char end[]={0xb8,42,0,0,0,0xc3}; std::memcpy(page+offset,end,sizeof end);
        FlushInstructionCache(GetCurrentProcess(),page,4096); base=reinterpret_cast<std::uintptr_t>(page);
        if (MH_Initialize()!=MH_OK || MH_CreateHook(page,reinterpret_cast<void*>(&test_replacement),&trampoline)!=MH_OK) throw std::runtime_error("test_minhook");
        Hook hook=input; hook.rva=0; hook.original=&trampoline;
        if (!register_unwind(hook,prefix.data())) throw std::runtime_error("test_unwind_registration");
        if (MH_EnableHook(page)!=MH_OK) throw std::runtime_error("test_enable");
        alignas(16) unsigned char argument[512]{};
        using Call=unsigned(*)(void*,std::uintptr_t,std::uintptr_t,std::uintptr_t);
        if (reinterpret_cast<Call>(page)(argument,0,0,0)!=99 || reinterpret_cast<Call>(trampoline)(argument,0,0,0)!=42) throw std::runtime_error("test_actual_trampoline");
        for (unsigned point=0;point<input.prefix_bytes+14;++point) {
            alignas(16) DWORD64 stack[160]{}; const auto entry=reinterpret_cast<DWORD64>(&stack[127]); constexpr DWORD64 return_to=0x12345678;
            *reinterpret_cast<DWORD64*>(entry)=return_to; CONTEXT context{}; context.ContextFlags=CONTEXT_FULL;
            context.Rip=reinterpret_cast<DWORD64>(trampoline)+point; context.Rsp=entry; auto* registers=&context.Rax;
            for (unsigned reg=0;reg<16;++reg) if (reg!=4) registers[reg]=0x10000+reg;
            unsigned pushed{};
            for (const auto& [after,reg] : pushes) if (point>=after) {
                ++pushed; *reinterpret_cast<DWORD64*>(entry-pushed*8)=registers[reg];
            }
            context.Rsp-=pushed*8;
            if (allocation_offset && point>=allocation_offset) context.Rsp-=allocation_bytes;
            DWORD64 owner{},establisher{}; void* data{};
            auto* function=RtlLookupFunctionEntry(context.Rip,&owner,nullptr);
            if (!input.unwind.empty()) {
                if (!function || function!=&tables.back()->function) throw std::runtime_error("test_lookup");
                RtlVirtualUnwind(UNW_FLAG_NHANDLER,owner,context.Rip,function,&context,&data,&establisher,nullptr);
            } else {
                if (function) throw std::runtime_error("test_leaf");
                context.Rip=*reinterpret_cast<DWORD64*>(context.Rsp); context.Rsp+=8;
            }
            if (context.Rip!=return_to || context.Rsp!=entry+8) throw std::runtime_error("test_stack");
            for (const auto& [after,reg] : pushes) { (void)after; if (registers[reg]!=0x10000+reg) throw std::runtime_error("test_register"); }
        }
        ok=true;
    } catch (const std::exception& error) { std::fprintf(stderr,"trampoline component: %s prefix=%s\n",error.what(),input.prefix.data()); ok=false; }
    catch (...) { ok=false; }
    MH_DisableHook(page); MH_RemoveHook(page); MH_Uninitialize();
    while (tables.size()>prior_count) { auto& item=*tables.back(); RtlDeleteFunctionTable(&item.function); VirtualFree(item.page,0,MEM_RELEASE); tables.pop_back(); }
    base=previous_base; VirtualFree(page,0,MEM_RELEASE); return ok;
}
#endif
}
