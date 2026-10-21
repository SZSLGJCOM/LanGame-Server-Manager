// SCUM build 25247308's SQLite 3.39.4. No backup API or schema mutations.
// This is only payload readback: the caller must separately prove owned SQL
// transactions, native queue/continuation completion, and OS I/O results.
#pragma once
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <algorithm>
#include <array>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <stdexcept>
#include <string>
#include <vector>

namespace scum_save {
namespace fs=std::filesystem;
inline void sqlite_require(bool value,const char* issue) {
    if(!value)throw std::runtime_error(issue);
}
struct SqlitePin {
    HANDLE value{INVALID_HANDLE_VALUE};
    explicit SqlitePin(HANDLE input) noexcept:value(input){}
    SqlitePin(SqlitePin&& other) noexcept:value(other.value){other.value=INVALID_HANDLE_VALUE;}
    SqlitePin& operator=(SqlitePin&& other) noexcept {
        if(this!=&other){if(value!=INVALID_HANDLE_VALUE)CloseHandle(value);value=other.value;other.value=INVALID_HANDLE_VALUE;}return *this;
    }
    SqlitePin(const SqlitePin&)=delete;
    SqlitePin& operator=(const SqlitePin&)=delete;
    ~SqlitePin(){if(value!=INVALID_HANDLE_VALUE)CloseHandle(value);}
};
inline bool sqlite_path_equal(const fs::path& a,const fs::path& b) noexcept {
    return CompareStringOrdinal(a.c_str(),-1,b.c_str(),-1,TRUE)==CSTR_EQUAL;
}
inline fs::path sqlite_absolute(const fs::path& input) {
    sqlite_require(input.is_absolute(),"sqlite_path_not_absolute");
    const auto normalized=input.lexically_normal();
    std::array<wchar_t,32768> buffer{};
    const auto size=GetFullPathNameW(normalized.c_str(),static_cast<DWORD>(buffer.size()),buffer.data(),nullptr);
    sqlite_require(size>0&&size<buffer.size(),"sqlite_path_length");
    auto result=fs::path(std::wstring(buffer.data(),size)).lexically_normal();
    sqlite_require(result.wstring().find(L':',2)==std::wstring::npos,"sqlite_path_stream");
    return result;
}
inline std::vector<SqlitePin> sqlite_pin_parents(const fs::path& leaf) {
    std::vector<fs::path> chain;
    for(auto current=leaf.parent_path();!current.empty();) {
        chain.push_back(current);const auto next=current.parent_path();
        if(next==current)break;current=next;
        sqlite_require(chain.size()<128,"sqlite_directory_depth");
    }
    std::reverse(chain.begin(),chain.end());std::vector<SqlitePin> result;
    for(const auto& directory:chain) {
        SqlitePin pin(CreateFileW(directory.c_str(),FILE_LIST_DIRECTORY|FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
        BY_HANDLE_FILE_INFORMATION info{};
        sqlite_require(pin.value!=INVALID_HANDLE_VALUE&&GetFileInformationByHandle(pin.value,&info)&&
            (info.dwFileAttributes&FILE_ATTRIBUTE_DIRECTORY)&&!(info.dwFileAttributes&FILE_ATTRIBUTE_REPARSE_POINT),
            "sqlite_directory_not_ordinary");
        result.push_back(std::move(pin));
    }
    return result;
}
inline SqlitePin sqlite_pin_file(const fs::path& path) {
    // Actual data read access makes no-delete sharing effective while permitting
    // the game's SQLite reader/writer to retain its ordinary read/write sharing.
    SqlitePin pin(CreateFileW(path.c_str(),GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,
        nullptr,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
    BY_HANDLE_FILE_INFORMATION info{};
    sqlite_require(pin.value!=INVALID_HANDLE_VALUE&&GetFileInformationByHandle(pin.value,&info)&&
        info.nNumberOfLinks==1&&!(info.dwFileAttributes&(FILE_ATTRIBUTE_DIRECTORY|FILE_ATTRIBUTE_REPARSE_POINT)),
        "sqlite_file_not_ordinary");
    return pin;
}
inline bool sqlite_same_file(HANDLE a,HANDLE b) noexcept {
    BY_HANDLE_FILE_INFORMATION first{},second{};
    return GetFileInformationByHandle(a,&first)&&GetFileInformationByHandle(b,&second)&&
        first.dwVolumeSerialNumber==second.dwVolumeSerialNumber&&first.nFileIndexHigh==second.nFileIndexHigh&&
        first.nFileIndexLow==second.nFileIndexLow;
}
class Sqlite {
    std::uintptr_t image_;
    template<class T>T function(unsigned index,std::uint32_t expected) const {
        const auto pointer=*reinterpret_cast<const std::uintptr_t*>(image_+0x6162700+index*8);
        sqlite_require(pointer==image_+expected,"sqlite_api_changed");return reinterpret_cast<T>(pointer);
    }
public:
    explicit Sqlite(std::uintptr_t image):image_(image){}
    int open(const std::string& path,void** db,int flags) const {
        return function<int(*)(const char*,void**,int,const char*)>(135,0x28ecec0)(path.c_str(),db,flags,nullptr);
    }
    int close(void* db) const {return function<int(*)(void*)>(16,0x28ea320)(db);}
    void release(void* data) const {function<void(*)(void*)>(58,0x28ec500)(data);}
    unsigned char* serialize(void* db,std::int64_t* size) const {
        return function<unsigned char*(*)(void*,const char*,std::int64_t*,unsigned)>(263,0x28f8f70)(db,"main",size,0);
    }
    int autocommit(void* db) const {return function<int(*)(void*)>(60,0x28f1be0)(db);}
    int transaction(void* db) const {return function<int(*)(void*,const char*)>(252,0x28f1cd0)(db,"main");}
    const char* filename(void* db) const {return function<const char*(*)(void*,const char*)>(180,0x28f1c20)(db,"main");}
    const char* name(void* db,int index) const {return function<const char*(*)(void*,int)>(264,0x28f1c00)(db,index);}
    int readonly(void* db) const {return function<int(*)(void*,const char*)>(181,0x28f1c80)(db,"main");}
};
inline std::string sqlite_utf8(const fs::path& path) {
    const auto& text=path.native();
    sqlite_require(text.size()<32768,"sqlite_path_length");
    const int count=WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,text.c_str(),-1,nullptr,0,nullptr,nullptr);
    sqlite_require(count>1&&count<=131072,"sqlite_path_encoding");
    std::string result(static_cast<std::size_t>(count),'\0');
    sqlite_require(WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,text.c_str(),-1,result.data(),count,nullptr,nullptr)==count,
        "sqlite_path_encoding");result.pop_back();return result;
}
inline fs::path sqlite_filename(const Sqlite& api,void* db) {
    const char* value=api.filename(db);sqlite_require(value&&*value,"sqlite_non_file_database");
    const auto bytes=strnlen_s(value,131072);sqlite_require(bytes>0&&bytes<131072,"sqlite_path_length");
    const int size=MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,value,static_cast<int>(bytes),nullptr,0);
    sqlite_require(size>0&&size<32768,"sqlite_path_encoding");
    std::wstring wide(static_cast<std::size_t>(size),L'\0');
    sqlite_require(MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,value,static_cast<int>(bytes),wide.data(),size)==size,
        "sqlite_path_encoding");return sqlite_absolute(fs::path(wide));
}
struct SqliteBuffer {
    const Sqlite& api;std::int64_t bytes{};unsigned char* value{};
    SqliteBuffer(const Sqlite& input,void* db):api(input),value(api.serialize(db,&bytes)) {
        // A bound limits copying/comparison work. Native serialize allocation is
        // owned by SQLite and is always freed using this same fixed engine.
        if(!value||bytes<=0||bytes>512LL*1024*1024) {
            if(value)api.release(value);value=nullptr;throw std::runtime_error("sqlite_serialize_failed");
        }
    }
    SqliteBuffer(const SqliteBuffer&)=delete;
    ~SqliteBuffer() noexcept {
        try {if(value)api.release(value);}
        catch(...) {OutputDebugStringA("[LgsmWorldSave] SCUM SQLite buffer cleanup failed.\n");}
    }
};
struct SqliteConnection {
    const Sqlite& api;void* value{};
    explicit SqliteConnection(const Sqlite& input):api(input){}
    ~SqliteConnection() noexcept {
        try {if(value&&api.close(value)!=0)OutputDebugStringA("[LgsmWorldSave] SCUM SQLite reader close failed.\n");}
        catch(...) {OutputDebugStringA("[LgsmWorldSave] SCUM SQLite reader cleanup failed.\n");}
    }
    void finish(){sqlite_require(value&&api.close(value)==0,"sqlite_readonly_close_failed");value=nullptr;}
};
inline void sqlite_readback(const Sqlite& api,void* db,DWORD owner_thread,const fs::path& saved_root,std::uint64_t deadline) {
    sqlite_require(db&&owner_thread==GetCurrentThreadId(),"sqlite_connection_wrong_thread");
    sqlite_require(GetTickCount64()<=deadline,"sqlite_readback_timeout");
    sqlite_require(api.autocommit(db)==1&&api.transaction(db)==0,"sqlite_transaction_not_closed");
    // Main is the persistent database in this fixed adapter. Attached databases
    // need their own write and readback closure, so reject rather than omit one.
    for(int index=0;index<4;++index) {
        const auto* name=api.name(db,index);if(!name)break;
        sqlite_require(!std::strcmp(name,"main")||!std::strcmp(name,"temp"),"sqlite_attached_database");
        sqlite_require(index<3,"sqlite_attached_database");
    }
    const auto root=sqlite_absolute(saved_root);const auto path=sqlite_filename(api,db);
    const auto prefix=root.wstring()+L"\\";const auto text=path.wstring();
    sqlite_require(text.size()>prefix.size()&&CompareStringOrdinal(text.c_str(),static_cast<int>(prefix.size()),
        prefix.c_str(),static_cast<int>(prefix.size()),TRUE)==CSTR_EQUAL,"sqlite_database_outside_saved");
    auto parents=sqlite_pin_parents(path);auto file=sqlite_pin_file(path);
    // Pin existing WAL/SHM objects during both snapshots. Their absence is valid
    // in rollback-journal mode; unreadable or unusual objects are not absence.
    std::vector<SqlitePin> sidecars;
    for(const auto* suffix:{L"-wal",L"-shm"}) {
        const auto sidecar=fs::path(path.wstring()+suffix);const auto attributes=GetFileAttributesW(sidecar.c_str());
        if(attributes==INVALID_FILE_ATTRIBUTES)sqlite_require(GetLastError()==ERROR_FILE_NOT_FOUND,"sqlite_sidecar_metadata");
        else sidecars.push_back(sqlite_pin_file(sidecar));
    }
    SqliteBuffer source(api,db);SqliteConnection reader(api);
    const int opened=api.open(sqlite_utf8(path),&reader.value,1); // SQLITE_OPEN_READONLY, no URI/create flags.
    sqlite_require(opened==0&&reader.value&&api.readonly(reader.value)==1,"sqlite_readonly_open_failed");
    sqlite_require(sqlite_path_equal(sqlite_filename(api,reader.value),path),"sqlite_readonly_path_changed");
    {
        SqliteBuffer disk(api,reader.value);sqlite_require(source.bytes==disk.bytes,"sqlite_readback_mismatch");
        for(std::int64_t offset=0;offset<source.bytes;offset+=65536) {
            sqlite_require(GetTickCount64()<=deadline,"sqlite_readback_timeout");
            const auto bytes=static_cast<std::size_t>(std::min<std::int64_t>(65536,source.bytes-offset));
            sqlite_require(std::memcmp(source.value+offset,disk.value+offset,bytes)==0,"sqlite_readback_mismatch");
        }
    }
    reader.finish();auto final=sqlite_pin_file(path);
    sqlite_require(sqlite_same_file(file.value,final.value)&&sqlite_path_equal(sqlite_filename(api,db),path),
        "sqlite_database_changed");
    sqlite_require(api.autocommit(db)==1&&api.transaction(db)==0,"sqlite_transaction_not_closed");
}
}
