#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <array>
#include <filesystem>
#include <stdexcept>
#include <vector>

namespace {
namespace fs=std::filesystem;
struct Handle {
    HANDLE value{INVALID_HANDLE_VALUE};
    explicit Handle(HANDLE input):value(input){}
    Handle(const Handle&)=delete;
    Handle(Handle&& other) noexcept:value(other.value){other.value=INVALID_HANDLE_VALUE;}
    ~Handle(){if(value!=INVALID_HANDLE_VALUE)CloseHandle(value);}
};
void require(bool value){if(!value)throw std::runtime_error("native_early_bootstrap");}
fs::path path(HMODULE module) {
    std::array<wchar_t,32768> buffer{};const auto size=GetModuleFileNameW(module,buffer.data(),static_cast<DWORD>(buffer.size()));
    require(size && size<buffer.size());return fs::path(std::wstring(buffer.data(),size));
}
std::vector<Handle> pins;
HMODULE native{};
void* object{};
void*(*start)(){};
void(*stop)(void*){};
void load() {
    if(native)return;
    HMODULE self{};
    require(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_PIN,reinterpret_cast<LPCWSTR>(&load),&self)!=FALSE);
    const auto own=path(self).lexically_normal(),mod=own.parent_path().parent_path();
    require(own.filename()==L"main.dll" && own.parent_path().filename()==L"dlls" && mod.filename()==L"LgsmWorldSave" &&
        mod.parent_path().filename()==L"Mods" && mod.parent_path().parent_path().filename()==L"ue4ss" &&
        path(nullptr).filename()==L"SCUMServer.exe");
    std::vector<fs::path> directories;
    for(auto directory=mod/L"native";!directory.empty();) {
        directories.push_back(directory);const auto parent=directory.parent_path();if(parent==directory)break;directory=parent;
        require(directories.size()<=64);
    }
    for(auto at=directories.rbegin();at!=directories.rend();++at) {
        Handle file(CreateFileW(at->c_str(),FILE_LIST_DIRECTORY|FILE_READ_ATTRIBUTES,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,
            OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
        BY_HANDLE_FILE_INFORMATION info{};
        require(file.value!=INVALID_HANDLE_VALUE && GetFileInformationByHandle(file.value,&info) &&
            !(info.dwFileAttributes&FILE_ATTRIBUTE_REPARSE_POINT) && (info.dwFileAttributes&FILE_ATTRIBUTE_DIRECTORY));
        pins.push_back(std::move(file));
    }
    const auto target=mod/L"native/LgsmScumSaveControl.dll";
    Handle file(CreateFileW(target.c_str(),GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
    BY_HANDLE_FILE_INFORMATION info{};
    require(file.value!=INVALID_HANDLE_VALUE && GetFileInformationByHandle(file.value,&info) && info.nNumberOfLinks==1 &&
        !(info.dwFileAttributes&(FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_DIRECTORY)));
    native=LoadLibraryExW(target.c_str(),nullptr,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);require(native!=nullptr);
    start=reinterpret_cast<void*(*)()>(GetProcAddress(native,"lgsm_scum_cpp_start"));
    stop=reinterpret_cast<void(*)(void*)>(GetProcAddress(native,"lgsm_scum_cpp_stop"));require(start && stop);
}
}
extern "C" __declspec(dllexport) void* start_mod() noexcept {
    try { load();if(!object)object=start();return object; }
    catch(...) { OutputDebugStringA("[LgsmWorldSave] SCUM early bootstrap unavailable.\n");return nullptr; }
}
extern "C" __declspec(dllexport) void uninstall_mod(void* value) noexcept {
    if(value && value==object && stop){stop(value);object=nullptr;}
    // The native observer, bootstrap and outstanding owners stay process-pinned.
}
