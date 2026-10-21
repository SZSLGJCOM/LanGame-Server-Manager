// Conan Exiles Enhanced: flush the current world's dirty actors on verified builds.
#include <windows.h>
#include <cstdint>
#include <functional>
#include <memory>
#include <string_view>
#include "completion.hpp"
namespace dw::Database { class QueryResultBase; }
namespace {
constexpr std::string_view UE4SS_HASH = "91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958";
constexpr wchar_t GAME_NAME[] = L"ConanSandboxServer-Win64-Shipping.exe";
constexpr wchar_t BRIDGE_NAME[] = L"LgsmConanSaveControl.dll";
struct GameProfile {
    std::string_view image_hash, provider_hash, save_prologue;
    uintptr_t save, component_class, weak_owner;
};
constexpr GameProfile PROFILES[]{
    {"2c3fef162b159bd16124b7ea8030884aa1f66e7b1d966e63542407e590d04fa1",
     "d076d5d223360f133e365ef37901ff9789eb68186a8f6d66494038f4864684b2",
     "48895c24184889742420574881ec80000000488b05476f31064833c448894424", 0x44dd2a0, 0xaac3dd0, 0x167e230},
    {"d0ea317e7e037e68454414f12828d5995cc9e519b9c34eda264be1954d352b14",
     "2eec9aa9aa443fb476db583cfdc5044285ccc246df0a60e93155fb37e14324b6",
     "48895c24184889742420574881ec80000000488b0597e133064833c448894424", 0x44e3350, 0xaaf0d70, 0x167faf0},
};
const GameProfile* selected_profile{};
bool select_game_profile(std::string_view identity) {
    selected_profile = nullptr;
    for (const auto& profile : PROFILES) {
        if (identity == profile.image_hash) { selected_profile = &profile; return true; }
    }
    return false;
}
const GameProfile& game_profile();
void target_contract(uintptr_t, uintptr_t, uintptr_t, uintptr_t);
struct Request;
const char* native_save_async(uintptr_t, const Request&);
const char* native_observe_async(uintptr_t, const Request&);
}
#define LGSM_SAVE_EXPORT luaopen_lgsm_conan_save
#define LGSM_NATIVE_SAVE_ASYNC
#define LGSM_SELECT_GAME_PROFILE
#define SAVE_RVA game_profile().save
#define SAVE_PROLOGUE game_profile().save_prologue
#include "../../../apps/desktop/src-tauri/native/world_save_bridge.hpp"
namespace {
const GameProfile& game_profile() {
    require(selected_profile != nullptr, "game_profile_unavailable");
    return *selected_profile;
}
struct Provider {
    HMODULE module{};
    Handle file;
    void* (*thread_id)(uint32_t*){};
    uint64_t (*result_count)(void*){};
    int (*result_status)(void*, unsigned){};
    bool (*result_done)(void*){};
    unsigned (*result_index)(void*){};
    uintptr_t base{};
    Provider() {
        auto executable = module_path(GetModuleHandleW(nullptr));
        module = GetModuleHandleW(L"Dreamworld.dll");
        require(module && same_file(module_path(module), executable.parent_path() / L"Dreamworld.dll"), "provider_module");
        auto opened = open_file(module_path(module), GENERIC_READ, OPEN_EXISTING, FILE_SHARE_READ);
        file.value = opened.value; opened.value = INVALID_HANDLE_VALUE;
        require(hash_file(file.value) == game_profile().provider_hash, "provider_fingerprint");
        thread_id = reinterpret_cast<void* (*)(uint32_t*)>(GetProcAddress(module,
            "?GetCurrentThreadId@Thread@dw@@SA?AVid@thread@std@@XZ"));
        require(thread_id != nullptr, "provider_thread_api");
        base = reinterpret_cast<uintptr_t>(module);
        result_count = reinterpret_cast<decltype(result_count)>(GetProcAddress(module,
            "?GetNumResults@QueryResultSQLite@Database@dw@@UEBA_KXZ"));
        result_status = reinterpret_cast<decltype(result_status)>(GetProcAddress(module,
            "?GetStatus@QueryResultSQLite@Database@dw@@UEBA?AW4QueryStatus@23@I@Z"));
        result_done = reinterpret_cast<decltype(result_done)>(GetProcAddress(module,
            "?IsDone@QueryResultSQLite@Database@dw@@UEBA_NXZ"));
        result_index = reinterpret_cast<decltype(result_index)>(GetProcAddress(module,
            "?GetResultIndex@QueryResultSQLite@Database@dw@@UEBAIXZ"));
        require(reinterpret_cast<uintptr_t>(result_count) == base + 0x3378
            && reinterpret_cast<uintptr_t>(result_status) == base + 0x9fa2
            && reinterpret_cast<uintptr_t>(result_done) == base + 0x2fe0
            && reinterpret_cast<uintptr_t>(result_index) == base + 0x10dc, "provider_result_api");
        verify();
    }
    void verify() const {
        prologue(base, 0x3378, "e983501000");
        prologue(base, 0x9fa2, "e9e9e40f00");
        prologue(base, 0x2fe0, "e9fb591000");
        prologue(base, 0x10dc, "e99f731000");
        prologue(base, 0x108400, "488bc1488b4910482b480848b8797878787878787848f7e9");
        prologue(base, 0x108490, "4c8b410848b87978787878787878488b4910448bca492bc8");
        prologue(base, 0x1089e0, "0fb64124c3");
        prologue(base, 0x108480, "8b4120c3");
        prologue(base, 0x1182f0, "4055535657415441564157488d6c24f04881ec1001000048");
        prologue(base, 0x11bb90, "488bc4488958205556574154415541564157488da838faff");
    }
};
Provider& provider() { static Provider value; return value; }
void target_contract(uintptr_t image, uintptr_t component, uintptr_t game_mode, uintptr_t world) {
    provider().verify();
    auto cls = read<uintptr_t>(image + game_profile().component_class);
    require(cls && readable(component, 0x240) && read<uintptr_t>(component + 0x10) == cls
        && !(read<uint32_t>(component + 8) & 0x40018010), "component_class");
    require(readable(game_mode, 0x5b0) && !(read<uint32_t>(game_mode + 8) & 0x40018010)
        && read<uintptr_t>(component + 0x20) == game_mode, "component_owner");
    require(readable(world, 0x28) && !(read<uint32_t>(world + 8) & 0x40018010)
        && read<uintptr_t>(component + 0xc0) == world, "component_world");
    using WeakOwner = void* (*)(void*);
    require(reinterpret_cast<uintptr_t>(reinterpret_cast<WeakOwner>(image + game_profile().weak_owner)(
        reinterpret_cast<void*>(component + 0x150))) == game_mode, "component_game_mode");
    auto db = read<uintptr_t>(game_mode + 0x5a0);
    require(db && readable(db, sizeof(void*)), "database_unavailable");
    auto get_state = read<uintptr_t>(read<uintptr_t>(db) + 0x70);
    HMODULE state_module{};
    require(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCWSTR>(get_state), &state_module) && state_module == provider().module, "database_api");
    using State = int (*)(void*);
    require(reinterpret_cast<State>(get_state)(reinterpret_cast<void*>(db)) == 2, "database_not_ready");
}
using NativeResult = std::shared_ptr<dw::Database::QueryResultBase>;
struct NativeJob {
    std::function<void(NativeResult)> function;
    uint32_t thread{};
    uint32_t padding{};
};
static_assert(sizeof(NativeResult) == 0x10 && sizeof(std::function<void(NativeResult)>) == 0x40);
static_assert(sizeof(NativeJob) == 0x48 && offsetof(NativeJob, thread) == 0x40);
struct Pending {
    Request request;
    std::shared_ptr<lgsm_conan::Completion> completion;
};
std::unique_ptr<Pending> active;
bool same_request(const Request& a, const Request& b) {
    return a.nonce == b.nonce && a.operation == b.operation && a.requested_at == b.requested_at
        && a.proxy == b.proxy && a.instance == b.instance && a.world == b.world;
}
lgsm_conan::QueryResult query_result(void* object) {
    auto& api = provider();
    const auto address = reinterpret_cast<uintptr_t>(object);
    require(readable(address, 0x28) && read<uintptr_t>(address) == api.base + 0x37be08, "query_result_type");
    const auto begin = read<uintptr_t>(address + 8), end = read<uintptr_t>(address + 0x10);
    require(begin && end >= begin && (end - begin) % 0x88 == 0, "query_result_bounds");
    const auto count = (end - begin) / 0x88;
    require(count >= 1 && count <= lgsm_conan::MAX_SQL_RESULTS && readable(begin, static_cast<size_t>(end - begin))
        && api.result_count(object) == count, "query_result_count");
    bool error = false;
    for (unsigned i = 0; i < count; ++i) error = api.result_status(object, i) != 0 || error;
    return {api.result_index(object), count, api.result_done(object), error};
}
bool guarded_result(void* object, lgsm_conan::QueryResult* result) {
    __try { *result = query_result(object); return true; }
    __except (GetExceptionCode() == EXCEPTION_ACCESS_VIOLATION ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH) {
        return false;
    }
}
void pin_callback_modules() {
    HMODULE pinned{};
    require(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<LPCWSTR>(&pin_callback_modules), &pinned), "callback_module_pin");
    require(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<LPCWSTR>(provider().thread_id), &pinned) && pinned == provider().module, "provider_module_pin");
}
const char* native_save_async(uintptr_t image, const Request& request) {
    // There is at most one outstanding native job. Timeouts never cancel it or
    // allow another job to borrow its result; the native closure owns its state.
    require(!active || !active->completion->busy(), "native_save_busy");
    pin_callback_modules();
    auto state = std::make_shared<lgsm_conan::Completion>(GetTickCount64());
    active = std::make_unique<Pending>(Pending{request, state});
    NativeJob job{[state](NativeResult result) noexcept {
        lgsm_conan::QueryResult value;
        try {
            if (guarded_result(result.get(), &value)) { state->complete(value, GetTickCount64()); return; }
        } catch (...) { OutputDebugStringA("[LgsmWorldSave] Owned query result unavailable.\n"); }
        state->invalid();
    }};
    provider().thread_id(&job.thread);
    using Save = void (*)(void*, NativeJob*);
    reinterpret_cast<Save>(image + SAVE_RVA)(reinterpret_cast<void*>(request.proxy), &job);
    return lgsm_conan::text(state->observe(GetTickCount64()));
}
const char* native_observe_async(uintptr_t, const Request& request) {
    require(active && same_request(active->request, request), "native_save_identity");
    return lgsm_conan::text(active->completion->observe(GetTickCount64()));
}
}
