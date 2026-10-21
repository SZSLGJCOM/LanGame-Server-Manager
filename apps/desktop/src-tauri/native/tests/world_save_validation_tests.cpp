// Real transport invoke/validate/replay/receipt code. Only module discovery,
// game-thread API and the adapter are synthetic external environment boundaries.
// All files are physical copies/ordinary files in a fresh external fixture.
#include <windows.h>
#include <algorithm>
#include <array>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <string>
#include <string_view>

namespace {
namespace fixture_fs=std::filesystem;
fixture_fs::path fixture_root;
unsigned checks{},contract_calls{},save_calls{},observe_calls{};
bool on_game_thread=true,throw_target=true,nonstandard_exception{};
std::string target_reason="fixture_native_guard",fingerprint_hash;
std::string_view GAME_HASH,UE4SS_HASH;
constexpr wchar_t GAME_NAME[]=L"WorldSaveValidationTests.exe";
constexpr wchar_t BRIDGE_NAME[]=L"WorldSaveValidationControl.dll";
const unsigned char fingerprint[]{0x7a,0x3c,0x19,0x28};
uintptr_t SAVE_RVA{};
constexpr std::string_view SAVE_PROLOGUE="7a3c1928";
const auto core_handle=reinterpret_cast<HMODULE>(static_cast<uintptr_t>(2));
const auto bridge_handle=reinterpret_cast<HMODULE>(static_cast<uintptr_t>(3));
bool game_thread_boundary() {return on_game_thread;}
HMODULE WINAPI module_handle_boundary(LPCWSTR name) {
    if(!name)return GetModuleHandleW(nullptr);
    if(std::wstring_view(name)==L"UE4SS.dll")return core_handle;
    SetLastError(ERROR_MOD_NOT_FOUND);return nullptr;
}
DWORD WINAPI module_path_boundary(HMODULE module,LPWSTR output,DWORD size) {
    auto path=fixture_root/L"Server/Binaries/Win64";
    path/=module==core_handle ? L"ue4ss/UE4SS.dll" : module==bridge_handle ? L"ue4ss/Mods/LgsmWorldSave/native/WorldSaveValidationControl.dll" : GAME_NAME;
    const auto& text=path.native();
    if(text.size()>=size){SetLastError(ERROR_INSUFFICIENT_BUFFER);return size;}
    std::memcpy(output,text.c_str(),(text.size()+1)*sizeof(wchar_t));return static_cast<DWORD>(text.size());
}
BOOL WINAPI bridge_module_boundary(DWORD,LPCWSTR,HMODULE* output) {*output=bridge_handle;return TRUE;}
FARPROC WINAPI procedure_boundary(HMODULE module,LPCSTR name) {
    if(module==core_handle && std::string_view(name)=="?IsInGameThreadRaw@Unreal@RC@@YA_NXZ")return reinterpret_cast<FARPROC>(&game_thread_boundary);
    SetLastError(ERROR_PROC_NOT_FOUND);return nullptr;
}
void target_contract(uintptr_t,uintptr_t,uintptr_t,uintptr_t);
struct Request;
const char* native_save_async(uintptr_t,const Request&);
const char* native_observe_async(uintptr_t,const Request&);
}
#define LGSM_NATIVE_SAVE_ASYNC
#define LGSM_SAVE_EXPORT validation_fixture_export
#define GetModuleHandleW module_handle_boundary
#define GetModuleFileNameW module_path_boundary
#define GetModuleHandleExW bridge_module_boundary
#define GetProcAddress procedure_boundary
#include "../world_save_bridge.hpp"
#undef GetModuleHandleW
#undef GetModuleFileNameW
#undef GetModuleHandleExW
#undef GetProcAddress

