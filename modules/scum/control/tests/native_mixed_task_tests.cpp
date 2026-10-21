// Actual AutoSave(empty registered sets), three stages, native producer/queue,
// worker/batch/C0/pump, native SQL writers and disk readback. Only UE object/
// cache lookup, allocator and event boundaries are constructed by this fixture.
#include "native_queue_fixture.hpp"
#include <cstring>
namespace {
alignas(16) unsigned char system_object[0x800]{},instance[0x50]{},world[0x190]{},connection_object[0xc8]{};
alignas(16) unsigned char entity[0x280]{},global_cache[0x70]{},item_cache[0x160]{};
alignas(16) unsigned char root_type[64]{},other_type[64]{},instance_type[64]{},object_items[5*24]{};
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
 object_chunks[0]=(uintptr_t)object_items;put(mapped+0x753d660,(uintptr_t)object_chunks);put(mapped+0x753d674,5);
 put(mapped+0x7447070,(uintptr_t)root_type);put(mapped+0x7445308,(uintptr_t)other_type);
 const uintptr_t objects[]={(uintptr_t)system_object,(uintptr_t)worker,(uintptr_t)connection_object,(uintptr_t)instance,(uintptr_t)world};
 for(int index=0;index<5;++index) {
  put(objects[index]+0xc,index);put(objects[index]+0x10,index==0?(uintptr_t)root_type:index==3?(uintptr_t)instance_type:(uintptr_t)other_type);
  put((uintptr_t)object_items+index*24,objects[index]);put((uintptr_t)object_items+index*24+16,101+index);
 }
 put((uintptr_t)system_object+0x20,(uintptr_t)instance);system_object[0x30]=1;
 put((uintptr_t)system_object+0x58,1);put((uintptr_t)system_object+0x5c,102);
 put((uintptr_t)system_object+0x50,(std::int64_t)123456);put((uintptr_t)world+0x180,(uintptr_t)instance);
 put((uintptr_t)entity+0x58,0);put((uintptr_t)entity+0x5c,101);put((uintptr_t)entity+0xd8,(std::int64_t)77);
 entity_table[0x150/8]=(uintptr_t)notify;put((uintptr_t)entity,(uintptr_t)entity_table);
 instance_bases[0]=(uintptr_t)instance_type+0x30;put((uintptr_t)instance_type+0x30,(uintptr_t)instance_bases);put((uintptr_t)instance_type+0x38,0);
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
}
int wmain(int argc,wchar_t** argv){try {
 require(argc==3,"arguments");const auto scratch=fixture_root(argv[1]);const std::wstring mode=argv[2];delay_owned=mode==L"late";
 require(mode==L"mixed"||mode==L"late"||mode==L"aux_write"||mode==L"reorder"||mode==L"duplicate"||mode==L"missing_completion"||mode==L"world_epoch"||mode==L"unknown_producer","mode");fail_aux=mode==L"aux_write";
 auto input=scratch/L"image/SCUMServer.exe";auto dirs=pin_directories(input.parent_path());auto image=open_file(input,GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ);
 check(hash_file(image.value)=="4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad","fixed_physical_copy");
 allowed_root=component_directory(scratch,L"mixed-native-fixture");auto rootpins=pin_directories(allowed_root);
 SetEnvironmentVariableW(L"TEMP",allowed_root.c_str());SetEnvironmentVariableW(L"TMP",allowed_root.c_str());
 mapped=(uintptr_t)LoadLibraryExW(input.c_str(),nullptr,DONT_RESOLVE_DLL_REFERENCES);require(mapped,"copied_image_mapping");SetUnhandledExceptionFilter(fault);imports(mapped);sqlite_guards();
 check(scum_save::test_profile_copy(input,mapped),"actual_complete_native_profile");scum_save::test_saved_root(allowed_root);
 auto hooks=scum_save::task_hooks();auto sqlhooks=scum_save::sql_hooks();hooks.insert(hooks.end(),sqlhooks.begin(),sqlhooks.end());
 // These two exact functions only resolve the constructed UE/cache boundary;
 // their original SQL bodies, tasks, statements and lifecycle are not replaced.
 hooks.push_back({0x1ac4d30,(void*)cache_lookup,(void**)&cache_original,"4889542410",5,{}});
 hooks.push_back({0x1a4c3e0,(void*)connection_lookup,(void**)&connection_original,"48895c2410",5,{}});
 auto io=scum_save::file_imports();for(const auto& slot:io)scum_save::test_guarded_import(slot.rva,*(void**)(mapped+slot.rva));
 check(scum_save::install_hooks(hooks,io),"product_hooks_and_explicit_ue_boundaries");
 // Native class metadata factory is an external object boundary. The worker's
 // full first-party class-chain predicate below still executes unchanged.
 jump(mapped+0x26ead80,(uintptr_t)fixture_instance_class);
 jump(mapped+0x1afa560,(uintptr_t)error_log); // External UE error logging only; SQL results stay real.
 check(((int(*)())(mapped+0x298d5e0))()==0,"actual_native_sqlite_init");
 allocator_table[2]=(uintptr_t)allocate;allocator_table[4]=(uintptr_t)resize;allocator_table[6]=(uintptr_t)release;allocator_table[7]=(uintptr_t)quantize;
 allocator=allocator_table;put(mapped+0x74b8660,(uintptr_t)&allocator);
 native_objects();event_table[2]=(uintptr_t)event_signal;event_table[4]=(uintptr_t)mixed_wait;
 event={event_table,CreateEventW(nullptr,FALSE,FALSE,nullptr)};worker_ready=CreateEventW(nullptr,TRUE,FALSE,nullptr);
 owned_entered=CreateEventW(nullptr,TRUE,FALSE,nullptr);allow_owned=CreateEventW(nullptr,TRUE,FALSE,nullptr);
 first_wake=CreateEventW(nullptr,TRUE,FALSE,nullptr);two_drains=CreateEventW(nullptr,TRUE,FALSE,nullptr);
 require(event.ready&&worker_ready&&owned_entered&&allow_owned&&first_wake&&two_drains,"host_events");worker_table[0x160/8]=(uintptr_t)fixture_world;
 put((uintptr_t)worker,(uintptr_t)worker_table);put((uintptr_t)worker+0x30,(uintptr_t)runtime);
 put((uintptr_t)runtime,mapped+0x59a3198);put((uintptr_t)runtime+8,(uintptr_t)worker);put((uintptr_t)runtime+0x10,(uintptr_t)&event);
 for(const unsigned offset:{0x18U,0x40U,0x68U})InitializeCriticalSection((CRITICAL_SECTION*)(runtime+offset));put((uintptr_t)runtime+0x120,1);
 scum_save::opening_connection=(uintptr_t)connection_object;
 check(open(allowed_root/L"mixed.db",&database,6)==0&&database,"actual_native_database");scum_save::opening_connection=0;
 check(sql(database,"PRAGMA temp_store=MEMORY; CREATE TABLE owned(context INTEGER,ticks REAL); CREATE TABLE item_bool(id INTEGER,value INTEGER); CREATE TABLE item_float(id INTEGER,value REAL);")==0,"actual_native_schema");
 put((uintptr_t)connection_object+0x40,database);
 statement(global_cache,0x48,prepare("INSERT INTO owned VALUES(?1,?2);"));
 statement(item_cache,0x48,prepare("INSERT INTO item_bool VALUES(?1,?2);"));
 statement(item_cache,0xf0,prepare("INSERT INTO item_float VALUES(?1,?2);"));
 Handle native_thread(CreateThread(nullptr,0,run,nullptr,0,nullptr));require(native_thread.value,"actual_native_worker");
 require(WaitForSingleObject(worker_ready,5000)==WAIT_OBJECT_0,"native_worker_resolved_constructed_world_connection");
 scum_save::runtime().armed=true;auto owner=scum_save::begin((uintptr_t)system_object,(uintptr_t)instance,(uintptr_t)world);
 item_family_proofs(owner);
 if(!delay_owned)EnterCriticalSection(producer());
 scum_save::dispatch(owner);check(owner->root_calls==1&&!owner->root_entered,"actual_one_full_native_AutoSave_returned");
 if(!delay_owned)require(WaitForSingleObject(first_wake,5000)==WAIT_OBJECT_0,"native_first_wake_before_two_more_publications");
 if(delay_owned)require(WaitForSingleObject(owned_entered,5000)==WAIT_OBJECT_0,"owned_native_worker_still_inflight");
 if(mode==L"unknown_producer") {
  Function unknown{mapped+0x1b97290};put((uintptr_t)unknown.inlined,mapped+0x5196950);
  put((uintptr_t)unknown.inlined+8,(std::int64_t)77);unknown.inlined[16]=1;
  ((void(*)(void*,uintptr_t))(mapped+0x1ba6690))(entity,(uintptr_t)&unknown);
  ((void(*)(void*,float))(mapped+0x1bcbd30))(entity,1.25F);
 }else publish_items();
 if(mode==L"reorder") {
  auto* data=*(unsigned char**)(runtime+0x90);require(*(int*)(runtime+0x98)==3,"three_native_queued_tasks");
  for(unsigned at=0;at<80;++at)std::swap(data[at],data[80+at]);
 }
 if(mode==L"duplicate") {
  auto* data=*(unsigned char**)(runtime+0x90);require(*(int*)(runtime+0x98)==3,"three_native_queued_tasks");
  // A duplicate real ID independently tests the closed sequence ledger. The
  // original native callbacks still run and none is counted as an owned debt.
  std::memcpy(data+80,data,sizeof(int));
 }
 if(delay_owned)SetEvent(allow_owned);else LeaveCriticalSection(producer());
 wait_count(3);
 require(WaitForSingleObject(two_drains,5000)==WAIT_OBJECT_0,"native_two_complete_drains_without_sleep");
 check(waits>=3,"actual_worker_automatic_second_drain");
 if(mode==L"missing_completion") {
  // Drop only the owned integer completion ID after all three genuine SQL
  // callbacks returned. No callable/lifetime or success flag is substituted.
  auto* ids=*(int**)(runtime+0xc0);
  require(*(int*)(runtime+0xb8)==0&&*(int*)(runtime+0xc8)==3&&ids,"three_native_C0_receipts");
  std::memmove(ids,ids+1,2*sizeof(int));put((uintptr_t)runtime+0xc8,2);
 }
 if(mode==L"world_epoch")put((uintptr_t)object_items+4*24+16,999);
 ((void(*)(void*))(mapped+0x1ab9df0))(runtime);
 const auto start=GetTickCount64();const char* observed="pending";
 for(;;) {
  {std::lock_guard lock(scum_save::runtime().mutex);if(!scum_save::runtime().task_inflight)break;}
  require(GetTickCount64()-start<5000,"native_batch_terminal");Sleep(1);
 }
 fail_write=false;
 observed=scum_save::observe(owner,(uintptr_t)system_object,(uintptr_t)instance,(uintptr_t)world);
 if(mode==L"missing_completion") {
  check(std::string_view(observed)=="pending"&&!owner->completion.native_settled(),"item_receipts_cannot_pay_missing_owned_main_completion");
  owner->deadline=0; // Explicit clock boundary; never manufacture completion.
  observed=scum_save::observe(owner,(uintptr_t)system_object,(uintptr_t)instance,(uintptr_t)world);
 }
 const bool healthy=mode==L"mixed"||mode==L"late";
 std::cout<<"FINITE_TERMINAL "<<observed<<" issue="<<owner->completion.issue()<<" cache_calls="<<cache_calls<<" native_logs="<<native_logs<<'\n';
 check(healthy?std::string_view(observed)=="confirmed":(std::string_view(observed)=="failed"||std::string_view(observed)=="unknown")&&owner->completion.rejected(),"actual_mixed_receipt_terminal");
 if(!healthy)std::cout<<"FINITE_REJECTION "<<owner->completion.issue()<<'\n';
 if(healthy) {
  item_proofs(owner);
  check(owner->completion.native_settled()&&owner->sql.at(database).closed(),"owned_root_actual_writer_and_full_page_readback_closed");
 }
 EnterCriticalSection(producer());runtime[0x124]=1;LeaveCriticalSection(producer());SetEvent(event.ready);
 check(WaitForSingleObject(native_thread.value,5000)==WAIT_OBJECT_0,"native_worker_reaped");
 scum_save::runtime().active.reset();for(auto* value:statements)require(api<int(*)(ST*)>(57)(value)==0,"native_statement_cleanup");
 require(close(database)==0,"native_database_cleanup");check(scum_save::code_matches(),"complete_product_pins_remain_exact");
 check(MH_DisableHook(MH_ALL_HOOKS)==MH_OK&&MH_Uninitialize()==MH_OK,"hooks_released_after_worker_exit");
 ((void(*)(void*))(mapped+0xc5c0e0))(runtime+0xd0);for(const unsigned offset:{0x90U,0xa0U,0xb0U,0xc0U})if(auto* data=*(void**)(runtime+offset))_aligned_free(data);
 for(const unsigned offset:{0x18U,0x40U,0x68U})DeleteCriticalSection((CRITICAL_SECTION*)(runtime+offset));
 for(auto handle:{event.ready,worker_ready,owned_entered,allow_owned,first_wake,two_drains})CloseHandle(handle);
 check(writable_handles.empty()&&guard_rejected==0,"only_owned_fixture_files");
 std::cout<<"SCUM full native AutoSave mixed task checks="<<checks<<" game_processes=0\n";return 0;
}catch(const std::exception& error){std::cerr<<"MIXED_NATIVE_FAIL "<<error.what()<<'\n';return 1;}}
