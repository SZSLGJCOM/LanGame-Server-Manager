#include "native_profile.h"
#include "control_files.h"
#include "trampoline_unwind.h"
#include "request_history.h"
#include "vendor/minhook/include/MinHook.h"
#include <cwchar>
#include <cstring>
extern "C" void ControlUpdateObserver(), ControlCopyObserver();
extern "C" { void* g_updateOriginal{}, *g_copyOriginal{}; }
namespace {
using namespace enshrouded_control;
HMODULE g_self{};
HANDLE g_stop{}, g_worker{}, g_realDbghelp{};
SRWLOCK g_stateLock = SRWLOCK_INIT, g_stopLock = SRWLOCK_INIT;
IdentityCache g_cache{};
Snapshot g_latest{};
NativeProfile g_profile{};
TrampolineUnwind g_updateUnwind{}, g_copyUnwind{};
ProcessIdentity g_identity{};
Request g_pending{};
Response g_completed{};
bool g_hasPending{}, g_hasCompleted{}, g_dispatching{};
volatile LONG g_stopping{}, g_ownerThread{}, g_ready{};
std::uint64_t g_sequence{};
struct NativeEvent { std::uint64_t reserved{}, accountHash{}; std::uint32_t reserved2{}; std::uint8_t action{}; std::uint8_t padding[3]{}; };
static_assert(sizeof(NativeEvent) == 0x18 && offsetof(NativeEvent, accountHash) == 8 && offsetof(NativeEvent, action) == 0x14);

std::wstring ControlRoot() {
    const auto path = ModuleFile(g_self); const auto end = path.find_last_of(L"\\/");
    return end == std::wstring::npos ? std::wstring() : path.substr(0, end + 1) + L".langame-enshrouded-control";
}
bool Status(const std::wstring& root, const char* state, const char* issue) {
    return WriteAtomicFile(root + L"\\status.json", "{\"protocol\":1,\"pid\":" + std::to_string(GetCurrentProcessId())
        + ",\"state\":\"" + state + "\",\"issue\":\"" + issue + "\",\"exe_sha256\":\"" + kExeSha256 + "\"}");
}
void RejectPending(const char* issue) noexcept {
    if (!g_hasPending) return;
    g_completed = {g_pending, false, issue}; g_hasCompleted = true; g_hasPending = false;
}
void StopSampling() noexcept {
    InterlockedExchange(&g_stopping, 1);
    AcquireSRWLockExclusive(&g_stateLock); RejectPending("server_stopping"); ReleaseSRWLockExclusive(&g_stateLock);
    // Hooks, observer code and trampolines stay pinned until process exit. A
    // returned callback alone cannot establish that the assembly path has left.
}
bool InstallHooks() noexcept {
    HMODULE pinned{};
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<const wchar_t*>(&ControlUpdateObserver), &pinned) || MH_Initialize() != MH_OK) return false;
    if (MH_CreateHook(g_profile.updatePlayers, reinterpret_cast<void*>(&ControlUpdateObserver), &g_updateOriginal) != MH_OK
        || MH_CreateHook(g_profile.localCopy, reinterpret_cast<void*>(&ControlCopyObserver), &g_copyOriginal) != MH_OK) return false;
    if (!RegisterTrampolineUnwind(g_updateUnwind, g_updateOriginal, g_profile.updatePlayers, TrampolineKind::UpdatePlayers)
        || !RegisterTrampolineUnwind(g_copyUnwind, g_copyOriginal, g_profile.localCopy, TrampolineKind::LocalCopy)) return false;
    InterlockedExchange(&g_ready, 1);
    return MH_EnableHook(g_profile.localCopy) == MH_OK && MH_EnableHook(g_profile.updatePlayers) == MH_OK;
}
DWORD WINAPI Worker(void*) noexcept {
    try {
        const auto root = ControlRoot(); std::string enabled;
        if (root.empty() || !PlainDirectory(root) || !ReadBoundedFile(root + L"\\enable", 64, enabled)
            || enabled != kHeader) return 0;
        if (!ValidateNativeProfile(g_profile)) { Status(root, "blocked", "unsupported_executable"); return 0; }
        if (!CreateProcessIdentity(g_identity)) { Status(root, "failed", "run_identity_unavailable"); return 1; }
        if (!InstallHooks()) { StopSampling(); Status(root, "failed", "native_hook_installation_failed"); return 1; }
        if (!Status(root, "waiting", "awaiting_native_update")) { StopSampling(); return 1; }
        RequestHistory history{};
        std::uint64_t writtenSequence{}; std::string lastRequest;
        const char* writtenState = "waiting", *writtenIssue = "awaiting_native_update";
        auto publishStatus = [&](const char* state, const char* issue) {
            if (!std::strcmp(state, writtenState) && !std::strcmp(issue, writtenIssue)) return true;
            if (!Status(root, state, issue)) return false;
            writtenState = state; writtenIssue = issue; return true;
        };
        auto writeResponse = [&](const Response& response) { return WriteAtomicFile(root + L"\\response.json", SerializeResponse(g_identity, response)); };
        while (WaitForSingleObject(g_stop, 100) == WAIT_TIMEOUT) {
            if (!PlainDirectory(root) || !ReadBoundedFile(root + L"\\enable", 64, enabled) || enabled != kHeader) break;
            Snapshot current{}; Response completed{}; bool hasCompleted{};
            AcquireSRWLockExclusive(&g_stateLock);
            current = g_latest; hasCompleted = g_hasCompleted;
            if (hasCompleted) { completed = g_completed; g_hasCompleted = false; }
            ReleaseSRWLockExclusive(&g_stateLock);
            if (hasCompleted && (!history.Complete(completed) || !writeResponse(completed))) break;
            if (current.sequence && current.sequence != writtenSequence) {
                if (!WriteAtomicFile(root + L"\\players.json", SerializeSnapshot(g_identity, current))) break;
                if (!publishStatus(current.complete ? "ready" : "incomplete", current.issue)) break;
                writtenSequence = current.sequence;
            }
            std::string raw;
            if (!ReadBoundedFile(root + L"\\request.txt", kMaxRequestBytes, raw) || raw == lastRequest) continue;
            lastRequest = raw; Request request{};
            if (!ParseRequest(raw, request)) {
                if (!publishStatus(current.complete ? "ready" : "incomplete", "invalid_request")) break;
                continue;
            }
            HistoryRecord* record{}; const char* issue{};
            const auto admission = history.Admit(request, g_identity, GetTickCount64(), record, issue);
            if (admission != HistoryAdmission::New) {
                if (admission == HistoryAdmission::Conflict) {
                    bool cancelled{};
                    AcquireSRWLockExclusive(&g_stateLock);
                    if (g_hasPending && !std::strcmp(g_pending.requestId, request.requestId)) { g_hasPending = false; cancelled = true; }
                    ReleaseSRWLockExclusive(&g_stateLock);
                    const Response rejected{cancelled ? record->request : request, false, issue};
                    if ((cancelled && !history.Complete(rejected)) || !writeResponse(rejected)) break;
                } else if (admission == HistoryAdmission::Replay) {
                    if (!writeResponse(record->result)) break;
                } else if (admission != HistoryAdmission::Pending && !writeResponse({request, false, issue})) break;
                continue;
            }
            AcquireSRWLockExclusive(&g_stateLock);
            if (g_hasPending || g_hasCompleted || g_dispatching) issue = "request_queue_busy";
            else if (InterlockedCompareExchange(&g_stopping, 0, 0)) issue = "server_stopping";
            else { g_pending = request; g_hasPending = true; }
            ReleaseSRWLockExclusive(&g_stateLock);
            if (std::strcmp(issue, "none")) {
                const Response rejected{request, false, issue};
                if (!history.Complete(rejected) || !writeResponse(rejected)) break;
            }
        }
        StopSampling();
        Response pending{}; bool hasPending{};
        AcquireSRWLockExclusive(&g_stateLock); hasPending = g_hasCompleted; pending = g_completed; g_hasCompleted = false;
        ReleaseSRWLockExclusive(&g_stateLock);
        if (hasPending) writeResponse(pending);
        Status(root, "stopped", "server_stopping"); return 0;
    } catch (...) { StopSampling(); return 1; }
}
}

