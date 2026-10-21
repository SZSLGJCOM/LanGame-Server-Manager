
#include "native_fixture.hpp"
#include "../native_state.hpp"
namespace {
HANDLE WINAPI guarded_mapping_a(HANDLE h,LPSECURITY_ATTRIBUTES a,DWORD p,DWORD high,DWORD low,LPCSTR n) {
 if((p&(PAGE_READWRITE|PAGE_EXECUTE_READWRITE))&&!writable_handles.contains(h)){SetLastError(ERROR_ACCESS_DENIED);return nullptr;}
 return CreateFileMappingA(h,a,p,high,low,n);
}
}
int wmain(int argc,wchar_t** argv){try {
 require(argc==3,"arguments");auto scratch=fixture_root(argv[1]);const std::wstring mode=argv[2];

 auto input=scratch/L"image/SCUMServer.exe";auto dirs=pin_directories(input.parent_path());auto file=open_file(input,GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ);
 check(hash_file(file.value)=="4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad","fixed_physical_image");
 allowed_root=component_directory(scratch,L"product-sql-fixture");auto rootpins=pin_directories(allowed_root);
 SetEnvironmentVariableW(L"TEMP",allowed_root.c_str());SetEnvironmentVariableW(L"TMP",allowed_root.c_str());
 mapped=(uintptr_t)LoadLibraryExW(input.c_str(),nullptr,DONT_RESOLVE_DLL_REFERENCES);require(mapped!=0,"copied_image_mapping");SetUnhandledExceptionFilter(fault);imports(mapped);sqlite_guards();
 const uintptr_t guard=(uintptr_t)guarded_mapping_a;protect_copy(mapped+0x513c9c0,&guard,8);
 check(scum_save::test_profile_copy(input,mapped),"actual_product_profile");scum_save::test_saved_root(allowed_root);
 auto hooks=scum_save::task_hooks();auto sqlhooks=scum_save::sql_hooks();hooks.insert(hooks.end(),sqlhooks.begin(),sqlhooks.end());auto io=scum_save::file_imports();
 for(const auto& slot:io)scum_save::test_guarded_import(slot.rva,*(void**)(mapped+slot.rva));
 check(scum_save::install_hooks(hooks,io),"all_actual_product_hooks_installed");check(scum_save::code_matches(),"product_code_and_api_pins_after_hooks");
 check(((int(*)())(mapped+0x298d5e0))()==0,"actual_game_sqlite_init");
 alignas(16) unsigned char object[0xc8]{},item[24]{},type[64]{};uintptr_t chunks[1]={(uintptr_t)item};
 auto put=[&](uintptr_t at,const auto& value){protect_copy(at,&value,sizeof(value));};
 put(mapped+0x753d660,(uintptr_t)chunks);put(mapped+0x753d674,1);put(mapped+0x753d700,1000);put(mapped+0x7445308,(uintptr_t)type);
 put((uintptr_t)object+0xc,0);put((uintptr_t)object+0x10,(uintptr_t)type);put((uintptr_t)item,(uintptr_t)object);
 auto identity=scum_save::identity((uintptr_t)object,(uintptr_t)type);check(identity.serial==1001&&*(int*)(item+16)==1001,"actual_native_serial_allocator");
 DB* db{};auto path=allowed_root/L"product.db";check(open(path,&db,6)==0&&db,"actual_native_open");check(sql(db,"PRAGMA temp_store=MEMORY; CREATE TABLE progress(value TEXT);")==0,"native_schema_boundary");
 put((uintptr_t)object+0x40,db);put((uintptr_t)object+0x48,GetCurrentThreadId());
 scum_save::Database entry;entry.object=(uintptr_t)object;entry.identity=identity;entry.thread=GetCurrentThreadId();
 scum_save::runtime().databases.emplace(db,std::move(entry));scum_save::runtime().armed=true;
 auto owner=std::make_shared<scum_save::Epoch>();owner->deadline=GetTickCount64()+30000;scum_save::runtime().active=owner;
 {
 scum_save::Scope scope(owner,(uintptr_t)object);
 if(mode==L"write")fail_write=true;if(mode==L"flush")fail_flush=true;
 const int rc=sql(db,"INSERT INTO progress VALUES('actual-product-hooks');");fail_write=false;fail_flush=false;
 if(mode==L"write"||mode==L"flush"){
  check(rc!=0,"native_write_or_flush_failure_reaches_sqlite");check(owner->completion.failed()&&owner->io_failed,"product_captures_actual_os_failure");
 }else{
  check(rc==0,"native_owned_insert");check(owner->payload_connections.contains(db)&&owner->sql.at(db).unverified(),"product_records_actual_mutation_debt");
  scum_save::verify_databases(owner,(uintptr_t)object);
  if(owner->completion.rejected())std::cerr<<"PRODUCT_ISSUE "<<owner->completion.issue()<<"\n";
  check(!owner->completion.rejected()&&owner->sql.at(db).closed(),"product_native_readback_closes_healthy_debt");
  if(mode==L"healthy"||mode==L"rollback") {
   ST *begin{},*commit{},*rollback{};const char* tail{};
   auto prepare=api<int(*)(DB*,const char*,int,ST**,const char**)>(116);
   check(prepare(db,"BEGIN IMMEDIATE;",-1,&begin,&tail)==0&&prepare(db,"COMMIT;",-1,&commit,&tail)==0&&prepare(db,"ROLLBACK;",-1,&rollback,&tail)==0,"actual_native_cached_transaction_statements");
   put((uintptr_t)object+0x38,(unsigned char)1);put((uintptr_t)object+0x50,begin);put((uintptr_t)object+0x60,commit);put((uintptr_t)object+0x68,rollback);
   check(((bool(*)(void*,bool))(mapped+0x1ab4ad0))(object,true),"actual_scum_begin_wrapper");
   check(sql(db,"INSERT INTO progress VALUES('explicit-native-transaction');")==0,"native_transaction_mutation");
   check(!owner->sql.at(db).closed(),"product_waits_actual_native_commit");
   if(mode==L"rollback")put((uintptr_t)object+0x74,(unsigned char)1);
   const bool committed=((bool(*)(void*))(mapped+0x1ab6640))(object);
   check(committed,mode==L"rollback"?"native_commit_wrapper_returns_true_after_rollback":"actual_scum_commit_result");
   scum_save::verify_databases(owner,(uintptr_t)object);
   if(mode==L"rollback") {check(std::string(owner->completion.issue())=="native_sql_rolled_back"&&!owner->sql.at(db).closed(),"product_identifies_actual_rollback_not_commit_boolean");++scum_save::readback_depth;const bool matching=snapshot(db)==readonly_image(path);--scum_save::readback_depth;check(matching&&owner->completion.rejected(),"product_rejects_native_rollback_even_matching_payload");}
   else check(!owner->completion.rejected()&&owner->sql.at(db).closed(),"product_closes_actual_scum_transaction_and_readback");
   for(auto* statement:{begin,commit,rollback})require(api<int(*)(ST*)>(57)(statement)==0,"cached_transaction_cleanup");
  }else if(mode==L"bind") {
   ST* statement{};const char* tail{};check(api<int(*)(DB*,const char*,int,ST**,const char**)>(116)(db,"INSERT INTO progress VALUES(?);",-1,&statement,&tail)==0,"native_owned_prepare");
   check(api<int(*)(ST*,int,int)>(4)(statement,2,7)==25,"native_bind_range_failure");
   check(api<int(*)(ST*,int,int)>(4)(statement,1,8)==0&&api<int(*)(ST*)>(94)(statement)==101,"later_native_binding_and_step_succeed");
   check(api<int(*)(ST*)>(57)(statement)==0,"native_owned_finalize");scum_save::verify_databases(owner,(uintptr_t)object);
   check(owner->completion.failed()&&!owner->sql.at(db).closed(),"product_retains_earlier_bind_failure");
  }else if(mode==L"close") {
   fail_close=true;const auto result=close(db);fail_close=false;
   check(result==0&&close_failures>0,"native_sqlite_hides_actual_os_close_failure");
   check(owner->io_failed&&owner->completion.failed(),"product_rejects_hidden_close_failure");db=nullptr;
  }else {
   check(sql(db,"INSERT INTO progress VALUES('second-write');")==0,"native_second_write");check(!owner->sql.at(db).closed(),"product_reopens_readback_debt");scum_save::verify_databases(owner,(uintptr_t)object);check(owner->sql.at(db).closed()&&!owner->completion.rejected(),"product_second_payload_readback");
  }
 }
 }
 scum_save::runtime().active.reset();if(db)require(close(db)==0,"native_cleanup_close");check(guard_rejected==0,"no_outside_owned_fixture_io");check(scum_save::code_matches(),"product_profile_remains_exact");
 std::cout<<"SCUM real product hook checks="<<checks<<"\n";return 0;
}catch(const std::exception& e){std::cerr<<"PRODUCT_FAIL "<<e.what()<<"\n";return 1;}}
