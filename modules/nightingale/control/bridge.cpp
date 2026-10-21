// Nightingale Steam build 22204011. One native snapshot, writer and disk readback.
#include <windows.h>
#include <atomic>
#include <cstdint>
#include <string_view>
#include <thread>
#include "disk_snapshot.hpp"
namespace {
constexpr std::string_view GAME_HASH = "8e06c60524066655aee66da2e06b52c702a951afc3c28b59a2923f9dd51bd7eb";
constexpr std::string_view UE4SS_HASH = "91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958";
constexpr wchar_t GAME_NAME[] = L"NWXServer-Win64-Shipping.exe";
constexpr wchar_t BRIDGE_NAME[] = L"LgsmNightingaleSaveControl.dll";
constexpr uintptr_t SAVE_RVA = 0x3f8d190;
constexpr std::string_view SAVE_PROLOGUE = "48895c240848896c24104889742418574883ec30488bf1e834a6ffff488bd848";
void target_contract(uintptr_t, uintptr_t, uintptr_t, uintptr_t);
struct Request;
const char* native_save_async(uintptr_t, const Request&);
const char* native_observe_async(uintptr_t, const Request&);
}
#define LGSM_SAVE_EXPORT luaopen_lgsm_nightingale_save
#define LGSM_NATIVE_SAVE_ASYNC
#include "../../../apps/desktop/src-tauri/native/world_save_bridge.hpp"
namespace {
struct NativeString { wchar_t* data{}; int32_t length{}, capacity{}; };
static_assert(sizeof(NativeString) == 16);
struct OwnedString {
    uintptr_t image; NativeString value{};
    explicit OwnedString(uintptr_t base) : image(base) {}
    ~OwnedString() { if (value.data) reinterpret_cast<void (*)(void*)>(image + 0x109e3d0)(value.data); }
};
NativeString borrowed(std::wstring& text) {
    require(text.size() < nightingale_save::MAX_BYTES / 2, "native_string_size");
    return {text.data(), static_cast<int32_t>(text.size() + 1), static_cast<int32_t>(text.size() + 1)};
}
std::wstring text_at(uintptr_t address, size_t maximum, const char* issue = "native_string", bool allow_empty = false) {
    const auto value = read<NativeString>(address);
    require(value.length >= (allow_empty ? 0 : 2) && value.length <= value.capacity
        && static_cast<size_t>(value.length) <= maximum + 1, issue);
    // FString's native path combiner explicitly treats Num==0 as an empty
    // component. The dedicated offline namespace may use this representation;
    // record IDs, JSON and directory strings still require non-empty text.
    if (!value.length) return {};
    require(readable(reinterpret_cast<uintptr_t>(value.data), static_cast<size_t>(value.length) * 2), issue);
    require(value.data[value.length - 1] == 0, issue);
    std::wstring result(value.data, static_cast<size_t>(value.length - 1));
    require(result.find(L'\0') == result.npos, issue); return result;
}
std::wstring terminated_text(uintptr_t address, size_t maximum) {
    std::wstring result;
    for (size_t index = 0; index < maximum; ++index) {
        const auto value = read<wchar_t>(address + index * sizeof(wchar_t));
        if (!value) { require(!result.empty(), "native_base_directory"); return result; }
        result.push_back(value);
    }
    throw std::runtime_error("native_base_directory");
}
bool leaf(std::wstring_view name) {
    return !name.empty() && name.size() <= 192 && name.find_first_not_of(
        L"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-") == name.npos;
}
struct NativeJson {
    uintptr_t object{}, control{};
    NativeJson() = default;
    NativeJson(const NativeJson&) = delete;
    ~NativeJson() {
        // Same thread-safe shared/weak counters as the fixed native decoder's
        // callers. This owns only the new claims object returned by that call.
        if (!control) return;
        if (InterlockedDecrement(reinterpret_cast<volatile LONG*>(control + 8)) == 0) {
            auto table = *reinterpret_cast<uintptr_t**>(control);
            reinterpret_cast<void (*)(void*)>(table[0])(reinterpret_cast<void*>(control));
            if (InterlockedDecrement(reinterpret_cast<volatile LONG*>(control + 12)) == 0)
                reinterpret_cast<void (*)(void*, int)>(table[1])(reinterpret_cast<void*>(control), 1);
        }
    }
};
bool token_identity(uintptr_t image, const NativeString& token, std::wstring& provider, std::wstring& principal) {
    require(token.length >= 0 && token.length <= token.capacity, "native_storage_token");
    if (token.length <= 1) return false;
    require(token.length <= token.capacity && token.length <= 32769
        && readable(reinterpret_cast<uintptr_t>(token.data), static_cast<size_t>(token.length) * 2)
        && token.data[token.length - 1] == 0, "native_storage_token");
    NativeJson claims;
    OwnedString error(image), native_provider(image), native_principal(image);
    // The actual writer decodes its x-client-token first, then x-server-token,
    // and takes both `prv` and `prvid`. Configuration namespace is unrelated.
    if (!reinterpret_cast<uint8_t (*)(const NativeString*, void*, NativeJson*, bool, void*, NativeString*)>(
        image + 0x3d3c9a0)(&token, nullptr, &claims, false, nullptr, &error.value) || !claims.object) return false;
    NativeString provider_key{const_cast<wchar_t*>(L"prv"), 3, 3};
    NativeString principal_key{const_cast<wchar_t*>(L"prvid"), 5, 5};
    const auto field = reinterpret_cast<uint8_t (*)(void*, const NativeString*, NativeString*)>(image + 0x124f040);
    if (!field(reinterpret_cast<void*>(claims.object), &provider_key, &native_provider.value)
        || !field(reinterpret_cast<void*>(claims.object), &principal_key, &native_principal.value)) return false;
    provider = text_at(reinterpret_cast<uintptr_t>(&native_provider.value), 96, "native_storage_provider", true);
    principal = text_at(reinterpret_cast<uintptr_t>(&native_principal.value), 512, "native_storage_principal", true);
    return true;
}
fs::path native_primary(uintptr_t image, const fs::path& base, const fs::path& saved, uintptr_t backend,
    std::wstring provider, std::wstring record) {
    require(leaf(provider) && leaf(record), "offline_provider_record");
    OwnedString directory(image), path(image);
    auto provider_string = borrowed(provider), record_string = borrowed(record);
    reinterpret_cast<NativeString* (*)(NativeString*, const NativeString*, const NativeString*)>(image + 0x3d51f50)(
        &directory.value, &provider_string, reinterpret_cast<const NativeString*>(backend + 0x16c0));
    reinterpret_cast<NativeString* (*)(NativeString*, const NativeString*, const NativeString*, const NativeString*)>(
        image + 0x3d52060)(&path.value, &directory.value, &record_string,
            reinterpret_cast<const NativeString*>(backend + 0x16e0));
    auto primary = fs::path(text_at(reinterpret_cast<uintptr_t>(&path.value), 4096, "native_state_path"));
    primary = (primary.is_absolute() ? primary : base / primary).lexically_normal();
    const auto expected = (saved / L"Offline" / provider / L"Deploy" / record / L"state").lexically_normal();
    require(equal_path(primary, expected), "offline_state_ownership");
    return primary;
}
void native_profile(uintptr_t image) {
    prologue(image, 0x3f9c440, "48895c241048896c2418565741564883ec40488b5948488b");
    prologue(image, 0x3f9bab0, "48895c2408488974241848897c2420554154415541564157");
    prologue(image, 0x3f877e0, "4c8bdc4883ec78488b05b2d9fc054885c00f858b00000048");
    prologue(image, 0x1193a50, "4c8974242055488bec4883ec70e81e4fffff4c8bf080b819");
    prologue(image, 0x120cee0, "48895c2408574883ec30418bf84c8bd2488bd94885d20f84");
    prologue(image, 0x11a71b0, "e9ab0a0000");
    prologue(image, 0x11a9a50, "41b801000000e975feffff");
    prologue(image, 0x3d1be00, "803dab32fd0500740332c0c380b9c8000000000f94c0fec0");
    prologue(image, 0x3d51f50, "405356574881ec90000000488b055e21f1054833c4488984");
    prologue(image, 0x3d52060, "48895c24104889742420574883ec70488b054a20f1054833");
    prologue(image, 0x3d20f20, "48895c240848896c24104889742418574883ec20418b4008");
    prologue(image, 0x3d3c9a0, "48895c24084c894c24204c89442418555657415441554156");
    prologue(image, 0x124f040, "488bc45356574883ec500f100248896810488bd94c897018");
    prologue(image, 0x1067970, "48895c24084533d24c8bda41b9ffffff7f4585c00f8ed900");
    prologue(image, 0x105ff50, "48894c2408534883ec2089542438458bd14d8bd8488bd945");
    prologue(image, 0x109e3d0, "4885c90f84b5000000574883ec2048833de20cc50800488b");
    prologue(image, 0x122f100, "48895c2410488974241848897c24205541564157488bec4881ec80000000");
    prologue(image, 0x6b55880, "4053574883ec288bfa488bd94d85c00f8475020000418038");
    prologue(image, 0x6b53fb0, "895424105355565741544155415641574883ec58448bf24c");
    prologue(image, 0x6b55800, "40534883ec20488bd94885c9745d488379300074564c8b41");
}
struct Storage {
    uintptr_t module{}, backend{}, settings{};
    std::wstring name_space, provider, principal, record;
    bool client_token{};
    fs::path saved, primary;
};
Storage storage(uintptr_t image) {
    uint64_t name{};
    // This fixed overload scans ANSI bytes; the adjacent wide overload has a
    // different entry point. Passing UTF-16 here looks up only FName("N").
    reinterpret_cast<void* (*)(uint64_t*, const char*, int)>(image + 0x120cee0)(&name, "NWXOnlineServices", 1);
    auto manager = reinterpret_cast<void* (*)()>(image + 0x11a71b0)();
    const auto module = reinterpret_cast<uintptr_t>(reinterpret_cast<void* (*)(void*, uint64_t)>(image + 0x11a9a50)(manager, name));
    require(readable(module, 0x108), "offline_module_missing");
    require(read<uintptr_t>(module) == image + 0x8061fb0
        && read<uintptr_t>(image + 0x8061fb0 + 0x50) == image + 0x3d1be00, "offline_module_identity");
    require(reinterpret_cast<uint8_t (*)(void*)>(image + 0x3d1be00)(reinterpret_cast<void*>(module)) == 1,
        "offline_module_mode");
    const auto backend = read<uintptr_t>(module + 0x90);
    require(readable(backend, 0x16f0), "offline_backend_missing");
    require(read<uintptr_t>(backend) == image + 0x80ae5a0
        && read<uintptr_t>(backend + 0x170) == image + 0x80ae8e0
        && read<uintptr_t>(image + 0x80ae8e0 + 0x10) == image + 0x3d5d8f0, "offline_backend");
    auto name_space = text_at(backend + 0x298, 96, "native_backend_namespace", true);
    require((name_space.empty() || leaf(name_space)) && name_space == text_at(module + 0xe8, 96, "native_module_namespace", true)
        && text_at(backend + 0x16c0, 16, "native_deploy_name") == L"Deploy"
        && text_at(backend + 0x16e0, 16, "native_state_name") == L"state", "offline_namespace");
    const auto globals = reinterpret_cast<uintptr_t>(reinterpret_cast<void* (*)()>(image + 0x3f877e0)());
    const auto settings = read<uintptr_t>(globals + 0x110);
    auto record = text_at(settings + 0x48, 192, "native_record");
    require(leaf(record), "offline_record");
    auto saved_string = reinterpret_cast<uintptr_t>(reinterpret_cast<void* (*)()>(image + 0x1193a50)());
    auto saved = fs::path(text_at(saved_string, 2048, "native_saved_directory"));
    // Native OpenWrite normalizes through 0x1199b80/0x1199be0 using the
    // engine's BaseDir getter, independently of the inherited Win32 CWD.
    const auto executable = module_path(GetModuleHandleW(nullptr));
    auto base = fs::path(terminated_text(reinterpret_cast<uintptr_t>(
        reinterpret_cast<const wchar_t* (*)()>(image + 0x122f100)()), 512)).lexically_normal();
    if (base.filename().empty()) base = base.parent_path();
    require(base.is_absolute() && equal_path(base, executable.parent_path()), "native_base_directory");
    saved = (saved.is_absolute() ? saved : base / saved).lexically_normal();
    if (saved.filename().empty()) saved = saved.parent_path();
    // The literal contains a forward slash; CompareStringOrdinal does not
    // treat it as the backslash emitted by lexically_normal on Windows.
    const auto expected = (initialization().image->directory.parent_path() / L"NWX/Saved").lexically_normal();
    require(equal_path(saved, expected), "offline_root_ownership");
    std::wstring provider, principal;
    bool client_token = false, found = false;
    for (const auto key : {L"", L"server"}) {
        OwnedString token(image);
        std::wstring key_text(key);
        auto token_key = key_text.empty() ? NativeString{} : borrowed(key_text);
        reinterpret_cast<NativeString* (*)(void*, NativeString*, const NativeString*)>(image + 0x3d20f20)(
            reinterpret_cast<void*>(module), &token.value, &token_key);
        found = token_identity(image, token.value, provider, principal);
        if (found) { client_token = key_text.empty(); break; }
    }
    require(found && leaf(provider), "offline_storage_identity");
    const auto primary = native_primary(image, base, saved, backend, provider, record);
    return {module, backend, settings, std::move(name_space), std::move(provider), std::move(principal),
        std::move(record), client_token, std::move(saved), primary};
}
bool same_storage(const Storage& a, const Storage& b) {
    return a.module == b.module && a.backend == b.backend && a.settings == b.settings
        && a.name_space == b.name_space && a.provider == b.provider && a.principal == b.principal
        && a.record == b.record && a.client_token == b.client_token
        && equal_path(a.saved, b.saved) && equal_path(a.primary, b.primary);
}
void target_contract(uintptr_t image, uintptr_t subsystem, uintptr_t instance, uintptr_t world) {
    native_profile(image);
    auto cls = read<uintptr_t>(image + 0x9f551e0);
    require(cls && readable(subsystem, 0x98) && read<uintptr_t>(subsystem + 0x10) == cls
        && !(read<uint32_t>(subsystem + 8) & 0x40018010), "subsystem_class");
    require(readable(instance, 0x28) && !(read<uint32_t>(instance + 8) & 0x40018010)
        && read<uintptr_t>(subsystem + 0x20) == instance, "subsystem_instance");
    using WeakWorld = void* (*)(void*);
    auto native_world = reinterpret_cast<WeakWorld>(image + 0x1594190)(reinterpret_cast<void*>(subsystem + 0x60));
    require(reinterpret_cast<uintptr_t>(native_world) == world && readable(world, 0x28)
        && !(read<uint32_t>(world + 8) & 0x40018010), "subsystem_world");
    require(read<uint8_t>(subsystem + 0x90) == 1, "persistence_disabled");
    auto state = read<uintptr_t>(subsystem + 0x48);
    require(state && readable(state, 0x372)
        && !(read<uint8_t>(state + 0x370) && !read<uint8_t>(state + 0x371)), "persistence_not_loaded");
}
struct Pending {
    Request request;
    Storage target;
    std::wstring json;
    nightingale_save::Content content;
    uint64_t started{};
    std::atomic<bool> done{}, matched{}, abandoned{};
    std::atomic<const char*> issue{"worker_pending"};
    std::atomic<uintptr_t> fault_rva{};
};
std::shared_ptr<Pending> active;
nightingale_save::SnapshotIssue inspect_snapshot(const fs::path& primary, const nightingale_save::Gzip& codec, const nightingale_save::Content& content) {
    auto pins = pin_directories(primary.parent_path());
    auto file = open_file(primary, GENERIC_READ, OPEN_EXISTING, FILE_SHARE_READ);
    LARGE_INTEGER length{};
    require(GetFileSizeEx(file.value, &length) && length.QuadPart >= 18
        && length.QuadPart <= static_cast<int64_t>(nightingale_save::MAX_BYTES), "snapshot_size");
    std::vector<uint8_t> compressed(static_cast<size_t>(length.QuadPart));
    DWORD count{};
    require(ReadFile(file.value, compressed.data(), static_cast<DWORD>(compressed.size()), &count, nullptr)
        && count == compressed.size(), "snapshot_read");
    return codec.verify(compressed, content);
}
[[maybe_unused]] bool pinned_snapshot(const fs::path& primary, const nightingale_save::Gzip& codec, const nightingale_save::Content& content) {
    return inspect_snapshot(primary, codec, content) == nightingale_save::SnapshotIssue::none;
}
bool disk_matches(uintptr_t image, Pending& job) {
    const nightingale_save::Gzip codec{
        reinterpret_cast<int (*)(nightingale_save::InflateStream*, int, const char*, int)>(image + 0x6b55880),
        reinterpret_cast<int (*)(nightingale_save::InflateStream*, int)>(image + 0x6b53fb0),
        reinterpret_cast<int (*)(nightingale_save::InflateStream*)>(image + 0x6b55800), reinterpret_cast<const char*>(image + 0x7461474)};
    const auto issue = inspect_snapshot(job.target.primary, codec, job.content);
    job.issue.store(nightingale_save::issue_code(issue), std::memory_order_release);
    return issue == nightingale_save::SnapshotIssue::none;
}
int write_fault(EXCEPTION_POINTERS* exception, uintptr_t image, Pending* job) {
    if (exception->ExceptionRecord->ExceptionCode != EXCEPTION_ACCESS_VIOLATION) return EXCEPTION_CONTINUE_SEARCH;
    job->issue.store("worker_write_access_violation", std::memory_order_release);
    const auto address = reinterpret_cast<uintptr_t>(exception->ExceptionRecord->ExceptionAddress);
    // Relative instruction offset only; never expose process pointers or the
    // exception's referenced memory, which could contain snapshot content.
    if (address >= image && address - image < 0x0b000000) job->fault_rva.store(address - image, std::memory_order_release);
    return EXCEPTION_EXECUTE_HANDLER;
}
void report_worker_issue(const Pending& job) noexcept {
    try {
        const auto& state = initialization();
        if (!state.image) return;
        auto pins = pin_directories(state.image->directory);
        auto file = open_file(state.image->directory / L"native-diagnostic.txt", GENERIC_WRITE, OPEN_ALWAYS, 0);
        std::string message = job.issue.load(std::memory_order_acquire);
        if (const auto rva = job.fault_rva.load(std::memory_order_acquire)) message += "\nrva=" + std::to_string(rva);
        write_receipt(file.value, message + "\n");
    } catch (...) { OutputDebugStringA("[LgsmWorldSave] Worker diagnostic unavailable; receipt stays unknown.\n"); }
}
bool write_and_read(uintptr_t image, Pending* job) {
    __try {
        auto json = borrowed(job->json), record = borrowed(job->target.record);
        NativeString filename{};
        // This fixed native helper reads only JSON and recordID. Its sync SDK
        // branch waits for its own storage result/retries; no UObject is used
        // from this worker. The return alone never confirms persistence.
        reinterpret_cast<void (*)(void*, NativeString*, NativeString*, NativeString*, bool)>(image + 0x3f9bab0)(
            nullptr, &json, &record, &filename, false);
        return true;
    }
    __except (write_fault(GetExceptionInformation(), image, job)) { return false; }
}
const char* native_save_async(uintptr_t image, const Request& request) {
    if (active && !active->done.load(std::memory_order_acquire)) return "unknown";
    const auto target = storage(image);
    auto job = std::make_shared<Pending>();
    job->request = request; job->target = target; job->started = GetTickCount64();
    OwnedString json(image);
    auto record = borrowed(job->target.record);
    reinterpret_cast<void* (*)(void*, NativeString*, NativeString*)>(image + 0x3f9c440)(
        reinterpret_cast<void*>(request.proxy), &json.value, &record);
    job->json = text_at(reinterpret_cast<uintptr_t>(&json.value), nightingale_save::MAX_BYTES / 2 - 1, "native_json_snapshot");
    const auto length = reinterpret_cast<int (*)(void*, const wchar_t*, int)>(image + 0x1067970)(
        nullptr, job->json.data(), static_cast<int>(job->json.size()));
    require(length > 0 && length <= static_cast<int>(nightingale_save::MAX_BYTES), "snapshot_size");
    std::vector<uint8_t> bytes(static_cast<size_t>(length));
    reinterpret_cast<void (*)(uint8_t*, int, const wchar_t*, int)>(image + 0x105ff50)(
        bytes.data(), length, job->json.data(), static_cast<int>(job->json.size()));
    job->content = nightingale_save::Content::capture(bytes);
    target_contract(image, request.proxy, request.instance, request.world);
    require(same_storage(job->target, storage(image)), "storage_changed");
    HMODULE pin{};
    require(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        reinterpret_cast<LPCWSTR>(&native_save_async), &pin) != FALSE, "callback_lifetime");
    active = job;
    try {
        std::thread([image, job] {
            try {
                if (write_and_read(image, job.get()) && !job->abandoned.load(std::memory_order_acquire)
                    && GetTickCount64() >= job->started && GetTickCount64() - job->started <= 30000)
                {
                    const auto matched = disk_matches(image, *job);
                    const auto finished = GetTickCount64();
                    job->matched.store(matched && finished >= job->started && finished - job->started <= 30000,
                        std::memory_order_release);
                }
            } catch (const std::runtime_error& error) {
                // The exceptions here originate from our closed filesystem
                // and snapshot checks. Never copy arbitrary exception text.
                const std::string_view reason(error.what());
                job->issue.store(reason == "file_open" ? "worker_snapshot_open" :
                    reason == "directory_open" ? "worker_snapshot_directory" : reason == "file_type" ? "worker_snapshot_file_type" :
                    reason == "snapshot_size" ? "worker_snapshot_size" : reason == "snapshot_read" ? "worker_snapshot_read" :
                    "worker_snapshot_exception", std::memory_order_release);
            } catch (...) {
                job->issue.store("worker_exception", std::memory_order_release);
            }
            std::wstring{}.swap(job->json);
            job->done.store(true, std::memory_order_release);
        }).detach();
    } catch (...) { job->done.store(true, std::memory_order_release); throw; }
    return "pending";
}
const char* native_observe_async(uintptr_t image, const Request& request) {
    if (!active || active->request.nonce != request.nonce || active->request.proxy != request.proxy
        || active->request.instance != request.instance || active->request.world != request.world
        || active->request.requested_at != request.requested_at || active->request.operation != request.operation) return "unknown";
    auto job = active;
    Storage observed;
    try { observed = storage(image); }
    catch (...) {
        job->abandoned.store(true, std::memory_order_release); job->issue.store("worker_storage_unavailable", std::memory_order_release);
        report_worker_issue(*job); throw;
    }
    if (!same_storage(job->target, observed)) {
        job->abandoned.store(true, std::memory_order_release); job->issue.store("worker_storage_changed", std::memory_order_release);
        report_worker_issue(*job); return "unknown";
    }
    const auto tick = GetTickCount64();
    if (tick < job->started || tick - job->started > 30000) {
        job->abandoned.store(true, std::memory_order_release); job->issue.store("worker_deadline", std::memory_order_release);
        report_worker_issue(*job); return "unknown";
    }
    if (!job->done.load(std::memory_order_acquire)) return "pending";
    const auto finished = GetTickCount64();
    if (!job->matched.load(std::memory_order_acquire)) report_worker_issue(*job);
    return finished >= job->started && finished - job->started <= 30000
        && !job->abandoned.load(std::memory_order_acquire) && job->matched.load(std::memory_order_acquire) ? "confirmed" : "unknown";
}
}
