#pragma once
#include <Windows.h>
#include <cstdint>
#include <cstring>
#include <stdexcept>
#include <vector>
namespace humanitz_inspect {
inline void need(bool value) { if (!value) throw std::runtime_error("script_snapshot_rejected"); }
inline void memory(std::uintptr_t address,void* result,std::size_t bytes) {
    SIZE_T count{}; need(address && address<=UINTPTR_MAX-bytes
        && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),result,bytes,&count) && count==bytes);
}
template<class T> T read(std::uintptr_t address) { T result{}; memory(address,&result,sizeof result); return result; }
struct Identity {
    std::uintptr_t object{},type{}; std::int32_t index{},serial{};
    bool operator==(const Identity&) const = default;
};
inline Identity identity(std::uintptr_t base,std::uintptr_t object) {
    const auto flags=read<std::uint32_t>(object+8); const auto index=read<std::int32_t>(object+0xc);
    const auto count=read<std::int32_t>(base+0x4a7b304);
    need(!(flags&0x18000) && index>=0 && index<count && count<=8*1024*1024);
    const auto chunks=read<std::uintptr_t>(base+0x4a7b2f0);
    const auto chunk=read<std::uintptr_t>(chunks+static_cast<unsigned>(index>>16)*8);
    const auto item=chunk+static_cast<unsigned>(index&65535)*24;
    need(read<std::uintptr_t>(item)==object);
    return {object,read<std::uintptr_t>(object+0x10),index,read<std::int32_t>(item+16)};
}
struct ScriptArray { std::uintptr_t data{}; std::int32_t size{},capacity{}; bool operator==(const ScriptArray&) const = default; };
struct Snapshot { std::uintptr_t object{}; std::uint32_t flags{}; std::vector<unsigned char> bytes; };
inline Snapshot snapshot(std::uintptr_t base,std::uintptr_t object) {
    const auto before=identity(base,object);
    need(before.type && read<std::uintptr_t>(object)==base+0x39b4908); // Exact fixed UFunction vtable.
    const auto flags=read<std::uint32_t>(object+0xb0);
    const auto array=read<ScriptArray>(object+0x60);
    need(array.size>=0 && array.capacity>=array.size && array.capacity<=1024*1024
        && (!(flags&0x400) || array.size==0));
    Snapshot result{object,flags,std::vector<unsigned char>(static_cast<std::size_t>(array.size))};
    if (!result.bytes.empty()) memory(array.data,result.bytes.data(),result.bytes.size());
    need(identity(base,object)==before && read<std::uintptr_t>(object)==base+0x39b4908
        && read<std::uint32_t>(object+0xb0)==flags && read<ScriptArray>(object+0x60)==array);
    return result;
}
}
