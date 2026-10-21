#include "native_state.hpp"
#include <algorithm>

namespace scum_save {
thread_local Owner scope;
thread_local std::uintptr_t connection_scope{},opening_connection{};
thread_local unsigned readback_depth{};
thread_local std::vector<std::int32_t>* pump_sequences{};
Runtime& runtime() noexcept {static auto* state=new Runtime;return *state;}
void unknown(const Owner& owner,const char* issue) noexcept {
    if(owner){std::lock_guard lock(runtime().mutex);if(!owner->confirmed)owner->completion.unknown(issue);
        if(owner->item_scope||owner->virtualized_scope)if(const auto parent=owner->parent.lock();parent&&!parent->confirmed)parent->completion.unknown(issue);}
}
void failed(const Owner& owner,const char* issue) noexcept {
    if(owner){std::lock_guard lock(runtime().mutex);if(!owner->confirmed)owner->completion.failed(issue);
        if(owner->item_scope||owner->virtualized_scope)if(const auto parent=owner->parent.lock();parent&&!parent->confirmed)parent->completion.failed(issue);}
}
Owner admitted_scope() noexcept {
    const auto root=runtime().active;
    if(!root||root->confirmed||!scope)return {};
    if(scope==root)return root;
    if(root->items==scope&&scope->item_scope&&scope->parent.lock()==root)return scope;
    if(root->virtualized==scope&&scope->virtualized_scope&&scope->parent.lock()==root)return scope;
    return {};
}
namespace {
Owner auxiliary_receipt(const Owner& root,bool virtualized) {
    if(!root||root!=runtime().active||root->confirmed||root->item_scope||root->virtualized_scope)throw std::runtime_error("native_item_epoch");
    auto& receipt=virtualized?root->virtualized:root->items;
    if(!receipt) {
        auto item=std::make_shared<Epoch>();item->item_scope=!virtualized;item->virtualized_scope=virtualized;item->parent=root;
        item->system=root->system;item->instance=root->instance;item->world=root->world;item->worker=root->worker;
        item->native_worker=root->native_worker;item->worker_thread=root->worker_thread;item->main_thread=root->main_thread;
        item->deadline=root->deadline;receipt=std::move(item);
    }
    return receipt;
}
}
Owner item_receipt(const Owner& root) {return auxiliary_receipt(root,false);}
Owner virtualized_receipt(const Owner& root) {return auxiliary_receipt(root,true);}
Owner task_owner(const Owner& root,std::int32_t sequence) {
    const auto found=root->publications.find(sequence);
    if(found==root->publications.end())throw std::runtime_error("native_task_unpublished");
    if(found->second.kind==TaskKind::save)return root;
    if(found->second.kind!=TaskKind::item&&found->second.kind!=TaskKind::virtualized)throw std::runtime_error("native_task_kind");
    const auto receipt=found->second.kind==TaskKind::item?root->items:root->virtualized;
    if(!receipt||receipt->parent.lock()!=root||
        (found->second.kind==TaskKind::item?!receipt->item_scope:!receipt->virtualized_scope))throw std::runtime_error("native_item_epoch");
    return receipt;
}
Scope::Scope(Owner owner,std::uintptr_t connection):previous(scope),previous_connection(connection_scope) {
    scope=std::move(owner);connection_scope=connection;
    if(scope){std::lock_guard lock(runtime().mutex);++scope->owned_scopes;}
}
Scope::~Scope() {
    if(scope){std::lock_guard lock(runtime().mutex);--scope->owned_scopes;}
    scope=std::move(previous);connection_scope=previous_connection;
}
fs::path saved_root() {return storage_root();}
ObjectIdentity identity(std::uintptr_t pointer,std::uintptr_t expected) {
    ObjectIdentity value{pointer};const auto base=image_base();
    if(!pointer||(require_read<unsigned>(pointer+8)&0x40018010U))throw std::runtime_error("native_object");
    value.index=require_read<std::int32_t>(pointer+0xc);value.type=require_read<std::uintptr_t>(pointer+0x10);
    const auto count=require_read<std::int32_t>(base+0x753d674);
    if(!value.type||(expected&&value.type!=expected)||count<0||count>8000000||value.index<0||value.index>=count)
        throw std::runtime_error("native_object");
    const auto chunks=require_read<std::uintptr_t>(base+0x753d660);
    const auto chunk=require_read<std::uintptr_t>(chunks+static_cast<std::uintptr_t>(value.index/65536)*8);
    const auto item=chunk+static_cast<std::uintptr_t>(value.index%65536)*24;
    if(require_read<std::uintptr_t>(item)!=pointer||(require_read<unsigned>(item+8)&0x30000000U))
        throw std::runtime_error("native_object");
    value.serial=require_read<std::int32_t>(item+0x10);
    if(!value.serial) {
        // Exact FUObjectArray::AllocateSerialNumber: native atomic counter and
        // item cmpxchg, not a private serial or the loader's diagnostic helper.
        value.serial=reinterpret_cast<int(*)(std::uintptr_t,int)>(base+0x2cfc330)(base+0x753d650,value.index);
    }
    if(value.serial<=0||require_read<int>(item+0x10)!=value.serial||require_read<std::uintptr_t>(item)!=pointer)
        throw std::runtime_error("native_object_serial");
    return value;
}
bool same_identity(const ObjectIdentity& value) noexcept {
    try {const auto actual=identity(value.pointer,value.type);return actual.index==value.index&&actual.serial==value.serial;}
    catch(...){return false;}
}
namespace {
bool early() {
    const auto base=image_base();const auto count=require_read<int>(base+0x753d674);
    if(count<0||count>8000000)return false;
    const auto system=require_read<std::uintptr_t>(base+0x7447070),database=require_read<std::uintptr_t>(base+0x7445308);
    const auto chunks=require_read<std::uintptr_t>(base+0x753d660);
    for(int index=0;index<count;++index) {
        const auto chunk=require_read<std::uintptr_t>(chunks+static_cast<std::uintptr_t>(index/65536)*8);
        const auto pointer=require_read<std::uintptr_t>(chunk+static_cast<std::uintptr_t>(index%65536)*24);
        if(!pointer)continue;const auto flags=require_read<unsigned>(pointer+8);if(flags&0x10)continue;
        const auto type=require_read<std::uintptr_t>(pointer+0x10);
        if((system&&type==system)||(database&&type==database))return false;
    }
    return require_read<int>(base+0x753d674)==count;
}
void context(const Owner& owner,std::uintptr_t system,std::uintptr_t instance,std::uintptr_t world) {
    if(owner->system.pointer!=system||owner->instance.pointer!=instance||owner->world.pointer!=world||
        !same_identity(owner->system)||!same_identity(owner->instance)||!same_identity(owner->world)||!same_identity(owner->worker)||
        require_read<std::uintptr_t>(owner->worker.pointer+0x30)!=owner->native_worker||
        require_read<std::uintptr_t>(system+0x20)!=instance||require_read<std::uintptr_t>(world+0x180)!=instance)
        throw std::runtime_error("native_context_changed");
}
}
bool initialize() noexcept {
    auto& state=runtime();std::lock_guard lock(state.mutex);
    if(state.initialized)return state.armed&&!state.broken;state.initialized=true;
    try {
        if(!prepare_image()){state.issue="native_profile";return false;}
        if(!early()){state.issue="native_observer_loaded_late";return false;}
        auto hooks=task_hooks();const auto sql=sql_hooks();hooks.insert(hooks.end(),sql.begin(),sql.end());
        if(!install_hooks(hooks,file_imports())){state.issue="native_hook_installation";state.broken=true;return false;}
        if(!early()){state.issue="native_observer_loaded_late";return false;}
        state.armed=true;state.issue="none";return true;
    } catch(...){state.issue="native_initialization";state.broken=true;return false;}
}
Owner prepare_begin(std::uintptr_t system,std::uintptr_t instance,std::uintptr_t world,std::uint64_t deadline) {
    if(GetTickCount64()>deadline)throw std::runtime_error("native_admission_timeout");
    const auto native_object=reinterpret_cast<std::uintptr_t(*)(std::uintptr_t)>(image_base()+0x2d230b0)(system+0x58);
    auto owner=std::make_shared<Epoch>();owner->main_thread=GetCurrentThreadId();owner->deadline=deadline;
    owner->system=identity(system,require_read<std::uintptr_t>(image_base()+0x7447070));
    owner->instance=identity(instance);owner->world=identity(world);owner->worker=identity(native_object);
    owner->native_worker=require_read<std::uintptr_t>(native_object+0x30);
    if(require_read<std::uintptr_t>(owner->native_worker)!=image_base()+0x59a3198)throw std::runtime_error("native_worker_identity");
    auto& state=runtime();std::lock_guard lock(state.mutex);
    if(!state.armed||state.broken)throw std::runtime_error(state.issue);
    if(!code_matches())throw std::runtime_error("native_profile_changed");
    if(state.active&&!state.active->confirmed)throw std::runtime_error("native_previous_save_unsettled");
    const auto known=state.workers.find(owner->native_worker);
    if(known==state.workers.end()||!known->second)throw std::runtime_error("native_worker_unobserved");
    owner->worker_thread=known->second;context(owner,system,instance,world);return owner;
}
bool try_begin(const Owner& owner,std::uintptr_t system,std::uintptr_t instance,std::uintptr_t world) {
    if(!owner||GetCurrentThreadId()!=owner->main_thread)throw std::runtime_error("native_admission_thread");
    if(GetTickCount64()>owner->deadline)throw std::runtime_error("native_admission_timeout");
    context(owner,system,instance,world);
    if(require_read<std::uintptr_t>(owner->native_worker)!=image_base()+0x59a3198)throw std::runtime_error("native_worker_identity");
    {
        auto& state=runtime();std::lock_guard lock(state.mutex);
        if(!state.armed||state.broken)throw std::runtime_error(state.issue);
        if(!code_matches())throw std::runtime_error("native_profile_changed");
        if(state.active&&!state.active->confirmed)throw std::runtime_error("native_previous_save_unsettled");
        const auto known=state.workers.find(owner->native_worker);
        if(known==state.workers.end()||known->second!=owner->worker_thread)throw std::runtime_error("native_worker_unobserved");
    }
    // Do not block the game-thread pump that may retire preexisting B0/C0.
    // Queue/pump hooks own native locks before the observer mutex.
    struct Locks {
        std::uintptr_t worker;unsigned count{};
        explicit Locks(std::uintptr_t input):worker(input) {
            for(const unsigned at:{0x18U,0x40U,0x68U}) {
                if(!TryEnterCriticalSection(reinterpret_cast<CRITICAL_SECTION*>(worker+at)))return;
                ++count;
            }
        }
        ~Locks(){constexpr unsigned offsets[]={0x18,0x40,0x68};while(count)LeaveCriticalSection(reinterpret_cast<CRITICAL_SECTION*>(worker+offsets[--count]));}
    } native_locks(owner->native_worker);
    if(native_locks.count!=3)return false;
    auto& state=runtime();std::lock_guard lock(state.mutex);
    if(!state.armed||state.broken)throw std::runtime_error(state.issue);
    if(!code_matches())throw std::runtime_error("native_profile_changed");
    context(owner,system,instance,world);
    if(state.active&&!state.active->confirmed)throw std::runtime_error("native_previous_save_unsettled");
    const auto known=state.workers.find(owner->native_worker);
    if(known==state.workers.end()||known->second!=owner->worker_thread)throw std::runtime_error("native_worker_unobserved");
    const auto readiness=worker_readiness_locked(owner->native_worker);
    if(readiness==WorkerReadiness::stopping)throw std::runtime_error("native_worker_stopping");
    if(GetTickCount64()>owner->deadline)throw std::runtime_error("native_admission_timeout");
    if(readiness!=WorkerReadiness::ready||state.task_inflight||state.sql_inflight||state.io_inflight)return false;
    state.active=owner;return true;
}
Owner begin(std::uintptr_t system,std::uintptr_t instance,std::uintptr_t world) {
    auto owner=prepare_begin(system,instance,world,GetTickCount64()+30000);
    if(!try_begin(owner,system,instance,world))throw std::runtime_error("native_worker_busy");
    return owner;
}
void dispatch(const Owner& owner) {
    Scope entered(owner);reinterpret_cast<void(*)(std::uintptr_t,bool)>(image_base()+0x1b94040)(owner->system.pointer,true);
}
const char* observe(const Owner& owner,std::uintptr_t system,std::uintptr_t instance,std::uintptr_t world) {
    auto& state=runtime();std::lock_guard lock(state.mutex);
    try {
        context(owner,system,instance,world);
        if(!state.armed||state.broken||!code_matches())unknown(owner,"native_profile_changed");
        if(GetTickCount64()>owner->deadline&&!owner->confirmed)unknown(owner,"native_save_timeout");
        if(owner->completion.rejected())return owner->completion.failed()?"failed":"unknown";
        if(owner->confirmed)return "confirmed";
        bool sql=!owner->sql.empty()&&!owner->payload_connections.empty();
        for(const auto& [db,proof]:owner->sql){(void)db;sql=sql&&proof.closed();}
        bool items=true;
        for(const auto& item:{owner->items,owner->virtualized})if(item) {
            items=items&&(item->item_scope!=item->virtualized_scope)&&item->parent.lock()==owner&&item->completion.tasks_settled()&&
                !item->completion.rejected()&&!item->owned_scopes&&!item->io_inflight&&!item->io_failed&&
                !item->sql.empty()&&(item->virtualized_scope||!item->payload_connections.empty());
            for(const auto& [database,proof]:item->sql){(void)database;items=items&&proof.closed();}
        }
        const bool closed=!owner->owned_scopes&&!owner->io_inflight&&!state.task_inflight&&!state.sql_inflight&&!state.io_inflight;
        if(owner->completion.confirmed(closed&&sql&&items&&!owner->io_failed,true)) {owner->confirmed=true;return "confirmed";}
        return "pending";
    } catch(...){unknown(owner,"native_context_changed");return "unknown";}
}
// Caller holds the existing observer mutex, before its new SQL Flight.
// A completed receipt ends at its own native debts, not at the next Lua poll.
void settle_before_sql_locked() noexcept {
    const auto error=GetLastError();Owner owner;
    try {
        auto& state=runtime();owner=state.active;
        if(owner&&!owner->confirmed&&!owner->completion.rejected()&&owner->completion.native_settled()&&
            !scope&&!readback_depth&&GetCurrentThreadId()==owner->main_thread&&
            !state.task_inflight&&!state.sql_inflight&&!state.io_inflight)
            (void)observe(owner,owner->system.pointer,owner->instance.pointer,owner->world.pointer);
    }catch(...){unknown(owner,"native_context_changed");}
    SetLastError(error);
}
}
