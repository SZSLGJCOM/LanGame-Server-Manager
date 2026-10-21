#include "save_profile.hpp"
#include "save_profile_data.hpp"
#include "../../enshrouded/control/vendor/minhook/include/MinHook.h"
#include <algorithm>
#include <cstring>
#include <memory>
#include <stdexcept>
#ifdef LGSM_DRAGON_SAVE_TESTS
#include <cstdio>
#endif
namespace dragon_save {
bool file_imports_match() noexcept;
namespace {
std::uintptr_t base{};
Handle executable,mapping;
const unsigned char* mapped{};
std::size_t mapped_size{};
struct Pin { std::uint32_t rva{},bytes{}; unsigned skip{}; std::array<unsigned char,32> patch{}; bool pointers{}; };
std::vector<Pin> pins;
struct Unwind { RUNTIME_FUNCTION function{}; void* page{}; std::uintptr_t base{}; };
std::vector<std::unique_ptr<Unwind>> tables;
const unsigned char* file_rva(std::uint32_t rva,std::size_t bytes) noexcept {
    if (!mapped || mapped_size<sizeof(IMAGE_DOS_HEADER)) return nullptr;
    const auto* dos=reinterpret_cast<const IMAGE_DOS_HEADER*>(mapped);
    if (dos->e_magic!=IMAGE_DOS_SIGNATURE || dos->e_lfanew<0
        || static_cast<std::size_t>(dos->e_lfanew)>mapped_size-sizeof(IMAGE_NT_HEADERS64)) return nullptr;
    const auto* nt=reinterpret_cast<const IMAGE_NT_HEADERS64*>(mapped+dos->e_lfanew);
    if (nt->Signature!=IMAGE_NT_SIGNATURE || nt->FileHeader.Machine!=IMAGE_FILE_MACHINE_AMD64
        || nt->FileHeader.NumberOfSections>128) return nullptr;
    const auto* sections=IMAGE_FIRST_SECTION(nt);
    for (unsigned index=0;index<nt->FileHeader.NumberOfSections;++index) {
        const auto offset=reinterpret_cast<const unsigned char*>(&sections[index])-mapped;
        if (offset<0 || static_cast<std::size_t>(offset)>mapped_size-sizeof(IMAGE_SECTION_HEADER)) return nullptr;
        const auto& section=sections[index]; if (rva<section.VirtualAddress) continue;
        const auto relative=static_cast<std::size_t>(rva-section.VirtualAddress);
        if (relative>section.SizeOfRawData || bytes>section.SizeOfRawData-relative) continue;
        const auto raw=static_cast<std::size_t>(section.PointerToRawData)+relative;
        if (raw<=mapped_size && bytes<=mapped_size-raw) return mapped+raw;
    }
    return nullptr;
}
bool decode(std::string_view text,unsigned char* result,std::size_t count) noexcept {
    if (text.size()!=count*2) return false;
    const auto digit=[](char value) { return value>='0' && value<='9' ? value-'0' : value>='a' && value<='f' ? value-'a'+10 : -1; };
    for (std::size_t index=0;index<count;++index) {
        const auto high=digit(text[index*2]),low=digit(text[index*2+1]); if (high<0 || low<0) return false;
        result[index]=static_cast<unsigned char>(high*16+low);
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
    // V2 suppresses epilogue guessing at MinHook's final indirect jump. There
    // are no trampoline epilog scopes: the copied prologue remains active.
    if (hook.unwind.size()<4 || hook.unwind[0]!=2 || hook.unwind.size()>64) return false;
    const auto code=reinterpret_cast<std::uintptr_t>(*hook.original);
    std::array<unsigned char,64> bytes{};
    if (!hook.bytes || hook.bytes>32 || !read_memory(code,bytes.data(),hook.bytes+14)
        || std::memcmp(bytes.data(),prefix,hook.bytes)) return false;
    constexpr unsigned char jump[]={0xff,0x25,0,0,0,0}; std::uintptr_t destination{};
    std::memcpy(&destination,bytes.data()+hook.bytes+6,sizeof destination);
    if (std::memcmp(bytes.data()+hook.bytes,jump,sizeof jump) || destination!=base+hook.rva+hook.bytes) return false;
    auto entry=std::make_unique<Unwind>(); entry->page=near_page(code); if (!entry->page) return false;
    entry->base=(std::min)(code,reinterpret_cast<std::uintptr_t>(entry->page));
    const auto metadata=reinterpret_cast<std::uintptr_t>(entry->page)+16;
    if (code-entry->base>UINT32_MAX-64 || metadata-entry->base>UINT32_MAX) { VirtualFree(entry->page,0,MEM_RELEASE); return false; }
    std::memcpy(reinterpret_cast<void*>(metadata),hook.unwind.data(),hook.unwind.size()); DWORD previous{};
    if (!VirtualProtect(entry->page,128,PAGE_READONLY,&previous)) { VirtualFree(entry->page,0,MEM_RELEASE); return false; }
    entry->function={static_cast<DWORD>(code-entry->base),static_cast<DWORD>(code-entry->base+hook.bytes+14),static_cast<DWORD>(metadata-entry->base)};
    if (!RtlAddFunctionTable(&entry->function,1,entry->base)) { VirtualFree(entry->page,0,MEM_RELEASE); return false; }
    tables.push_back(std::move(entry)); return true;
}
}
bool read_memory(std::uintptr_t address,void* output,std::size_t bytes) noexcept {
    SIZE_T read{};
    return address && address<=UINTPTR_MAX-bytes && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,bytes,&read)
        && read==bytes;
}
bool profile_matches() noexcept {
    std::array<unsigned char,65536> bytes{};
    for (const auto& pin : pins) {
        const auto* original=file_rva(pin.rva,pin.bytes);
        if (!original || !pin.bytes || pin.bytes>bytes.size() || !read_memory(base+pin.rva,bytes.data(),pin.bytes)) return false;
        if (pin.pointers) {
            const auto* dos=reinterpret_cast<const IMAGE_DOS_HEADER*>(mapped);
            const auto* nt=reinterpret_cast<const IMAGE_NT_HEADERS64*>(mapped+dos->e_lfanew);
            if (pin.bytes%8) return false;
            for (unsigned offset=0;offset<pin.bytes;offset+=8) {
                std::uintptr_t expected{},actual{};
                std::memcpy(&expected,original+offset,sizeof expected); std::memcpy(&actual,bytes.data()+offset,sizeof actual);
                if (expected>=nt->OptionalHeader.ImageBase && expected-nt->OptionalHeader.ImageBase<nt->OptionalHeader.SizeOfImage)
                    expected=base+(expected-nt->OptionalHeader.ImageBase);
                if (actual!=expected) return false;
            }
        } else if ((pin.skip && std::memcmp(bytes.data(),pin.patch.data(),pin.skip))
            || std::memcmp(bytes.data()+pin.skip,original+pin.skip,pin.bytes-pin.skip)) return false;
    }
    // The reviewed Close/Write chain reaches these exact system APIs. A game
    // import detour can otherwise invalidate a successful native bool.
    struct Import { std::uint32_t rva; const char* name; };
    constexpr Import imports[]={{0x8885b10,"FlushFileBuffers"},{0x8885850,"WriteFile"},{0x88855f0,"ReadFile"},
        {0x8885a00,"GetOverlappedResult"},{0x8885370,"CloseHandle"},{0x8885b28,"CreateFileW"},
        {0x8885940,"MoveFileExW"},{0x88855e0,"MoveFileW"}};
    const auto kernel=GetModuleHandleW(L"kernel32.dll");
    if (!kernel) return false;
    for (const auto& imported : imports) {
        if (imported.rva==0x8885370 && file_imports_match()) continue;
        std::uintptr_t value{};
        if (!read(base+imported.rva,value) || value!=reinterpret_cast<std::uintptr_t>(GetProcAddress(kernel,imported.name))) return false;
    }
    return !pins.empty();
}
bool prepare_profile(std::uintptr_t image,std::span<const Hook> hooks) noexcept {
    try {
        if (base || !image || hooks.empty()) return false;
        base=image;
        std::vector<wchar_t> path(32768); const auto count=GetModuleFileNameW(nullptr,path.data(),static_cast<DWORD>(path.size()));
        if (!count || count>=path.size()) return false;
        executable.value=CreateFileW(path.data(),GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,nullptr);
        LARGE_INTEGER size{};
        if (executable.value==INVALID_HANDLE_VALUE || !GetFileSizeEx(executable.value,&size)
            || size.QuadPart<=0 || size.QuadPart>320LL*1024*1024) return false;
        mapping.value=CreateFileMappingW(executable.value,nullptr,PAGE_READONLY,0,0,nullptr);
        if (mapping.value==INVALID_HANDLE_VALUE || !mapping.value) return false;
        mapped=static_cast<const unsigned char*>(MapViewOfFile(mapping.value,FILE_MAP_READ,0,0,0));
        mapped_size=static_cast<std::size_t>(size.QuadPart); if (!mapped) return false;
        for (const auto& range : native_ranges) {
            const bool pointers=range.rva==0x88fd2d0 || range.rva==0x88fd058 || range.rva==0x8937788 || range.rva==0x88fcf30;
            pins.push_back({range.rva,range.bytes,0,{},pointers});
        }
        if (!profile_matches() || MH_Initialize()!=MH_OK) return false;
        for (const auto& hook : hooks) {
            std::array<unsigned char,32> prefix{};
            const auto found=std::find_if(pins.begin(),pins.end(),[&](const auto& pin) { return pin.rva==hook.rva; });
            const auto* original=file_rva(hook.rva,hook.bytes);
            if (found==pins.end() || hook.bytes>32 || !decode(hook.prefix,prefix.data(),hook.bytes)
                || !original || std::memcmp(original,prefix.data(),hook.bytes)
                || MH_CreateHook(reinterpret_cast<void*>(base+hook.rva),hook.replacement,hook.original)!=MH_OK
                || !register_unwind(hook,prefix.data())) return false;
        }
        for (const auto& hook : hooks) if (MH_QueueEnableHook(reinterpret_cast<void*>(base+hook.rva))!=MH_OK) return false;
        if (MH_ApplyQueued()!=MH_OK) return false;
        for (const auto& hook : hooks) {
            auto& pin=*std::find_if(pins.begin(),pins.end(),[&](const auto& value) { return value.rva==hook.rva; });
            pin.skip=hook.bytes;
            if (!read_memory(base+pin.rva,pin.patch.data(),pin.skip)) return false;
        }
        return profile_matches();
    } catch (...) { return false; }
}
#ifdef LGSM_DRAGON_SAVE_TESTS
namespace {
unsigned __declspec(noinline) test_replacement() { return 99; }
}
bool test_trampoline(const Hook& input,std::span<const std::pair<unsigned,unsigned>> pushes,
    unsigned allocation_after,unsigned allocation_bytes) noexcept {
    auto* page=static_cast<unsigned char*>(VirtualAlloc(nullptr,4096,MEM_RESERVE|MEM_COMMIT,PAGE_EXECUTE_READWRITE));
    if (!page) return false;
    const auto previous_base=base,prior_tables=tables.size(); bool ok{}; void* trampoline{};
    try {
        std::array<unsigned char,32> prefix{};
        if (!decode(input.prefix,prefix.data(),input.bytes) || input.bytes>32) throw std::runtime_error("prefix");
        std::memcpy(page,prefix.data(),input.bytes); auto offset=input.bytes;
        if (allocation_bytes) {
            constexpr unsigned char add[]={0x48,0x81,0xc4}; std::memcpy(page+offset,add,sizeof add); offset+=sizeof add;
            std::memcpy(page+offset,&allocation_bytes,sizeof allocation_bytes); offset+=sizeof allocation_bytes;
        }
        for (auto item=pushes.rbegin();item!=pushes.rend();++item) {
            if (item->second>=8) page[offset++]=0x41;
            page[offset++]=static_cast<unsigned char>(0x58+(item->second&7));
        }
        constexpr unsigned char end[]={0xb8,42,0,0,0,0xc3}; std::memcpy(page+offset,end,sizeof end);
        FlushInstructionCache(GetCurrentProcess(),page,4096); base=reinterpret_cast<std::uintptr_t>(page);
        if (MH_Initialize()!=MH_OK || MH_CreateHook(page,reinterpret_cast<void*>(&test_replacement),&trampoline)!=MH_OK) throw std::runtime_error("minhook");
        Hook hook=input; hook.rva=0; hook.original=&trampoline;
        if (!register_unwind(hook,prefix.data()) || MH_EnableHook(page)!=MH_OK) throw std::runtime_error("unwind_install");
        using Call=unsigned(*)();
        if (reinterpret_cast<Call>(page)()!=99 || reinterpret_cast<Call>(trampoline)()!=42) throw std::runtime_error("actual_trampoline");
        for (unsigned point=0;point<input.bytes+14;++point) {
            alignas(16) DWORD64 stack[160]{}; const auto entry=reinterpret_cast<DWORD64>(&stack[120]);
            constexpr DWORD64 return_to=0x12345678; *reinterpret_cast<DWORD64*>(entry)=return_to;
            CONTEXT context{}; context.ContextFlags=CONTEXT_FULL; context.Rip=reinterpret_cast<DWORD64>(trampoline)+point; context.Rsp=entry;
            auto* registers=&context.Rax;
            for (unsigned reg=0;reg<16;++reg) if (reg!=4) registers[reg]=0x10000+reg;
            unsigned pushed{};
            for (const auto& [after,reg] : pushes) if (point>=after) {
                ++pushed; *reinterpret_cast<DWORD64*>(entry-pushed*8)=registers[reg];
            }
            // Nonvolatile register stores copied before stack allocation.
            if (point>=5 && input.prefix=="48895c2408") stack[121]=context.Rbx;
            if (point>=5 && input.prefix=="48895c2410") stack[122]=context.Rbx;
            if (point>=5 && input.prefix=="48895c2420") stack[124]=context.Rbx;
            if (point>=5 && input.prefix=="48896c2418") stack[123]=context.Rbp;
            context.Rsp-=pushed*8;
            if (allocation_after && point>=allocation_after) context.Rsp-=allocation_bytes;
            DWORD64 owner{},establisher{}; void* data{};
            auto* function=RtlLookupFunctionEntry(context.Rip,&owner,nullptr);
            if (!function || function!=&tables.back()->function) throw std::runtime_error("lookup");
            RtlVirtualUnwind(UNW_FLAG_NHANDLER,owner,context.Rip,function,&context,&data,&establisher,nullptr);
            if (context.Rip!=return_to || context.Rsp!=entry+8) throw std::runtime_error("stack");
            for (unsigned reg : {3U,5U,6U,7U,12U,13U,14U,15U})
                if (registers[reg]!=0x10000+reg) throw std::runtime_error("register");
        }
        ok=true;
    } catch (const std::exception& error) { std::fprintf(stderr,"Dragonwilds trampoline: %s prefix=%s\n",error.what(),input.prefix.data()); }
    catch (...) {}
    MH_DisableHook(page); MH_RemoveHook(page); MH_Uninitialize();
    while (tables.size()>prior_tables) { auto& item=*tables.back(); RtlDeleteFunctionTable(&item.function); VirtualFree(item.page,0,MEM_RELEASE); tables.pop_back(); }
    base=previous_base; VirtualFree(page,0,MEM_RELEASE); return ok;
}
#endif
}
