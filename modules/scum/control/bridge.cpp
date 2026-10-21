// Fixed full AutoSave(true): native global/transforms/entities causal closure.
#include "native_state.hpp"
#include "early_mod.hpp"
#include <windows.h>
#include <cstdint>
#include <string_view>
namespace {
constexpr std::string_view GAME_HASH = "4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad";
constexpr std::string_view UE4SS_HASH = "91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958";
constexpr wchar_t GAME_NAME[] = L"SCUMServer.exe";
constexpr wchar_t BRIDGE_NAME[] = L"LgsmScumSaveControl.dll";
// Unhooked fixed GetWorld accessor is the transport fingerprint. The real
// AutoSave entry below is independently pinned and observed by the profile.
constexpr uintptr_t SAVE_RVA = 0x15501c0;
constexpr std::string_view SAVE_PROLOGUE = "488b49204885c9740a488b0148ffa06001000033c0c3";
void target_contract(uintptr_t, uintptr_t, uintptr_t, uintptr_t);
struct Request;
const char* native_save_async(uintptr_t,const Request&);
const char* native_observe_async(uintptr_t,const Request&);
void native_observation_aborted(const Request&) noexcept;
}
#define LGSM_SAVE_EXPORT luaopen_lgsm_scum_save
#define LGSM_NATIVE_SAVE_ASYNC
#define LGSM_NATIVE_SAVE_OBSERVATION_ABORT
#include "../../../apps/desktop/src-tauri/native/world_save_bridge.hpp"
namespace {
void alive(uintptr_t object, size_t size) {
    require(readable(object, size) && !(read<uint32_t>(object + 8) & 0x40018010), "object_unavailable");
}
void entity_contract(uintptr_t image, uintptr_t system, uintptr_t instance, uintptr_t world) {
    alive(system, 0x79c);
    auto cls = read<uintptr_t>(image + 0x7447070);
    require(cls && read<uintptr_t>(system + 0x10) == cls
        && read<uintptr_t>(system) == image + 0x5e40c60, "entity_system_class");
    alive(instance, 0x28);
    require(read<uintptr_t>(system + 0x20) == instance, "entity_system_instance");
    alive(world, 0x188);
    // These offsets are the fixed image's generated UWorld property metadata.
    require(read<uintptr_t>(world + 0x180) == instance, "world_instance");
    auto game_mode = read<uintptr_t>(world + 0x118);
    auto game_state = read<uintptr_t>(world + 0x120);
    alive(game_mode, 0xf1); alive(game_state, 0x5d);
    require(read<uint8_t>(game_mode + 0xf0) == 3, "world_authority");
    // AActor::EndPlay (3ec3aa0) accepts 0x80 in this two-bit native state,
    // then immediately clears it. This is not GameState's replication flag.
    require((read<uint8_t>(game_mode + 0x5c) & 0xc0) == 0x80
        && (read<uint8_t>(game_state + 0x5c) & 0xc0) == 0x80, "world_not_begun_play");
    require(read<uint8_t>(system + 0x30) == 1
        && read<uint8_t>(system + 0x798) == 0
        && read<uint8_t>(system + 0x79a) == 0
        && read<uint8_t>(system + 0x79b) == 0, "entity_system_not_ready");
}
void target_contract(uintptr_t image, uintptr_t system, uintptr_t instance, uintptr_t world) {
    const auto& observer=scum_save::runtime();
    require(observer.armed&&!observer.broken&&scum_save::image_base()==image,observer.issue);
    require(scum_save::code_matches(),"native_profile_changed");
    entity_contract(image, system, instance, world);
    prologue(image, 0x15501c0, "488b49204885c9740a488b0148ffa06001000033c0c3");
    require(read<uintptr_t>(image + 0x5e40c60 + 0x160) == image + 0x15501c0, "world_accessor");
    using GetWorld = void* (*)(void*);
    require(reinterpret_cast<uintptr_t>(reinterpret_cast<GetWorld>(image + 0x15501c0)(
        reinterpret_cast<void*>(system))) == world, "entity_system_world");
}
scum_save::Owner active;
Request active_request;
enum class AdmissionPhase { none,waiting,running,terminal };
AdmissionPhase admission_phase{AdmissionPhase::none};
bool same_request(const Request& request) noexcept {
    return active_request.nonce==request.nonce&&active_request.operation==request.operation&&
        active_request.requested_at==request.requested_at&&active_request.proxy==request.proxy&&
        active_request.instance==request.instance&&active_request.world==request.world;
}
void native_observation_aborted(const Request& request) noexcept {
    if(!active||!same_request(request))return;
    admission_phase=AdmissionPhase::terminal;
    // An entered root and all of its IO/pins remain retained by runtime.active.
    scum_save::unknown(active,"native_observation_lost");
}
std::string debt_wire_fields(const scum_save::ForeignSqlDiagnostic& value) {
    using namespace scum_save::DebtWire;
    const auto& debt=value.debt;
    const std::array<std::uint32_t,FieldCount> values={version,
        static_cast<std::uint32_t>(debt.flags)|(value.stack_attempted?StackAttempted:0u),
        debt.owned_scopes,debt.owned_io,debt.task_inflight,debt.sql_inflight,debt.io_inflight,debt.self_sql_inflight,
        value.origin_rva,value.origin_bits,value.stack_caller_rva,value.stack_upper_rva,value.stack_state};
    std::array<unsigned char,record_bytes> bytes{};unsigned at{};
    for(unsigned index=0;index<FieldCount;++index)
        for(unsigned byte=0;byte<fields[index].bytes;++byte)bytes[at++]=static_cast<unsigned char>(values[index]>>(8*byte));
    constexpr char alphabet[]="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    std::string result="sd=";result.reserve(63);
    for(unsigned index=0;index<record_bytes;index+=3) {
        const auto remaining=record_bytes-index;
        const auto group=(static_cast<unsigned>(bytes[index])<<16)|
            (remaining>1?static_cast<unsigned>(bytes[index+1])<<8:0u)|(remaining>2?bytes[index+2]:0u);
        result+=alphabet[(group>>18)&63u];result+=alphabet[(group>>12)&63u];
        if(remaining>1)result+=alphabet[(group>>6)&63u];if(remaining>2)result+=alphabet[group&63u];
    }
    result+='\n';return result;
}
std::string sql_origin_fields(const scum_save::ForeignSqlDiagnostic& value) {
    if(value.debt.captured)return debt_wire_fields(value);
    if(!value.origin_bits)return {};
    auto result="sql_origin_rva="+std::to_string(value.origin_rva)+"\nsql_origin_bits="+std::to_string(value.origin_bits)+"\n";
    if(value.stack_attempted) {
        constexpr char digits[]="0123456789abcdef";
        const auto hex=[&](std::uint32_t rva) {
            char text[8]{};unsigned size{};
            do{text[size++]=digits[rva&15u];rva>>=4;}while(rva);
            std::string output;while(size)output+=text[--size];return output;
        };
        result+="bt="+hex(value.stack_caller_rva)+","+hex(value.stack_upper_rva)+","+std::to_string(value.stack_state)+"\n";
    }
    return result;
}
std::string diagnostic_message(const scum_save::Epoch& owner) {
        auto message=std::string(owner.completion.issue())+"\n";
        const auto& first=owner.first_foreign_sql;
        if(first.api_rva) {
            message+="first_foreign_sql_api_rva="+std::to_string(first.api_rva)+"\n";
            message+="first_foreign_sql_caller_rva="+std::to_string(first.caller_rva)+"\n";
            message+="first_foreign_sql_context="+std::to_string(first.context)+"\n";
            message+=std::string("statement_readonly_complete=")+(first.statement_readonly_complete?"1\n":"0\n");
            message+=std::string("statement_readonly=")+(first.statement_readonly?"1\n":"0\n");
            message+=std::string("file_relation_complete=")+(first.file_relation_complete?"1\n":"0\n");
            message+=std::string("shares_owned_file=")+(first.shares_owned_file?"1\n":"0\n");
        }
        const auto& queue=owner.first_foreign_queue;
        if(queue.captured) {
            message+="first_foreign_queue_caller_rva="+std::to_string(queue.caller_rva)+"\n";
            message+="first_foreign_queue_context="+std::to_string(queue.context)+"\n";
            message+="first_foreign_queue_other_task_inflight="+std::to_string(queue.other_task_inflight)+"\n";
            message+="queue_parent_rva="+std::to_string(queue.parent_rva)+"\n";
            message+="queue_parent_context="+std::to_string(queue.parent_context)+"\n";
        }
        message+=sql_origin_fields(first);
        return message;
}
void diagnostic() noexcept {
    try {
        if(!active)return;const auto& state=initialization();if(!state.image)return;
        auto pins=pin_directories(state.image->directory);
        auto file=open_file(state.image->directory/L"native-diagnostic.txt",GENERIC_WRITE,OPEN_ALWAYS,0);
        std::lock_guard lock(scum_save::runtime().mutex);
        write_receipt(file.value,diagnostic_message(*active));
    }catch(...){OutputDebugStringA("[LgsmWorldSave] SCUM diagnostic unavailable.\n");}
}
const char* admission_issue(const char* issue) noexcept {
    if(issue)for(const char* known:{"native_admission_timeout","native_admission_thread","native_context_changed",
        "native_worker_identity","native_worker_unobserved","native_worker_state","native_worker_stopping",
        "native_queue_shape","native_worker_pool_shape","native_previous_save_unsettled","native_profile_changed"})
        if(std::string_view(issue)==known)return known;
    return "native_admission_rejected";
}
const char* advance_admission(const Request& request) {
    // Internal Query advances this already accepted request; it is not read-only.
    if(admission_phase!=AdmissionPhase::waiting)return "unknown";
    // An exception/SEH must never leave a resumable entry attempt behind.
    admission_phase=AdmissionPhase::terminal;
    try {
        if(!scum_save::try_begin(active,request.proxy,request.instance,request.world)) {
            admission_phase=AdmissionPhase::waiting;return "pending";
        }
        admission_phase=AdmissionPhase::running;
        scum_save::dispatch(active);return "pending";
    }catch(const std::exception& error){scum_save::unknown(active,admission_issue(error.what()));}
    catch(...){scum_save::unknown(active,"native_admission_exception");}
    admission_phase=AdmissionPhase::terminal;diagnostic();return "unknown";
}
const char* native_save_async(uintptr_t,const Request& request) {
    if(admission_phase==AdmissionPhase::waiting&&active&&GetTickCount64()>active->deadline) {
        admission_phase=AdmissionPhase::terminal;scum_save::unknown(active,"native_admission_timeout");
    }
    if(admission_phase==AdmissionPhase::waiting||admission_phase==AdmissionPhase::running)
        throw std::runtime_error("native_previous_save_unsettled");
    admission_phase=AdmissionPhase::terminal;
    auto candidate=scum_save::prepare_begin(request.proxy,request.instance,request.world,GetTickCount64()+30000);
    active=std::move(candidate);active_request=request;
    admission_phase=AdmissionPhase::waiting;return advance_admission(request);
}
const char* native_observe_async(uintptr_t,const Request& request) {
    if(!active||!same_request(request))return "unknown";
    if(admission_phase==AdmissionPhase::waiting)return advance_admission(request);
    if(admission_phase!=AdmissionPhase::running)return "unknown";
    const auto status=scum_save::observe(active,request.proxy,request.instance,request.world);
    if(std::string_view(status)!="pending")admission_phase=AdmissionPhase::terminal;
    if(std::string_view(status)=="failed"||std::string_view(status)=="unknown")diagnostic();return status;
}
}
extern "C" __declspec(dllexport) int luaopen_lgsm_scum_arm(void*) noexcept {
    try {
        const auto& state=initialization();
        if(!state.image||!scum_save::initialize())std::fprintf(stderr,"[LgsmWorldSave] SCUM observer unavailable (%s).\n",scum_save::runtime().issue);
    }catch(...){OutputDebugStringA("[LgsmWorldSave] SCUM startup observer unavailable.\n");}return 0;
}
namespace {void* cpp_object{};uintptr_t cpp_core{};}
extern "C" __declspec(dllexport) void* lgsm_scum_cpp_start() noexcept {
    try {
        const auto& state=initialization();if(!state.image)return nullptr;
        cpp_core=reinterpret_cast<uintptr_t>(GetModuleHandleW(L"UE4SS.dll"));
        if(!cpp_object)cpp_object=scum_save::early_mod::create(cpp_core);
        luaopen_lgsm_scum_arm(nullptr);return cpp_object;
    }catch(...){OutputDebugStringA("[LgsmWorldSave] SCUM C++ mod ABI unavailable.\n");return nullptr;}
}
extern "C" __declspec(dllexport) void lgsm_scum_cpp_stop(void* value) noexcept {
    try{if(value&&value==cpp_object){scum_save::early_mod::destroy(cpp_core,value);cpp_object=nullptr;}}
    catch(...){OutputDebugStringA("[LgsmWorldSave] SCUM C++ mod cleanup unavailable.\n");}
}
