#include "native_queue_fixture.hpp"
namespace {
DB* product_database{};
// Only cache/object lookup is a host boundary. The queued invoker and its
// complete native 1b92150 continuation forwarding now execute first-party code.
uintptr_t(*cache_original)(uintptr_t,uintptr_t){};
uintptr_t cache_boundary(uintptr_t,uintptr_t){return (uintptr_t)&product_database;}
struct NativeStorage {uintptr_t table{},pad{},context{},pad2{};Function body;};
static_assert(sizeof(NativeStorage)==96);
Function* product_body(Function* output,Functor* value,void**) {
 require(value->cookie==0xadd5,"actual_task_payload");++bodies;
 if(sql(product_database,"INSERT INTO progress VALUES('actual-owned-worker');")==0)++body_sql_ok;
 *output={};if(value->continuation){output->invoke=(uintptr_t)callback;clone(value,(void**)&output->storage);}
 SetEvent(body_done);return output;
}
void simple_signal(Event* self){SetEvent(self->ready);}
}
int wmain(int argc,wchar_t** argv){try {
 require(argc==3,"arguments");const auto scratch=fixture_root(argv[1]);const bool cancel=std::wstring(argv[2])==L"cancel";

 auto input=scratch/L"image/SCUMServer.exe";auto dirs=pin_directories(input.parent_path());auto file=open_file(input,GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ);
 check(hash_file(file.value)=="4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad","fixed_physical_copy");
 allowed_root=component_directory(scratch,L"product-task-fixture");auto rootpins=pin_directories(allowed_root);
 SetEnvironmentVariableW(L"TEMP",allowed_root.c_str());SetEnvironmentVariableW(L"TMP",allowed_root.c_str());
 mapped=(uintptr_t)LoadLibraryExW(input.c_str(),nullptr,DONT_RESOLVE_DLL_REFERENCES);require(mapped!=0,"copied_image_mapping");SetUnhandledExceptionFilter(fault);imports(mapped);sqlite_guards();
 check(scum_save::test_profile_copy(input,mapped),"actual_product_profile");scum_save::test_saved_root(allowed_root);
 auto hooks=scum_save::task_hooks();auto sqlhooks=scum_save::sql_hooks();hooks.insert(hooks.end(),sqlhooks.begin(),sqlhooks.end());
 hooks.push_back({0x1ac4d30,(void*)cache_boundary,(void**)&cache_original,"4889542410",5,{}});auto io=scum_save::file_imports();
 for(const auto& slot:io)scum_save::test_guarded_import(slot.rva,*(void**)(mapped+slot.rva));
 check(scum_save::install_hooks(hooks,io),"all_product_task_sql_io_hooks");check(((int(*)())(mapped+0x298d5e0))()==0,"actual_native_sqlite_init");
 allocator_table[2]=(uintptr_t)allocate;allocator_table[4]=(uintptr_t)resize;allocator_table[6]=(uintptr_t)release;allocator_table[7]=(uintptr_t)quantize;
 allocator=allocator_table;*(uintptr_t*)(mapped+0x74b8660)=(uintptr_t)&allocator;
 functor_table[0]=(uintptr_t)clone;functor_table[1]=(uintptr_t)get_callable;functor_table[2]=(uintptr_t)destroy;
 event_table[2]=(uintptr_t)simple_signal;event_table[4]=(uintptr_t)wait_event;event={event_table,CreateEventW(nullptr,FALSE,FALSE,nullptr)};
 require(event.ready!=nullptr,"host_event");worker_table[0x160/8]=(uintptr_t)no_world;
 *(uintptr_t*)worker=(uintptr_t)worker_table;*(uintptr_t*)(worker+0x30)=(uintptr_t)runtime;
 *(uintptr_t*)runtime=mapped+0x59a3198;*(uintptr_t*)(runtime+8)=(uintptr_t)worker;*(uintptr_t*)(runtime+0x10)=(uintptr_t)&event;
 for(auto offset:{0x18,0x40,0x68})InitializeCriticalSection((CRITICAL_SECTION*)(runtime+offset));*(int*)(runtime+0x120)=1;
 check(scum_save::tasks_quiet((uintptr_t)runtime),"product_admission_actual_empty_native_queues");
 alignas(16) unsigned char connection[0xc8]{},items[48]{},type[64]{};uintptr_t chunks[1]={(uintptr_t)items};
 auto put=[&](uintptr_t at,const auto& value){protect_copy(at,&value,sizeof(value));};
 put(mapped+0x753d660,(uintptr_t)chunks);put(mapped+0x753d674,2);put(mapped+0x7445308,(uintptr_t)type);
 put((uintptr_t)worker+0x10,(uintptr_t)type);put((uintptr_t)items,(uintptr_t)worker);put((uintptr_t)items+16,1001);
 put((uintptr_t)connection+0xc,1);put((uintptr_t)connection+0x10,(uintptr_t)type);put((uintptr_t)items+24,(uintptr_t)connection);put((uintptr_t)items+40,1002);
 check(open(allowed_root/L"worker.db",&product_database,6)==0&&product_database,"native_connection_boundary");check(sql(product_database,"PRAGMA temp_store=MEMORY; CREATE TABLE progress(value TEXT);")==0,"native_schema_boundary");
 body_done=CreateEventW(nullptr,TRUE,FALSE,nullptr);callback_done=CreateEventW(nullptr,TRUE,FALSE,nullptr);
 Handle native_thread(CreateThread(nullptr,0,run,nullptr,0,nullptr));require(native_thread.value!=nullptr,"actual_worker_thread");
 const auto start=GetTickCount64();for(;;){{std::lock_guard lock(scum_save::runtime().mutex);if(scum_save::runtime().workers.contains((uintptr_t)runtime))break;}require(GetTickCount64()-start<5000,"product_worker_entry_timeout");Sleep(1);}
 put((uintptr_t)connection+0x40,product_database);put((uintptr_t)connection+0x48,worker_thread);
 scum_save::Database entry;entry.object=(uintptr_t)connection;entry.identity=scum_save::identity((uintptr_t)connection);entry.thread=worker_thread;
 scum_save::runtime().databases.emplace(product_database,std::move(entry));
 auto owner=std::make_shared<scum_save::Epoch>();owner->deadline=GetTickCount64()+30000;owner->worker=scum_save::identity((uintptr_t)worker);owner->native_worker=(uintptr_t)runtime;owner->worker_thread=worker_thread;owner->main_thread=GetCurrentThreadId();
 owner->completion.stage(1);owner->completion.stage(2);owner->completion.stage(4);scum_save::runtime().active=owner;
 Functor original{functor_table,0xadd5,true};NativeStorage storage;storage.table=mapped+0x571e488;
 storage.body.invoke=(uintptr_t)product_body;clone(&original,(void**)&storage.body.storage);
 Function fn{mapped+0x1b98080};fn.storage=(uintptr_t)&storage;
 int sequence{};{scum_save::Scope scope(owner);sequence=((int(*)(void*,Function*))(mapped+0x1abced0))(worker,&fn);owner->completion.root_returned();}destroy((Functor*)storage.body.storage);
 check(sequence==1,"product_publishes_actual_native_queue_id");check(WaitForSingleObject(body_done,5000)==WAIT_OBJECT_0,"actual_worker_body");wait_count(1);
 if(cancel)check(((bool(*)(void*,int))(mapped+0x1ab5d90))(runtime,sequence),"actual_native_cancel");
 ((void(*)(void*))(mapped+0x1ab9df0))(runtime);
 const auto settled_start=GetTickCount64();for(;;){{std::lock_guard lock(scum_save::runtime().mutex);if(!scum_save::runtime().task_inflight)break;}require(GetTickCount64()-settled_start<5000,"native_batch_return_timeout");Sleep(1);}
 check(body_sql_ok==1,"actual_native_worker_mutation");check(owner->sql.at(product_database).closed(),"product_worker_native_sql_and_disk_readback");
 check(owner->completion.native_settled(),"product_actual_queue_batch_main_causal_closure");
 if(owner->completion.rejected())std::cerr<<"PRODUCT_TASK_ISSUE "<<owner->completion.issue()<<"\n";
 check(cancel?!owner->completion.confirmed(true,true):owner->completion.confirmed(true,true),cancel?"product_rejects_native_cancel_despite_committed_sql":"product_actual_task_sql_disk_complete");
 check(cancel?continuations==0:continuations==1,"original_native_continuation_invocation_policy");owner->confirmed=!cancel;
 EnterCriticalSection(producer());runtime[0x124]=1;LeaveCriticalSection(producer());SetEvent(event.ready);check(WaitForSingleObject(native_thread.value,5000)==WAIT_OBJECT_0,"native_worker_reaped");
 scum_save::runtime().active.reset();require(close(product_database)==0,"native_connection_cleanup");check(functors==0,"all_native_callable_copies_released");check(scum_save::code_matches(),"product_code_and_slots_remain_exact");
 check(MH_DisableHook(MH_ALL_HOOKS)==MH_OK&&MH_Uninitialize()==MH_OK,"test_hooks_removed_only_after_worker_exit");
 ((void(*)(void*))(mapped+0xc5c0e0))(runtime+0xd0);for(auto offset:{0x90,0xa0,0xb0,0xc0})if(auto* data=*(void**)(runtime+offset))_aligned_free(data);
 for(auto offset:{0x18,0x40,0x68})DeleteCriticalSection((CRITICAL_SECTION*)(runtime+offset));for(auto h:{body_done,callback_done,event.ready})CloseHandle(h);
 check(writable_handles.empty()&&guard_rejected==0,"only_owned_fixture_io");std::cout<<"SCUM product actual queue-SQL-main checks="<<checks<<"\n";return 0;
}catch(const std::exception& e){std::cerr<<"PRODUCT_QUEUE_FAIL "<<e.what()<<"\n";return 1;}}
