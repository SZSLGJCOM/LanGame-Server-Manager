// Bounded copied-image host and OS guards; no game process is launched.
#pragma once
// Component only. The original package is never mapped or executed.
#include "../../../palworld/control/tests/copied_image.hpp"
#include "../native_sqlite.hpp"
#include "../sql_completion.hpp"
#include <bcrypt.h>
#include <set>
#include <map>
#include <mutex>
#include <sstream>
namespace palworld_save {
bool path_equal(const fs::path& a,const fs::path& b) noexcept {
 return CompareStringOrdinal(a.c_str(),-1,b.c_str(),-1,TRUE)==CSTR_EQUAL;
}
bool buffer_hash(const void* bytes,size_t count,Digest& result) noexcept {
 BCRYPT_ALG_HANDLE algorithm{};BCRYPT_HASH_HANDLE hash{};
 bool ok=count<=ULONG_MAX && BCryptOpenAlgorithmProvider(&algorithm,BCRYPT_SHA256_ALGORITHM,nullptr,0)>=0 &&
  BCryptCreateHash(algorithm,&hash,nullptr,0,nullptr,0,0)>=0 &&
  BCryptHashData(hash,(PUCHAR)bytes,(ULONG)count,0)>=0 && BCryptFinishHash(hash,result.data(),32,0)>=0;
 if(hash)BCryptDestroyHash(hash);if(algorithm)BCryptCloseAlgorithmProvider(algorithm,0);return ok;
}
}
namespace {
std::set<HANDLE> writable_handles;
std::vector<Handle> leaf_pins;
unsigned checks{};
bool fail_write{},fail_flush{},fail_close{};
unsigned write_failures{},flush_failures{},close_failures{};
void check(bool value,const char* label) {
 require(value,label);++checks;std::cout<<"PASS "<<label<<std::endl;
}
bool ordinary_path(const wchar_t* path) {
 if(!path || !write_allowed(path))return false;
 const auto attr=GetFileAttributesW(path);
 if(attr==INVALID_FILE_ATTRIBUTES)return GetLastError()==ERROR_FILE_NOT_FOUND;
 if(attr&(FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_DIRECTORY))return false;
 Handle pin(CreateFileW(path,GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
 BY_HANDLE_FILE_INFORMATION info{};
 if(pin.value==INVALID_HANDLE_VALUE || !GetFileInformationByHandle(pin.value,&info) || info.nNumberOfLinks!=1)return false;
 leaf_pins.push_back(std::move(pin));return true;
}
HANDLE WINAPI sqlite_open_w(LPCWSTR path,DWORD access,DWORD sharing,LPSECURITY_ATTRIBUTES attrs,DWORD create,DWORD flags,HANDLE pattern) {
 const bool writes=(access&(GENERIC_WRITE|DELETE|WRITE_DAC|WRITE_OWNER)) || create!=OPEN_EXISTING;
 if(!path || !write_allowed(path) || (writes&&!ordinary_path(path))) { SetLastError(ERROR_ACCESS_DENIED);return INVALID_HANDLE_VALUE; }
 HANDLE result=CreateFileW(path,access,sharing,attrs,create,flags,pattern);
 if(result!=INVALID_HANDLE_VALUE && writes)writable_handles.insert(result);
 return result;
}
std::wstring widen(const char* text) {
 if(!text)return {};
 const int n=MultiByteToWideChar(CP_ACP,MB_ERR_INVALID_CHARS,text,-1,nullptr,0);
 if(n<=0 || n>32768)return {};
 std::wstring result(n,L'\0');MultiByteToWideChar(CP_ACP,MB_ERR_INVALID_CHARS,text,-1,result.data(),n);result.pop_back();return result;
}
HANDLE WINAPI sqlite_open_a(LPCSTR path,DWORD access,DWORD sharing,LPSECURITY_ATTRIBUTES attrs,DWORD create,DWORD flags,HANDLE pattern) {
 auto wide=widen(path);if(wide.empty()){SetLastError(ERROR_ACCESS_DENIED);return INVALID_HANDLE_VALUE;}
 return sqlite_open_w(wide.c_str(),access,sharing,attrs,create,flags,pattern);
}
BOOL WINAPI sqlite_write(HANDLE h,LPCVOID data,DWORD bytes,LPDWORD written,LPOVERLAPPED overlapped) {
 if(!writable_handles.contains(h)){SetLastError(ERROR_ACCESS_DENIED);++guard_rejected;return FALSE;}
 if(fail_write){++write_failures;if(written)*written=0;SetLastError(ERROR_DISK_FULL);return FALSE;}
 return WriteFile(h,data,bytes,written,overlapped);
}
BOOL WINAPI sqlite_end(HANDLE h) {
 if(!writable_handles.contains(h)){SetLastError(ERROR_ACCESS_DENIED);++guard_rejected;return FALSE;}
 return SetEndOfFile(h);
}
BOOL WINAPI sqlite_flush(HANDLE h) {
 if(!writable_handles.contains(h)){SetLastError(ERROR_ACCESS_DENIED);++guard_rejected;return FALSE;}
 if(fail_flush){++flush_failures;SetLastError(ERROR_WRITE_FAULT);return FALSE;}
 return FlushFileBuffers(h);
}
BOOL WINAPI sqlite_close(HANDLE h) {
 const bool writable=writable_handles.erase(h)>0;const auto closed=CloseHandle(h);
 if(fail_close&&writable){++close_failures;SetLastError(ERROR_WRITE_FAULT);return FALSE;}return closed;
}
BOOL WINAPI sqlite_delete_w(LPCWSTR path) {
 if(!path || !write_allowed(path)){SetLastError(ERROR_ACCESS_DENIED);return FALSE;}
 // SQLite removes its owned journal/WAL only after closing these handles.
 leaf_pins.clear();return DeleteFileW(path);
}
BOOL WINAPI sqlite_delete_a(LPCSTR path) {auto wide=widen(path);return sqlite_delete_w(wide.c_str());}
HANDLE WINAPI sqlite_mapping(HANDLE h,LPSECURITY_ATTRIBUTES attrs,DWORD protect,DWORD high,DWORD low,LPCWSTR name) {
 if((protect&(PAGE_READWRITE|PAGE_EXECUTE_READWRITE)) && !writable_handles.contains(h)) {SetLastError(ERROR_ACCESS_DENIED);++guard_rejected;return nullptr;}
 return CreateFileMappingW(h,attrs,protect,high,low,name);
}
void sqlite_guards() {
 const std::map<std::string,uintptr_t> replacements={
 {"CreateFileW",(uintptr_t)sqlite_open_w},{"CreateFileA",(uintptr_t)sqlite_open_a},
 {"WriteFile",(uintptr_t)sqlite_write},{"SetEndOfFile",(uintptr_t)sqlite_end},
 {"FlushFileBuffers",(uintptr_t)sqlite_flush},{"CloseHandle",(uintptr_t)sqlite_close},
 {"DeleteFileW",(uintptr_t)sqlite_delete_w},{"DeleteFileA",(uintptr_t)sqlite_delete_a},
 {"CreateFileMappingW",(uintptr_t)sqlite_mapping}};
 auto* dos=(IMAGE_DOS_HEADER*)mapped;auto* nt=(IMAGE_NT_HEADERS64*)(mapped+dos->e_lfanew);
 auto* descriptor=(IMAGE_IMPORT_DESCRIPTOR*)(mapped+nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress);
 std::set<std::string> seen;
 for(;descriptor->Name;++descriptor) {
  auto* names=(IMAGE_THUNK_DATA64*)(mapped+descriptor->OriginalFirstThunk);
  auto* addresses=(IMAGE_THUNK_DATA64*)(mapped+descriptor->FirstThunk);
  for(;names->u1.AddressOfData;++names,++addresses) {
   if(IMAGE_SNAP_BY_ORDINAL64(names->u1.Ordinal))continue;
   const char* name=((IMAGE_IMPORT_BY_NAME*)(mapped+names->u1.AddressOfData))->Name;
   if(auto found=replacements.find(name);found!=replacements.end()) {
    auto value=found->second;protect_copy((uintptr_t)&addresses->u1.Function,&value,8);seen.insert(name);
   }
  }
 }
 check(seen.contains("CreateFileW")&&seen.contains("CreateFileA")&&seen.contains("WriteFile")&&seen.contains("DeleteFileW"),"all_required_io_guards_before_native");
}
template<class T>T api(unsigned index) {return (T)(*(uintptr_t*)(mapped+0x6162700+index*8));}
using DB=void;using ST=void;
int open(const fs::path& path,DB** db,int flags) {return api<int(*)(const char*,DB**,int,const char*)>(135)(path.string().c_str(),db,flags,nullptr);}
int close(DB* db){return api<int(*)(DB*)>(16)(db);}
int sql(DB* db,const char* value) {
 char* error{};int rc=api<int(*)(DB*,const char*,void*,void*,char**)>(55)(db,value,nullptr,nullptr,&error);
 if(error)api<void(*)(void*)>(58)(error);return rc;
}
std::vector<unsigned char> snapshot(DB* db) {
 long long size{};auto* value=api<unsigned char*(*)(DB*,const char*,long long*,unsigned)>(263)(db,"main",&size,0);
 require(value&&size>0&&size<=32*1024*1024,"native_serialize_buffer");
 std::vector<unsigned char> result(value,value+size);api<void(*)(void*)>(58)(value);return result;
}
int autocommit(DB* db){return api<int(*)(DB*)>(60)(db);}
std::vector<unsigned char> readonly_image(const fs::path& path) {
 DB* other{};require(open(path,&other,1)==0 && other,"readonly_open");
 require(api<int(*)(DB*,const char*)>(181)(other,"main")==1,"readonly_mode");
 auto result=snapshot(other);require(close(other)==0,"readonly_close");return result;
}
}
namespace {
fs::path fixture_root(const wchar_t* value) {
 auto root=fs::path(value).lexically_normal();require(root.is_absolute(),"absolute_component_root");
 const auto repository=fs::path(__FILE__).parent_path().parent_path().parent_path().parent_path().parent_path().lexically_normal();
 for(const auto& protected_root:{repository,fs::path(L"D:/LanGame/server-files").lexically_normal()}) {
  auto prefix=protected_root.wstring()+L"\\";auto text=root.wstring();
  require(!equal_path(root,protected_root)&&!(text.size()>=prefix.size()&&CompareStringOrdinal(text.c_str(),static_cast<int>(prefix.size()),prefix.c_str(),static_cast<int>(prefix.size()),TRUE)==CSTR_EQUAL),"component_root_is_protected");
 }
 return root;
}
fs::path component_directory(const fs::path& root,const wchar_t* label) {
 auto parent=root/label;fs::create_directories(parent);auto pins=pin_directories(parent);
 auto leaf=parent/(std::to_wstring(GetCurrentProcessId())+L"-"+std::to_wstring(GetTickCount64()));
 require(fs::create_directory(leaf),"fresh_component_directory");return leaf.lexically_normal();
}
}
