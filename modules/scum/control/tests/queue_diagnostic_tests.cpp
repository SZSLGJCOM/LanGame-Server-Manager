// Only the native object-identity/image boundary is replaced. Production queue
// ownership, observer state, Completion, SQL closure and original-call flow run.
#include "../native_state.hpp"
#include <array>
#include <cstring>
#include <iostream>
namespace scum_save {
unsigned identity_calls{};bool identity_result{};
bool queue_test_same_identity(const ObjectIdentity&) noexcept {++identity_calls;return identity_result;}
// This pre-existing finite diagnostic unit replaces only the native callable
// metadata/queue-publication boundary. Native mixed-task tests execute it real.
TaskRecord queue_test_task_record(std::uintptr_t function) {TaskRecord value;value.function=function;value.binding.invoker=0x1b976c0;value.binding.table=0x571e488;return value;}
std::uintptr_t queue_test_image_base() noexcept {return reinterpret_cast<std::uintptr_t>(GetModuleHandleW(nullptr));}
}
#define same_identity queue_test_same_identity
#define image_base queue_test_image_base
#define task_record queue_test_task_record
#include "../native_tasks.cpp"
#undef task_record
#undef image_base
#undef same_identity
namespace {
unsigned checks{},original_calls{};DWORD original_error{};int next_sequence{20};
inline void check(bool value,const char* reason){if(!value)throw std::runtime_error(reason);++checks;}
alignas(16) std::array<unsigned char,80> native_record{};
inline int original_queue(std::uintptr_t object,void*) {
    ++original_calls;original_error=GetLastError();const auto sequence=++next_sequence;
    const auto worker=*reinterpret_cast<const std::uintptr_t*>(object+0x30);
    const auto data=reinterpret_cast<std::uintptr_t>(native_record.data());const int one=1;
    std::memcpy(native_record.data(),&sequence,sizeof sequence);std::memcpy(reinterpret_cast<void*>(worker+0x90),&data,sizeof data);
    std::memcpy(reinterpret_cast<void*>(worker+0x98),&one,sizeof one);std::memcpy(reinterpret_cast<void*>(worker+0x9c),&one,sizeof one);
    SetLastError(4312);return sequence;
}
template<class T> bool cases(){
    using namespace scum_save;
    if constexpr(requires(T& value){value.first_foreign_queue;}) {
        alignas(CRITICAL_SECTION) std::array<unsigned char,0x140> worker{};
        alignas(void*) std::array<unsigned char,0x40> object{};
        const auto worker_address=reinterpret_cast<std::uintptr_t>(worker.data());
        std::memcpy(object.data()+0x30,&worker_address,sizeof worker_address);
        auto* producer=reinterpret_cast<CRITICAL_SECTION*>(worker.data()+0x18);InitializeCriticalSection(producer);
        queue_original=original_queue;
        auto& state=runtime();state.armed=true;state.broken=false;
        auto fresh=[&]{scope.reset();state.task_inflight=state.sql_inflight=state.io_inflight=0;
            auto owner=std::make_shared<T>();owner->native_worker=worker_address;owner->main_thread=GetCurrentThreadId();
            owner->worker_thread=GetCurrentThreadId()+1;state.active=owner;identity_calls=0;identity_result=true;original_calls=0;return owner;};
        auto call=[&]{SetLastError(2718);const auto result=queue_hook(reinterpret_cast<std::uintptr_t>(object.data()),nullptr);
            check(result==next_sequence,"queue_original_return_changed");check(original_calls>0,"queue_original_not_called");
            check(original_error==2718&&GetLastError()==4312,"queue_diagnostic_changed_last_error");};
        auto owner=fresh();call();
        check(owner->first_foreign_queue.captured,"foreign_queue_diagnostic_missing");
        check(identity_calls==0,"foreign_scope_performed_extra_worker_check");
        check(owner->first_foreign_queue.other_task_inflight==0,"current_queue_counted_as_prior_debt");
        check((owner->first_foreign_queue.context&(4u|8u|128u|8192u))==0,"foreign_queue_initial_context_wrong");
        check((owner->first_foreign_queue.context&(32u|262144u|524288u))==(32u|262144u|524288u),"main_and_other_task_complete_missing");
        check(owner->first_foreign_queue.caller_rva>0&&owner->first_foreign_queue.caller_rva<0x7e10000,"caller_rva_not_finite");
        check(owner->completion.rejected()&&!owner->completion.failed()&&!owner->confirmed,"foreign_queue_guard_was_relaxed");
        check(std::string_view(owner->completion.issue())=="foreign_native_queue","foreign_queue_issue_changed");
        const auto first=owner->first_foreign_queue;state.task_inflight=9;owner->root_entered=true;call();
        check(owner->first_foreign_queue.context==first.context&&owner->first_foreign_queue.other_task_inflight==first.other_task_inflight,"first_queue_snapshot_overwritten");
        owner=fresh();scope=owner;identity_result=false;call();
        check(identity_calls==1,"owned_scope_worker_check_not_once");
        check((owner->first_foreign_queue.context&7u)==7u&&(owner->first_foreign_queue.context&8u)==0,"worker_check_execution_not_distinguished");
        check(owner->completion.rejected(),"invalid_worker_guard_was_relaxed");
        owner=fresh();scope=owner;call();
        check(identity_calls==1&&!owner->first_foreign_queue.captured&&!owner->completion.rejected(),"valid_owned_queue_changed");
        owner=fresh();state.task_inflight=2;state.sql_inflight=3;state.io_inflight=4;owner->owned_scopes=1;owner->io_inflight=1;owner->io_failed=true;call();
        check(owner->first_foreign_queue.other_task_inflight==2,"prior_task_debt_not_discounted_exactly_once");
        check((owner->first_foreign_queue.context&(16384u|32768u|65536u|131072u|524288u))==0,"nonzero_scope_or_io_marked_quiet");
        check((owner->first_foreign_queue.context&4096u)!=0,"io_failure_not_observed");
        owner=fresh();owner->completion.unknown("native_prior_rejection");call();
        check((owner->first_foreign_queue.context&8192u)!=0&&std::string_view(owner->completion.issue())=="native_prior_rejection","prior_rejection_was_hidden");
        owner=fresh();owner->completion.stage(1);owner->completion.stage(2);owner->completion.stage(4);owner->completion.published(7);owner->completion.root_returned();
        owner->completion.batch_entered({7});owner->completion.batch_returned({7});owner->completion.pump_entered({7});owner->completion.pump_returned({7});
        SqlCompletion proof;proof.step(SqlKind::write,101,true,true);proof.readback_verified();
        owner->sql.emplace(reinterpret_cast<void*>(1),proof);owner->payload_connections.insert(reinterpret_cast<void*>(1));call();
        constexpr unsigned settled=128u|256u|512u|1024u|2048u|16384u|32768u|65536u|131072u|262144u|524288u;
        check((owner->first_foreign_queue.context&settled)==settled,"settled_sql_and_prior_quiet_snapshot_missing");
        check(owner->completion.rejected()&&!owner->completion.confirmed(true,true)&&!owner->confirmed,"diagnostic_snapshot_forged_confirmation");
        owner=fresh();for(std::uintptr_t i=1;i<=65;++i)owner->sql.emplace(reinterpret_cast<void*>(i),SqlCompletion{});call();
        check((owner->first_foreign_queue.context&(1024u|2048u))==0,"oversized_sql_observation_claimed_complete");
        owner=fresh();owner->sql.emplace(reinterpret_cast<void*>(1),SqlCompletion{});owner->sql.begin()->second.unknown();call();
        check((owner->first_foreign_queue.context&1024u)!=0&&(owner->first_foreign_queue.context&2048u)==0,"unclosed_sql_claimed_closed");
        state.active.reset();scope.reset();state.task_inflight=state.sql_inflight=state.io_inflight=0;
        DeleteCriticalSection(producer);return true;
    } else {return false;}
}
}
int main(){try{if(cases<scum_save::Epoch>()){std::cout<<"SCUM foreign queue diagnostic checks="<<checks<<" native_game_functions_executed=false\n";return 0;}
else{std::cerr<<"foreign_queue_diagnostic_missing\n";return 1;}}
catch(const std::exception& error){std::cerr<<error.what()<<"\n";return 1;}}
