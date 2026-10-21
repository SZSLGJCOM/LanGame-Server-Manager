#include "native_state.hpp"
#include <tuple>
#include <intrin.h>
#ifdef LGSM_SCUM_SAVE_TESTS
#include <cstdio>
#endif

namespace scum_save {
namespace {
// This frame is observation only. It never creates Scope or SQL receipts.
struct StepOrigin;
thread_local StepOrigin* step_origin{};
struct StepOrigin {
    void* statement;std::uintptr_t caller;StepOrigin* previous;
    StepOrigin(void* value,std::uintptr_t source) noexcept : statement(value),caller(source),previous(step_origin) {step_origin=this;}
    ~StepOrigin() noexcept {step_origin=previous;}
};
constexpr std::uint32_t begin_returns[]={0x1adb738,0x1adc27e,0x1adce8c,0x1adf90b,
    0x1ae7ad8,0x1af2101,0x1af65c9,0x1af6f0e,0x1af8dcf,0x1afe085,0x1b04b5d,0x1b056ea};
bool known_begin_return(std::uint32_t rva) noexcept {
    for(const auto known:begin_returns)if(rva==known)return true;return false;
}
// Observation only. Never scan past an unknown parent or create a receipt.
// Only this POD helper handles observer faults; original APIs stay outside it.
__declspec(noinline) void capture_begin_stack(std::uintptr_t base,ForeignSqlDiagnostic& value) noexcept {
    const auto error=GetLastError();value.stack_attempted=true;value.stack_state=2;
    __try {
        CONTEXT context{};RtlCaptureContext(&context);
        const auto* tib=reinterpret_cast<const NT_TIB*>(NtCurrentTeb());
        const auto low=reinterpret_cast<std::uintptr_t>(tib->StackLimit);
        const auto high=reinterpret_cast<std::uintptr_t>(tib->StackBase);
        bool anchor{};
        for(unsigned index=0;index<16;++index) {
            if(!context.Rip){value.stack_state=anchor?2:0;break;}
            if(context.Rsp<low || context.Rsp>=high || high-context.Rsp<sizeof(void*))break;
            DWORD64 module{};auto* entry=RtlLookupFunctionEntry(context.Rip,&module,nullptr);
            if(!entry || entry->BeginAddress>=entry->EndAddress || context.Rip<module ||
                context.Rip-module<entry->BeginAddress || context.Rip-module>=entry->EndAddress)break;
            const bool native=module==base && context.Rip>base && context.Rip-base<0x7e10000U;
            const auto rva=native?static_cast<std::uint32_t>(context.Rip-base):0;
            if(anchor) {
                if(!native){value.stack_state=value.stack_caller_rva?6:4;break;}
                if(!value.stack_caller_rva) {
                    value.stack_caller_rva=rva;
                    if(!known_begin_return(rva)){value.stack_state=5;break;}
                } else {value.stack_upper_rva=rva;value.stack_state=7;break;}
            } else if(rva==0x1ab4b83) {
                // Exact fixed-PE pdata fragment, chained to logical 1ab4ad0.
                if(entry->BeginAddress!=0x1ab4b2a || entry->EndAddress!=0x1ab4b9f || entry->UnwindData!=0x6dd40c0)break;
                anchor=true;
            }
            if(index==15){value.stack_state=1;break;}
            const auto before=context.Rsp;PVOID handler{};DWORD64 establisher{};
            RtlVirtualUnwind(UNW_FLAG_NHANDLER,module,context.Rip,entry,&context,&handler,&establisher,nullptr);
            if(context.Rsp<=before || context.Rsp>high)break;
        }
    } __except(EXCEPTION_EXECUTE_HANDLER) {
        value.stack_caller_rva=0;value.stack_upper_rva=0;value.stack_state=3;
    }
    SetLastError(error);
}
bool(*open_original)(std::uintptr_t,void*,unsigned char){};
int(*step_original)(void*){};
int(*public_step_original)(void*){};
int(*exec_original)(void*,const char*,void*,void*,char**){};
int(*close_original)(void*,int){};
int(*prepare_original)(void*,const char*,int,unsigned,void*,void**,const char**){};
// Diagnostic only, mirroring entered-not-retired Flights on this thread.
// A skipped destructor leaves both the original global debt and this debt.
struct SqlFlights {unsigned count;bool complete;};
thread_local SqlFlights sql_flights{0,true};
struct Flight {
    Flight(){std::lock_guard lock(runtime().mutex);settle_before_sql_locked();++runtime().sql_inflight;
        if(sql_flights.complete&&sql_flights.count!=UINT32_MAX)++sql_flights.count;
        else {sql_flights.complete=false;sql_flights.count=0;}}
    ~Flight(){std::lock_guard lock(runtime().mutex);--runtime().sql_inflight;
        if(sql_flights.complete&&sql_flights.count)--sql_flights.count;
        else {sql_flights.complete=false;sql_flights.count=0;}}
};
// Runtime::mutex is already held. No native object, SQL, IO or authorization.
ForeignSqlDebt capture_sql_debt(const Owner& owner,const Runtime& state) noexcept {
    using namespace DebtWire;ForeignSqlDebt value{};value.captured=true;
    value.owned_scopes=owner->owned_scopes;value.owned_io=owner->io_inflight;
    value.task_inflight=state.task_inflight;value.sql_inflight=state.sql_inflight;value.io_inflight=state.io_inflight;
    const bool self_complete=sql_flights.complete&&sql_flights.count<=state.sql_inflight;
    value.self_sql_inflight=self_complete?sql_flights.count:0;
    value.flags=static_cast<std::uint16_t>((owner->completion.native_settled()?NativeSettled:0u)|
        (!owner->sql.empty()?SqlNonempty:0u)|(!owner->payload_connections.empty()?PayloadNonempty:0u)|
        ((owner->items||owner->virtualized)?AuxPresent:0u)|(owner->completion.rejected()?PriorRejection:0u)|
        (owner->io_failed?IoFailed:0u)|(self_complete?SelfComplete:0u)|
        ((owner->root_calls<2?owner->root_calls:2u)<<root_calls_shift));
    bool complete=owner->sql.size()<=64,closed=!owner->sql.empty(),verified=closed,aux_open{};
    if(complete)for(const auto& [database,proof]:owner->sql) {
        (void)database;closed=closed&&proof.closed();verified=verified&&!proof.unverified();
    }
    for(const auto& item:{owner->items,owner->virtualized})if(item) {
        if(item->sql.size()>64){complete=false;continue;}
        bool settled=(item->item_scope!=item->virtualized_scope)&&item->parent.lock()==owner&&item->completion.tasks_settled()&&
            !item->completion.rejected()&&!item->owned_scopes&&!item->io_inflight&&!item->io_failed&&
            !item->sql.empty()&&(item->virtualized_scope||!item->payload_connections.empty());
        for(const auto& [database,proof]:item->sql){(void)database;settled=settled&&proof.closed();}
        aux_open=aux_open||!settled;
    }
    if(complete)value.flags=static_cast<std::uint16_t>(value.flags|SummaryComplete|(closed?SqlClosed:0u)|
        (verified?NoUnverifiedSql:0u)|(aux_open?AuxOpen:0u));
    return value;
}
void* statement_database(void* statement) {
    return reinterpret_cast<void*(*)(void*)>(image_base()+0x28f1bf0)(statement);
}
// Finite observation only. First foreign call stays intact; no SQL, path,
// pointer, thread ID or request identity is serialized. The rejection below
// remains unchanged regardless of whether these observations are available.
void foreign_sql_diagnostic(const Owner& owner,void* database,std::uint32_t api,std::uintptr_t caller,void* statement) noexcept {
    if(owner->first_foreign_sql.api_rva)return;
    const auto error=GetLastError();ForeignSqlDiagnostic value{};value.api_rva=api;
    value.debt=capture_sql_debt(owner,runtime());
    const auto base=image_base();
    if(base&&caller>=base&&caller-base<0x7e10000)value.caller_rva=static_cast<std::uint32_t>(caller-base);
    const auto thread=GetCurrentThreadId();auto& state=runtime();
    value.context=(scope?1u:0u)|(scope!=owner?2u:0u)|(owner->main_thread==thread?16u:0u)
        |(owner->worker_thread==thread?32u:0u)|(owner->root_entered?256u:0u)|(owner->completion.rejected()?8192u:0u);
    const auto found=state.databases.find(database);
    if(owner->sql.contains(database))value.context|=8u;
    if(found!=state.databases.end()) {
        value.context|=4u;if(found->second.thread==thread)value.context|=64u;
        if(connection_scope&&connection_scope==found->second.object)value.context|=128u;
        void* native_database{};DWORD native_thread{};
        if(read(found->second.object+0x40,native_database)){value.context|=512u;if(native_database==database)value.context|=1024u;}
        if(read(found->second.object+0x48,native_thread)){value.context|=2048u;if(native_thread==thread)value.context|=4096u;}
        if(!found->second.pins.empty()&&!owner->sql.empty()&&owner->sql.size()<=64) {
            BY_HANDLE_FILE_INFORMATION target{};
            if(GetFileInformationByHandle(found->second.pins.back().value,&target)) {
                value.file_relation_complete=true;
                for(const auto& [owned,proof]:owner->sql) {
                    (void)proof;const auto item=state.databases.find(owned);BY_HANDLE_FILE_INFORMATION candidate{};
                    if(item==state.databases.end()||item->second.pins.empty()||
                        !GetFileInformationByHandle(item->second.pins.back().value,&candidate)){value.file_relation_complete=false;continue;}
                    if(target.dwVolumeSerialNumber==candidate.dwVolumeSerialNumber&&target.nFileIndexHigh==candidate.nFileIndexHigh
                        &&target.nFileIndexLow==candidate.nFileIndexLow){value.shares_owned_file=true;break;}
                }
                if(value.shares_owned_file)value.file_relation_complete=true;
            }
        }
    }
    // This exact read-only SQLite accessor is already used by kind() below.
    // Only valid statement paths pass a statement; prepare/exec/close do not.
    if(statement&&base&&code_matches()) {
        value.statement_readonly=reinterpret_cast<int(*)(void*)>(base+0x28ee010)(statement)!=0;
        value.statement_readonly_complete=true;
        // Only this exact statement's nearest public-step frame may identify
        // an outer caller. Cached kind/transaction state remain observations.
        if(api==0x296ab40 && step_origin && step_origin->statement==statement) {
            value.origin_bits=1;
            if(step_origin->caller>base && step_origin->caller-base<0x7e10000U) {
                value.origin_rva=static_cast<std::uint32_t>(step_origin->caller-base);value.origin_bits|=2;
            }
            try {
                if(found!=state.databases.end() && found->second.thread==thread && same_identity(found->second.identity) &&
                    require_read<void*>(found->second.object+0x40)==database &&
                    require_read<DWORD>(found->second.object+0x48)==thread) {
                    value.origin_bits|=4;
                    void* cached[4]{};bool complete=true;
                    for(unsigned index=0;index<4;++index)
                        if(!read(found->second.object+0x50+index*8,cached[index]) || !cached[index])complete=false;
                    if(complete) {
                        unsigned kind=5,matches=0;
                        for(unsigned index=0;index<4;++index)if(cached[index]==statement){kind=index+1;++matches;}
                        if(matches<=1)value.origin_bits|=kind<<3;
                    }
                    const auto autocommit=Sqlite(base).autocommit(database);
                    if(autocommit==0 || autocommit==1)value.origin_bits|=64u|(autocommit?128u:0u);
                }
            }catch(...) {} // Incomplete observation cannot affect rejection.
            if(value.origin_rva==0x1ab4b83)capture_begin_stack(base,value);
        }
    }
    owner->first_foreign_sql=value;SetLastError(error);
}
Owner admit(void* database,std::uint32_t api,std::uintptr_t caller,void* statement=nullptr) noexcept {
    try {
        if(readback_depth)return {};
        auto& state=runtime();std::lock_guard lock(state.mutex);const auto owner=state.active;
        if(!owner||owner->confirmed)return {};
        const auto receipt=admitted_scope();
        if(!receipt){foreign_sql_diagnostic(owner,database,api,caller,statement);unknown(owner,"foreign_native_sql");return {};}
        const auto found=state.databases.find(database);
        if(found==state.databases.end()||found->second.thread!=GetCurrentThreadId()||
            !same_identity(found->second.identity)||require_read<void*>(found->second.object+0x40)!=database||
            require_read<DWORD>(found->second.object+0x48)!=GetCurrentThreadId()) {
            unknown(owner,"native_sql_connection_identity");return {};
        }
        if((receipt->item_scope||receipt->virtualized_scope)&&(!connection_scope||connection_scope!=found->second.object)) {
            unknown(receipt,"native_item_sql_connection");return {};
        }
        receipt->sql.try_emplace(database);return receipt;
    }catch(...){unknown(scope,"native_sql_connection_identity");return {};}
}
void api_result(void* statement,int result,std::uintptr_t caller) noexcept {
    try {const auto owner=admit(statement_database(statement),0x28eeec0,caller,statement);if(owner) {
        std::lock_guard lock(runtime().mutex);owner->sql.at(statement_database(statement)).api_result(result);
        if(result)failed(owner,"native_sql_api_failed");
    }}catch(...){unknown(scope,"native_sql_api_result");}
}
SqlKind kind(void* database,void* statement) {
    const auto& entry=runtime().databases.at(database);const auto object=entry.object;
    if(statement==require_read<void*>(object+0x50)||statement==require_read<void*>(object+0x58))return SqlKind::begin;
    if(statement==require_read<void*>(object+0x60))return SqlKind::commit;
    if(statement==require_read<void*>(object+0x68))return SqlKind::rollback;
    return reinterpret_cast<int(*)(void*)>(image_base()+0x28ee010)(statement)?SqlKind::read:SqlKind::write;
}
bool open_hook(std::uintptr_t object,void* path,unsigned char mode) {
    Flight flight;const auto previous=opening_connection;opening_connection=object;
    bool result{};
    try {result=open_original(object,path,mode);}catch(...){opening_connection=previous;unknown(scope,"native_connection_open_exception");throw;}
    opening_connection=previous;
    try {
        if(result&&require_read<unsigned char>(object+0x38)) {
            const auto database=require_read<void*>(object+0x40);const auto thread=require_read<DWORD>(object+0x48);
            if(!database||thread!=GetCurrentThreadId())throw std::runtime_error("native_connection_thread");
            auto object_id=identity(object,require_read<std::uintptr_t>(image_base()+0x7445308));
            const auto filename=sqlite_filename(Sqlite(image_base()),database);
            auto pins=pin_parent(filename);
            Handle leaf(CreateFileW(filename.c_str(),GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,
                OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,nullptr));BY_HANDLE_FILE_INFORMATION info{};
            if(leaf.value==INVALID_HANDLE_VALUE||!GetFileInformationByHandle(leaf.value,&info)||info.nNumberOfLinks!=1||
                (info.dwFileAttributes&(FILE_ATTRIBUTE_DIRECTORY|FILE_ATTRIBUTE_REPARSE_POINT)))throw std::runtime_error("native_database_file");
            pins.push_back(std::move(leaf));
            std::lock_guard lock(runtime().mutex);
            auto found=runtime().databases.find(database);
            if(found!=runtime().databases.end())throw std::runtime_error("native_connection_reused");
            Database entry;entry.object=object;entry.identity=object_id;entry.thread=thread;entry.pins=std::move(pins);
            runtime().databases.emplace(database,std::move(entry));
        } else if(scope)failed(scope,"native_connection_open_failed");
    }catch(...){std::lock_guard lock(runtime().mutex);runtime().broken=true;runtime().issue="native_connection_unobserved";
        unknown(scope,"native_connection_unobserved");}
    return result;
}
int step_hook(void* statement) {
    Flight flight;void* database{};Owner owner;SqlKind operation{SqlKind::read};bool before{};
    try {
        database=statement_database(statement);owner=admit(database,0x296ab40,reinterpret_cast<std::uintptr_t>(_ReturnAddress()),statement);
        if(owner){std::lock_guard lock(runtime().mutex);operation=kind(database,statement);before=Sqlite(image_base()).autocommit(database)==1;}
    }catch(...){unknown(scope,"native_statement_identity");}
    const auto result=step_original(statement);
    if(owner)try {
        const bool after=Sqlite(image_base()).autocommit(database)==1;
        std::lock_guard lock(runtime().mutex);
        // SQLITE_SCHEMA may be transparently recompiled by the public API.
        // Its final public result is independently observed below.
        if(result!=17)owner->sql.at(database).step(operation,result,before,after);
        if(result!=17&&result!=100&&result!=101)failed(owner,"native_sql_step_failed");
        if(operation==SqlKind::rollback)unknown(owner,"native_sql_rolled_back");
        if(operation==SqlKind::write&&result==101)owner->payload_connections.insert(database);
    }catch(...){unknown(owner,"native_sql_step_result");}
    return result;
}
int public_step_hook(void* statement) {
    struct LastError {DWORD value;~LastError(){SetLastError(value);}} error{GetLastError()};
    Flight flight;StepOrigin origin(statement,reinterpret_cast<std::uintptr_t>(_ReturnAddress()));
    int result{};
    try {result=public_step_original(statement);}catch(...){error.value=GetLastError();throw;}
    error.value=GetLastError();
    if(result!=100&&result!=101)api_result(statement,result,reinterpret_cast<std::uintptr_t>(_ReturnAddress()));return result;
}
int exec_hook(void* database,const char* sql,void* callback,void* argument,char** message) {
    Flight flight;const auto owner=admit(database,0x28ea340,reinterpret_cast<std::uintptr_t>(_ReturnAddress()));const auto result=exec_original(database,sql,callback,argument,message);
    if(owner){std::lock_guard lock(runtime().mutex);owner->sql.at(database).api_result(result);if(result)failed(owner,"native_sql_exec_failed");}
    return result;
}
int prepare_hook(void* database,const char* sql,int bytes,unsigned flags,void* previous,void** statement,const char** tail) {
    Flight flight;const auto owner=admit(database,0x2956c20,reinterpret_cast<std::uintptr_t>(_ReturnAddress()));
    const auto result=prepare_original(database,sql,bytes,flags,previous,statement,tail);
    if(owner&&result){std::lock_guard lock(runtime().mutex);owner->sql.at(database).api_result(result);failed(owner,"native_sql_prepare_failed");}
    return result;
}
int close_hook(void* database,int zombie) {
    Flight flight;const auto owner=admit(database,0x293ba90,reinterpret_cast<std::uintptr_t>(_ReturnAddress()));const auto result=close_original(database,zombie);
    std::lock_guard lock(runtime().mutex);
    if(owner)unknown(owner,"native_connection_closed_during_save");
    // close_v2 may defer destruction while statements survive. Retain its pins
    // and reject reuse rather than infer a terminal close from SQLITE_OK.
    if(!result&&!zombie)runtime().databases.erase(database);
    return result;
}
template<unsigned Address,class Signature>struct Api;
template<unsigned Address,class... Args>struct Api<Address,int(void*,Args...)> {
    static inline int(*original)(void*,Args...){};
    static int call(void* statement,Args... arguments) {
        Flight flight;
        if(!statement)return original(statement,arguments...);
        // Capture db_handle before finalize destroys its statement.
        void* database{};Owner owner;
        try{database=statement_database(statement);owner=admit(database,Address,reinterpret_cast<std::uintptr_t>(_ReturnAddress()),statement);}catch(...){unknown(scope,"native_sql_api_identity");}
        const int result=original(statement,arguments...);
        if(owner){std::lock_guard lock(runtime().mutex);owner->sql.at(database).api_result(result);if(result)failed(owner,"native_sql_api_failed");}
        return result;
    }
    static Hook hook(){return native_hook(Address,reinterpret_cast<void*>(call),reinterpret_cast<void**>(&original));}
};
using Destructor=void(*)(void*);
}
void verify_sqlite_connection(const Owner& owner,void* database) {
    try {
        auto& state=runtime();DWORD thread{};
        {std::lock_guard lock(state.mutex);const auto found=state.databases.find(database);
            if(found==state.databases.end()||!same_identity(found->second.identity))throw std::runtime_error("native_connection_identity");
            thread=found->second.thread;if(!owner->sql.contains(database))return;}
        struct Readback {Readback(){++readback_depth;}~Readback(){--readback_depth;}} entered;
        sqlite_readback(Sqlite(image_base()),database,thread,saved_root(),owner->deadline);
        std::lock_guard lock(state.mutex);owner->sql.at(database).readback_verified();
    }catch(const std::exception& error){
#ifdef LGSM_SCUM_SAVE_TESTS
        std::fprintf(stderr,"SCUM component readback boundary: %s\n",error.what());
#else
        (void)error;
#endif
        failed(owner,"native_sql_readback_failed");
    }catch(...){failed(owner,"native_sql_readback_failed");}
}
void verify_databases(const Owner& owner,std::uintptr_t connection) {
    std::vector<void*> databases;
    {std::lock_guard lock(runtime().mutex);for(const auto& [database,proof]:owner->sql) {
        const auto found=runtime().databases.find(database);if(found==runtime().databases.end()){unknown(owner,"native_connection_lost");continue;}
        if(found->second.thread==GetCurrentThreadId()&&(!connection||found->second.object==connection)&&proof.unverified())databases.push_back(database);
    }}
    for(auto* database:databases)verify_sqlite_connection(owner,database);
}
bool sqlite_caller(std::uintptr_t caller) noexcept {return caller>=image_base()+0x28dc000&&caller<image_base()+0x29a9000;}
std::vector<Hook> sql_hooks() {
    return {native_hook(0x1acc820,reinterpret_cast<void*>(open_hook),reinterpret_cast<void**>(&open_original)),
        native_hook(0x296ab40,reinterpret_cast<void*>(step_hook),reinterpret_cast<void**>(&step_original)),
        native_hook(0x28eeec0,reinterpret_cast<void*>(public_step_hook),reinterpret_cast<void**>(&public_step_original)),
        native_hook(0x28ea340,reinterpret_cast<void*>(exec_hook),reinterpret_cast<void**>(&exec_original)),
        native_hook(0x2956c20,reinterpret_cast<void*>(prepare_hook),reinterpret_cast<void**>(&prepare_original)),
        native_hook(0x293ba90,reinterpret_cast<void*>(close_hook),reinterpret_cast<void**>(&close_original)),
        Api<0x28ee0c0,int(void*,int,const void*,int,Destructor)>::hook(),
        Api<0x28ee190,int(void*,int,const void*,std::uint64_t,Destructor)>::hook(),
        Api<0x28ee320,int(void*,int,double)>::hook(),Api<0x28ee3a0,int(void*,int,std::int64_t)>::hook(),
        Api<0x28ee430,int(void*,int)>::hook(),Api<0x28ee4a0,int(void*,int,const void*,int,Destructor)>::hook(),
        Api<0x28ee5b0,int(void*,int,const void*,int,Destructor)>::hook(),
        Api<0x28ee5d0,int(void*,int,const void*,std::uint64_t,Destructor,unsigned char)>::hook(),
        Api<0x28ee5f0,int(void*,int,void*)>::hook(),Api<0x28ee7e0,int(void*,int,void*,const char*,Destructor)>::hook(),
        Api<0x28ee990,int(void*,int,int)>::hook(),Api<0x28eea30,int(void*,int,std::uint64_t)>::hook(),
        Api<0x28eebd0,int(void*)>::hook(),Api<0x28f0130,int(void*)>::hook(),Api<0x28f0230,int(void*)>::hook()};
}
}
