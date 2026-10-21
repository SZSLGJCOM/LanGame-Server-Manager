#include "native_state.hpp"
#include <intrin.h>

namespace scum_save {
namespace {
void(*root_original)(std::uintptr_t,bool){};
void(*global_original)(std::uintptr_t,void*){};
int(*transforms_original)(std::uintptr_t){};
int(*entities_original)(std::uintptr_t,bool){};
int(*queue_original)(std::uintptr_t,void*){};
void(*wrapper_original)(std::uintptr_t,std::uintptr_t,void*){};
struct WrapperFrame {std::uintptr_t system{},caller{};bool present{};};
thread_local WrapperFrame wrapper_frame;
void(*item_original)(std::uintptr_t,std::uintptr_t){};
struct ItemFrame {std::uintptr_t receiver{},caller{},writer{};bool valid{};};
thread_local ItemFrame item_frame;
void(*virtualized_original)(std::uintptr_t,std::uintptr_t){};
struct VirtualizedFrame {TaskBinding binding;bool valid{};};
thread_local VirtualizedFrame virtualized_frame;
void virtualized_hook(std::uintptr_t manager,std::uintptr_t function) {
    const auto error=GetLastError();const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());
    struct Restore {VirtualizedFrame previous;~Restore(){const auto error=GetLastError();virtualized_frame=previous;SetLastError(error);}} restore{virtualized_frame};
    virtualized_frame={};
    try {
        std::lock_guard lock(runtime().mutex);const auto root=runtime().active;const auto base=image_base();
        if(root&&!root->confirmed&&!scope&&root->main_thread==GetCurrentThreadId()&&caller>=base&&caller-base<=UINT32_MAX) {
            virtualized_frame.binding=virtualized_input(function,manager,static_cast<std::uint32_t>(caller-base));
            virtualized_frame.valid=true;
        }
    }catch(...){virtualized_frame.valid=false;}
    SetLastError(error);virtualized_original(manager,function);
}
struct ItemProducer {std::uint32_t caller,writer,table;};
constexpr ItemProducer item_producers[]={
    {0x1b9990e,0x1b97650,0x51e8bd8},{0x1bc8ed4,0x1b978f0,0x5196950},
    {0x1bc8f58,0x1b97340,0x5196950},{0x1bc9a62,0x1b97290,0x5196950},
    {0x1bcabe2,0x1b98290,0x5196950},{0x1bcaee5,0x1b97ff0,0x59df8d8},
    {0x1bcbdcb,0x1b97750,0x5196950},{0x1bcc46a,0x1b981a0,0x5196950},
    {0x1bcc55f,0x1b98300,0x5196950},{0x1bcc917,0x1b973b0,0x58c9660},
    {0x1bcf020,0x1b97b10,0x5196950},{0x1bd09b3,0x1b97750,0x5196950}};
void item_hook(std::uintptr_t receiver,std::uintptr_t function) {
    const auto error=GetLastError();
    struct Restore {ItemFrame previous;~Restore(){const auto error=GetLastError();item_frame=previous;SetLastError(error);}} restore{item_frame};
    item_frame={receiver,reinterpret_cast<std::uintptr_t>(_ReturnAddress()),0,false};
    item_frame.valid=read(function,item_frame.writer);
    SetLastError(error);item_original(receiver,function);
}
TaskBinding item_binding(const Owner& owner,std::uintptr_t function) {
    const auto base=image_base();
    if(scope||!item_frame.valid||!wrapper_frame.present||wrapper_frame.caller!=base+0x1ba6771||
        wrapper_frame.system!=item_frame.receiver||GetCurrentThreadId()!=owner->main_thread||
        !same_identity(owner->system)||!same_identity(owner->worker))throw std::runtime_error("foreign_native_queue");
    const auto weak=reinterpret_cast<std::uintptr_t(*)(std::uintptr_t)>(base+0x2d230b0);
    if(weak(item_frame.receiver+0x58)!=owner->system.pointer||weak(owner->system.pointer+0x58)!=owner->worker.pointer)
        throw std::runtime_error("native_item_world");
    for(const auto& producer:item_producers)if(item_frame.caller==base+producer.caller&&item_frame.writer==base+producer.writer) {
        auto binding=task_record(function).binding;binding.kind=TaskKind::item;binding.producer=producer.caller;
        binding.writer=producer.writer;binding.writer_table=producer.table;validate_item_function(function,binding);return binding;
    }
    throw std::runtime_error("native_item_producer_unknown");
}
// Diagnostic provenance only: no Scope, ownership, lock or payload changes.
void wrapper_hook(std::uintptr_t system,std::uintptr_t context,void* callable) {
    const auto error=GetLastError();
    struct Restore {
        WrapperFrame previous;
        ~Restore() {const auto error=GetLastError();wrapper_frame=previous;SetLastError(error);}
    } restore{wrapper_frame};
    wrapper_frame={system,reinterpret_cast<std::uintptr_t>(_ReturnAddress()),true};
    SetLastError(error);wrapper_original(system,context,callable);
}
unsigned(*worker_original)(std::uintptr_t){};
void(*batch_original)(std::uintptr_t,std::uintptr_t){};
void(*pump_original)(std::uintptr_t){};
bool(*cancel_original)(std::uintptr_t,int){};
void(WINAPI*enter_original)(CRITICAL_SECTION*){};
thread_local std::uintptr_t pump_worker{};
thread_local bool pump_entered{};
thread_local std::vector<int>* pump_items{};
thread_local bool pump_items_entered{};
thread_local std::vector<int>* pump_virtualized{};
thread_local bool pump_virtualized_entered{};
struct NativeLock {
    CRITICAL_SECTION* value;
    explicit NativeLock(std::uintptr_t address):value(reinterpret_cast<CRITICAL_SECTION*>(address)){EnterCriticalSection(value);}
    ~NativeLock(){LeaveCriticalSection(value);}
};
struct Flight {
    Flight(){std::lock_guard lock(runtime().mutex);++runtime().task_inflight;}
    ~Flight(){std::lock_guard lock(runtime().mutex);--runtime().task_inflight;}
};
std::vector<int> records(std::uintptr_t worker,unsigned offset,unsigned stride) {
    const auto count=require_read<int>(worker+offset+8),capacity=require_read<int>(worker+offset+12);
    const auto data=require_read<std::uintptr_t>(worker+offset);
    if(count<0||count>capacity||capacity>262144||(count&&!data))throw std::runtime_error("native_queue_shape");
    std::vector<int> result;result.reserve(static_cast<std::size_t>(count));
    for(int i=0;i<count;++i)result.push_back(require_read<int>(data+static_cast<std::uintptr_t>(i)*stride));return result;
}
Owner owner_for(std::uintptr_t worker) {
    auto& state=runtime();const auto active=state.active;
    if(!active||active->confirmed)return {};
    if(worker!=active->native_worker)return {};
    return active;
}
void stage(unsigned which,std::uintptr_t system,bool force=true) noexcept {
    try {std::lock_guard lock(runtime().mutex);
        if(scope&&scope->system.pointer==system&&scope->root_entered&&force)scope->completion.stage(which);
        else if(runtime().active&&!runtime().active->confirmed)unknown(runtime().active,"foreign_native_save_stage");
    }catch(...){unknown(scope,"native_stage_contract");}
}
void root_hook(std::uintptr_t system,bool force) {
    Flight flight;const auto owner=scope;bool owned{};
    {std::lock_guard lock(runtime().mutex);
        owned=owner&&owner==runtime().active&&owner->system.pointer==system&&force&&
            GetCurrentThreadId()==owner->main_thread&&!owner->root_entered&&owner->root_calls++==0;
        if(owned)owner->root_entered=true;else unknown(runtime().active,"foreign_native_save_root");}
    try {root_original(system,force);}catch(...){unknown(owner,"native_root_exception");throw;}
    if(owned) {std::lock_guard lock(runtime().mutex);owner->root_entered=false;
        try{owner->completion.root_returned();}catch(...){unknown(owner,"native_root_incomplete");}}
}
void global_hook(std::uintptr_t system,void* callback){stage(1,system);global_original(system,callback);}
int transforms_hook(std::uintptr_t system){stage(2,system);return transforms_original(system);}
int entities_hook(std::uintptr_t system,bool force){stage(4,system,force);return entities_original(system,force);}
// Caller holds the existing observer mutex and native producer lock. This
// finite snapshot never acquires another native lock or changes admission.
void foreign_queue_diagnostic(const Owner& owner,std::uintptr_t caller,bool worker_checked,bool worker_matches) noexcept {
    if(owner->first_foreign_queue.captured)return;
    const auto error=GetLastError();ForeignQueueDiagnostic value{};value.captured=true;
    const auto base=image_base();
    if(base&&caller>=base&&caller-base<0x7e10000)value.caller_rva=static_cast<std::uint32_t>(caller-base);
    if(wrapper_frame.present) {
        const bool caller_valid=base&&wrapper_frame.caller>base&&wrapper_frame.caller-base<0x7e10000;
        if(caller_valid)value.parent_rva=static_cast<std::uint32_t>(wrapper_frame.caller-base);
        value.parent_context=1u|(caller_valid?2u:0u)|(wrapper_frame.system==owner->system.pointer?4u:0u);
    }
    const auto thread=GetCurrentThreadId();const auto& state=runtime();
    const bool sql_complete=owner->sql.size()<=64;bool sql_closed=sql_complete;
    if(sql_complete)for(const auto& [database,proof]:owner->sql){(void)database;sql_closed=sql_closed&&proof.closed();}
    const bool other_complete=state.task_inflight>0;
    if(other_complete)value.other_task_inflight=state.task_inflight-1; // current queue's Flight is already entered
    value.context=(scope?1u:0u)|(scope==owner?2u:0u)|(worker_checked?4u:0u)|(worker_matches?8u:0u)
        |(owner->root_entered?16u:0u)|(owner->main_thread==thread?32u:0u)|(owner->worker_thread==thread?64u:0u)
        |(owner->completion.native_settled()?128u:0u)|(!owner->sql.empty()?256u:0u)|(!owner->payload_connections.empty()?512u:0u)
        |(sql_complete?1024u:0u)|(sql_closed?2048u:0u)|(owner->io_failed?4096u:0u)|(owner->completion.rejected()?8192u:0u)
        |(!owner->owned_scopes?16384u:0u)|(!owner->io_inflight?32768u:0u)|(!state.sql_inflight?65536u:0u)|(!state.io_inflight?131072u:0u)
        |(other_complete?262144u:0u)|(other_complete&&!value.other_task_inflight?524288u:0u)|(state.armed?1048576u:0u)|(state.broken?2097152u:0u);
    owner->first_foreign_queue=value;SetLastError(error);
}
int queue_hook(std::uintptr_t object,void* callable) {
    const auto entry_error=GetLastError();const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());
    const auto worker=require_read<std::uintptr_t>(object+0x30);
    // Retain the existing recursive producer lock through real ID registration.
    struct LastError {DWORD value;~LastError(){SetLastError(value);}} last_error{entry_error};
    NativeLock producer(worker+0x18);Flight flight;Owner root,receipt;TaskBinding binding;bool checked{},matches{};
    {std::lock_guard lock(runtime().mutex);root=owner_for(worker);
        if(root)try {
            checked=scope==root;matches=checked&&same_identity(root->worker);
            if(matches) {
                receipt=root;binding=task_record(reinterpret_cast<std::uintptr_t>(callable)).binding;
                if(binding.invoker==0x1959490)throw std::runtime_error("native_virtualized_root_scope");
            }
            else if(caller==image_base()+0x1e2c692&&virtualized_frame.valid&&!scope&&
                GetCurrentThreadId()==root->main_thread) {
                binding=task_record(reinterpret_cast<std::uintptr_t>(callable)).binding;
                const auto& source=virtualized_frame.binding;binding.kind=TaskKind::virtualized;
                binding.producer=source.producer;binding.writer=source.writer;binding.writer_table=source.writer_table;
                binding.manager=source.manager;binding.payload_hash=source.payload_hash;
                validate_virtualized_function(reinterpret_cast<std::uintptr_t>(callable),binding,root);
                receipt=virtualized_receipt(root);
            }
            else {
                binding=item_binding(root,reinterpret_cast<std::uintptr_t>(callable));receipt=item_receipt(root);
            }
        }catch(...) {
            receipt.reset();foreign_queue_diagnostic(root,caller,checked,matches);
            unknown(root,"foreign_native_queue");
        }}
    int sequence{};SetLastError(entry_error);
    try {sequence=queue_original(object,callable);last_error.value=GetLastError();}
    catch(...){last_error.value=GetLastError();unknown(root,"native_queue_exception");throw;}
    if(root&&receipt)try {
        std::lock_guard lock(runtime().mutex);
        const auto count=require_read<int>(worker+0x98),capacity=require_read<int>(worker+0x9c);
        const auto data=require_read<std::uintptr_t>(worker+0x90);
        if(root!=runtime().active||root->confirmed||sequence<=0||count<=0||count>capacity||capacity>262144||!data||
            root->publications.size()>=262144)throw std::runtime_error("native_queue_publication");
        const auto record=data+static_cast<std::uintptr_t>(count-1)*80;
        auto actual=task_record(record+0x10);actual.binding.kind=binding.kind;actual.binding.producer=binding.producer;
        actual.binding.writer=binding.writer;actual.binding.writer_table=binding.writer_table;
        actual.binding.manager=binding.manager;actual.binding.payload_hash=binding.payload_hash;
        if(binding.kind==TaskKind::item)validate_item_function(record+0x10,actual.binding);
        if(binding.kind==TaskKind::virtualized)validate_virtualized_function(record+0x10,actual.binding,root);
        if(require_read<int>(record)!=sequence||actual.binding!=binding||
            (!root->publications.empty()&&static_cast<std::int64_t>(sequence)!=static_cast<std::int64_t>(root->publications.rbegin()->first)+1)||
            !root->publications.try_emplace(sequence,binding).second)
            throw std::runtime_error("native_queue_publication");
        receipt->completion.published(sequence);
    }catch(...){unknown(root,"native_queue_publication");}
    return sequence;
}
unsigned worker_hook(std::uintptr_t worker) {
    {std::lock_guard lock(runtime().mutex);auto& thread=runtime().workers[worker];
        if(thread&&thread!=GetCurrentThreadId())runtime().broken=true;thread=GetCurrentThreadId();}
    const auto result=worker_original(worker);
    {std::lock_guard lock(runtime().mutex);unknown(owner_for(worker),"native_worker_exited");runtime().workers.erase(worker);}
    return result;
}
void batch_hook(std::uintptr_t worker,std::uintptr_t connection) {
    Flight flight;Owner owner;BatchFrame frame;frame.previous=batch_frame;std::vector<int> saves,items,virtualized;bool admitted{};
    {std::lock_guard lock(runtime().mutex);owner=owner_for(worker);
        if(owner)try {
            if(scope||batch_frame||owner->worker_thread!=GetCurrentThreadId())throw std::runtime_error("native_worker_thread");
            const auto count=require_read<int>(worker+0xa8),capacity=require_read<int>(worker+0xac);
            const auto data=require_read<std::uintptr_t>(worker+0xa0);
            if(count<0||count>capacity||capacity>262144||(count&&!data))throw std::runtime_error("native_queue_shape");
            // The fixed worker calls this drain even after an event coalesces
            // to an empty A0. Native 1aceaba exits immediately for count zero:
            // it executes no callback or SQL, and pays no completion debt.
            if(count) {
            frame.root=owner;frame.connection=connection;frame.previous=batch_frame;frame.records.reserve(count);
            auto previous=owner->last_batched;
            for(int index=0;index<count;++index) {
                const auto at=data+static_cast<std::uintptr_t>(index)*80;auto record=task_record(at+0x10);
                record.sequence=require_read<int>(at);const auto found=owner->publications.find(record.sequence);
                if(found==owner->publications.end()||found!=owner->publications.upper_bound(previous))
                    throw std::runtime_error("native_batch_foreign_sequence");
                previous=record.sequence;
                auto expected=found->second;
                if(record.binding.invoker!=expected.invoker||record.binding.table!=expected.table)throw std::runtime_error("native_task_callable_changed");
                record.binding=expected;
                if(expected.kind==TaskKind::item)validate_item_function(at+0x10,record.binding);
                if(expected.kind==TaskKind::virtualized)validate_virtualized_function(at+0x10,record.binding,owner);
                const auto receipt=task_owner(owner,record.sequence);
                (receipt==owner?saves:receipt==owner->items?items:virtualized).push_back(record.sequence);frame.records.push_back(record);
            }
            if(!saves.empty())owner->completion.batch_entered(saves);
            if(!items.empty())owner->items->completion.batch_entered(items);
            if(!virtualized.empty())owner->virtualized->completion.batch_entered(virtualized);
            owner->last_batched=previous;admitted=true;batch_frame=&frame;}
        }catch(...){unknown(owner,"native_batch_ownership");}}
    {Scope entered({},connection);
        try {batch_original(worker,connection);}catch(...){batch_frame=frame.previous;unknown(owner,"native_batch_exception");throw;}}
    batch_frame=frame.previous;
    if(admitted) {
        if(frame.cursor!=frame.records.size())unknown(owner,"native_task_dispatch_missing");
        {Scope entered(owner,connection);verify_databases(owner,connection);}
        if(!items.empty()){Scope entered(owner->items,connection);verify_databases(owner->items,connection);}
        if(!virtualized.empty()){Scope entered(owner->virtualized,connection);verify_databases(owner->virtualized,connection);}
        std::lock_guard lock(runtime().mutex);
        try {
            if(!saves.empty())owner->completion.batch_returned(saves);
            if(!items.empty())owner->items->completion.batch_returned(items);
            if(!virtualized.empty())owner->virtualized->completion.batch_returned(virtualized);
        }catch(...){unknown(owner,"native_batch_return");}
    }
}
void pump_hook(std::uintptr_t worker) {
    Flight flight;Owner owner;
    {std::lock_guard lock(runtime().mutex);owner=owner_for(worker);}
    std::vector<int> sequences,item_sequences,virtualized_sequences;auto* previous=pump_sequences;auto* previous_items=pump_items;
    auto* previous_virtualized=pump_virtualized;const auto previous_virtualized_entered=pump_virtualized_entered;
    const auto previous_worker=pump_worker;const auto previous_entered=pump_entered,previous_item_entered=pump_items_entered;
    pump_sequences=&sequences;pump_items=&item_sequences;pump_worker=worker;pump_entered=false;pump_items_entered=false;
    pump_virtualized=&virtualized_sequences;pump_virtualized_entered=false;
    const auto restore=[&] {pump_sequences=previous;pump_items=previous_items;pump_worker=previous_worker;
        pump_entered=previous_entered;pump_items_entered=previous_item_entered;
        pump_virtualized=previous_virtualized;pump_virtualized_entered=previous_virtualized_entered;};
    Scope entered(owner);
    try {pump_original(worker);}catch(...){restore();unknown(owner,"native_pump_exception");throw;}
    const bool captured=pump_entered,item_captured=pump_items_entered,virtualized_captured=pump_virtualized_entered;restore();
    if(owner&&(captured||item_captured||virtualized_captured)) {
        verify_databases(owner,0);
        std::lock_guard lock(runtime().mutex);
        try {
            if(captured)owner->completion.pump_returned(sequences);
            if(item_captured)owner->items->completion.pump_returned(item_sequences);
            if(virtualized_captured)owner->virtualized->completion.pump_returned(virtualized_sequences);
        }catch(...){unknown(owner,"native_pump_return");}
    }
}
bool cancel_hook(std::uintptr_t worker,int sequence) {
    Flight flight;const auto result=cancel_original(worker,sequence);
    if(result){std::lock_guard lock(runtime().mutex);const auto owner=owner_for(worker);if(owner)try {
        task_owner(owner,sequence)->completion.cancelled(sequence);unknown(owner,"native_task_cancelled");
    }catch(...){unknown(owner,"native_task_cancelled");}}
    return result;
}
void WINAPI enter_hook(CRITICAL_SECTION* section) {
    const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());enter_original(section);pump_capture_locked(caller,section);
}
}
void pump_capture_locked(std::uintptr_t caller,CRITICAL_SECTION* section) noexcept {
    if(!pump_sequences||caller!=image_base()+0x1ab9e68||reinterpret_cast<std::uintptr_t>(section)!=pump_worker+0x68)return;
    try {
        // The original function already owns CS68. Copy only its original B0
        // and C0 IDs here; never extend the native lock across continuations.
        const auto callbacks=records(pump_worker,0xb0,80),completed=records(pump_worker,0xc0,4);
        if(!scope)return;
        std::lock_guard lock(runtime().mutex);std::vector<int> saves,items,virtualized;
        for(const auto sequence:callbacks) {
            if(task_owner(scope,sequence)!=scope)throw std::runtime_error("native_item_continuation");
            saves.push_back(sequence);
        }
        for(const auto sequence:completed) {
            const auto receipt=task_owner(scope,sequence);
            (receipt==scope?saves:receipt==scope->items?items:virtualized).push_back(sequence);
        }
        if(!saves.empty()){scope->completion.pump_entered(saves);*pump_sequences=std::move(saves);pump_entered=true;}
        if(!items.empty()) {
            if(!pump_items||!scope->items)throw std::runtime_error("native_item_epoch");
            scope->items->completion.pump_entered(items);*pump_items=std::move(items);pump_items_entered=true;
        }
        if(!virtualized.empty()) {
            if(!pump_virtualized||!scope->virtualized)throw std::runtime_error("native_virtualized_epoch");
            scope->virtualized->completion.pump_entered(virtualized);*pump_virtualized=std::move(virtualized);pump_virtualized_entered=true;
        }
    }catch(...){unknown(scope,"native_pump_capture");}
}
WorkerReadiness worker_readiness_locked(std::uintptr_t worker) {
    const auto stopping=require_read<unsigned char>(worker+0x124);
    if(stopping>1)throw std::runtime_error("native_worker_state");
    if(stopping)return WorkerReadiness::stopping;
    bool empty=true;
    for(const unsigned offset:{0x90U,0xa0U,0xb0U,0xc0U}) {
        const auto pending=records(worker,offset,offset==0xc0?4:80);
        empty=empty&&pending.empty();
    }
    const auto allocated=require_read<int>(worker+0xd8),free=require_read<int>(worker+0x104);
    if(allocated<0||free<0||free>allocated)throw std::runtime_error("native_worker_pool_shape");
    return empty&&allocated==free?WorkerReadiness::ready:WorkerReadiness::busy;
}
bool tasks_quiet(std::uintptr_t worker) {
    NativeLock producer(worker+0x18),batch(worker+0x40),completed(worker+0x68);
    return worker_readiness_locked(worker)==WorkerReadiness::ready;
}
std::vector<Hook> task_hooks() {
    auto hooks=std::vector<Hook>{native_hook(0x1b94040,reinterpret_cast<void*>(root_hook),reinterpret_cast<void**>(&root_original)),
        native_hook(0x1ba7060,reinterpret_cast<void*>(global_hook),reinterpret_cast<void**>(&global_original)),
        native_hook(0x1b94ee0,reinterpret_cast<void*>(transforms_hook),reinterpret_cast<void**>(&transforms_original)),
        native_hook(0x1b94250,reinterpret_cast<void*>(entities_hook),reinterpret_cast<void**>(&entities_original)),
        native_hook(0x1abced0,reinterpret_cast<void*>(queue_hook),reinterpret_cast<void**>(&queue_original)),
        native_hook(0x1ba6380,reinterpret_cast<void*>(wrapper_hook),reinterpret_cast<void**>(&wrapper_original)),
        native_hook(0x1ba6690,reinterpret_cast<void*>(item_hook),reinterpret_cast<void**>(&item_original)),
        native_hook(0x1e2c5b0,reinterpret_cast<void*>(virtualized_hook),reinterpret_cast<void**>(&virtualized_original)),
        native_hook(0x1ad3aa0,reinterpret_cast<void*>(worker_hook),reinterpret_cast<void**>(&worker_original)),
        native_hook(0x1acea90,reinterpret_cast<void*>(batch_hook),reinterpret_cast<void**>(&batch_original)),
        native_hook(0x1ab9df0,reinterpret_cast<void*>(pump_hook),reinterpret_cast<void**>(&pump_original)),
        native_hook(0x1ab5d90,reinterpret_cast<void*>(cancel_hook),reinterpret_cast<void**>(&cancel_original))};
    const auto dispatch=dispatch_hooks();hooks.insert(hooks.end(),dispatch.begin(),dispatch.end());return hooks;
}
IatHook task_enter_import(){return {0x513c770,"EnterCriticalSection",reinterpret_cast<void*>(enter_hook),reinterpret_cast<void**>(&enter_original)};}
}
