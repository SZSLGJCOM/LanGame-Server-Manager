#pragma once
#include "native_profile.hpp"
#include "native_sqlite.hpp"
#include "completion.hpp"
#include "sql_completion.hpp"
#include <map>
#include <mutex>
#include <set>

namespace scum_save {
struct ObjectIdentity {
    std::uintptr_t pointer{},type{};
    std::int32_t index{},serial{};
    bool operator==(const ObjectIdentity&) const=default;
};
// Authoritative finite diagnostic layout; never a completion/ownership input.
namespace DebtWire {
inline constexpr unsigned version=1;
enum Flag : std::uint16_t {
    SummaryComplete=1u<<0,NativeSettled=1u<<1,SqlNonempty=1u<<2,PayloadNonempty=1u<<3,
    SqlClosed=1u<<4,NoUnverifiedSql=1u<<5,AuxPresent=1u<<6,AuxOpen=1u<<7,
    PriorRejection=1u<<8,IoFailed=1u<<9,SelfComplete=1u<<10,StackAttempted=1u<<13
};
inline constexpr unsigned root_calls_shift=11,root_calls_mask=3u<<root_calls_shift,known_flags=0x3fff;
struct FlagDescription {std::string_view name;Flag mask;};
inline constexpr std::array<FlagDescription,12> flag_fields={{{"summary_complete",SummaryComplete},{"native_settled",NativeSettled},
    {"sql_nonempty",SqlNonempty},{"payload_nonempty",PayloadNonempty},{"sql_closed",SqlClosed},
    {"no_unverified_sql",NoUnverifiedSql},{"aux_present",AuxPresent},{"aux_open",AuxOpen},
    {"prior_rejection",PriorRejection},{"io_failed",IoFailed},{"self_complete",SelfComplete},{"stack_attempted",StackAttempted}}};
enum Field : unsigned {Version,Flags,OwnedScopes,OwnedIo,TaskInflight,SqlInflight,IoInflight,SelfSqlInflight,
    OriginRva,OriginBits,StackCaller,StackUpper,StackState,FieldCount};
struct Description {std::string_view name;unsigned bytes;};
inline constexpr std::array<Description,FieldCount> fields={{{"version",1},{"flags",2},{"owned_scopes",4},
    {"owned_io",4},{"task_inflight",4},{"sql_inflight",4},{"io_inflight",4},{"self_sql_inflight",4},
    {"origin_rva",4},{"origin_bits",4},{"stack_caller",4},{"stack_upper",4},{"stack_state",1}}};
constexpr unsigned offset(Field field) noexcept {unsigned result{};for(unsigned index=0;index<field;++index)result+=fields[index].bytes;return result;}
inline constexpr unsigned record_bytes=offset(FieldCount);
static_assert(record_bytes==44&&sizeof(unsigned)==4);
}
struct ForeignSqlDebt {
    std::uint16_t flags{};
    std::uint32_t owned_scopes{},owned_io{},task_inflight{},sql_inflight{},io_inflight{},self_sql_inflight{};
    bool captured{};
};
struct ForeignSqlDiagnostic {
    ForeignSqlDebt debt;
    std::uint32_t api_rva{},caller_rva{},context{};
    std::uint32_t origin_rva{},origin_bits{};
    std::uint32_t stack_caller_rva{},stack_upper_rva{};
    unsigned char stack_state{};
    bool stack_attempted{};
    bool statement_readonly_complete{},statement_readonly{},file_relation_complete{},shares_owned_file{};
};
struct ForeignQueueDiagnostic {
    std::uint32_t caller_rva{},context{},other_task_inflight{},parent_rva{},parent_context{};
    bool captured{};
};
enum class TaskKind { save,item,virtualized };
struct TaskBinding {
    TaskKind kind{TaskKind::save};
    std::uint32_t invoker{},table{},producer{},writer{},writer_table{};
    ObjectIdentity manager;
    Digest payload_hash{};
    bool operator==(const TaskBinding&) const=default;
};
struct TaskRecord {
    std::int32_t sequence{};TaskBinding binding;
    std::uintptr_t function{},payload{};
};
struct Epoch {
    Completion completion;
    // A distinct auxiliary receipt never pays this Save's roots or SQL debts.
    std::shared_ptr<Epoch> items;
    std::shared_ptr<Epoch> virtualized;
    std::weak_ptr<Epoch> parent;
    bool item_scope{},virtualized_scope{};
    std::map<std::int32_t,TaskBinding> publications;
    std::int32_t last_batched{};
    ObjectIdentity system,instance,world,worker;
    std::uintptr_t native_worker{};
    DWORD main_thread{},worker_thread{};
    std::uint64_t deadline{};
    std::map<void*,SqlCompletion> sql;
    std::set<void*> payload_connections;
    std::vector<SqlitePin> retained;
    unsigned root_calls{},owned_scopes{},io_inflight{};
    bool root_entered{},io_failed{},confirmed{};
    ForeignSqlDiagnostic first_foreign_sql;
    ForeignQueueDiagnostic first_foreign_queue;
};
using Owner=std::shared_ptr<Epoch>;
struct Database {
    std::uintptr_t object{};
    ObjectIdentity identity;
    DWORD thread{};
    bool poisoned{};
    std::vector<Handle> pins;
};
struct File {
    std::uintptr_t connection{};
    fs::path path;
    bool writable{};
    Owner owner;
};
struct Runtime {
    std::recursive_mutex mutex;
    Owner active;
    std::map<std::uintptr_t,DWORD> workers;
    unsigned task_inflight{},sql_inflight{},io_inflight{};
    std::map<void*,Database> databases;
    std::map<HANDLE,File> files;
    bool initialized{},armed{},broken{};
    const char* issue{"native_observer_unavailable"};
};
Runtime& runtime() noexcept;
extern thread_local Owner scope;
extern thread_local std::uintptr_t connection_scope;
extern thread_local std::uintptr_t opening_connection;
extern thread_local unsigned readback_depth;
extern thread_local std::vector<std::int32_t>* pump_sequences;
struct Scope {
    Owner previous;std::uintptr_t previous_connection{};
    explicit Scope(Owner owner,std::uintptr_t connection=0);
    ~Scope();
};
void unknown(const Owner& owner,const char* reason) noexcept;
void failed(const Owner& owner,const char* reason) noexcept;
ObjectIdentity identity(std::uintptr_t pointer,std::uintptr_t expected=0);
bool same_identity(const ObjectIdentity& value) noexcept;
bool initialize() noexcept;
Owner prepare_begin(std::uintptr_t system,std::uintptr_t instance,std::uintptr_t world,std::uint64_t deadline);
bool try_begin(const Owner& owner,std::uintptr_t system,std::uintptr_t instance,std::uintptr_t world);
Owner begin(std::uintptr_t system,std::uintptr_t instance,std::uintptr_t world);
void dispatch(const Owner& owner);
const char* observe(const Owner& owner,std::uintptr_t system,std::uintptr_t instance,std::uintptr_t world);
void settle_before_sql_locked() noexcept; // Caller holds Runtime::mutex, before new SQL Flight.
std::vector<Hook> task_hooks();
std::vector<Hook> sql_hooks();
std::vector<IatHook> file_imports();
void verify_databases(const Owner& owner,std::uintptr_t connection);
void verify_sqlite_connection(const Owner& owner,void* database);
enum class WorkerReadiness { ready,busy,stopping };
// Caller retains native CS18/40/68 in this order.
WorkerReadiness worker_readiness_locked(std::uintptr_t worker);
bool tasks_quiet(std::uintptr_t worker);
struct BatchFrame {
    Owner root;std::uintptr_t connection{};
    std::vector<TaskRecord> records;std::size_t cursor{};
    BatchFrame* previous{};
};
extern thread_local BatchFrame* batch_frame;
Owner admitted_scope() noexcept;
Owner task_owner(const Owner& root,std::int32_t sequence);
Owner item_receipt(const Owner& root);
Owner virtualized_receipt(const Owner& root);
TaskRecord task_record(std::uintptr_t function);
void validate_item_function(std::uintptr_t function,TaskBinding& binding);
TaskBinding virtualized_input(std::uintptr_t function,std::uintptr_t manager,std::uint32_t producer);
void validate_virtualized_function(std::uintptr_t function,const TaskBinding& binding,const Owner& root);
std::vector<Hook> dispatch_hooks();
bool sqlite_caller(std::uintptr_t caller) noexcept;
fs::path saved_root();
void pump_capture_locked(std::uintptr_t caller,CRITICAL_SECTION* section) noexcept;
Hook native_hook(std::uint32_t rva,void* replacement,void** original);
void file_failure(const Owner& owner,const char* reason) noexcept;
}
