#define LGSM_SCUM_SAVE_TESTS
#include "pending_admission_probe.hpp"
#include "../native_profile.cpp"
#include <iostream>
#include <thread>
#include <ctime>

namespace {
unsigned admission_checks{};
alignas(16) unsigned char system_data[0x800]{},instance_data[0x500]{},world_data[0x600]{},worker_object[0x40]{},native_worker[0x130]{};
alignas(16) unsigned char root_type[0x40]{},other_type[0x40]{},object_items[4*24]{};
std::uintptr_t object_chunks[1]{};
std::uintptr_t executable_image{};
std::int32_t queue_records[20]{};
void check(bool value,const char* issue){++admission_checks;if(!value)throw std::runtime_error(issue);}
template<class T>void put(std::uintptr_t at,T value){DWORD previous{};if(!VirtualProtect(reinterpret_cast<void*>(at),sizeof value,PAGE_READWRITE,&previous))throw std::runtime_error("component_protect");std::memcpy(reinterpret_cast<void*>(at),&value,sizeof value);DWORD ignored{};VirtualProtect(reinterpret_cast<void*>(at),sizeof value,previous,&ignored);}
template<class F>void rejects(F&& call,std::string_view expected) {
    bool rejected{};try{call();}catch(const std::exception& error){rejected=error.what()==expected;}check(rejected,"precise_terminal_reason");
}
ProbeRequest request() {return {"0123456789abcdef0123456789abcdef",static_cast<std::uint64_t>(std::time(nullptr)),reinterpret_cast<std::uintptr_t>(system_data),reinterpret_cast<std::uintptr_t>(instance_data),reinterpret_cast<std::uintptr_t>(world_data)};}
void empty_queues() {
    for(const unsigned at:{0x90U,0xa0U,0xb0U,0xc0U}){put(reinterpret_cast<std::uintptr_t>(native_worker)+at,std::uintptr_t{});put(reinterpret_cast<std::uintptr_t>(native_worker)+at+8,0);put(reinterpret_cast<std::uintptr_t>(native_worker)+at+12,0);}
    put(reinterpret_cast<std::uintptr_t>(native_worker)+0xd8,0);put(reinterpret_cast<std::uintptr_t>(native_worker)+0x104,0);native_worker[0x124]=0;
}
void busy_queue(unsigned at) {put(reinterpret_cast<std::uintptr_t>(native_worker)+at,reinterpret_cast<std::uintptr_t>(queue_records));put(reinterpret_cast<std::uintptr_t>(native_worker)+at+8,1);put(reinterpret_cast<std::uintptr_t>(native_worker)+at+12,1);}
void reset() {
    auto& state=scum_save::runtime();state.active.reset();state.task_inflight=state.sql_inflight=state.io_inflight=0;state.armed=true;state.broken=false;state.issue="none";
    state.workers.clear();state.workers[reinterpret_cast<std::uintptr_t>(native_worker)]=GetCurrentThreadId();
    reset_request();empty_queues();
}
void native_objects() {
    object_chunks[0]=reinterpret_cast<std::uintptr_t>(object_items);put(executable_image+0x753d660,reinterpret_cast<std::uintptr_t>(object_chunks));put(executable_image+0x753d674,4);
    put(executable_image+0x7447070,reinterpret_cast<std::uintptr_t>(root_type));
    const std::uintptr_t objects[]={reinterpret_cast<std::uintptr_t>(system_data),reinterpret_cast<std::uintptr_t>(worker_object),reinterpret_cast<std::uintptr_t>(instance_data),reinterpret_cast<std::uintptr_t>(world_data)};
    for(int index=0;index<4;++index) {
        put(objects[index]+0xc,index);put(objects[index]+0x10,reinterpret_cast<std::uintptr_t>(index?other_type:root_type));
        put(reinterpret_cast<std::uintptr_t>(object_items)+index*24,objects[index]);put(reinterpret_cast<std::uintptr_t>(object_items)+index*24+16,101+index);
    }
    put(reinterpret_cast<std::uintptr_t>(system_data)+0x20,reinterpret_cast<std::uintptr_t>(instance_data));
    put(reinterpret_cast<std::uintptr_t>(system_data)+0x58,1);put(reinterpret_cast<std::uintptr_t>(system_data)+0x5c,102);
    put(reinterpret_cast<std::uintptr_t>(world_data)+0x180,reinterpret_cast<std::uintptr_t>(instance_data));
    put(reinterpret_cast<std::uintptr_t>(worker_object)+0x30,reinterpret_cast<std::uintptr_t>(native_worker));
    put(reinterpret_cast<std::uintptr_t>(native_worker),executable_image+0x59a3198);
    for(const unsigned at:{0x18U,0x40U,0x68U})InitializeCriticalSection(reinterpret_cast<CRITICAL_SECTION*>(native_worker+at));
}
}
int wmain(int argc,wchar_t** argv){try {
    using namespace scum_save;
    if(argc!=3)throw std::runtime_error("arguments_fixture_and_output_required");
    const auto image=admission_image(argv[1]);
    const auto module=LoadLibraryExW(image.c_str(),nullptr,DONT_RESOLVE_DLL_REFERENCES);check(module!=nullptr,"owned_E_fixed_image_mapping");executable_image=reinterpret_cast<std::uintptr_t>(module);
    check(test_profile_copy(image,executable_image),"production_full_fixed_profile");native_objects();
    const auto scratch=admission_scratch(argv[2]);
    check(fs::is_directory(scratch),"fresh_owned_IPC");initialize_public_boundary(scratch);
    check(install_root_boundary()&&code_matches(),"production_root_hook_external_original_boundary_only");
    for(const unsigned at:{0x90U,0xa0U,0xb0U,0xc0U}) {
        reset();busy_queue(at);const auto input=request();
        check(std::string_view(start_request(input))=="pending"&&root_boundary_calls()==0&&!runtime().active,"queued_preexisting_work_parks_without_owner");
        const auto owner=parked_owner();const auto deadline=owner->deadline;
        check(owner->root_calls==0&&owner->publications.empty()&&owner->sql.empty()&&!owner->items&&!owner->virtualized,"parked_candidate_owns_no_receipt_or_SQL");
        check(std::string_view(query_request(input))=="pending"&&owner->deadline==deadline&&!runtime().active,"same_request_budget_not_reset");
        empty_queues();check(std::string_view(query_request(input))=="pending"&&root_boundary_calls()==1&&runtime().active==owner&&owner->root_calls==1,"exact_idle_starts_original_root_once");
        check(std::string_view(query_request(input))=="unknown"&&root_boundary_calls()==1,"original_incomplete_body_remains_unconfirmed");
        for(unsigned replay=0;replay<3;++replay)check(std::string_view(query_request(input))=="unknown"&&root_boundary_calls()==1,"terminal_replay_never_dispatches_again");
    }
    reset();put(reinterpret_cast<std::uintptr_t>(native_worker)+0xd8,2);put(reinterpret_cast<std::uintptr_t>(native_worker)+0x104,1);
    check(std::string_view(start_request(request()))=="pending"&&!runtime().active&&!root_boundary_calls(),"outstanding_native_ID_set_parks");
    for(unsigned flight=0;flight<3;++flight) {
        reset();(flight==0?runtime().task_inflight:flight==1?runtime().sql_inflight:runtime().io_inflight)=1;
        check(std::string_view(start_request(request()))=="pending"&&!runtime().active&&!root_boundary_calls(),"known_inflight_parks_without_active_owner");
    }
    for(const unsigned offset:{0x18U,0x40U,0x68U}) {
        reset();Handle locked(CreateEventW(nullptr,TRUE,FALSE,nullptr)),release(CreateEventW(nullptr,TRUE,FALSE,nullptr));bool completed{};
        std::thread holder([&]{EnterCriticalSection(reinterpret_cast<CRITICAL_SECTION*>(native_worker+offset));SetEvent(locked.value);completed=WaitForSingleObject(release.value,5000)==WAIT_OBJECT_0;LeaveCriticalSection(reinterpret_cast<CRITICAL_SECTION*>(native_worker+offset));});
        check(WaitForSingleObject(locked.value,5000)==WAIT_OBJECT_0,"other_thread_holds_native_CS");const auto input=request();
        const auto result=start_request(input);const bool pending=std::string_view(result)=="pending"&&!runtime().active&&!root_boundary_calls();
        bool partial_free{};if(offset!=0x18) {std::thread probe([&]{partial_free=TryEnterCriticalSection(reinterpret_cast<CRITICAL_SECTION*>(native_worker+0x18))!=FALSE;if(partial_free)LeaveCriticalSection(reinterpret_cast<CRITICAL_SECTION*>(native_worker+0x18));});probe.join();}else partial_free=true;
        SetEvent(release.value);holder.join();check(completed&&pending&&partial_free,"Try_failure_releases_partial_locks_and_returns_before_holder_release");
        check(std::string_view(query_request(input))=="pending"&&root_boundary_calls()==1,"release_event_allows_one_later_admission");
    }
    reset();busy_queue(0x90);auto input=request();start_request(input);const auto owner=parked_owner();owner->deadline=0;
    check(std::string_view(query_request(input))=="unknown"&&!runtime().active&&!root_boundary_calls()&&std::string_view(owner->completion.issue())=="native_admission_timeout","first_deadline_timeout_is_terminal_before_root");
    owner->deadline=GetTickCount64()+30000;empty_queues();check(std::string_view(query_request(input))=="unknown"&&!root_boundary_calls(),"late_idle_does_not_revive_timeout");
    for(unsigned mismatch=0;mismatch<5;++mismatch) {
        reset();busy_queue(0x90);input=request();start_request(input);auto other=input;
        if(mismatch==0)other.nonce[0]='f';if(mismatch==1)++other.requested_at;if(mismatch==2)++other.system;if(mismatch==3)++other.instance;if(mismatch==4)++other.world;
        check(std::string_view(query_request(other))=="unknown"&&!runtime().active&&!root_boundary_calls(),"wrong_complete_request_tuple_never_starts");
    }
    reset();busy_queue(0x90);input=request();start_request(input);auto other=input;other.nonce[0]='f';const auto retained_owner=parked_owner();
    rejects([&]{start_request(other);},"native_previous_save_unsettled");check(parked_owner()==retained_owner&&!runtime().active,"second_request_cannot_replace_parked_reservation");
    for(unsigned invalid=0;invalid<6;++invalid) {
        reset();busy_queue(0x90);input=request();start_request(input);
        if(invalid==0)native_worker[0x124]=1;
        if(invalid==1)native_worker[0x124]=2;
        if(invalid==2)put(reinterpret_cast<std::uintptr_t>(native_worker)+0x104,1);
        if(invalid==3)put(reinterpret_cast<std::uintptr_t>(native_worker)+0xac,-1);
        if(invalid==4)runtime().workers[reinterpret_cast<std::uintptr_t>(native_worker)]++;
        if(invalid==5)put(reinterpret_cast<std::uintptr_t>(native_worker),executable_image+0x59a31a0);
        check(std::string_view(query_request(input))=="unknown"&&!runtime().active&&!root_boundary_calls(),"nonrecoverable_worker_or_shape_is_terminal");
        put(reinterpret_cast<std::uintptr_t>(native_worker),executable_image+0x59a3198);empty_queues();check(std::string_view(query_request(input))=="unknown"&&!root_boundary_calls(),"repair_after_terminal_cannot_revive");
    }
    reset();busy_queue(0x90);input=request();start_request(input);put(reinterpret_cast<std::uintptr_t>(world_data)+0x180,reinterpret_cast<std::uintptr_t>(system_data));
    check(std::string_view(query_request(input))=="unknown"&&!runtime().active&&!root_boundary_calls(),"changed_world_is_terminal_before_root");
    put(reinterpret_cast<std::uintptr_t>(world_data)+0x180,reinterpret_cast<std::uintptr_t>(instance_data));empty_queues();check(std::string_view(query_request(input))=="unknown"&&!root_boundary_calls(),"restored_world_does_not_revive");
    reset();busy_queue(0x90);input=request();start_request(input);prime_cached_request(input);
    invoke_cached_request();check(cached_terminal_unknown()&&!runtime().active&&!root_boundary_calls(),"actual_shared_observe_validation_failure_clears_pending");
    check(std::string_view(query_request(input))=="unknown"&&!root_boundary_calls(),"shared_abort_also_terminates_private_admission");
    empty_queues();for(unsigned replay=0;replay<4;++replay){invoke_cached_request();check(cached_terminal_unknown()&&!runtime().active&&!root_boundary_calls(),"actual_shared_old_nonce_late_replay_cannot_resume_admission");}
    other=input;other.nonce[0]='f';check(std::string_view(start_request(other))=="pending"&&root_boundary_calls()==1,"aborted_unentered_reservation_allows_one_new_accepted_request");
    reset();busy_queue(0x90);input=request();start_request(input);other=input;other.nonce[0]='f';prime_cached_request(other);invoke_cached_request();
    check(std::string_view(query_request(input))=="pending"&&!root_boundary_calls()&&!runtime().active,"wrong_nonce_abort_cannot_cancel_accepted_request");
    empty_queues();check(std::string_view(query_request(input))=="pending"&&root_boundary_calls()==1,"owner_query_still_can_admit_after_foreign_abort");
    reset();busy_queue(0x90);input=request();start_request(input);const auto expired=parked_owner();expired->deadline=0;other=input;other.nonce[0]='f';
    check(std::string_view(start_request(other))=="pending"&&parked_owner()!=expired&&expired->root_calls==0&&expired->completion.rejected()&&!runtime().active,"new_start_retires_only_expired_zero_root_reservation");
    empty_queues();check(std::string_view(query_request(input))=="unknown"&&!root_boundary_calls(),"retired_old_request_cannot_advance_new_reservation");
    check(std::string_view(query_request(other))=="pending"&&root_boundary_calls()==1,"new_reservation_original_root_once");
    reset();input=request();root_boundary_throws(true);check(std::string_view(start_request(input))=="unknown"&&root_boundary_calls()==1,"original_root_exception_does_not_resend");
    const auto failed_owner=runtime().active;check(failed_owner&&parked_owner()==failed_owner&&failed_owner->completion.rejected(),"uncertain_root_owner_retained");
    failed_owner->io_inflight=1;runtime().io_inflight=1;failed_owner->retained.emplace_back(CreateEventW(nullptr,TRUE,FALSE,nullptr));const auto pin=failed_owner->retained.back().value;
    prime_cached_request(input);invoke_cached_request();check(cached_terminal_unknown()&&runtime().active==failed_owner&&failed_owner->io_inflight==1,"shared_abort_retains_entered_root_and_IO_debt");
    other=input;other.nonce[0]='f';rejects([&]{start_request(other);},"native_previous_save_unsettled");
    DWORD flags{};check(runtime().active==failed_owner&&failed_owner->io_inflight==1&&runtime().io_inflight==1&&GetHandleInformation(pin,&flags),"outer_unknown_new_request_keeps_original_IO_debt_and_pin");
    check(code_matches(),"fixed_profile_intact_after_all_components");reset();
    for(const unsigned at:{0x18U,0x40U,0x68U})DeleteCriticalSection(reinterpret_cast<CRITICAL_SECTION*>(native_worker+at));
    std::cout<<"SCUM pending admission checks="<<admission_checks<<" native_game_processes=0 original_AutoSave_body_external_boundary=true actual_weak_getter_profile_root_hook_and_shared_IPC=true no_sleep=true\n";return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}}