extern "C" void ControlCaptureUpdate(std::uintptr_t server) noexcept {
    using namespace enshrouded_control;
    if (!InterlockedCompareExchange(&g_ready, 0, 0) || InterlockedCompareExchange(&g_stopping, 0, 0)
        || !TryAcquireSRWLockExclusive(&g_stateLock)) return;
    const auto thread = static_cast<LONG>(GetCurrentThreadId());
    const auto owner = InterlockedCompareExchange(&g_ownerThread, thread, 0);
    if (owner && owner != thread) g_cache.degraded = true;
    g_latest = Capture(g_cache, server, GetTickCount64(), ReadCurrentMemory, nullptr);
    if (g_sequence == UINT64_MAX) { g_cache.degraded = true; g_latest.complete = false; g_latest.issue = "sequence_exhausted"; }
    else g_latest.sequence = ++g_sequence;
    Request request{}; bool execute = false;
    if (g_hasPending && !g_hasCompleted) {
        request = g_pending; g_hasPending = false;
        const auto issue = g_cache.degraded ? "native_context_changed"
            : ValidateRequest(request, g_identity, g_latest, GetTickCount64());
        g_completed = {request, false, issue};
        if (!std::strcmp(issue, "none")) { execute = true; g_dispatching = true; }
        else g_hasCompleted = true;
    }
    ReleaseSRWLockExclusive(&g_stateLock);
    if (!execute) return;
    // Two native inputs, independently verified for this exact executable. Its
    // own hash/session/immunity checks are preserved. No native return is acks.
    NativeEvent event{}; event.accountHash = request.accountId; event.action = static_cast<std::uint8_t>(request.action);
    using AccountAction = void(*)(void*, const NativeEvent*);
    reinterpret_cast<AccountAction>(g_profile.accountAction)(reinterpret_cast<void*>(server), &event);
    AcquireSRWLockExclusive(&g_stateLock);
    g_completed = {request, true, "none"}; g_hasCompleted = true; g_dispatching = false;
    ReleaseSRWLockExclusive(&g_stateLock);
}
extern "C" void ControlCaptureCopy(const enshrouded_control::NativeLookupContext* lookup,
    std::uint64_t nativeResult) noexcept {
    using namespace enshrouded_control;
    if (!InterlockedCompareExchange(&g_ready, 0, 0) || !lookup || lookup->caller < g_profile.imageBase
        || InterlockedCompareExchange(&g_stopping, 0, 0)) return;
    const auto caller = lookup->caller - g_profile.imageBase;
    if (caller != kCallerA && caller != kCallerB) return;
    if (!TryAcquireSRWLockExclusive(&g_stateLock)) return;
    const auto owner = InterlockedCompareExchange(&g_ownerThread, 0, 0);
    if (!owner) { ReleaseSRWLockExclusive(&g_stateLock); return; }
    if (owner != static_cast<LONG>(GetCurrentThreadId())) g_cache.degraded = true;
    ObserveLocalCopy(g_cache, *lookup, caller, nativeResult, GetTickCount64(), ReadCurrentMemory, nullptr);
    ReleaseSRWLockExclusive(&g_stateLock);
}
extern "C" __declspec(dllexport) BOOL WINAPI EnshroudedControlStop() noexcept {
    AcquireSRWLockExclusive(&g_stopLock);
    if (!g_worker) { ReleaseSRWLockExclusive(&g_stopLock); return TRUE; }
    SetEvent(g_stop); const BOOL stopped = WaitForSingleObject(g_worker, 10000) == WAIT_OBJECT_0;
    if (stopped) { CloseHandle(g_worker); CloseHandle(g_stop); g_worker = nullptr; g_stop = nullptr; }
    ReleaseSRWLockExclusive(&g_stopLock); return stopped;
}
extern "C" __declspec(dllexport) BOOL WINAPI MiniDumpWriteDump(HANDLE process, DWORD pid, HANDLE file,
    int type, void* exception, void* stream, void* callback) noexcept {
    using WriteDump = BOOL(WINAPI*)(HANDLE, DWORD, HANDLE, int, void*, void*, void*);
    auto library = reinterpret_cast<HMODULE>(InterlockedCompareExchangePointer(&g_realDbghelp, nullptr, nullptr));
    if (!library) {
        wchar_t path[MAX_PATH]{}; const auto n = GetSystemDirectoryW(path, MAX_PATH);
        if (!n || n + 13 >= MAX_PATH) { SetLastError(ERROR_INSUFFICIENT_BUFFER); return FALSE; }
        if (wcscpy_s(path + n, MAX_PATH - n, L"\\dbghelp.dll") != 0) return FALSE;
        auto loaded = LoadLibraryExW(path, nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32); if (!loaded) return FALSE;
        const auto prior = InterlockedCompareExchangePointer(&g_realDbghelp, loaded, nullptr);
        if (prior) { FreeLibrary(loaded); library = static_cast<HMODULE>(prior); } else library = loaded;
    }
    const auto function = reinterpret_cast<WriteDump>(GetProcAddress(library, "MiniDumpWriteDump"));
    if (!function) { SetLastError(ERROR_PROC_NOT_FOUND); return FALSE; }
    return function(process, pid, file, type, exception, stream, callback);
}
BOOL WINAPI DllMain(HINSTANCE self, DWORD reason, void*) {
    if (reason == DLL_PROCESS_ATTACH) {
        g_self = self; DisableThreadLibraryCalls(self);
        g_stop = CreateEventW(nullptr, TRUE, FALSE, nullptr); if (!g_stop) return FALSE;
        g_worker = CreateThread(nullptr, 0, Worker, nullptr, 0, nullptr);
        if (!g_worker) { CloseHandle(g_stop); g_stop = nullptr; return FALSE; }
    }
    return TRUE;
}
