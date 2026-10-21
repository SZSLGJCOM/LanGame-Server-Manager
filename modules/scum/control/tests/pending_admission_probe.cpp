#include "pending_admission_probe.hpp"
#include "../native_tasks.cpp"
#include <fstream>
#include <algorithm>
namespace {
std::wstring synthetic_module_path;
unsigned boundary_calls{};bool boundary_throws{};
DWORD WINAPI fixture_module_path(HMODULE,wchar_t* output,DWORD maximum) {
    if(synthetic_module_path.size()>=maximum)return 0;
    std::copy(synthetic_module_path.begin(),synthetic_module_path.end(),output);
    output[synthetic_module_path.size()]=0;return static_cast<DWORD>(synthetic_module_path.size());
}
void original_root_boundary(std::uintptr_t system,bool force) {
    ++boundary_calls;const auto root=scum_save::runtime().active;
    if(!root||scum_save::scope!=root||system!=root->system.pointer||!force)throw std::runtime_error("original_root_arguments");
    if(boundary_throws)throw std::runtime_error("component_original_exception");
    // No stage/SQL/receipt is manufactured. This empty external native body
    // therefore remains unconfirmed by the real production root hook.
}
}
// Only the OS module-path boundary is constructed for the actual shared IPC
// replay test. The component process is never named or launched as the game.
#define GetModuleFileNameW fixture_module_path
#include "../bridge.cpp"
#undef GetModuleFileNameW
namespace {
Request native_request(const ProbeRequest& input) {
    return {input.nonce,"save",input.requested_at,input.system,input.instance,input.world};
}
}
const char* start_request(const ProbeRequest& input){return native_save_async(0,native_request(input));}
const char* query_request(const ProbeRequest& input){return native_observe_async(0,native_request(input));}
scum_save::Owner parked_owner(){return active;}
void reset_request(){active.reset();active_request={};admission_phase=AdmissionPhase::none;boundary_calls=0;boundary_throws=false;}
bool install_root_boundary() {
    const auto hooks=scum_save::task_hooks();const auto root=std::find_if(hooks.begin(),hooks.end(),[](const auto& h){return h.rva==0x1b94040;});
    if(root==hooks.end()||!scum_save::install_hooks({*root},{}))return false;
    scum_save::root_original=original_root_boundary;return true;
}
unsigned root_boundary_calls(){return boundary_calls;}
void root_boundary_throws(bool value){boundary_throws=value;}
void initialize_public_boundary(const scum_save::fs::path& root) {
    synthetic_module_path=(root/L"SCUM/Binaries/Win64/SCUMServer.exe").native();
    scum_save::fs::create_directories(root/L"langame_world_save");
}
void prime_cached_request(const ProbeRequest& input) {
    auto request=native_request(input);const auto path=scum_save::fs::path(synthetic_module_path).parent_path().parent_path().parent_path().parent_path()/L"langame_world_save";
    std::ofstream stream(path/L"native-request.txt",std::ios::binary);
    stream<<"schema=1\nnonce="<<request.nonce<<"\nop=save\nrequested_at="<<request.requested_at<<"\nproxy_address="<<request.proxy<<"\ninstance_address="<<request.instance<<"\nworld_address="<<request.world<<"\n";
    stream.close();if(!stream)throw std::runtime_error("component_request_write");
    const auto key=request.nonce+"\n"+request.operation+"\n"+std::to_string(request.requested_at)+"\n"+
        std::to_string(request.proxy)+"\n"+std::to_string(request.instance)+"\n"+std::to_string(request.world);
    recent={};recent[0]={key,receipt(request,"pending"),true,GetTickCount64()};
}
void invoke_cached_request(){invoke();}
bool cached_terminal_unknown(){return !recent[0].pending&&recent[0].text.find("\nstatus=unknown\n")!=std::string::npos;}

namespace {
std::vector<Handle> admission_pins;
fs::path external_component_root(const fs::path& input) {
    require(input.is_absolute(),"absolute_component_root");const auto root=scum_save::full_path(input);
    const auto repository=fs::path(__FILE__).parent_path().parent_path().parent_path().parent_path().parent_path().lexically_normal();
    for(const auto& protected_root:{repository,fs::path(L"D:/LanGame/server-files").lexically_normal()}) {
        const auto prefix=protected_root.native()+L"\\";const auto text=root.native();
        require(!equal_path(root,protected_root)&&!(text.size()>=prefix.size()&&CompareStringOrdinal(text.c_str(),static_cast<int>(prefix.size()),prefix.c_str(),static_cast<int>(prefix.size()),TRUE)==CSTR_EQUAL),"component_root_is_protected");
    }
    return root;
}
void retain_admission_parents(const fs::path& path) {
    for(auto& pin:pin_directories(path))admission_pins.push_back(std::move(pin));
}
}
scum_save::fs::path admission_image(const scum_save::fs::path& input) {
    const auto root=external_component_root(input);const auto image=root/L"image/SCUMServer.exe";
    retain_admission_parents(image.parent_path());
    auto file=open_file(image,GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ);
    require(hash_file(file.value)==GAME_HASH,"exact_ordinary_physical_image");
    admission_pins.push_back(std::move(file));return image;
}
scum_save::fs::path admission_scratch(const scum_save::fs::path& input) {
    const auto root=external_component_root(input);retain_admission_parents(root);
    const auto leaf=root/(L"pending-admission-"+std::to_wstring(GetCurrentProcessId())+L"-"+std::to_wstring(GetTickCount64()));
    require(fs::create_directory(leaf),"fresh_component_directory");retain_admission_parents(leaf);return leaf;
}