namespace {
void target_contract(uintptr_t,uintptr_t,uintptr_t,uintptr_t) {
    ++contract_calls;if(nonstandard_exception)throw 7;
    if(throw_target)throw std::runtime_error(target_reason);
}
const char* native_save_async(uintptr_t,const Request&) {++save_calls;return "unknown";}
const char* native_observe_async(uintptr_t,const Request&) {++observe_calls;return "unknown";}
void check(bool result,const char* reason) {++checks;if(!result)throw std::runtime_error(reason);}
std::string read_plain(const fs::path& path) {
    auto file=open_file(path,GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ);
    std::array<char,1024> bytes{};DWORD count{};
    check(ReadFile(file.value,bytes.data(),static_cast<DWORD>(bytes.size()),&count,nullptr) && count<bytes.size(),"fixture_read");
    return {bytes.data(),count};
}
void publish(const Request& request) {
    auto file=open_file(fixture_root/L"langame_world_save/native-request.txt",GENERIC_WRITE,OPEN_ALWAYS,0);
    write_receipt(file.value,"schema=1\nnonce="+request.nonce+"\nop="+request.operation+"\nrequested_at="+std::to_string(request.requested_at)+"\nproxy_address="+std::to_string(request.proxy)+"\ninstance_address="+std::to_string(request.instance)+"\nworld_address="+std::to_string(request.world)+"\n");
}
Request request(std::string operation="probe") {
    static unsigned sequence{};std::array<char,33> nonce{};
    check(std::snprintf(nonce.data(),nonce.size(),"%032x",++sequence)==32,"fixture_nonce");
    return {nonce.data(),std::move(operation),now(),1,2,3};
}
void call_rejected(const Request& value) {
    publish(value);check(validation_fixture_export(nullptr)==0,"export_return_changed");
    check(read_plain(fixture_root/L"langame_world_save/native-response.txt")==receipt(value,"rejected"),"rejected_receipt_or_identity_changed");
    check(save_calls==0 && observe_calls==0,"rejected_request_entered_native_save");
}
void expect_reason(std::string_view expected) {
    const auto path=fixture_root/L"langame_world_save/native-diagnostic.txt";
    check(GetFileAttributesW(path.c_str())!=INVALID_FILE_ATTRIBUTES,"validate_rejection_reason_missing");
    const auto value=read_plain(path);
    check(value==std::string(expected)+"\n" && value.size()<=64,"unbounded_or_unsafe_diagnostic_reason");
}
}
int wmain(int argc,wchar_t** argv) {try {
    check(argc==2,"external_fixture_required");fixture_root=fs::path(argv[1]);
    check(fixture_root.is_absolute(),"absolute_fixture_required");fixture_root=fixture_root.lexically_normal();
    const auto leaf=fixture_root.filename().native();const std::wstring prefix=L"WorldSaveValidationFixture-";
    check(leaf.starts_with(prefix) && leaf.size()==prefix.size()+32 && leaf.find_first_not_of(L"0123456789abcdef",prefix.size())==std::wstring::npos,"owned_fixture_required");
    const auto source=fs::path(__FILE__).parent_path().parent_path().parent_path().parent_path().parent_path().parent_path();
    for(const auto& protected_root:{source,fs::path(L"D:/LanGame/server-files")}) {
        const auto boundary=protected_root.lexically_normal().native()+L'\\';
        check(!equal_path(fixture_root,protected_root) && !(fixture_root.native().size()>boundary.size() && CompareStringOrdinal(fixture_root.c_str(),static_cast<int>(boundary.size()),boundary.c_str(),static_cast<int>(boundary.size()),TRUE)==CSTR_EQUAL),"protected_fixture_root");
    }
    check(GetFileAttributesW(fixture_root.c_str())!=INVALID_FILE_ATTRIBUTES,"fixture_must_be_precreated");
    auto root_pins=pin_directories(fixture_root);
    const auto win64=fixture_root/L"Server/Binaries/Win64";
    fs::create_directories(win64/L"ue4ss/Mods/LgsmWorldSave/native");
    fs::create_directories(fixture_root/L"langame_world_save");
    std::array<wchar_t,32768> self{};const auto self_length=GetModuleFileNameW(nullptr,self.data(),static_cast<DWORD>(self.size()));
    check(self_length>0 && self_length<self.size(),"fixture_self_path");
    for(const auto& path:{win64/GAME_NAME,win64/L"ue4ss/UE4SS.dll",win64/L"ue4ss/Mods/LgsmWorldSave/native"/BRIDGE_NAME})
        check(CopyFileW(self.data(),path.c_str(),TRUE)!=FALSE,"fixture_physical_copy");
    {auto file=open_file(win64/GAME_NAME,GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ);fingerprint_hash=hash_file(file.value);}
    GAME_HASH=UE4SS_HASH=fingerprint_hash;
    SAVE_RVA=reinterpret_cast<uintptr_t>(fingerprint)-reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));

    auto value=request();call_rejected(value);expect_reason("fixture_native_guard");
    check(contract_calls==1,"validate_did_not_execute_real_adapter_boundary_once");
    const auto first_response=read_plain(fixture_root/L"langame_world_save/native-response.txt");
    publish(value);check(validation_fixture_export(nullptr)==0,"replay_export");
    check(contract_calls==1 && read_plain(fixture_root/L"langame_world_save/native-response.txt")==first_response && save_calls==0,"rejected_nonce_replayed_side_effect");
    for(const auto& reason:{std::string(),std::string("C:/private/player/secret"),std::string("MixedCase"),std::string(64,'a'),std::string("native_\xc3\xa9")}) {
        target_reason=reason;call_rejected(request());expect_reason("native_exception");
    }
    target_reason=std::string(63,'a');call_rejected(request());expect_reason(target_reason);
    on_game_thread=false;const auto before_thread=contract_calls;call_rejected(request());expect_reason("wrong_thread");
    check(contract_calls==before_thread,"wrong_thread_reached_adapter");on_game_thread=true;
    value=request();value.requested_at-=4;const auto before_expired=contract_calls;call_rejected(value);expect_reason("request_expired");
    check(contract_calls==before_expired,"expired_request_reached_adapter");
    {
        const auto diagnostic=fixture_root/L"langame_world_save/native-diagnostic.txt";
        const auto previous=read_plain(diagnostic);
        auto pinned=open_file(diagnostic,GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ);
        target_reason="fixture_locked_diagnostic";call_rejected(request("save"));
        check(read_plain(diagnostic)==previous && save_calls==0,"optional_diagnostic_failure_changed_receipt_or_entered_save");
    }
    target_reason="fixture_last_reason";call_rejected(request());expect_reason(target_reason);
    const auto previous=read_plain(fixture_root/L"langame_world_save/native-diagnostic.txt");
    nonstandard_exception=true;call_rejected(request());nonstandard_exception=false;
    check(read_plain(fixture_root/L"langame_world_save/native-diagnostic.txt")==previous,"nonstandard_catch_semantics_changed");
    throw_target=false;value=request();publish(value);check(validation_fixture_export(nullptr)==0,"ready_probe_export");
    check(read_plain(fixture_root/L"langame_world_save/native-response.txt")==receipt(value,"ready") && save_calls==0 && observe_calls==0,"valid_probe_state_changed");
    const auto before_conflict=contract_calls;value.operation="save";publish(value);check(validation_fixture_export(nullptr)==0,"conflict_export");
    expect_reason("nonce_conflict");check(contract_calls==before_conflict && save_calls==0,"outer_exception_changed_nonce_replay_guard");
    std::cout<<"world_save_validation_checks="<<checks<<" real_transport_validate_receipt_replay=true optional_diagnostic_failure_preserves_rejected=true physical_self_copy_only=true game_images_loaded=0 game_launches=0 save_calls=0\n";return 0;
}catch(const std::exception& error){std::cerr<<error.what()<<'\n';return 1;}}
