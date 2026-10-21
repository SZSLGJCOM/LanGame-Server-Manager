// Production wrapper/queue hooks; only native call/image identity boundaries
// are host functions. No game image, Save, worker body or SQL API executes.
#define main queue_component_main
#include "queue_diagnostic_tests.cpp"
#undef main
#include <atomic>
namespace {
using Wrapper=void(*)(std::uintptr_t,std::uintptr_t,void*);
Wrapper forward{};
uintptr_t test_object{},expected_system=0x1234,expected_context=0x5678;
void* expected_callable=reinterpret_cast<void*>(0x9abc);
unsigned wrapper_calls{},wrapper_depth{};
int body_mode{};
std::atomic<DWORD> thread_error{};
HANDLE entered{},release_body{};
struct ParentSnapshot {unsigned context{},rva{};};
ParentSnapshot inner_snapshot{},outer_snapshot{};
template<class D> ParentSnapshot snapshot(const D& value) {
    if constexpr(requires{value.parent_context;value.parent_rva;})return {value.parent_context,value.parent_rva};
    else return {};
}
void original_wrapper(uintptr_t system,uintptr_t context,void* callable) {
    ++wrapper_calls;
    check(context==expected_context && callable==expected_callable,"wrapper_original_opaque_arguments_changed");
    check(system==(wrapper_depth ? expected_system+1 : expected_system),"wrapper_original_system_changed");
    check(GetLastError()==883,"wrapper_diagnostic_changed_original_entry_error");
    if(body_mode==1 && !wrapper_depth) {
        ++wrapper_depth;forward(expected_system+1,context,callable);--wrapper_depth;
        inner_snapshot=snapshot(scum_save::runtime().active->first_foreign_queue);
        scum_save::runtime().active->first_foreign_queue={};
    }
    if(body_mode==2) {SetLastError(6464);throw std::runtime_error("original_wrapper_exception");}
    if(body_mode==3) {
        SetEvent(entered);check(WaitForSingleObject(release_body,5000)==WAIT_OBJECT_0,"wrapper_thread_release");
    } else {
        SetLastError(2718);scum_save::queue_hook(test_object,nullptr);
        check(GetLastError()==4312,"wrapper_changed_original_queue_exit_error");
    }
    if(body_mode==1 && !wrapper_depth)outer_snapshot=snapshot(scum_save::runtime().active->first_foreign_queue);
    SetLastError(5112);
}
DWORD WINAPI independent_wrapper(void*) {
    SetLastError(883);forward(expected_system,expected_context,expected_callable);thread_error=GetLastError();return 0;
}
template<class T> bool wrapper_cases() {
    using namespace scum_save;
    if constexpr(requires(T& value){value.first_foreign_queue.parent_rva;value.first_foreign_queue.parent_context;}) {
        alignas(CRITICAL_SECTION) std::array<unsigned char,0x140> worker{};
        alignas(void*) std::array<unsigned char,0x40> object{};
        const auto worker_address=reinterpret_cast<uintptr_t>(worker.data());
        std::memcpy(object.data()+0x30,&worker_address,sizeof worker_address);test_object=reinterpret_cast<uintptr_t>(object.data());
        auto* producer=reinterpret_cast<CRITICAL_SECTION*>(worker.data()+0x18);InitializeCriticalSection(producer);
        bool installed{};
        for(const auto& hook:task_hooks())if(hook.rva==0x1ba6380) {
            *hook.original=reinterpret_cast<void*>(original_wrapper);forward=reinterpret_cast<Wrapper>(hook.replacement);installed=true;
            check(hook.prefix=="48895c2410" && hook.prefix_bytes==5,"wrapper_hook_prologue_contract");
        }
        check(installed,"generic_wrapper_hook_missing");queue_original=original_queue;
        auto& state=runtime();state.armed=true;state.broken=false;
        auto fresh=[&] {
            scope.reset();state.task_inflight=state.sql_inflight=state.io_inflight=0;body_mode=0;wrapper_depth=wrapper_calls=0;
            auto owner=std::make_shared<T>();owner->native_worker=worker_address;owner->system.pointer=expected_system;
            owner->main_thread=GetCurrentThreadId();owner->worker_thread=GetCurrentThreadId()+1;state.active=owner;
            identity_calls=original_calls=0;identity_result=true;return owner;
        };
        auto call=[&] {
            SetLastError(883);forward(expected_system,expected_context,expected_callable);
            check(GetLastError()==5112,"wrapper_changed_original_exit_error");
        };
        auto owner=fresh();call();
        check(wrapper_calls==1 && original_calls==1,"wrapper_original_call_count_changed");
        check(owner->first_foreign_queue.parent_context==7 && owner->first_foreign_queue.parent_rva>0,"exact_wrapper_parent_or_matching_system_missing");
        check(owner->completion.rejected() && std::string_view(owner->completion.issue())=="foreign_native_queue" && identity_calls==0,"metadata_adopted_foreign_work_or_changed_worker_check");
        const auto first=owner->first_foreign_queue;call();
        check(owner->first_foreign_queue.parent_rva==first.parent_rva && owner->first_foreign_queue.parent_context==first.parent_context,"first_parent_snapshot_overwritten");
        owner->first_foreign_queue={};SetLastError(2718);queue_hook(test_object,nullptr);
        check(owner->first_foreign_queue.parent_context==0 && owner->first_foreign_queue.parent_rva==0,"wrapper_frame_leaked_after_return");
        owner=fresh();body_mode=1;call();
        check(wrapper_calls==2 && inner_snapshot.context==3 && outer_snapshot.context==7,"nested_parent_frame_not_restored_exactly");
        check(inner_snapshot.rva>0 && outer_snapshot.rva>0,"nested_real_return_addresses_missing");
        owner=fresh();body_mode=2;bool caught{};
        try{call();}catch(const std::runtime_error& error){caught=std::string_view(error.what())=="original_wrapper_exception";}
        check(caught && GetLastError()==6464,"native_wrapper_exception_or_last_error_swallowed");
        check(wrapper_calls==1,"throwing_original_repeated");
        SetLastError(2718);queue_hook(test_object,nullptr);
        check(owner->first_foreign_queue.parent_context==0 && owner->first_foreign_queue.parent_rva==0,"wrapper_frame_leaked_after_exception");
        owner=fresh();body_mode=3;entered=CreateEventW(nullptr,TRUE,FALSE,nullptr);release_body=CreateEventW(nullptr,TRUE,FALSE,nullptr);
        check(entered && release_body,"thread_events");HANDLE thread=CreateThread(nullptr,0,independent_wrapper,nullptr,0,nullptr);
        check(thread && WaitForSingleObject(entered,5000)==WAIT_OBJECT_0,"thread_wrapper_entered");
        SetLastError(2718);queue_hook(test_object,nullptr);const auto isolated=owner->first_foreign_queue.parent_context==0;
        SetEvent(release_body);check(WaitForSingleObject(thread,5000)==WAIT_OBJECT_0,"thread_wrapper_reaped");
        CloseHandle(thread);CloseHandle(entered);CloseHandle(release_body);
        check(isolated && thread_error==5112,"wrapper_metadata_not_thread_local_or_exit_error_changed");
        check(state.task_inflight==0 && state.sql_inflight==0 && state.io_inflight==0 && !owner->confirmed,"wrapper_diagnostic_changed_work_or_confirmation");
        state.active.reset();scope.reset();DeleteCriticalSection(producer);return true;
    } else return false;
}
}
int main() {try {
    if(!cases<scum_save::Epoch>() || !wrapper_cases<scum_save::Epoch>()) {std::cerr<<"wrapper_parent_diagnostic_missing\n";return 1;}
    std::cout<<"SCUM wrapper parent diagnostic checks="<<checks<<" production_hooks=true original_args_error_exception_nested_tls=true native_game_functions_executed=false\n";return 0;
} catch(const std::exception& error){std::cerr<<error.what()<<'\n';return 1;}}
