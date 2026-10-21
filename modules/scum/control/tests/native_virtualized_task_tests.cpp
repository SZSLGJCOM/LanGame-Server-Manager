// Actual AutoSave(empty registered sets), three stages, native producer/queue,
// worker/batch/C0/pump, native SQL writers and disk readback. Only UE object/
// cache lookup, allocator and event boundaries are constructed by this fixture.
#include "native_queue_fixture.hpp"
#include <cstring>
#include <tuple>
#include <climits>
namespace {
alignas(16) unsigned char system_object[0x800]{},instance[0x500]{},world[0x600]{},connection_object[0xc8]{};
alignas(16) unsigned char entity[0x280]{},global_cache[0x70]{},item_cache[0x160]{};
alignas(16) unsigned char root_type[64]{},other_type[64]{},instance_type[64]{},manager_type[64]{},level_type[64]{},object_items[9*24]{};
alignas(16) unsigned char manager[0x580]{},level[0xc0]{},entity_system[0x60]{},foreign_worker[0x40]{},foreign_world[0x600]{};
uintptr_t level_bases[1]{};
uintptr_t object_chunks[1]{},instance_bases[1]{},root_descriptor[4]{},item_descriptor[4]{},entity_table[64]{};
DB* database{};std::vector<ST*> statements;
HANDLE worker_ready{},owned_entered{},allow_owned{},first_wake{},two_drains{};
bool delay_owned{},fail_aux{};unsigned cache_calls{},native_logs{};std::atomic<unsigned> waits{};
uintptr_t(*cache_original)(uintptr_t,uintptr_t){};
uintptr_t(*connection_original)(uintptr_t){};
void put(uintptr_t at,const auto& value){protect_copy(at,&value,sizeof value);}
void* fixture_world(void*){return world;}
uintptr_t fixture_instance_class(){return (uintptr_t)instance_type;}
void event_signal(Event* self){SetEvent(self->ready);}
bool mixed_wait(Event* self,unsigned timeout,bool) {
 const auto entered=++waits;if(entered>=3)SetEvent(two_drains);
 const auto result=WaitForSingleObject(self->ready,timeout)==WAIT_OBJECT_0;
 if(entered==1&&result)SetEvent(first_wake);return result;
}
void notify(void*){}
void error_log(uintptr_t,int){++native_logs;}
uintptr_t connection_lookup(uintptr_t value) {
 require(value==(uintptr_t)instance,"native_worker_instance_boundary");
 {std::lock_guard lock(scum_save::runtime().mutex);if(scum_save::runtime().databases.contains(database))return (uintptr_t)connection_object;}
 put((uintptr_t)connection_object+0x48,GetCurrentThreadId());
 scum_save::Database entry;entry.object=(uintptr_t)connection_object;entry.identity=scum_save::identity(entry.object);
 entry.thread=GetCurrentThreadId();entry.pins=scum_save::pin_parent(allowed_root/L"mixed.db");
 entry.pins.emplace_back(CreateFileW((allowed_root/L"mixed.db").c_str(),GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,
  nullptr,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
 require(entry.pins.back().value!=INVALID_HANDLE_VALUE,"native_worker_connection_pin");
 {std::lock_guard lock(scum_save::runtime().mutex);require(scum_save::runtime().databases.emplace(database,std::move(entry)).second,"connection_not_reused");}
 SetEvent(worker_ready);return (uintptr_t)connection_object;
}
uintptr_t cache_lookup(uintptr_t object,uintptr_t descriptor) {
 const auto error=GetLastError();require(object==(uintptr_t)connection_object&&GetCurrentThreadId()==worker_thread,"cache_boundary_thread_identity");++cache_calls;
 if(descriptor==(uintptr_t)root_descriptor) {
  if(delay_owned){SetEvent(owned_entered);require(WaitForSingleObject(allow_owned,5000)==WAIT_OBJECT_0,"owned_body_gate");}
  SetLastError(error);return (uintptr_t)global_cache;
 }
 require(descriptor==(uintptr_t)item_descriptor,"cache_descriptor_identity");if(fail_aux)fail_write=true;
 SetLastError(error);return (uintptr_t)item_cache;
}
ST* prepare(const char* sql_text) {
 ST* value{};const char* tail{};
 require(api<int(*)(DB*,const char*,int,ST**,const char**)>(116)(database,sql_text,-1,&value,&tail)==0&&value,"real_cached_statement_prepare");
 statements.push_back(value);return value;
}
void statement(unsigned char* cache,unsigned offset,ST* value) {
 put((uintptr_t)cache+offset,(uintptr_t)connection_object);put((uintptr_t)cache+offset+8,value);
}
void native_objects() {
 object_chunks[0]=(uintptr_t)object_items;put(mapped+0x753d660,(uintptr_t)object_chunks);put(mapped+0x753d674,9);
 put(mapped+0x7447070,(uintptr_t)root_type);put(mapped+0x7445308,(uintptr_t)other_type);
 const uintptr_t objects[]={(uintptr_t)system_object,(uintptr_t)worker,(uintptr_t)connection_object,(uintptr_t)instance,(uintptr_t)world,(uintptr_t)manager,(uintptr_t)level,(uintptr_t)foreign_worker,(uintptr_t)foreign_world};
 for(int index=0;index<9;++index) {
  put(objects[index]+0xc,index);put(objects[index]+0x10,index==0?(uintptr_t)root_type:index==3?(uintptr_t)instance_type:index==5?(uintptr_t)manager_type:index==6?(uintptr_t)level_type:(uintptr_t)other_type);
  put((uintptr_t)object_items+index*24,objects[index]);put((uintptr_t)object_items+index*24+16,101+index);
 }
 put((uintptr_t)system_object+0x20,(uintptr_t)instance);system_object[0x30]=1;
 put((uintptr_t)system_object+0x58,1);put((uintptr_t)system_object+0x5c,102);
 put((uintptr_t)system_object+0x50,(std::int64_t)123456);put((uintptr_t)world+0x180,(uintptr_t)instance);
 put((uintptr_t)entity+0x58,0);put((uintptr_t)entity+0x5c,101);put((uintptr_t)entity+0xd8,(std::int64_t)77);
 entity_table[0x150/8]=(uintptr_t)notify;put((uintptr_t)entity,(uintptr_t)entity_table);
 instance_bases[0]=(uintptr_t)instance_type+0x30;put((uintptr_t)instance_type+0x30,(uintptr_t)instance_bases);put((uintptr_t)instance_type+0x38,0);
 put(mapped+0x744b648,(uintptr_t)manager_type);put((uintptr_t)manager,mapped+0x5a68b00);
 put((uintptr_t)manager+0x20,(uintptr_t)level);put((uintptr_t)level+0xb8,(uintptr_t)world);
 put((uintptr_t)manager+0x520,1);put((uintptr_t)manager+0x524,102);
 put((uintptr_t)foreign_worker+0x30,(uintptr_t)runtime);put((uintptr_t)foreign_world+0x180,(uintptr_t)instance);
 level_bases[0]=(uintptr_t)level_type+0x30;put((uintptr_t)level_type+0x30,(uintptr_t)level_bases);put((uintptr_t)level_type+0x38,0);
 put((uintptr_t)instance+0x410,(uintptr_t)entity_system);entity_system[0x30]=1;put((uintptr_t)entity_system+0x50,(std::int64_t)123456);
 root_descriptor[0]=mapped+0x59dde98;item_descriptor[0]=mapped+0x59d8488;
 put(mapped+0x742a330,(uintptr_t)root_descriptor);put(mapped+0x742a1d0,(uintptr_t)item_descriptor);
}
void publish_items() {
 ((void(*)(void*,bool))(mapped+0x1bc99e0))(entity,true);
 ((void(*)(void*,float))(mapped+0x1bcbd30))(entity,1.25F);
}
template<class T>void item_proofs(const std::shared_ptr<T>& owner) {
 if constexpr(requires {owner->publications;owner->items;}) {
  check(owner->publications.size()==3&&owner->items&&owner->items->completion.tasks_settled(),"distinct_item_sequence_receipt_closed");
  check(owner->items->sql.at(database).closed()&&cache_calls==3,"item_actual_writers_and_full_page_readback_closed");
 }else check(false,"distinct_item_sequence_receipt_missing");
}

template<class T>void item_family_proofs(const std::shared_ptr<T>& owner) {
 if constexpr(requires {owner->publications;}) {
  using Binding=typename std::remove_reference_t<decltype(owner->publications)>::mapped_type;
  struct Storage {uintptr_t table{},pad{},descriptor{},pad2{};Function wrapped;};
  struct WrappedStorage {uintptr_t table{},pad{};Function body;};
  constexpr unsigned family[][3]={
   {0x1b9990e,0x1b97650,0x51e8bd8},{0x1bc8ed4,0x1b978f0,0x5196950},
   {0x1bc8f58,0x1b97340,0x5196950},{0x1bc9a62,0x1b97290,0x5196950},
   {0x1bcabe2,0x1b98290,0x5196950},{0x1bcaee5,0x1b97ff0,0x59df8d8},
   {0x1bcbdcb,0x1b97750,0x5196950},{0x1bcc46a,0x1b981a0,0x5196950},
   {0x1bcc55f,0x1b98300,0x5196950},{0x1bcc917,0x1b973b0,0x58c9660},
   {0x1bcf020,0x1b97b10,0x5196950},{0x1bd09b3,0x1b97750,0x5196950}};
  for(const auto& row:family) {
   WrappedStorage wrapped{mapped+0x58c4b08,0,Function{mapped+row[1]}};
   put((uintptr_t)wrapped.body.inlined,mapped+row[2]);
   Storage storage{mapped+0x571e488,0,(uintptr_t)item_descriptor,0,Function{mapped+0x1b5bca0}};
   storage.wrapped.storage=(uintptr_t)&wrapped;Function outer{mapped+0x1b976c0};outer.storage=(uintptr_t)&storage;
   Binding binding;binding.kind=decltype(binding.kind)::item;binding.producer=row[0];binding.writer=row[1];binding.writer_table=row[2];
   validate_item_function((uintptr_t)&outer,binding);check(true,"fixed_item_family_nested_callable_metadata");
   const auto original=wrapped.body.invoke;wrapped.body.invoke=mapped+0x1b94040;
   bool rejected{};try{validate_item_function((uintptr_t)&outer,binding);}catch(...){rejected=true;}
   check(rejected,"changed_item_writer_rejected");wrapped.body.invoke=original;
  }
 }
}
// Only external world-coordinate / spatial-index / ULevel metadata boundaries.
uintptr_t fixture_level_class(){return (uintptr_t)level_type;}
uintptr_t coordinates(uintptr_t output,uintptr_t actor,uintptr_t input) {
 require(actor==(uintptr_t)manager,"coordinate_actor_boundary");std::memcpy((void*)output,(void*)input,12);return output;
}
void spatial_candidates(void*,void*,void* output) {std::memset(output,0,16);}
void publish_virtualized() {
 float bounds[6]={-10.0F,-10.0F,0.0F,10.0F,10.0F,0.0F};
 ((void(*)(void*,void*))(mapped+0x1e32980))(manager,bounds);
}
std::int64_t source_entity_id=77;
void* fixture_entity_id(void*) {return &source_entity_id;}
void* fixture_missing_actor(void*) {return nullptr;}
void* fixture_serialization(void*,void* output) {std::memset(output,0,16);return output;}
void publish_readonly_virtualized() {
 alignas(16) unsigned char data[0x80]{},outer_control[16]{},inner_control[16]{};
 uintptr_t weak[2]={(uintptr_t)data,(uintptr_t)outer_control};
 put((uintptr_t)outer_control+8,3);put((uintptr_t)outer_control+12,3);
 put((uintptr_t)inner_control+8,3);put((uintptr_t)inner_control+12,3);
 put((uintptr_t)data+8,(uintptr_t)entity);put((uintptr_t)data+16,(uintptr_t)inner_control);
 ((void(*)(void*,void*))(mapped+0x1e25ad0))(manager,weak);
}
HANDLE auxiliary_write_entered{},allow_auxiliary_write{};bool gate_auxiliary{},fault_auxiliary{};
BOOL WINAPI gated_write(HANDLE handle,LPCVOID data,DWORD count,LPDWORD written,LPOVERLAPPED overlapped) {
#ifndef LGSM_SCUM_BASELINE
 const auto receipt=scum_save::admitted_scope();
 if(receipt&&receipt->virtualized_scope) {
  if(gate_auxiliary){SetEvent(auxiliary_write_entered);require(WaitForSingleObject(allow_auxiliary_write,5000)==WAIT_OBJECT_0,"auxiliary_os_gate");}
  if(fault_auxiliary){if(written)*written=0;SetLastError(ERROR_DISK_FULL);return FALSE;}
 }
#endif
 return sqlite_write(handle,data,count,written,overlapped);
}
void pump_and_wait() {
 ((void(*)(void*))(mapped+0x1ab9df0))(runtime);
 const auto start=GetTickCount64();
 for(;;){{std::lock_guard lock(scum_save::runtime().mutex);if(!scum_save::runtime().task_inflight)break;}
  require(GetTickCount64()-start<5000,"native_batch_terminal");SwitchToThread();}
}
#ifndef LGSM_SCUM_BASELINE
void deep_clone_proofs() {
 for(const auto [producer,writer,table,bytes]:{std::tuple{0x1e25f1du,0x1e28e30u,0x5a73b78u,88u},
      std::tuple{0x1e32d14u,0x1e28db0u,0x56c6b10u,40u},std::tuple{0x1e45225u,0x1e28cb0u,0x51e8bd8u,8u}}) {
  auto* original=(unsigned char*)_aligned_malloc(bytes+8,16);require(original,"clone_source");std::memset(original,0,bytes+8);
  put((uintptr_t)original,mapped+table);put((uintptr_t)original+8,(std::int64_t)77);
  wchar_t name[]=L"finite-item";unsigned char userdata[]={4,3,2,1};
  if(bytes==88) {
   put((uintptr_t)original+16,(uintptr_t)name);put((uintptr_t)original+24,12);put((uintptr_t)original+28,12);
   put((uintptr_t)original+8+0x28,(uintptr_t)userdata);put((uintptr_t)original+8+0x30,4);put((uintptr_t)original+8+0x34,4);
   original[8+0x50]=1;put((uintptr_t)original+8+0x38,1.25F);put((uintptr_t)original+8+0x40,(std::int64_t)9);original[8+0x48]=1;
  }
  Function source{mapped+writer};source.storage=(uintptr_t)original;
  Function cloned{mapped+writer};((void(*)(void*,void**))(*(uintptr_t*)(mapped+table)))(original,(void**)&cloned.storage);
  auto* copy=cloned.storage?(void*)cloned.storage:(void*)cloned.inlined;require(copy!=original,"actual_native_clone_storage");
  auto a=scum_save::virtualized_input((uintptr_t)&source,(uintptr_t)manager,producer);
  auto b=scum_save::virtualized_input((uintptr_t)&cloned,(uintptr_t)manager,producer);
  check(a.payload_hash==b.payload_hash,"actual_native_clone_logical_equivalence");
  if(bytes==88)check(*(uintptr_t*)((unsigned char*)copy+16)!=(uintptr_t)name&&*(uintptr_t*)((unsigned char*)copy+8+0x28)!=(uintptr_t)userdata,"both_native_dynamic_buffers_are_deep_copied");
  if(bytes==88) {
   for(const unsigned offset:{8u,0x28u}) {
    auto* bytes_copy=*(unsigned char**)((unsigned char*)copy+8+offset);bytes_copy[0]^=1;
    check(scum_save::virtualized_input((uintptr_t)&cloned,(uintptr_t)manager,producer).payload_hash!=a.payload_hash,"dynamic_content_mutation_rejected_by_logical_hash");bytes_copy[0]^=1;
   }
  }
  ((unsigned char*)copy)[8]^=1;
  check(scum_save::virtualized_input((uintptr_t)&cloned,(uintptr_t)manager,producer).payload_hash!=a.payload_hash,"changed_payload_value_cannot_match");
  ((unsigned char*)copy)[8]^=1;
  ((void(*)(void*))(*(uintptr_t*)(mapped+table+16)))(copy);_aligned_free(original);
 }
}
#endif
}
int wmain(int argc,wchar_t** argv){try {
 require(argc==3,"arguments");const auto scratch=fixture_root(argv[1]);const std::wstring mode=argv[2];
 const bool late=mode==L"late_aux",missing_root=mode==L"missing_root",missing_aux=mode==L"missing_aux";
 const bool readonly_aux=mode==L"readonly_aux";
 const bool after_foreign=mode==L"foreign_sql",pending_foreign=mode==L"foreign_sql_pending";
 const bool healthy=mode==L"mixed"||late||readonly_aux||after_foreign;fault_auxiliary=mode==L"os_failure";
 require(healthy||missing_root||missing_aux||mode==L"payload"||mode==L"world"||mode==L"worker"||mode==L"replay"||mode==L"unknown"||mode==L"os_failure"||mode==L"table"||pending_foreign||mode==L"root_no_write"||mode==L"rollback","mode");
 auto input=scratch/L"image/SCUMServer.exe";auto dirs=pin_directories(input.parent_path());auto image=open_file(input,GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ);
 check(hash_file(image.value)=="4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad","fixed_physical_copy");
 allowed_root=component_directory(scratch,L"virtualized-native-fixture");auto rootpins=pin_directories(allowed_root);
 SetEnvironmentVariableW(L"TEMP",allowed_root.c_str());SetEnvironmentVariableW(L"TMP",allowed_root.c_str());
 mapped=(uintptr_t)LoadLibraryExW(input.c_str(),nullptr,DONT_RESOLVE_DLL_REFERENCES);require(mapped,"copied_image_mapping");SetUnhandledExceptionFilter(fault);imports(mapped);sqlite_guards();
 put(mapped+0x513c758,(uintptr_t)gated_write);
 check(scum_save::test_profile_copy(input,mapped),"actual_complete_native_profile");scum_save::test_saved_root(allowed_root);
 auto hooks=scum_save::task_hooks();auto sqlhooks=scum_save::sql_hooks();hooks.insert(hooks.end(),sqlhooks.begin(),sqlhooks.end());
 hooks.push_back({0x1ac4d30,(void*)cache_lookup,(void**)&cache_original,"4889542410",5,{}});
 hooks.push_back({0x1a4c3e0,(void*)connection_lookup,(void**)&connection_original,"48895c2410",5,{}});
 auto io=scum_save::file_imports();for(const auto& slot:io)scum_save::test_guarded_import(slot.rva,*(void**)(mapped+slot.rva));
 check(scum_save::install_hooks(hooks,io),"product_hooks_and_explicit_ue_boundaries");
 jump(mapped+0x26ead80,(uintptr_t)fixture_instance_class);jump(mapped+0x466d2d0,(uintptr_t)fixture_level_class);
 jump(mapped+0x4162000,(uintptr_t)coordinates);jump(mapped+0x1947d20,(uintptr_t)spatial_candidates);
 jump(mapped+0x1bb84d0,(uintptr_t)fixture_entity_id);jump(mapped+0x197e160,(uintptr_t)fixture_missing_actor);jump(mapped+0x1f32160,(uintptr_t)fixture_serialization);
 jump(mapped+0x1afa560,(uintptr_t)error_log);
 // Keep native spatial TLS initialization in its already initialized branch;
 // no source producer/queue/callback/SQL instruction is replaced.
 put(mapped+0x742c8f0,INT_MIN);
 check(((int(*)())(mapped+0x298d5e0))()==0,"actual_native_sqlite_init");
 allocator_table[2]=(uintptr_t)allocate;allocator_table[4]=(uintptr_t)resize;allocator_table[6]=(uintptr_t)release;allocator_table[7]=(uintptr_t)quantize;
 allocator=allocator_table;put(mapped+0x74b8660,(uintptr_t)&allocator);native_objects();
 event_table[2]=(uintptr_t)event_signal;event_table[4]=(uintptr_t)mixed_wait;
 event={event_table,CreateEventW(nullptr,FALSE,FALSE,nullptr)};worker_ready=CreateEventW(nullptr,TRUE,FALSE,nullptr);
 owned_entered=CreateEventW(nullptr,TRUE,FALSE,nullptr);allow_owned=CreateEventW(nullptr,TRUE,FALSE,nullptr);
 first_wake=CreateEventW(nullptr,TRUE,FALSE,nullptr);two_drains=CreateEventW(nullptr,TRUE,FALSE,nullptr);
 auxiliary_write_entered=CreateEventW(nullptr,TRUE,FALSE,nullptr);allow_auxiliary_write=CreateEventW(nullptr,TRUE,FALSE,nullptr);
 require(event.ready&&worker_ready&&first_wake&&two_drains&&auxiliary_write_entered&&allow_auxiliary_write,"host_events");
 worker_table[0x160/8]=(uintptr_t)fixture_world;put((uintptr_t)worker,(uintptr_t)worker_table);put((uintptr_t)worker+0x30,(uintptr_t)runtime);
 put((uintptr_t)runtime,mapped+0x59a3198);put((uintptr_t)runtime+8,(uintptr_t)worker);put((uintptr_t)runtime+0x10,(uintptr_t)&event);
 for(const unsigned offset:{0x18U,0x40U,0x68U})InitializeCriticalSection((CRITICAL_SECTION*)(runtime+offset));put((uintptr_t)runtime+0x120,1);
 scum_save::opening_connection=(uintptr_t)connection_object;
 check(open(allowed_root/L"mixed.db",&database,6)==0&&database,"actual_native_database");scum_save::opening_connection=0;
 check(sql(database,"PRAGMA temp_store=MEMORY; CREATE TABLE owned(context INTEGER,ticks REAL); CREATE TABLE item_bool(id INTEGER,value INTEGER); CREATE TABLE item_float(id INTEGER,value REAL); CREATE TABLE entity(id INTEGER,entity_system_id INTEGER,location_x REAL,location_y REAL); CREATE TABLE virtualized_item(item_entity_id INTEGER,queued_visit_time INTEGER); INSERT INTO entity VALUES(1,123456,0,0); INSERT INTO virtualized_item VALUES(1,0);")==0,"actual_native_schema");
 put((uintptr_t)connection_object+0x40,database);
 connection_object[0x38]=1;
 put((uintptr_t)connection_object+0x50,prepare("BEGIN IMMEDIATE;"));put((uintptr_t)connection_object+0x58,prepare("BEGIN DEFERRED;"));
 put((uintptr_t)connection_object+0x60,prepare("COMMIT;"));put((uintptr_t)connection_object+0x68,prepare("ROLLBACK;"));
 if(mode==L"rollback")require(sql(database,"CREATE TRIGGER reject_aux BEFORE UPDATE OF queued_visit_time ON virtualized_item WHEN NEW.queued_visit_time!=111 BEGIN SELECT RAISE(ROLLBACK,'finite'); END;")==0,"rollback_boundary_trigger");
 statement(global_cache,0x48,prepare(mode==L"root_no_write"?"SELECT ?1,?2 WHERE 0;":"UPDATE virtualized_item SET queued_visit_time=111 WHERE item_entity_id=1 AND ?1=123456 AND ?2 IS NOT NULL;"));
 statement(item_cache,0x48,prepare("INSERT INTO item_bool VALUES(?1,?2);"));statement(item_cache,0xf0,prepare("INSERT INTO item_float VALUES(?1,?2);"));
 Handle native_thread(CreateThread(nullptr,0,run,nullptr,0,nullptr));require(native_thread.value,"actual_native_worker");
 require(WaitForSingleObject(worker_ready,5000)==WAIT_OBJECT_0,"native_worker_world_connection");
 scum_save::runtime().armed=true;auto owner=scum_save::begin((uintptr_t)system_object,(uintptr_t)instance,(uintptr_t)world);
#ifndef LGSM_SCUM_BASELINE
 deep_clone_proofs();
#endif
 EnterCriticalSection(producer());scum_save::dispatch(owner);publish_items();
 check(owner->root_calls==1&&!owner->root_entered,"one_original_AutoSave");
 if(late) {
  LeaveCriticalSection(producer());wait_count(3);require(WaitForSingleObject(two_drains,5000)==WAIT_OBJECT_0,"root_drained");pump_and_wait();
  check(owner->completion.native_settled()&&owner->sql.at(database).closed(),"root_causal_and_sql_debts_closed_before_auxiliary");
  gate_auxiliary=true;EnterCriticalSection(producer());
 }
 if(mode==L"world")put((uintptr_t)level+0xb8,(uintptr_t)foreign_world);
 if(mode==L"worker"){put((uintptr_t)manager+0x520,7);put((uintptr_t)manager+0x524,108);}
 if(mode==L"unknown") {
  Function unknown{mapped+0x1e28db0};put((uintptr_t)unknown.inlined,mapped+0x56c6b10);
  ((void(*)(void*,void*))(mapped+0x1e2c5b0))(manager,&unknown);
 }else if(readonly_aux)publish_readonly_virtualized();else publish_virtualized();
 if(mode==L"world")put((uintptr_t)level+0xb8,(uintptr_t)world);
 if(mode==L"worker"){put((uintptr_t)manager+0x520,1);put((uintptr_t)manager+0x524,102);}
 auto* queued=*(unsigned char**)(runtime+0x90);const auto queued_count=*(int*)(runtime+0x98);require(queued_count>0,"pending_virtualized_record");
 auto* queued_outer=(Function*)(queued+(queued_count-1)*80+16);
 auto* queued_nested=(Function*)((unsigned char*)queued_outer->storage+16);
 const auto expected_aux_time=*(std::int64_t*)((unsigned char*)queued_nested->storage+8);
 require(mode==L"unknown"||expected_aux_time>0,"original_finite_queue_time_payload");
#ifndef LGSM_SCUM_BASELINE
 if(mode==L"table")owner->publications.rbegin()->second.writer_table=0x51e8bd8;
#endif
 if(mode==L"payload") {
  auto* data=*(unsigned char**)(runtime+0x90);const auto count=*(int*)(runtime+0x98);require(count>0,"pending_virtualized_task");
  auto* outer=(Function*)(data+(count-1)*80+16);auto* outer_storage=(unsigned char*)outer->storage;
  auto* nested=(Function*)(outer_storage+16);auto* nested_storage=(unsigned char*)nested->storage;nested_storage[8]^=1;
 }
 LeaveCriticalSection(producer());
 if(late) {
  require(WaitForSingleObject(auxiliary_write_entered,5000)==WAIT_OBJECT_0,"auxiliary_real_os_write_inflight");
  check(std::string_view(scum_save::observe(owner,(uintptr_t)system_object,(uintptr_t)instance,(uintptr_t)world))=="pending","root_cannot_confirm_before_auxiliary_os_and_C0_close");
  SetEvent(allow_auxiliary_write);wait_count(1);
 }else wait_count(4);
 require(WaitForSingleObject(two_drains,5000)==WAIT_OBJECT_0,"native_drains_without_sleep");
 if(missing_root||missing_aux||mode==L"replay") {
  auto* ids=*(int**)(runtime+0xc0);require(ids&&*(int*)(runtime+0xc8)==4,"four_C0_ids");
  if(mode==L"replay")ids[3]=ids[0];
  else {const unsigned drop=missing_root?0:3;std::memmove(ids+drop,ids+drop+1,(3-drop)*sizeof(int));put((uintptr_t)runtime+0xc8,3);}
 }
 if(pending_foreign) {
  check(!owner->completion.native_settled(),"pending_foreign_SQL_starts_before_original_C0_pump_return");
  check(sql(database,"UPDATE virtualized_item SET queued_visit_time=222;")==0,"pending_foreign_SQL_original_still_executes");
 }
 pump_and_wait();fault_auxiliary=false;
 if(after_foreign) {
  check(!owner->confirmed&&owner->completion.native_settled()&&owner->sql.at(database).closed()&&
   owner->items->completion.tasks_settled()&&owner->items->sql.at(database).closed()&&
   owner->virtualized->completion.tasks_settled()&&owner->virtualized->sql.at(database).closed()&&
   !owner->owned_scopes&&!owner->io_inflight&&!owner->items->owned_scopes&&!owner->items->io_inflight&&
   !owner->virtualized->owned_scopes&&!owner->virtualized->io_inflight&&
   !scum_save::runtime().task_inflight&&!scum_save::runtime().sql_inflight&&!scum_save::runtime().io_inflight,
   "all_native_and_auxiliary_debts_closed_before_later_SQL_without_poll");
  check(sql(database,"UPDATE virtualized_item SET queued_visit_time=222;")==0,"independent_foreign_SQL_original_still_executes");
  check(owner->confirmed&&!owner->completion.rejected()&&!owner->first_foreign_sql.api_rva&&
   owner->sql.at(database).closed()&&owner->items->sql.at(database).closed()&&owner->virtualized->sql.at(database).closed(),
   "later_SQL_neither_poisoned_nor_paid_the_already_closed_receipt");
 }
 const char* observed=scum_save::observe(owner,(uintptr_t)system_object,(uintptr_t)instance,(uintptr_t)world);
 if(missing_root||missing_aux||mode==L"root_no_write") {
  if(mode==L"root_no_write") {
#ifndef LGSM_SCUM_BASELINE
   check(owner->completion.native_settled()&&owner->payload_connections.empty()&&!owner->virtualized->payload_connections.empty(),"auxiliary_SQL_writes_cannot_pay_root_payload_debt");
#endif
  }
  check(std::string_view(observed)=="pending","missing_C0_keeps_independent_receipt_pending");owner->deadline=0;
  observed=scum_save::observe(owner,(uintptr_t)system_object,(uintptr_t)instance,(uintptr_t)world);
 }
 std::cout<<"FINITE_TERMINAL "<<observed<<" issue="<<owner->completion.issue()<<'\n';
 check(healthy?std::string_view(observed)=="confirmed":owner->completion.rejected(),"actual_virtualized_receipt_terminal");
 if(!healthy) {
  const auto expected=(missing_root||missing_aux||mode==L"root_no_write")?"native_save_timeout":mode==L"payload"||mode==L"table"?"native_batch_ownership":
   mode==L"replay"?"native_pump_foreign_sequence":mode==L"os_failure"?"native_os_write_failed":mode==L"rollback"?"native_sql_step_failed":pending_foreign?"foreign_native_sql":"foreign_native_queue";
  check(std::string_view(owner->completion.issue())==expected,"targeted_rejection_reason");
 }
#ifndef LGSM_SCUM_BASELINE
 if(healthy) {
  check(owner->publications.size()==4&&owner->items&&owner->virtualized&&owner->items!=owner->virtualized,"three_independent_receipts");
  check(owner->completion.native_settled()&&owner->sql.at(database).closed()&&owner->virtualized->completion.tasks_settled()&&owner->virtualized->sql.at(database).closed(),"root_and_virtualized_SQL_closed_independently");
  scum_save::readback_depth++;
  auto* result=prepare("SELECT queued_visit_time FROM virtualized_item WHERE item_entity_id=1;");
  require(api<int(*)(ST*)>(94)(result)==100,"same_row_result");
  if(readonly_aux)check(owner->virtualized->payload_connections.empty()&&((long long(*)(ST*,int))(mapped+0x28ef6f0))(result,0)==111,"actual_readonly_noop_auxiliary_cannot_supply_root_write");
  else if(after_foreign)check(((long long(*)(ST*,int))(mapped+0x28ef6f0))(result,0)==222,"independent_later_SQL_really_overwrote_same_row_after_seal");
  else check(expected_aux_time!=111&&((long long(*)(ST*,int))(mapped+0x28ef6f0))(result,0)==expected_aux_time,"native_FIFO_auxiliary_overwrites_same_row_after_root");
  scum_save::readback_depth--;
 }
#endif
 EnterCriticalSection(producer());runtime[0x124]=1;LeaveCriticalSection(producer());SetEvent(event.ready);
 check(WaitForSingleObject(native_thread.value,5000)==WAIT_OBJECT_0,"native_worker_reaped");
 scum_save::runtime().active.reset();for(auto* value:statements)require(api<int(*)(ST*)>(57)(value)==0,"native_statement_cleanup");
 require(close(database)==0,"native_database_cleanup");check(scum_save::code_matches(),"complete_product_pins_remain_exact");
 check(MH_DisableHook(MH_ALL_HOOKS)==MH_OK&&MH_Uninitialize()==MH_OK,"hooks_released_after_worker_exit");
 ((void(*)(void*))(mapped+0xc5c0e0))(runtime+0xd0);for(const unsigned offset:{0x90U,0xa0U,0xb0U,0xc0U})if(auto* value=*(void**)(runtime+offset))_aligned_free(value);
 for(const unsigned offset:{0x18U,0x40U,0x68U})DeleteCriticalSection((CRITICAL_SECTION*)(runtime+offset));
 for(auto handle:{event.ready,worker_ready,owned_entered,allow_owned,first_wake,two_drains,auxiliary_write_entered,allow_auxiliary_write})CloseHandle(handle);
 check(writable_handles.empty()&&guard_rejected==0,"only_owned_fixture_files");
 std::cout<<"SCUM actual virtualization/AutoSave mixed checks="<<checks<<" game_processes=0\n";return 0;
}catch(const std::exception& error){std::cerr<<"VIRTUALIZED_NATIVE_FAIL "<<error.what()<<'\n';return 1;}}
