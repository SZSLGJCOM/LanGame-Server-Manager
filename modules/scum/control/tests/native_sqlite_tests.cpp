#include "native_fixture.hpp"
int wmain(int argc,wchar_t** argv) {
 try {
 require(argc==2,"arguments");auto scratch=fixture_root(argv[1]);

 auto input=scratch/L"image/SCUMServer.exe";auto dirs=pin_directories(input.parent_path());
 auto file=open_file(input,GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ);
 check(hash_file(file.value)=="4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad","fixed_physical_image_sha");
 allowed_root=component_directory(scratch,L"sqlite-fixture");
 auto rootpins=pin_directories(allowed_root);SetEnvironmentVariableW(L"TEMP",allowed_root.c_str());SetEnvironmentVariableW(L"TMP",allowed_root.c_str());
 mapped=(uintptr_t)LoadLibraryExW(input.c_str(),nullptr,DONT_RESOLVE_DLL_REFERENCES);require(mapped!=0,"copied_image_mapping");
 SetUnhandledExceptionFilter(fault);imports(mapped);sqlite_guards();
 check(std::string(api<const char*(*)()>(66)())=="3.39.4","actual_game_sqlite_version");
 check(((int(*)())(mapped+0x298d5e0))()==0,"actual_game_sqlite_initialize");
 DB* db{};auto path=allowed_root/L"world.db";
 check(open(path,&db,6)==0&&db,"native_database_open");
 check(sql(db,"PRAGMA temp_store=MEMORY; CREATE TABLE progress(id INTEGER PRIMARY KEY,value TEXT);")==0,"native_create_and_write");
 check(snapshot(db)==readonly_image(path),"rollback_journal_native_ro_exact_pages");
 scum_save::Sqlite production(mapped);
 scum_save::sqlite_readback(production,db,GetCurrentThreadId(),allowed_root,GetTickCount64()+30000);
 check(true,"production_readback_real_rollback_journal");
 check(sql(db,"PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; INSERT INTO progress VALUES(1,'committed');")==0,"native_wal_commit");
 check(fs::exists(path.wstring()+L"-wal")&&fs::file_size(path.wstring()+L"-wal")>0,"wal_is_present");
 auto committed=snapshot(db);
 check(committed==readonly_image(path),"wal_native_ro_exact_pages");
 scum_save::sqlite_readback(production,db,GetCurrentThreadId(),allowed_root,GetTickCount64()+30000);
 check(true,"production_readback_real_wal");
 check(sql(db,"BEGIN IMMEDIATE; INSERT INTO progress VALUES(2,'uncommitted');")==0 && !autocommit(db),"native_open_transaction");
 check(snapshot(db)!=readonly_image(path),"uncommitted_native_pages_do_not_match_disk");
 bool denied=false;try{scum_save::sqlite_readback(production,db,GetCurrentThreadId(),allowed_root,GetTickCount64()+30000);}catch(const std::exception& e){denied=std::string(e.what())=="sqlite_transaction_not_closed";}
 check(denied,"production_rejects_active_transaction");
 check(sql(db,"ROLLBACK")==0&&autocommit(db),"rollback_restores_autocommit");
 check(snapshot(db)==readonly_image(path),"rollback_can_match_old_disk_so_not_success");
 check(sql(db,"BEGIN IMMEDIATE; INSERT INTO progress VALUES(3,'committed-two'); COMMIT;")==0&&autocommit(db),"explicit_native_commit");
 check(snapshot(db)==readonly_image(path)&&snapshot(db)!=committed,"commit_full_payload_exact");
 DB* foreign{};check(open(path,&foreign,2)==0&&foreign,"native_competing_connection");
 check(sql(foreign,"BEGIN IMMEDIATE")==0,"native_foreign_lock");
 check(sql(db,"BEGIN IMMEDIATE")==5&&autocommit(db),"busy_can_leave_autocommit_true");
 check(sql(foreign,"ROLLBACK")==0&&close(foreign)==0,"native_foreign_rollback_close");
 check(sql(db,"INSERT INTO progress VALUES(1,'duplicate');")!=0,"native_constraint_failure");
 check(snapshot(db)==readonly_image(path),"failed_statement_can_match_disk_so_not_success");
 const char* filename=api<const char*(*)(DB*,const char*)>(180)(db,"main");
 check(filename&&fs::path(filename).is_absolute()&&equal_path(fs::path(filename),path),"native_filename_exact_owned_path");
 auto rejects=[&](DB* item,DWORD thread,const fs::path& root,unsigned long long deadline,const char* issue,const char* label) {
  bool rejected=false;try{scum_save::sqlite_readback(production,item,thread,root,deadline);}catch(const std::exception& e){rejected=std::string(e.what())==issue;}
  check(rejected,label);
 };
 rejects(db,GetCurrentThreadId()+1,allowed_root,GetTickCount64()+30000,"sqlite_connection_wrong_thread","production_rejects_wrong_connection_thread");
 rejects(db,GetCurrentThreadId(),allowed_root,0,"sqlite_readback_timeout","production_rejects_expired_readback");
 rejects(db,GetCurrentThreadId(),allowed_root/L"other",GetTickCount64()+30000,"sqlite_database_outside_saved","production_rejects_foreign_database_path");
 auto hardlink=allowed_root/L"world-hardlink.db";
 check(CreateHardLinkW(hardlink.c_str(),path.c_str(),nullptr)!=FALSE,"actual_owned_hardlink_created");
 rejects(db,GetCurrentThreadId(),allowed_root,GetTickCount64()+30000,"sqlite_file_not_ordinary","production_rejects_multiply_linked_db");
 check(DeleteFileW(hardlink.c_str())!=FALSE,"owned_hardlink_removed");
 auto attached=allowed_root/L"attached.db";auto attach_sql="ATTACH DATABASE '"+attached.string()+"' AS other";
 check(sql(db,attach_sql.c_str())==0,"actual_native_attached_database");
 rejects(db,GetCurrentThreadId(),allowed_root,GetTickCount64()+30000,"sqlite_attached_database","production_rejects_unclosed_attached_schema");
 check(sql(db,"DETACH DATABASE other")==0,"native_detach");
 DB* memory_db{};
 check(api<int(*)(const char*,DB**,int,const char*)>(135)(":memory:",&memory_db,6,nullptr)==0&&memory_db,"actual_native_memory_database");
 rejects(memory_db,GetCurrentThreadId(),allowed_root,GetTickCount64()+30000,"sqlite_non_file_database","production_rejects_memory_database");
 check(close(memory_db)==0,"native_memory_database_closed");
 scum_save::sqlite_readback(production,db,GetCurrentThreadId(),allowed_root,GetTickCount64()+30000);
 check(true,"production_healthy_readback_after_rejection_cases");
 check(close(db)==0&&writable_handles.empty(),"all_native_handles_closed");
 auto fault_db=[&](const wchar_t* name,bool writes,bool flushes) {
  DB* item{};auto target=allowed_root/name;require(open(target,&item,6)==0,"fault_open");
  require(sql(item,"CREATE TABLE progress(id INTEGER PRIMARY KEY); BEGIN IMMEDIATE; INSERT INTO progress VALUES(1);")==0,"fault_prepare");
  const auto before_write=write_failures,before_flush=flush_failures;
  fail_write=writes;fail_flush=flushes;const auto result=sql(item,"COMMIT");fail_write=false;fail_flush=false;
  check(result!=0&&(writes?write_failures>before_write:flush_failures>before_flush),writes?"actual_native_write_error_reaches_commit":"actual_native_flush_error_reaches_commit");
  const auto rollback=sql(item,"ROLLBACK");require(rollback==0||autocommit(item),"fault_rollback");require(close(item)==0,"fault_close");
 };
 fault_db(L"write-failure.db",true,false);fault_db(L"flush-failure.db",false,true);
 DB* close_item{};auto close_path=allowed_root/L"close-failure.db";require(open(close_path,&close_item,6)==0,"close_fault_open");
 require(sql(close_item,"CREATE TABLE progress(id INTEGER PRIMARY KEY);")==0,"close_fault_prepare");
 fail_close=true;const int closed=close(close_item);fail_close=false;
 std::cout<<"native_close_sqlite_rc="<<closed<<" os_close_failures="<<close_failures<<std::endl;
 check(close_failures>0,"actual_native_close_fault_observed");
 check(writable_handles.empty(),"fault_native_handles_reclaimed");
 {
  using Kind=scum_save::SqlKind;using Proof=scum_save::SqlCompletion;
  DB* item{};auto target=allowed_root/L"sql-result-proof.db";require(open(target,&item,6)==0,"proof_open");
  require(sql(item,"CREATE TABLE progress(id INTEGER PRIMARY KEY);")==0,"proof_schema");
  ST *begin{},*insert{},*commit{},*rollback{};
  auto prepare=[&](const char* text,ST** output){require(api<int(*)(DB*,const char*,int,ST**,const char**)>(116)(item,text,-1,output,nullptr)==0&&*output,"proof_prepare");};
  prepare("BEGIN IMMEDIATE",&begin);prepare("INSERT INTO progress VALUES(?1)",&insert);prepare("COMMIT",&commit);prepare("ROLLBACK",&rollback);
  auto step=[&](Proof& proof,ST* statement,Kind kind) {
   const bool before=autocommit(item)!=0;const auto code=api<int(*)(ST*)>(94)(statement);const bool after=autocommit(item)!=0;
   proof.step(kind,code,before,after);proof.api_result(api<int(*)(ST*)>(77)(statement));return code;
  };
  auto bind=[&](Proof& proof,int index,long long value){const auto code=api<int(*)(ST*,int,long long)>(5)(insert,index,value);proof.api_result(code);return code;};
  auto verify=[&](Proof& proof){scum_save::sqlite_readback(production,item,GetCurrentThreadId(),allowed_root,GetTickCount64()+30000);proof.readback_verified();};
  Proof implicit;require(bind(implicit,1,1)==0&&step(implicit,insert,Kind::write)==101,"implicit_write");
  check(!implicit.closed(),"production_sql_requires_full_readback_after_native_write");verify(implicit);
  check(implicit.closed(),"production_sql_accepts_native_implicit_commit_and_payload");
  Proof explicit_tx;require(step(explicit_tx,begin,Kind::begin)==101&&bind(explicit_tx,1,2)==0&&step(explicit_tx,insert,Kind::write)==101,"explicit_write");
  check(!explicit_tx.closed(),"production_sql_waits_for_actual_commit");require(step(explicit_tx,commit,Kind::commit)==101,"explicit_commit");verify(explicit_tx);
  check(explicit_tx.closed(),"production_sql_accepts_actual_commit_and_payload");
  require(bind(explicit_tx,1,4)==0&&step(explicit_tx,insert,Kind::write)==101,"later_write");
  check(!explicit_tx.closed(),"production_sql_later_native_write_reopens_readback_debt");verify(explicit_tx);check(explicit_tx.closed(),"production_sql_later_payload_reverified");
  Proof rolled;require(step(rolled,begin,Kind::begin)==101&&bind(rolled,1,3)==0&&step(rolled,insert,Kind::write)==101&&step(rolled,rollback,Kind::rollback)==101,"native_rollback_sequence");verify(rolled);
  check(!rolled.closed(),"production_sql_rejects_rollback_despite_matching_disk");
  Proof wrong_bind;check(bind(wrong_bind,2,100)==25,"actual_native_bind_range_failure");
  require(bind(wrong_bind,1,5)==0&&step(wrong_bind,insert,Kind::write)==101,"bind_failure_followed_by_successful_native_step");verify(wrong_bind);
  check(wrong_bind.failed()&&!wrong_bind.closed(),"production_sql_bind_error_cannot_be_erased_by_later_step_reset");
  Proof constraint;require(bind(constraint,1,1)==0,"constraint_bind");check(step(constraint,insert,Kind::write)!=101,"actual_native_step_failure");verify(constraint);
  check(constraint.failed()&&!constraint.closed(),"production_sql_rejects_failed_step_despite_matching_disk");
  Proof foreign_transaction;require(sql(item,"BEGIN IMMEDIATE")==0&&step(foreign_transaction,commit,Kind::commit)==101,"foreign_begin_and_actual_commit");verify(foreign_transaction);
  check(!foreign_transaction.closed(),"production_sql_cannot_borrow_foreign_transaction_begin");
  for(auto* statement:{begin,insert,commit,rollback})require(api<int(*)(ST*)>(57)(statement)==0,"proof_finalize");
  require(close(item)==0,"proof_close");check(writable_handles.empty(),"sql_proof_actual_handles_closed");
 }
 check(guard_rejected==0,"no_unowned_native_write_attempts");
 std::cout<<"SCUM fixed-package SQLite component checks="<<checks<<std::endl;return 0;
 }catch(const std::exception& e){std::cerr<<"FAIL "<<e.what()<<std::endl;return 1;}
}
