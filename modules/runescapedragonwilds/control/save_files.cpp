#include "save_files.hpp"
#include <bcrypt.h>
#include <algorithm>
#include <stdexcept>
namespace dragon_save {
namespace {
void need(bool value) { if (!value) throw std::runtime_error("native_readback_unavailable"); }
void regular(HANDLE file, bool directory) {
    BY_HANDLE_FILE_INFORMATION info{};
    need(GetFileInformationByHandle(file,&info) && !(info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT)
        && ((info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0) == directory
        && (directory || info.nNumberOfLinks == 1));
}
}
fs::path full_path(const fs::path& input) {
    const auto& text=input.native();
    need(!text.empty() && text.size()<32767 && text.find(L':',2)==text.npos);
    for (auto character : text) need(character>=32 && character!=L'"');
    std::vector<wchar_t> result(32768);
    const auto count=GetFullPathNameW(text.c_str(),static_cast<DWORD>(result.size()),result.data(),nullptr);
    need(count && count<result.size());
    return fs::path(std::wstring(result.data(),count)).lexically_normal();
}
bool equal_path(const fs::path& first,const fs::path& second) noexcept {
    return CompareStringOrdinal(first.c_str(),-1,second.c_str(),-1,TRUE)==CSTR_EQUAL;
}
std::vector<Handle> pin_parents(const fs::path& file) {
    std::vector<fs::path> paths;
    for (auto current=full_path(file).parent_path();!current.empty();) {
        paths.push_back(current); need(paths.size()<=128);
        const auto parent=current.parent_path(); if (parent==current) break; current=parent;
    }
    std::reverse(paths.begin(),paths.end()); std::vector<Handle> result;
    for (const auto& path : paths) {
        Handle directory(CreateFileW(path.c_str(),GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,
            OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT|FILE_FLAG_BACKUP_SEMANTICS,nullptr));
        need(directory.value!=INVALID_HANDLE_VALUE); regular(directory.value,true);
        result.push_back(std::move(directory));
    }
    return result;
}
Snapshot snapshot(const fs::path& input) {
    const auto path=full_path(input); Snapshot result; result.parents=pin_parents(path);
    Handle file(CreateFileW(path.c_str(),GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,
        FILE_FLAG_OPEN_REPARSE_POINT|FILE_FLAG_SEQUENTIAL_SCAN,nullptr));
    need(file.value!=INVALID_HANDLE_VALUE); regular(file.value,false);
    BY_HANDLE_FILE_INFORMATION info{}; LARGE_INTEGER size{};
    need(GetFileInformationByHandle(file.value,&info) && GetFileSizeEx(file.value,&size)
        && size.QuadPart>0 && size.QuadPart<=512LL*1024*1024);
    result.size=static_cast<std::uint64_t>(size.QuadPart);
    result.volume=info.dwVolumeSerialNumber;
    result.index=static_cast<std::uint64_t>(info.nFileIndexHigh)<<32 | info.nFileIndexLow;
    BCRYPT_ALG_HANDLE algorithm{};
    need(BCryptOpenAlgorithmProvider(&algorithm,BCRYPT_SHA256_ALGORITHM,nullptr,0)>=0);
    struct Algorithm { BCRYPT_ALG_HANDLE value; ~Algorithm() { BCryptCloseAlgorithmProvider(value,0); } } provider{algorithm};
    BCRYPT_HASH_HANDLE hash{}; need(BCryptCreateHash(algorithm,&hash,nullptr,0,nullptr,0,0)>=0);
    struct Hash { BCRYPT_HASH_HANDLE value; ~Hash() { BCryptDestroyHash(value); } } owned{hash};
    std::array<unsigned char,65536> bytes{}; std::uint64_t total{};
    for (;;) {
        DWORD count{}; need(ReadFile(file.value,bytes.data(),static_cast<DWORD>(bytes.size()),&count,nullptr)!=FALSE);
        if (!count) break;
        need(total<=result.size && count<=result.size-total && BCryptHashData(hash,bytes.data(),count,0)>=0);
        total+=count;
    }
    need(total==result.size && BCryptFinishHash(hash,result.digest.data(),static_cast<ULONG>(result.digest.size()),0)>=0);
    regular(file.value,false);
    BY_HANDLE_FILE_INFORMATION after{}; need(GetFileInformationByHandle(file.value,&after)
        && after.dwVolumeSerialNumber==info.dwVolumeSerialNumber && after.nFileIndexHigh==info.nFileIndexHigh
        && after.nFileIndexLow==info.nFileIndexLow && after.nFileSizeHigh==info.nFileSizeHigh
        && after.nFileSizeLow==info.nFileSizeLow);
    return result;
}
bool matches(const Snapshot& expected,const fs::path& actual,bool same_identity) noexcept {
    try {
        const auto value=snapshot(actual);
        return value.size==expected.size && value.digest==expected.digest
            && (!same_identity || (value.index==expected.index && value.volume==expected.volume));
    } catch (...) { return false; }
}
}
