#include "save_native.hpp"
#include <algorithm>
#include <stdexcept>
namespace dragon_save {
thread_local std::shared_ptr<Record> scope_record;
std::atomic<std::shared_ptr<Record>> active_record;
std::uintptr_t image_base{};
fs::path saved_root;
bool (*on_game_thread)(){};
Scope::Scope(std::shared_ptr<Record> value) : record(std::move(value)),previous(scope_record) {
    if (record) { Lock lock(record->lock); record->completion.enter_native(); }
    scope_record=record;
}
Scope::~Scope() {
    scope_record=previous;
    if (record) { Lock lock(record->lock); record->completion.leave_native(); }
}
namespace {
thread_local IdentityDiagnostic last_identity_diagnostic;
bool inspect_identity(std::uintptr_t object,Identity& result,const char*& reason,bool allow_unallocated=false,std::uintptr_t* observed_item=nullptr) noexcept {
    std::int32_t index{},count{},serial{}; std::uintptr_t chunks{},chunk{},actual{},type{};
    std::uint32_t flags{};
    // Reading never assigns a serial. Only the provider admission below may
    // request a proof of the otherwise-valid, not-yet-allocated native identity.
    if (!object) { reason="null_object"; return false; }
    if (!read(object+8,flags)) { reason="flags_read"; return false; }
    if (flags&0x40018010) { reason="flags_invalid"; return false; }
    if (!read(object+0xc,index)) { reason="index_read"; return false; }
    if (index<0) { reason="index_negative"; return false; }
    if (!read(object+0x10,type)) { reason="class_read"; return false; }
    if (!type) { reason="class_null"; return false; }
    if (!read(image_base+0xb91ed94,count)) { reason="count_read"; return false; }
    if (index>=count) { reason="index_range"; return false; }
    if (!read(image_base+0xb91ed80,chunks)) { reason="chunks_read"; return false; }
    if (!read(chunks+static_cast<unsigned>(index>>16)*8,chunk)) { reason="chunk_read"; return false; }
    const auto item=chunk+static_cast<unsigned>(index&0xffff)*24;
    if (!read(item,actual)) { reason="item_read"; return false; }
    if (actual!=object) { reason="item_changed"; return false; }
    if (!read(item+16,serial)) { reason="serial_read"; return false; }
    if (serial<0 || (!serial && !allow_unallocated)) { reason=serial==0 ? "serial_zero" : "serial_negative"; return false; }
    if (observed_item) *observed_item=item;
    result={object,type,index,serial}; return true;
}
bool admission_identity(std::uintptr_t object,Identity& result,const char* stage,const char* role) noexcept {
    const char* reason="none";
    if (inspect_identity(object,result,reason)) return true;
    last_identity_diagnostic={stage,role,reason}; return false;
}
bool provider_admission_identity(const Record& record,Identity& result) noexcept {
    const char* reason="none"; Identity prior{};std::uintptr_t prior_item{};
    const auto reject=[&](const char* issue) { last_identity_diagnostic={"admit","provider",issue};return false; };
    if(!inspect_identity(record.provider,prior,reason,true,&prior_item)) return reject(reason);
    if(prior.serial>0) { result=prior;return true; }
    // The stock provider itself constructs this weak pointer before async work.
    // Allocation is confined to actual Save admission on the verified game
    // thread. Probe, ordinary identity and later epoch checks remain read-only.
    if(!on_game_thread || !on_game_thread()) return reject("serial_thread");
    if(!profile_matches()) return reject("serial_profile");
    const auto linked=[&] {
        std::uintptr_t provider{},api{},outer{},vtable{};
        return read(record.target.persistence+0x1b8,provider) && provider==prior.object
            && read(record.target.persistence+0x1c0,api) && api==record.provider_interface
            && prior.object<=UINTPTR_MAX-0x28 && api==prior.object+0x28
            && read(prior.object+0x20,outer) && outer==record.target.persistence
            && read(api,vtable) && (vtable==image_base+0x9e27408 || vtable==image_base+0x9e270c0);
    };
    if(!linked()) return reject("serial_target");
    for(const auto& expected:record.identities) if(!same_identity(expected)) return reject("serial_target");
    std::int32_t counter{};
    if(!read(image_base+0xb91ee20,counter) || counter<1000 || counter==INT32_MAX) return reject("serial_counter");
    std::int32_t weak[2]{};const auto pointer=prior.object;
    try {
        // MSVC x64: RCX=FWeakObjectPtr*, RDX=UObject* const*. This build
        // dereferences RDX; passing the UObject pointer itself is the wrong ABI.
        reinterpret_cast<void(*)(void*,const std::uintptr_t*)>(image_base+0x1693d00)(weak,&pointer);
    } catch(...) { return reject("serial_allocate"); }
    Identity actual{};std::uintptr_t item{};
    if(!inspect_identity(prior.object,actual,reason,false,&item)) return reject(reason);
    if(weak[0]!=prior.index || weak[1]<=0 || actual.object!=prior.object || actual.type!=prior.type
        || actual.index!=prior.index || actual.serial!=weak[1] || item!=prior_item || !linked()) return reject("serial_changed");
    for(const auto& expected:record.identities) if(!same_identity(expected)) return reject("serial_target");
    result=actual;return true;
}
}
void reset_identity_diagnostic() noexcept { last_identity_diagnostic={}; }
IdentityDiagnostic identity_diagnostic() noexcept { return last_identity_diagnostic; }
bool identity(std::uintptr_t object,Identity& result) noexcept {
    const char* reason="none"; return inspect_identity(object,result,reason);
}
bool same_identity(const Identity& expected) noexcept {
    Identity actual;
    return identity(expected.object,actual) && actual.type==expected.type && actual.index==expected.index && actual.serial==expected.serial;
}
std::wstring native_string(std::uintptr_t string) {
    struct FString { std::uintptr_t data; std::int32_t size,capacity; } value{};
    if (!read(string,value) || value.size<2 || value.size>1025 || value.capacity<value.size || value.capacity>65536)
        throw std::runtime_error("native_string");
    std::wstring result(static_cast<std::size_t>(value.size),L'\0');
    if (!read_memory(value.data,result.data(),result.size()*sizeof(wchar_t)) || result.back()!=L'\0') throw std::runtime_error("native_string");
    result.pop_back();
    if (result.find(L'\0')!=result.npos) throw std::runtime_error("native_string");
    return result;
}
namespace {
using Root=void(*)(std::uintptr_t,bool,bool);
using WorldStage=void(*)(std::uintptr_t,std::uintptr_t,bool);
using PlayerQueue=void(*)(std::uintptr_t,const std::int32_t*,std::uintptr_t);
using StoreWorld=void(*)(std::uintptr_t,std::int32_t,std::uintptr_t,std::uintptr_t);
using WorldResult=void(*)(std::uintptr_t,std::int32_t,bool,std::uintptr_t,std::uintptr_t);
using PlayerResult=void(*)(std::uintptr_t,std::int32_t,bool,std::uintptr_t);
using Aggregate=void(*)(std::uintptr_t,std::uintptr_t);
using Slot=bool(*)(std::uintptr_t,std::uintptr_t);
Root original_root{};
WorldStage original_world_stage{};
PlayerQueue original_player_queue{};
StoreWorld original_local_world{},original_null_world{};
WorldResult original_world_result{};
PlayerResult original_player_result{};
Aggregate original_aggregate{};
Slot original_slot{};
std::atomic<unsigned> foreign_roots{};
thread_local bool authorized_root{};
template<class T> T value(std::uintptr_t address) { T result{}; if (!read(address,result)) throw std::runtime_error("native_memory"); return result; }
void mark_unknown(const std::shared_ptr<Record>& record) noexcept { if (record) { Lock lock(record->lock); record->completion.unknown(); } }
bool owns(const std::shared_ptr<Record>& record,std::uintptr_t persistence) noexcept { return record && record->target.persistence==persistence; }
std::shared_ptr<Record> for_persistence(std::uintptr_t persistence) noexcept {
    const auto record=active_record.load(); return owns(record,persistence) ? record : nullptr;
}
bool player_handle(const std::shared_ptr<Record>& record,std::int32_t handle) noexcept {
    if (!record) return false;
    Lock lock(record->lock);
    for (std::size_t index=0;index<record->completion.player_count;++index)
        if (record->completion.players[index].handle==handle) return true;
    return false;
}
void hook_root(std::uintptr_t persistence,bool logging,bool force) {
    const auto record=for_persistence(persistence);
    if (!record || !authorized_root || scope_record!=record) {
        if (record) mark_unknown(record);
        foreign_roots.fetch_add(1);
        try { original_root(persistence,logging,force); } catch (...) { foreign_roots.fetch_sub(1); throw; }
        foreign_roots.fetch_sub(1); return;
    }
    authorized_root=false;
    { Lock lock(record->lock); record->completion.enter_root(); }
    try { original_root(persistence,logging,force); }
    catch (...) { mark_unknown(record); throw; }
    const auto expected=value<std::int32_t>(persistence+0x2dc);
    { Lock lock(record->lock); record->completion.leave_root(expected<0 ? SIZE_MAX : static_cast<std::size_t>(expected)); }
}
void hook_world_stage(std::uintptr_t persistence,std::uintptr_t slot,bool overwrite) {
    auto record=scope_record;
    if (!owns(record,persistence)) record.reset();
    if (record) {
        try {
            if (overwrite || native_string(slot)!=record->slot || !stable(record)) throw std::runtime_error("native_world_stage");
            Lock lock(record->lock); record->completion.begin_world_stage();
        } catch (...) { mark_unknown(record); }
    }
    Scope scope(record); original_world_stage(persistence,slot,overwrite);
}
void hook_player_queue(std::uintptr_t persistence,const std::int32_t* handle,std::uintptr_t payload) {
    const auto record=scope_record;
    if (owns(record,persistence)) {
        std::int32_t id{};
        Lock lock(record->lock);
        if (!read(reinterpret_cast<std::uintptr_t>(handle),id)) record->completion.unknown();
        else record->completion.player(id);
    }
    original_player_queue(persistence,handle,payload);
}
void store_world(StoreWorld original,std::uintptr_t provider,std::int32_t handle,std::uintptr_t slot,std::uintptr_t source) {
    const auto record=scope_record;
    if (record) {
        try {
            if (provider!=record->provider_interface || native_string(slot)!=record->slot || !stable(record)) throw std::runtime_error("native_store");
            Lock lock(record->lock); record->completion.world_handle(handle);
        } catch (...) { mark_unknown(record); }
    }
    Scope scope(record); original(provider,handle,slot,source);
}
void hook_local_world(std::uintptr_t provider,std::int32_t handle,std::uintptr_t slot,std::uintptr_t source) {
    store_world(original_local_world,provider,handle,slot,source);
}
void hook_null_world(std::uintptr_t provider,std::int32_t handle,std::uintptr_t slot,std::uintptr_t source) {
    store_world(original_null_world,provider,handle,slot,source);
}
void hook_world_result(std::uintptr_t persistence,std::int32_t handle,bool success,std::uintptr_t slot,std::uintptr_t source) {
    auto record=for_persistence(persistence);
    if (record) {
        bool matches{};
        { Lock lock(record->lock); matches=record->completion.world.present && record->completion.world.handle==handle; }
        if (!matches) { mark_unknown(record); record.reset(); }
    }
    Scope scope(record);
    if (record) {
        try {
            if (native_string(slot)!=record->slot || !stable(record)) throw std::runtime_error("native_result");
            Lock lock(record->lock); record->completion.result(Kind::world,handle,success);
        } catch (...) { mark_unknown(record); }
    }
    original_world_result(persistence,handle,success,slot,source);
}
void hook_player_result(std::uintptr_t persistence,std::int32_t handle,bool success,std::uintptr_t payload) {
    auto record=for_persistence(persistence);
    if (record && !player_handle(record,handle)) { mark_unknown(record); record.reset(); }
    Scope scope(record);
    if (record) {
        if (!stable(record)) mark_unknown(record);
        Lock lock(record->lock); record->completion.result(Kind::player,handle,success);
    }
    original_player_result(persistence,handle,success,payload);
}
void hook_aggregate(std::uintptr_t context,std::uintptr_t results) {
    std::uintptr_t persistence{}; read(context,persistence);
    const auto record=for_persistence(persistence); Scope scope(record);
    if (record) {
        struct Array { std::uintptr_t data; std::int32_t count,capacity; } array{};
        std::array<unsigned char,257> values{}; bool valid=read(results,array) && array.count>=1
            && array.count<=257 && array.capacity>=array.count && array.capacity<=1024
            && read_memory(array.data,values.data(),static_cast<std::size_t>(array.count));
        bool all=true;
        if (valid) for (std::int32_t index=0;index<array.count;++index) {
            if (values[static_cast<std::size_t>(index)]>1) valid=false;
            all=all && values[static_cast<std::size_t>(index)]==1;
        }
        Lock lock(record->lock);
        if (!valid) record->completion.unknown();
        record->completion.enter_aggregate(valid ? static_cast<std::size_t>(array.count) : 0,all);
    }
    original_aggregate(context,results);
    if (record) { Lock lock(record->lock); record->completion.leave_aggregate(); }
}
bool hook_slot(std::uintptr_t slot,std::uintptr_t state) {
    auto record=scope_record;
    if (record) {
        try {
            // This freshly constructed state is consumed synchronously by
            // SaveToSlot. It has not necessarily acquired a UE weak serial.
            // Its exact class/vptr and owned native call stack supply identity;
            // no pointer to it survives this hook's original call.
            std::uintptr_t type{},vtable{},expected_type{};
            if (native_string(slot)!=record->slot || !read(state+0x10,type) || !read(state,vtable) || !read(image_base+0xbb0f520,expected_type)
                || type!=expected_type || vtable!=image_base+0x9a36d78) throw std::runtime_error("native_slot");
            record->version_backup=record->published.native()+L"."+std::to_wstring(value<std::int32_t>(state+0x348))+L".verbackup";
            Lock lock(record->lock); record->completion.enter_slot();
        } catch (...) { mark_unknown(record); }
    }
    Scope scope(record); const auto success=original_slot(slot,state);
    if (record) { Lock lock(record->lock); record->completion.leave_slot(success); }
    return success;
}
constexpr unsigned char root_unwind[]={2,5,4,0,5,0x70,4,0x60,3,0x30,2,0x50};
constexpr unsigned char save_rbp_24[]={2,5,2,0,5,0x54,3,0};
constexpr unsigned char save_rbx_16[]={2,5,2,0,5,0x34,2,0};
constexpr unsigned char save_r9_32[]={2,5,0,0};
constexpr unsigned char allocate_38[]={2,9,1,0,4,0x62,0,0};
constexpr unsigned char player_unwind[]={2,6,4,0,6,0xe0,4,0x70,3,0x30,2,0x50};
std::vector<Hook> root_hooks() {
    return {
        {0x5d98ba0,5,"4055535657",reinterpret_cast<void*>(&hook_root),reinterpret_cast<void**>(&original_root),root_unwind},
        {0x5d9a9d0,5,"48896c2418",reinterpret_cast<void*>(&hook_world_stage),reinterpret_cast<void**>(&original_world_stage),save_rbp_24},
        {0x5d99a60,6,"405553574156",reinterpret_cast<void*>(&hook_player_queue),reinterpret_cast<void**>(&original_player_queue),player_unwind},
        {0x5d9a810,9,"4883ec384c894c2420",reinterpret_cast<void*>(&hook_local_world),reinterpret_cast<void**>(&original_local_world),allocate_38},
        {0x5d9a830,5,"48895c2410",reinterpret_cast<void*>(&hook_null_world),reinterpret_cast<void**>(&original_null_world),save_rbx_16},
        {0x5d90c70,5,"48895c2410",reinterpret_cast<void*>(&hook_world_result),reinterpret_cast<void**>(&original_world_result),save_rbx_16},
        {0x5d902a0,5,"4c894c2420",reinterpret_cast<void*>(&hook_player_result),reinterpret_cast<void**>(&original_player_result),save_r9_32},
        {0x5d77190,5,"48895c2410",reinterpret_cast<void*>(&hook_aggregate),reinterpret_cast<void**>(&original_aggregate),save_rbx_16},
        {0x5483680,5,"48895c2410",reinterpret_cast<void*>(&hook_slot),reinterpret_cast<void**>(&original_slot),save_rbx_16}
    };
}
}
bool stable(const std::shared_ptr<Record>& record) noexcept {
    if (!record || !on_game_thread || !on_game_thread() || !profile_matches()) return false;
    for (const auto& object : record->identities) if (!same_identity(object)) return false;
    if (!same_identity(record->provider_identity)) return false;
    std::uintptr_t instance{},context{},world{},owner{},provider{},provider_interface{};
    return read(record->target.persistence+0x20,instance) && instance==record->target.instance
        && read(instance+0x30,context) && read(context+0x2c0,world) && world==record->target.world
        && read(world+0x228,owner) && owner==instance
        && read(record->target.persistence+0x1b8,provider) && provider==record->provider
        && read(record->target.persistence+0x1c0,provider_interface) && provider_interface==record->provider_interface;
}
const char* probe_target(const Target& target) noexcept {
    try {
        if (!on_game_thread || !on_game_thread() || !profile_matches()) return "native_profile_unavailable";
        auto active=active_record.load();
        if (active) {
            refresh(active); Lock lock(active->lock);
            if (!active->completion.quiescent()) return "save_busy";
        }
        if (foreign_roots.load()) return "save_busy";
        const std::uintptr_t objects[]={target.persistence,target.spud,target.instance,target.world};
        constexpr const char* roles[]={"persistence","spud","instance","world"};
        for (std::size_t index=0;index<4;++index) { Identity proof; if (!admission_identity(objects[index],proof,"probe",roles[index])) return "native_identity_unavailable"; }
        if (value<std::uintptr_t>(target.persistence)!=image_base+0x9e25af8
            || value<std::uintptr_t>(target.persistence+0x10)!=value<std::uintptr_t>(image_base+0xbb33a80)
            || value<std::uintptr_t>(target.spud)!=image_base+0x9a3e260
            || value<std::uintptr_t>(target.spud+0x10)!=value<std::uintptr_t>(image_base+0xbb0f828)
            || value<std::uintptr_t>(target.persistence+0x20)!=target.instance
            || value<std::uintptr_t>(target.spud+0x20)!=target.instance
            || value<std::uintptr_t>(target.world+0x228)!=target.instance) return "native_target_changed";
        const auto context=value<std::uintptr_t>(target.instance+0x30);
        if (value<std::uintptr_t>(context+0x2c0)!=target.world || !value<std::uintptr_t>(target.world+0x1a8)) return "native_target_changed";
        if (value<unsigned char>(target.persistence+0x2b0) || value<unsigned char>(target.persistence+0x4c9)
            || value<unsigned char>(target.persistence+0x520) || value<unsigned char>(target.spud+0x340)!=1
            || value<std::int32_t>(target.persistence+0x518)!=0) return "save_busy";
        const auto provider=value<std::uintptr_t>(target.persistence+0x1b8),api=value<std::uintptr_t>(target.persistence+0x1c0);
        const auto vtable=value<std::uintptr_t>(api);
        if (!provider || api!=provider+0x28 || (vtable!=image_base+0x9e27408 && vtable!=image_base+0x9e270c0)) return "native_provider_unsupported";
        return nullptr;
    } catch (...) { return "native_target_unavailable"; }
}
std::shared_ptr<Record> admit(const Target& target,std::string nonce,const char*& rejection) {
    rejection=probe_target(target); if (rejection) return nullptr;
    auto record=std::make_shared<Record>(GetTickCount64()); record->target=target; record->nonce=std::move(nonce);
    const std::uintptr_t objects[]={target.persistence,target.spud,target.instance,target.world};
    constexpr const char* roles[]={"persistence","spud","instance","world"};
    for (std::size_t index=0;index<4;++index) if (!admission_identity(objects[index],record->identities[index],"admit",roles[index])) { rejection="native_identity_unavailable"; return nullptr; }
    record->provider=value<std::uintptr_t>(target.persistence+0x1b8); record->provider_interface=value<std::uintptr_t>(target.persistence+0x1c0);
    if (!provider_admission_identity(*record,record->provider_identity)) { rejection="native_identity_unavailable"; return nullptr; }
    record->local_player_provider=value<std::uintptr_t>(record->provider_interface)==image_base+0x9e270c0;
    record->slot=native_string(target.persistence+0x1f0);
    if (record->slot.size()>128 || record->slot.find_first_of(L"/\\:.\r\n\t\"*?<>|")!=record->slot.npos) { rejection="native_slot_unavailable"; return nullptr; }
    record->published=saved_root/L"SaveGames"/(record->slot+L".sav");
    record->incomplete=record->published.native()+L".incomplete"; record->backup=record->published.native()+L".backup";
    record->initial_counter=value<std::int32_t>(image_base+0xbb548c8);
    active_record.store(record); return record;
}
void dispatch(const std::shared_ptr<Record>& record) {
    if (!stable(record)) { mark_unknown(record); return; }
    Scope scope(record); authorized_root=true;
    try { reinterpret_cast<Root>(image_base+0x5d98ba0)(record->target.persistence,false,false); }
    catch (...) { authorized_root=false; mark_unknown(record); throw; }
    authorized_root=false;
}
void refresh(const std::shared_ptr<Record>& record) noexcept {
    if (!record) return;
    const bool valid=stable(record);
    Lock lock(record->lock); record->completion.tick(GetTickCount64());
    if (!valid) record->completion.unknown(Issue::identity_changed);
    if (record->completion.quiescent()) {
        for (auto& writer : record->writers) writer.parents.clear();
        record->closed_world.reset();
    }
}
bool initialize_native(std::uintptr_t image,const fs::path& saved,bool (*game_thread)()) noexcept {
    try {
        image_base=image; saved_root=full_path(saved); on_game_thread=game_thread;
        auto hooks=root_hooks();
        const auto sinks=sink_hooks(); hooks.insert(hooks.end(),sinks.begin(),sinks.end());
        return prepare_profile(image,hooks) && install_file_imports() && profile_matches();
    } catch (...) { return false; }
}
#ifdef LGSM_DRAGON_SAVE_TESTS
std::vector<Hook> test_native_hooks() {
    auto result=root_hooks(); const auto files=sink_hooks(); result.insert(result.end(),files.begin(),files.end()); return result;
}
#endif
}
