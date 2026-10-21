#include "script_snapshot.hpp"
#include <bcrypt.h>
#include <array>
#include <charconv>
#include <filesystem>
#include <memory>
#include <set>
#include <string>
#include <string_view>
namespace humanitz_inspect {
namespace fs=std::filesystem;
namespace {
struct Handle {
    HANDLE value=INVALID_HANDLE_VALUE;
    explicit Handle(HANDLE file=INVALID_HANDLE_VALUE):value(file) {}
    ~Handle() { if (value!=INVALID_HANDLE_VALUE && value) CloseHandle(value); }
    Handle(Handle&& other) noexcept:value(other.value) { other.value=INVALID_HANDLE_VALUE; }
    Handle& operator=(Handle&& other) noexcept {
        if (this!=&other) { if (value!=INVALID_HANDLE_VALUE && value) CloseHandle(value); value=other.value; other.value=INVALID_HANDLE_VALUE; }
        return *this;
    }
    Handle(const Handle&)=delete;
};
bool nonce(std::string_view text) { return text.size()==32 && text.find_first_not_of("0123456789abcdef")==text.npos; }
fs::path module(HMODULE value) {
    std::vector<wchar_t> path(32768); const auto size=GetModuleFileNameW(value,path.data(),static_cast<DWORD>(path.size()));
    need(size && size<path.size()); return fs::path(std::wstring(path.data(),size));
}
bool equal(const fs::path& first,const fs::path& second) {
    auto left=first.lexically_normal(),right=second.lexically_normal(); left.make_preferred(); right.make_preferred();
    return CompareStringOrdinal(left.c_str(),-1,right.c_str(),-1,TRUE)==CSTR_EQUAL;
}
void regular(HANDLE file,bool directory) {
    BY_HANDLE_FILE_INFORMATION info{};
    need(file!=INVALID_HANDLE_VALUE && GetFileInformationByHandle(file,&info)
        && !(info.dwFileAttributes&FILE_ATTRIBUTE_REPARSE_POINT)
        && static_cast<bool>(info.dwFileAttributes&FILE_ATTRIBUTE_DIRECTORY)==directory
        && (directory || info.nNumberOfLinks==1));
}
std::vector<Handle> pin(const fs::path& input) {
    std::vector<fs::path> paths;
    for (auto path=input;!path.empty();) {
        paths.push_back(path); need(paths.size()<=128); const auto parent=path.parent_path(); if (parent==path) break; path=parent;
    }
    std::vector<Handle> result;
    for (auto at=paths.rbegin();at!=paths.rend();++at) {
        Handle file(CreateFileW(at->c_str(),GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT|FILE_FLAG_BACKUP_SEMANTICS,nullptr));
        regular(file.value,true); result.push_back(std::move(file));
    }
    return result;
}
Handle file(const fs::path& path,DWORD access,DWORD disposition,DWORD share) {
    Handle value(CreateFileW(path.c_str(),access,share,nullptr,disposition,FILE_FLAG_OPEN_REPARSE_POINT,nullptr)); regular(value.value,false); return value;
}
std::string hash(HANDLE input) {
    LARGE_INTEGER size{}; need(GetFileSizeEx(input,&size) && size.QuadPart>0 && size.QuadPart<=128LL*1024*1024);
    BCRYPT_ALG_HANDLE algorithm{}; need(BCryptOpenAlgorithmProvider(&algorithm,BCRYPT_SHA256_ALGORITHM,nullptr,0)>=0);
    struct Algorithm { BCRYPT_ALG_HANDLE value; ~Algorithm(){BCryptCloseAlgorithmProvider(value,0);} } provider{algorithm};
    BCRYPT_HASH_HANDLE digest{}; need(BCryptCreateHash(algorithm,&digest,nullptr,0,nullptr,0,0)>=0);
    struct Hash { BCRYPT_HASH_HANDLE value; ~Hash(){BCryptDestroyHash(value);} } owned{digest};
    std::array<unsigned char,65536> bytes{}; std::uint64_t total{};
    for (;;) { DWORD count{}; need(ReadFile(input,bytes.data(),static_cast<DWORD>(bytes.size()),&count,nullptr)); if (!count) break;
        need(total<=static_cast<std::uint64_t>(size.QuadPart) && count<=static_cast<std::uint64_t>(size.QuadPart)-total
            && BCryptHashData(digest,bytes.data(),count,0)>=0); total+=count; }
    std::array<unsigned char,32> value{}; need(total==static_cast<std::uint64_t>(size.QuadPart) && BCryptFinishHash(digest,value.data(),32,0)>=0);
    std::string result; for (auto byte:value) { result+="0123456789abcdef"[byte>>4]; result+="0123456789abcdef"[byte&15]; } return result;
}
struct Image {
    std::uintptr_t base{}; fs::path directory; std::string run; bool (*game_thread)(){};
    std::vector<Handle> pins; Handle game,core;
    Image() {
        const auto path=module(nullptr),win64=path.parent_path();
        need(equal(path.filename(),L"HumanitZServer-Win64-Shipping.exe"));
        HMODULE self{}; need(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCWSTR>(&module),&self));
        need(equal(module(self),win64/L"ue4ss/Mods/LgsmHumanitZInspect/native/LgsmHumanitZInspect.dll"));
        const auto loader=GetModuleHandleW(L"UE4SS.dll"); need(loader && equal(module(loader),win64/L"ue4ss/UE4SS.dll"));
        const auto root=win64.parent_path().parent_path().parent_path();
        // This diagnostic refuses the original Steam package tree even if it
        // is accidentally copied there. Only a physical disposable test copy.
        const auto text=root.native(); constexpr wchar_t original[]=L"D:\\LanGame\\server-files";
        need(text.size()<std::size(original)-1 || CompareStringOrdinal(text.c_str(),static_cast<int>(std::size(original)-1),original,-1,TRUE)!=CSTR_EQUAL);
        directory=root/L"langame_humanitz_inspect"; pins=pin(directory);
        game=file(path,GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ);
        core=file(module(loader),GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ);
        need(hash(game.value)=="c3f46f78220605e7a73151261b9bdb072786170dc0edc8c75da4ed872d67671a"
            && hash(core.value)=="91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958");
        std::array<char,34> environment{}; const auto count=GetEnvironmentVariableA("LGSM_HUMANITZ_INSPECT_RUN",environment.data(),34);
        need(count==32 && nonce(std::string_view(environment.data(),count))); run.assign(environment.data(),count);
        base=reinterpret_cast<std::uintptr_t>(GetModuleHandleW(nullptr));
        game_thread=reinterpret_cast<bool(*)()>(GetProcAddress(loader,"?IsInGameThreadRaw@Unreal@RC@@YA_NXZ"));
        need(game_thread && game_thread());
    }
};
std::string request(const fs::path& path) {
    auto input=file(path,GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ); LARGE_INTEGER size{};
    need(GetFileSizeEx(input.value,&size) && size.QuadPart>0 && size.QuadPart<=65536);
    std::string body(static_cast<std::size_t>(size.QuadPart),'\0'); DWORD count{};
    need(ReadFile(input.value,body.data(),static_cast<DWORD>(body.size()),&count,nullptr) && count==body.size()); return body;
}
std::string_view field(std::string_view& body,std::string_view key) {
    const auto end=body.find('\n'); need(end!=body.npos && body.starts_with(key));
    const auto value=body.substr(key.size(),end-key.size()); body.remove_prefix(end+1); return value;
}
void append(std::vector<unsigned char>& bytes,const void* data,std::size_t count) {
    const auto* begin=static_cast<const unsigned char*>(data); bytes.insert(bytes.end(),begin,begin+count);
}
void inspect() {
    static Image image; static std::set<std::string> recent;
    need(image.game_thread() && recent.size()<8);
    const auto input=request(image.directory/L"scripts-request.txt"); std::string_view body=input;
    need(field(body,"schema=")=="1" && field(body,"run=")==image.run);
    const std::string id(field(body,"nonce=")); need(nonce(id) && !recent.contains(id));
    std::vector<Snapshot> snapshots; std::set<std::uintptr_t> seen; std::size_t total{};
    while (!body.empty()) {
        const auto text=field(body,"function="); need(!text.empty() && text.size()<=20 && text.front()!='0');
        std::uintptr_t object{}; const auto [end,error]=std::from_chars(text.data(),text.data()+text.size(),object);
        need(error==std::errc{} && end==text.data()+text.size() && object && seen.insert(object).second && seen.size()<=512);
        auto value=snapshot(image.base,object); total+=value.bytes.size(); need(total<=32*1024*1024); snapshots.push_back(std::move(value));
    }
    need(!snapshots.empty()); recent.insert(id);
    FILETIME created{},ended{},kernel{},user{}; need(GetProcessTimes(GetCurrentProcess(),&created,&ended,&kernel,&user));
    const auto creation=static_cast<std::uint64_t>(created.dwHighDateTime)<<32|created.dwLowDateTime;
    const auto header="LGSM_HUMANITZ_SCRIPT_1\nrun="+image.run+"\nnonce="+id+"\npid="+std::to_string(GetCurrentProcessId())
        +"\ncreation_time="+std::to_string(creation)+"\ncount="+std::to_string(snapshots.size())+"\n\n";
    std::vector<unsigned char> output; append(output,header.data(),header.size());
    for (const auto& value:snapshots) {
        const auto size=static_cast<std::uint32_t>(value.bytes.size());
        append(output,&value.object,8); append(output,&value.flags,4); append(output,&size,4);
        if (size) append(output,value.bytes.data(),size);
    }
    auto destination=file(image.directory/(L"scripts-"+std::wstring(id.begin(),id.end())+L".bin"),GENERIC_WRITE,CREATE_NEW,0);
    DWORD count{}; need(WriteFile(destination.value,output.data(),static_cast<DWORD>(output.size()),&count,nullptr)
        && count==output.size() && FlushFileBuffers(destination.value));
    const auto closed=destination.value; destination.value=INVALID_HANDLE_VALUE; need(CloseHandle(closed));
}
}
}
extern "C" __declspec(dllexport) int luaopen_lgsm_humanitz_inspect(void*) noexcept {
    static SRWLOCK lock=SRWLOCK_INIT; if (!TryAcquireSRWLockExclusive(&lock)) return 0;
    try { humanitz_inspect::inspect(); }
    catch (...) { OutputDebugStringA("[LgsmHumanitZInspect] Script snapshot rejected.\n"); }
    ReleaseSRWLockExclusive(&lock); return 0;
}
