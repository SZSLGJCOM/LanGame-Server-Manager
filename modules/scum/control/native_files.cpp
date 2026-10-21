#include "native_state.hpp"
#include <intrin.h>
#ifdef LGSM_SCUM_SAVE_TESTS
#include <cstdio>
#endif

namespace scum_save {
IatHook task_enter_import();
namespace {
HANDLE(WINAPI*open_w_original)(LPCWSTR,DWORD,DWORD,LPSECURITY_ATTRIBUTES,DWORD,DWORD,HANDLE){};
HANDLE(WINAPI*open_a_original)(LPCSTR,DWORD,DWORD,LPSECURITY_ATTRIBUTES,DWORD,DWORD,HANDLE){};
BOOL(WINAPI*write_original)(HANDLE,LPCVOID,DWORD,LPDWORD,LPOVERLAPPED){};
BOOL(WINAPI*end_original)(HANDLE){};
BOOL(WINAPI*flush_original)(HANDLE){};
BOOL(WINAPI*close_original)(HANDLE){};
BOOL(WINAPI*delete_w_original)(LPCWSTR){};
BOOL(WINAPI*delete_a_original)(LPCSTR){};
HANDLE(WINAPI*mapping_w_original)(HANDLE,LPSECURITY_ATTRIBUTES,DWORD,DWORD,DWORD,LPCWSTR){};
HANDLE(WINAPI*mapping_a_original)(HANDLE,LPSECURITY_ATTRIBUTES,DWORD,DWORD,DWORD,LPCSTR){};
BOOL(WINAPI*view_flush_original)(LPCVOID,SIZE_T){};
struct Flight {
    Flight(){std::lock_guard lock(runtime().mutex);++runtime().io_inflight;}
    ~Flight(){std::lock_guard lock(runtime().mutex);--runtime().io_inflight;}
};
fs::path narrow(LPCSTR input) {
    if(!input)throw std::runtime_error("native_sql_file_path");
    const auto size=MultiByteToWideChar(CP_ACP,MB_ERR_INVALID_CHARS,input,-1,nullptr,0);
    if(size<=1||size>32768)throw std::runtime_error("native_sql_file_path");
    std::wstring output(static_cast<std::size_t>(size),L'\0');
    if(MultiByteToWideChar(CP_ACP,MB_ERR_INVALID_CHARS,input,-1,output.data(),size)!=size)throw std::runtime_error("native_sql_file_path");
    output.pop_back();return fs::path(output);
}
Owner current_owner() {
    std::lock_guard lock(runtime().mutex);const auto owner=runtime().active;
    if(!owner||owner->confirmed)return {};
    if(const auto receipt=admitted_scope())return receipt;
    unknown(owner,"foreign_native_sql_io");return owner;
}
Owner file_owner(HANDLE handle,bool mutation) {
    std::lock_guard lock(runtime().mutex);const auto found=runtime().files.find(handle);
    const auto root=runtime().active;
    const auto receipt=admitted_scope();const auto owner=receipt?receipt:root;
    if(found==runtime().files.end()) {
        if(mutation&&owner&&!owner->confirmed)unknown(owner,"native_sql_handle_unobserved");return {};
    }
    if(owner&&!owner->confirmed&&mutation) {
        if(!receipt)unknown(owner,"foreign_native_sql_io");
        if(receipt&&(receipt->item_scope||receipt->virtualized_scope)&&(!connection_scope||found->second.connection!=connection_scope))unknown(owner,"native_item_file_connection");
        if(!found->second.writable)unknown(owner,"native_sql_handle_readonly");return owner;
    }
    if(owner&&!owner->confirmed) {
        if(!receipt&&found->second.writable)unknown(owner,"foreign_native_sql_close");
        return owner;
    }
    return found->second.owner&&!found->second.owner->confirmed?found->second.owner:Owner{};
}
struct OpenGuard {
    bool observed{},writable{};fs::path path;Owner owner;
    std::vector<Handle> parents;SqlitePin leaf{INVALID_HANDLE_VALUE};
};
OpenGuard open_before(const fs::path& path,DWORD access,DWORD create,std::uintptr_t caller) noexcept {
    OpenGuard result;
    if(!sqlite_caller(caller))return result;result.observed=true;
    result.writable=(access&(GENERIC_WRITE|DELETE|FILE_WRITE_DATA|FILE_APPEND_DATA))||create!=OPEN_EXISTING;
    try {
        result.owner=current_owner();result.path=full_path(path);result.parents=pin_parent(result.path);
        // Observe the leaf before the native truncating open and pin real read
        // access without delete sharing. No claim that an external hard-link
        // creation race is prevented: post-open link/identity checks also gate
        // the receipt, and original game calls retain their own semantics.
        const auto attributes=GetFileAttributesW(result.path.c_str());
        if(attributes!=INVALID_FILE_ATTRIBUTES)result.leaf=sqlite_pin_file(result.path);
        else if(GetLastError()!=ERROR_FILE_NOT_FOUND)throw std::runtime_error("native_sql_file_metadata");
    }catch(...){unknown(result.owner,"native_sql_file_scope");}
    return result;
}
void open_after(OpenGuard& guard,HANDLE handle,DWORD error) noexcept {
    if(!guard.observed)return;
    try {
        std::lock_guard lock(runtime().mutex);
        if(handle==INVALID_HANDLE_VALUE) {
            // SQLite legitimately probes optional sidecars. Failed opens are
            // retained by SQL/VFS terminal results; absence alone is not IO
            // failure. No file from a failed probe can lend a completion.
            return;
        }
        BY_HANDLE_FILE_INFORMATION info{};
        if(guard.path.empty()||!GetFileInformationByHandle(handle,&info)||info.nNumberOfLinks!=1||
            (info.dwFileAttributes&(FILE_ATTRIBUTE_DIRECTORY|FILE_ATTRIBUTE_REPARSE_POINT))||
            (guard.leaf.value!=INVALID_HANDLE_VALUE&&!sqlite_same_file(guard.leaf.value,handle)))
            unknown(guard.owner,"native_sql_file_changed");
        if(runtime().files.contains(handle))unknown(guard.owner,"native_sql_handle_reused");
        runtime().files[handle]={opening_connection?opening_connection:connection_scope,guard.path,guard.writable,guard.owner};
    }catch(...){unknown(guard.owner,"native_sql_file_tracking");}
    SetLastError(error);
}
HANDLE WINAPI open_w(LPCWSTR path,DWORD access,DWORD sharing,LPSECURITY_ATTRIBUTES attributes,DWORD create,DWORD flags,HANDLE pattern) {
    const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());Flight flight;
    auto guard=open_before(path?fs::path(path):fs::path{},access,create,caller);
    if(guard.observed&&(flags&FILE_FLAG_OVERLAPPED))unknown(guard.owner,"native_sql_async_file");
    const auto result=open_w_original(path,access,sharing,attributes,create,flags,pattern);const auto error=GetLastError();
    open_after(guard,result,error);SetLastError(error);return result;
}
HANDLE WINAPI open_a(LPCSTR path,DWORD access,DWORD sharing,LPSECURITY_ATTRIBUTES attributes,DWORD create,DWORD flags,HANDLE pattern) {
    const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());Flight flight;OpenGuard guard;
    try{guard=open_before(narrow(path),access,create,caller);}catch(...){unknown(scope,"native_sql_file_path");}
    if(guard.observed&&(flags&FILE_FLAG_OVERLAPPED))unknown(guard.owner,"native_sql_async_file");
    const auto result=open_a_original(path,access,sharing,attributes,create,flags,pattern);const auto error=GetLastError();
    open_after(guard,result,error);SetLastError(error);return result;
}
BOOL WINAPI write(HANDLE handle,LPCVOID bytes,DWORD count,LPDWORD written,LPOVERLAPPED overlapped) {
    Flight flight;const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());
    const auto owner=sqlite_caller(caller)?file_owner(handle,true):Owner{};
    const auto result=write_original(handle,bytes,count,written,overlapped);const auto error=GetLastError();
    // Native SQLite supplies OVERLAPPED solely for the offset on its ordinary
    // synchronous handle. TRUE and the full byte count are its terminal write;
    // FILE_FLAG_OVERLAPPED opens are separately rejected above.
    if(owner&&(!result||!written||*written!=count)) {
#ifdef LGSM_SCUM_SAVE_TESTS
        std::fprintf(stderr,"SCUM component WriteFile result=%d requested=%lu written=%lu overlapped=%d error=%lu\n",result,count,written?*written:0,overlapped!=nullptr,error);
#endif
        file_failure(owner,"native_os_write_failed");
    }SetLastError(error);return result;
}
BOOL WINAPI end(HANDLE handle) {
    Flight flight;const auto owner=sqlite_caller(reinterpret_cast<std::uintptr_t>(_ReturnAddress()))?file_owner(handle,true):Owner{};
    const auto result=end_original(handle);const auto error=GetLastError();if(owner&&!result)file_failure(owner,"native_os_truncate_failed");SetLastError(error);return result;
}
BOOL WINAPI flush(HANDLE handle) {
    Flight flight;const auto owner=sqlite_caller(reinterpret_cast<std::uintptr_t>(_ReturnAddress()))?file_owner(handle,true):Owner{};
    const auto result=flush_original(handle);const auto error=GetLastError();if(owner&&!result)file_failure(owner,"native_os_flush_failed");SetLastError(error);return result;
}
BOOL WINAPI close(HANDLE handle) {
    Flight flight;Owner owner;
    {std::lock_guard lock(runtime().mutex);const auto found=runtime().files.find(handle);
        if(found!=runtime().files.end())owner=file_owner(handle,false);}
    const auto result=close_original(handle);const auto error=GetLastError();
    {std::lock_guard lock(runtime().mutex);if(result)runtime().files.erase(handle);else if(owner)file_failure(owner,"native_os_close_failed");}
    SetLastError(error);return result;
}
void delete_result(const fs::path& path,BOOL result,DWORD error,std::uintptr_t caller) noexcept {
    if(!sqlite_caller(caller))return;
    try{const auto owner=current_owner();if(!owner)return;
        if(!within_saved(full_path(path)))unknown(owner,"native_sql_delete_scope");
        if(!result&&error!=ERROR_FILE_NOT_FOUND)file_failure(owner,"native_os_delete_failed");
    }catch(...){unknown(scope,"native_sql_delete_scope");}
}
BOOL WINAPI remove_w(LPCWSTR path) {
    Flight flight;const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());
    const auto result=delete_w_original(path);const auto error=GetLastError();delete_result(path?fs::path(path):fs::path{},result,error,caller);SetLastError(error);return result;
}
BOOL WINAPI remove_a(LPCSTR path) {
    Flight flight;const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());
    const auto result=delete_a_original(path);const auto error=GetLastError();
    try{delete_result(narrow(path),result,error,caller);}catch(...){unknown(scope,"native_sql_delete_scope");}SetLastError(error);return result;
}
HANDLE WINAPI mapping_w(HANDLE file,LPSECURITY_ATTRIBUTES attributes,DWORD protect,DWORD high,DWORD low,LPCWSTR name) {
    Flight flight;const auto owner=sqlite_caller(reinterpret_cast<std::uintptr_t>(_ReturnAddress()))?file_owner(file,true):Owner{};
    const auto result=mapping_w_original(file,attributes,protect,high,low,name);const auto error=GetLastError();
    if(owner&&!result)file_failure(owner,"native_os_mapping_failed");SetLastError(error);return result;
}
HANDLE WINAPI mapping_a(HANDLE file,LPSECURITY_ATTRIBUTES attributes,DWORD protect,DWORD high,DWORD low,LPCSTR name) {
    Flight flight;const auto owner=sqlite_caller(reinterpret_cast<std::uintptr_t>(_ReturnAddress()))?file_owner(file,true):Owner{};
    const auto result=mapping_a_original(file,attributes,protect,high,low,name);const auto error=GetLastError();
    if(owner&&!result)file_failure(owner,"native_os_mapping_failed");SetLastError(error);return result;
}
BOOL WINAPI view_flush(LPCVOID address,SIZE_T bytes) {
    Flight flight;const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());
    const auto owner=sqlite_caller(caller)?current_owner():Owner{};
    const auto result=view_flush_original(address,bytes);const auto error=GetLastError();
    if(owner&&!result)file_failure(owner,"native_os_view_flush_failed");SetLastError(error);return result;
}
}
void file_failure(const Owner& owner,const char* reason) noexcept {
    if(!owner)return;std::lock_guard lock(runtime().mutex);owner->io_failed=true;failed(owner,reason);
}
std::vector<IatHook> file_imports() {
    return {task_enter_import(),
        {0x513c850,"CreateFileW",reinterpret_cast<void*>(open_w),reinterpret_cast<void**>(&open_w_original)},
        {0x513ca18,"CreateFileA",reinterpret_cast<void*>(open_a),reinterpret_cast<void**>(&open_a_original)},
        {0x513c758,"WriteFile",reinterpret_cast<void*>(write),reinterpret_cast<void**>(&write_original)},
        {0x513c868,"SetEndOfFile",reinterpret_cast<void*>(end),reinterpret_cast<void**>(&end_original)},
        {0x513c968,"FlushFileBuffers",reinterpret_cast<void*>(flush),reinterpret_cast<void**>(&flush_original)},
        {0x513c878,"CloseHandle",reinterpret_cast<void*>(close),reinterpret_cast<void**>(&close_original)},
        {0x513c9f0,"DeleteFileW",reinterpret_cast<void*>(remove_w),reinterpret_cast<void**>(&delete_w_original)},
        {0x513c9f8,"DeleteFileA",reinterpret_cast<void*>(remove_a),reinterpret_cast<void**>(&delete_a_original)},
        {0x513c888,"CreateFileMappingW",reinterpret_cast<void*>(mapping_w),reinterpret_cast<void**>(&mapping_w_original)},
        {0x513c9c0,"CreateFileMappingA",reinterpret_cast<void*>(mapping_a),reinterpret_cast<void**>(&mapping_a_original)},
        {0x513c898,"FlushViewOfFile",reinterpret_cast<void*>(view_flush),reinterpret_cast<void**>(&view_flush_original)}};
}
}
