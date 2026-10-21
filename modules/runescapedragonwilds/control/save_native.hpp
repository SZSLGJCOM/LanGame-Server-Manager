#pragma once
#include "save_completion.hpp"
#include "save_profile.hpp"
#include <atomic>
#include <memory>
#include <string>
#include <vector>
namespace dragon_save {
struct Target {
    std::uintptr_t persistence{},spud{},instance{},world{};
    bool operator==(const Target&) const = default;
};
struct Identity { std::uintptr_t object{},type{}; std::int32_t index{},serial{}; };
struct IdentityDiagnostic { const char* stage="none"; const char* role="none"; const char* reason="none"; };
void reset_identity_diagnostic() noexcept;
IdentityDiagnostic identity_diagnostic() noexcept;
struct Lock {
    SRWLOCK& value;
    explicit Lock(SRWLOCK& input) : value(input) { AcquireSRWLockExclusive(&value); }
    ~Lock() { ReleaseSRWLockExclusive(&value); }
    Lock(const Lock&)=delete;
};
struct Writer {
    std::uintptr_t object{};
    fs::path path;
    std::uint64_t index{};
    DWORD volume{};
    std::uintptr_t native_handle{};
    bool world{},closed{},os_closed{},reader{};
    std::vector<Handle> parents;
    std::unique_ptr<Snapshot> input;
};
struct Player { std::int32_t handle{}; Identity controller; };
struct Record {
    SRWLOCK lock=SRWLOCK_INIT;
    Completion completion;
    Target target;
    std::array<Identity,4> identities{};
    std::string nonce;
    std::wstring slot;
    fs::path published,incomplete,backup,version_backup;
    std::uintptr_t provider{},provider_interface{},active_state{};
    Identity provider_identity;
    bool local_player_provider{};
    std::int32_t initial_counter{};
    std::vector<Player> players;
    std::vector<Writer> writers;
    std::unique_ptr<Snapshot> closed_world;
    explicit Record(std::uint64_t now) : completion(now) {}
};
struct Scope {
    std::shared_ptr<Record> record,previous;
    explicit Scope(std::shared_ptr<Record> value);
    ~Scope();
    Scope(const Scope&)=delete;
};
extern thread_local std::shared_ptr<Record> scope_record;
extern std::atomic<std::shared_ptr<Record>> active_record;
extern std::uintptr_t image_base;
extern fs::path saved_root;
extern bool (*on_game_thread)();
bool identity(std::uintptr_t object,Identity& result) noexcept;
bool same_identity(const Identity& expected) noexcept;
bool stable(const std::shared_ptr<Record>& record) noexcept;
std::wstring native_string(std::uintptr_t string);
std::vector<Hook> sink_hooks();
bool install_file_imports() noexcept;
bool file_imports_match() noexcept;
bool initialize_native(std::uintptr_t image,const fs::path& saved,bool (*game_thread)()) noexcept;
const char* probe_target(const Target& target) noexcept;
std::shared_ptr<Record> admit(const Target& target,std::string nonce,const char*& rejection);
void dispatch(const std::shared_ptr<Record>& record);
void refresh(const std::shared_ptr<Record>& record) noexcept;
#ifdef LGSM_DRAGON_SAVE_TESTS
std::vector<Hook> test_native_hooks();
void* test_os_close(void* original) noexcept;
#endif
}
