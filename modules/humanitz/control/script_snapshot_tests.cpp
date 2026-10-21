#include "script_snapshot.hpp"
#include <array>
#include <cstdio>
#include <cstdlib>
using namespace humanitz_inspect;
unsigned checks{};
void check(bool value) { ++checks; if (!value) std::exit(1); }
template<class T> void put(void* object,std::size_t offset,T value) { std::memcpy(static_cast<unsigned char*>(object)+offset,&value,sizeof value); }
int main() {
    auto* base=static_cast<unsigned char*>(VirtualAlloc(nullptr,128ULL*1024*1024,MEM_RESERVE,PAGE_NOACCESS)); check(base!=nullptr);
    auto* globals=base+(0x4a7b2f0&~4095U); check(VirtualAlloc(globals,4096,MEM_COMMIT,PAGE_READWRITE)==globals);
    std::array<unsigned char,0xe0> function{}; std::array<unsigned char,24> item{};
    std::array<unsigned char,4> code={0x1d,1,0x53,0x16}; std::uintptr_t chunk=reinterpret_cast<std::uintptr_t>(item.data());
    const auto image=reinterpret_cast<std::uintptr_t>(base),object=reinterpret_cast<std::uintptr_t>(function.data());
    put(base,0x4a7b2f0,reinterpret_cast<std::uintptr_t>(&chunk)); put<std::int32_t>(base,0x4a7b304,1);
    put(function.data(),0,image+0x39b4908); put<std::uintptr_t>(function.data(),0x10,0x12345678);
    put(item.data(),0,object); put<std::int32_t>(item.data(),16,17);
    put(function.data(),0x60,ScriptArray{reinterpret_cast<std::uintptr_t>(code.data()),4,4}); put<std::uint32_t>(function.data(),0xb0,0x800);
    const auto accepted=snapshot(image,object); check(accepted.bytes==std::vector<unsigned char>(code.begin(),code.end()));
    auto rejects=[&] { try { (void)snapshot(image,object); return false; } catch (...) { return true; } };
    put<std::int32_t>(function.data(),0xc,1); check(rejects()); put<std::int32_t>(function.data(),0xc,0);
    put<std::uintptr_t>(item.data(),0,object+8); check(rejects()); put(item.data(),0,object);
    put<std::uintptr_t>(function.data(),0,image+0x39b4900); check(rejects()); put(function.data(),0,image+0x39b4908);
    put<std::uint32_t>(function.data(),8,0x8000); check(rejects()); put<std::uint32_t>(function.data(),8,0);
    put<std::uint32_t>(function.data(),0xb0,0x400); check(rejects()); put<std::uint32_t>(function.data(),0xb0,0x800);
    for (auto array : {ScriptArray{0,-1,0},ScriptArray{0,5,4},ScriptArray{0,0,1024*1024+1},ScriptArray{reinterpret_cast<std::uintptr_t>(base),4,4}}) {
        put(function.data(),0x60,array); check(rejects());
    }
    put(function.data(),0x60,ScriptArray{}); put<std::uint32_t>(function.data(),0xb0,0x400);
    check(snapshot(image,object).bytes.empty());
    VirtualFree(base,0,MEM_RELEASE);
    std::printf("PASS %u HumanitZ actual process-memory snapshot boundaries (synthetic UObject data; not game integration)\n",checks);
}
