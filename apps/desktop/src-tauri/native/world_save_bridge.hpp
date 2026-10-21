// Closed save transport used only by the two fixed game contracts. No Lua ABI imports.
// The game adapter declares its exact image, member ABI, ownership and readiness checks.
#include <windows.h>
#include <cstdio>
#include <bcrypt.h>
#include <array>
#include <charconv>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

namespace {
namespace fs = std::filesystem;
constexpr size_t MAX_REQUEST = 512;
struct Handle {
    HANDLE value = INVALID_HANDLE_VALUE;
    Handle() = default;
    explicit Handle(HANDLE handle) : value(handle) {}
    Handle(const Handle&) = delete;
    Handle& operator=(const Handle&) = delete;
    Handle(Handle&& other) noexcept : value(other.value) { other.value = INVALID_HANDLE_VALUE; }
    ~Handle() { if (value != INVALID_HANDLE_VALUE) CloseHandle(value); }
};
void require(bool condition, const char* code) { if (!condition) throw std::runtime_error(code); }
fs::path module_path(HMODULE module) {
    std::array<wchar_t, 32768> buffer{};
    auto length = GetModuleFileNameW(module, buffer.data(), static_cast<DWORD>(buffer.size()));
    require(length > 0 && length < buffer.size(), "module_path");
    return fs::path(std::wstring(buffer.data(), length));
}
bool equal_path(const fs::path& left, const fs::path& right) {
    return CompareStringOrdinal(left.c_str(), -1, right.c_str(), -1, TRUE) == CSTR_EQUAL;
}
bool same_file(const fs::path& left, const fs::path& right) {
    Handle first(CreateFileW(left.c_str(), FILE_READ_ATTRIBUTES, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        nullptr, OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT, nullptr));
    Handle second(CreateFileW(right.c_str(), FILE_READ_ATTRIBUTES, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        nullptr, OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT, nullptr));
    BY_HANDLE_FILE_INFORMATION a{}, b{};
    return first.value != INVALID_HANDLE_VALUE && second.value != INVALID_HANDLE_VALUE
        && GetFileInformationByHandle(first.value, &a) && GetFileInformationByHandle(second.value, &b)
        && !(a.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) && !(b.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT)
        && a.dwVolumeSerialNumber == b.dwVolumeSerialNumber && a.nFileIndexHigh == b.nFileIndexHigh
        && a.nFileIndexLow == b.nFileIndexLow;
}
void regular(HANDLE file, bool directory = false) {
    BY_HANDLE_FILE_INFORMATION info{};
    require(GetFileInformationByHandle(file, &info) != FALSE, "file_metadata");
    require((info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) == 0
        && ((info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0) == directory
        && (directory || info.nNumberOfLinks == 1), "file_type");
}
Handle open_file(const fs::path& path, DWORD access, DWORD creation, DWORD sharing) {
    Handle file(CreateFileW(path.c_str(), access, sharing, nullptr, creation,
        FILE_FLAG_OPEN_REPARSE_POINT, nullptr));
    require(file.value != INVALID_HANDLE_VALUE, "file_open");
    regular(file.value);
    return file;
}
std::vector<Handle> pin_directories(const fs::path& path) {
    std::vector<fs::path> ancestors;
    for (auto current = path; !current.empty();) {
        ancestors.push_back(current);
        auto parent = current.parent_path();
        if (parent == current) break;
        current = parent;
    }
    require(ancestors.size() <= 64, "directory_depth");
    std::vector<Handle> pins;
    for (auto it = ancestors.rbegin(); it != ancestors.rend(); ++it) {
        // LIST_DIRECTORY makes the no-delete sharing constraint effective.
        Handle file(CreateFileW(it->c_str(), FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES, FILE_SHARE_READ | FILE_SHARE_WRITE,
            nullptr, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT, nullptr));
        require(file.value != INVALID_HANDLE_VALUE, "directory_open");
        regular(file.value, true);
        pins.push_back(std::move(file));
    }
    return pins;
}
std::string hash_file(HANDLE file) {
    BCRYPT_ALG_HANDLE algorithm{};
    require(BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) >= 0, "hash_provider");
    struct Algorithm { BCRYPT_ALG_HANDLE value; ~Algorithm() { BCryptCloseAlgorithmProvider(value, 0); } } owner{algorithm};
    BCRYPT_HASH_HANDLE hash{};
    require(BCryptCreateHash(algorithm, &hash, nullptr, 0, nullptr, 0, 0) >= 0, "hash_create");
    struct Hash { BCRYPT_HASH_HANDLE value; ~Hash() { BCryptDestroyHash(value); } } hash_owner{hash};
    LARGE_INTEGER size{};
    require(GetFileSizeEx(file, &size) && size.QuadPart > 0 && size.QuadPart <= 320LL * 1024 * 1024, "module_size");
    std::array<UCHAR, 65536> buffer{};
    for (;;) {
        DWORD count{};
        require(ReadFile(file, buffer.data(), static_cast<DWORD>(buffer.size()), &count, nullptr) != FALSE, "hash_read");
        if (!count) break;
        require(BCryptHashData(hash, buffer.data(), count, 0) >= 0, "hash_update");
    }
    std::array<UCHAR, 32> digest{};
    require(BCryptFinishHash(hash, digest.data(), static_cast<ULONG>(digest.size()), 0) >= 0, "hash_finish");
    std::string result;
    for (auto byte : digest) { result += "0123456789abcdef"[byte >> 4]; result += "0123456789abcdef"[byte & 15]; }
    return result;
}
bool readable(uintptr_t address, size_t length) {
    if (!address || address > UINTPTR_MAX - length) return false;
    auto end = address + length;
    while (address < end) {
        MEMORY_BASIC_INFORMATION region{};
        if (!VirtualQuery(reinterpret_cast<void*>(address), &region, sizeof(region))
            || region.State != MEM_COMMIT || (region.Protect & (PAGE_GUARD | PAGE_NOACCESS))) return false;
        auto next = reinterpret_cast<uintptr_t>(region.BaseAddress) + region.RegionSize;
        if (next <= address) return false;
        address = next;
    }
    return true;
}
template<class T> T read(uintptr_t address) {
    require(readable(address, sizeof(T)), "native_memory");
    T result{}; std::memcpy(&result, reinterpret_cast<void*>(address), sizeof(T)); return result;
}
void prologue(uintptr_t image, uintptr_t rva, std::string_view expected) {
    require(readable(image + rva, expected.size() / 2), "native_code");
    auto bytes = reinterpret_cast<const unsigned char*>(image + rva);
    constexpr std::string_view digits = "0123456789abcdef";
    for (size_t i = 0; i < expected.size(); i += 2) {
        auto byte = (digits.find(expected[i]) << 4) | digits.find(expected[i + 1]);
        require(bytes[i / 2] == byte, "native_code");
    }
}
struct Image {
    uintptr_t base{};
    bool (*game_thread)(){};
    fs::path directory;
    Handle shipping, ue4ss;
    std::vector<Handle> pins;
    Image() {
        auto path = module_path(GetModuleHandleW(nullptr));
        require(equal_path(path.filename(), fs::path(GAME_NAME)), "game_module");
        auto win64 = path.parent_path();
        auto core = GetModuleHandleW(L"UE4SS.dll");
        require(core && same_file(module_path(core), win64 / L"ue4ss/UE4SS.dll"), "loader_module");
        HMODULE bridge{};
        require(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCWSTR>(&module_path), &bridge) != FALSE, "bridge_module");
        require(same_file(module_path(bridge), win64 / L"ue4ss/Mods/LgsmWorldSave/native" / BRIDGE_NAME), "bridge_path");
        directory = win64.parent_path().parent_path().parent_path() / L"langame_world_save";
        pins = pin_directories(directory);
        auto shipping_file = open_file(path, GENERIC_READ, OPEN_EXISTING, FILE_SHARE_READ);
        auto core_file = open_file(module_path(core), GENERIC_READ, OPEN_EXISTING, FILE_SHARE_READ);
        require(hash_file(shipping_file.value) == GAME_HASH && hash_file(core_file.value) == UE4SS_HASH, "module_fingerprint");
        shipping.value = shipping_file.value; shipping_file.value = INVALID_HANDLE_VALUE;
        ue4ss.value = core_file.value; core_file.value = INVALID_HANDLE_VALUE;
        base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        prologue(base, SAVE_RVA, SAVE_PROLOGUE);
        game_thread = reinterpret_cast<bool (*)()>(GetProcAddress(core, "?IsInGameThreadRaw@Unreal@RC@@YA_NXZ"));
        require(game_thread != nullptr, "thread_api");
    }
};
struct Request { std::string nonce, operation; uint64_t requested_at{}; uintptr_t proxy{}, instance{}, world{}; };
uint64_t decimal(std::string_view text) {
    require(!text.empty() && text.size() <= 20 && (text.size() == 1 || text.front() != '0'), "request_decimal");
    uint64_t value{};
    auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), value);
    require(error == std::errc{} && end == text.data() + text.size(), "request_decimal"); return value;
}
Request parse(std::string_view text) {
    constexpr std::array<std::string_view, 7> fields{ "schema=", "nonce=", "op=", "requested_at=", "proxy_address=", "instance_address=", "world_address=" };
    std::array<std::string_view, 7> values{};
    for (size_t i = 0; i < fields.size(); ++i) {
        auto end = text.find('\n');
        require(end != text.npos && text.substr(0, end).starts_with(fields[i]), "request_format");
        values[i] = text.substr(fields[i].size(), end - fields[i].size()); text.remove_prefix(end + 1);
    }
    require(text.empty() && values[0] == "1" && values[1].size() == 32
        && values[1].find_first_not_of("0123456789abcdef") == values[1].npos
        && (values[2] == "save" || values[2] == "probe"), "request_format");
    Request result{std::string(values[1]), std::string(values[2]), decimal(values[3]), decimal(values[4]), decimal(values[5]), decimal(values[6])};
    require(result.proxy && result.instance && result.world, "request_address");
    return result;
}
Request read_request(const fs::path& directory) {
    auto file = open_file(directory / L"native-request.txt", GENERIC_READ, OPEN_EXISTING, FILE_SHARE_READ);
    LARGE_INTEGER length{};
    require(GetFileSizeEx(file.value, &length) && length.QuadPart > 0 && length.QuadPart <= MAX_REQUEST, "request_size");
    std::array<char, MAX_REQUEST + 1> bytes{}; DWORD count{};
    require(ReadFile(file.value, bytes.data(), static_cast<DWORD>(bytes.size()), &count, nullptr)
        && count == length.QuadPart, "request_read"); return parse(std::string_view(bytes.data(), count));
}
uint64_t now() {
    FILETIME time{}; GetSystemTimeAsFileTime(&time);
    ULARGE_INTEGER value{}; value.LowPart = time.dwLowDateTime; value.HighPart = time.dwHighDateTime;
    return value.QuadPart / 10000000ULL - 11644473600ULL;
}
void validate(const Image& image, const Request& request) {
    require(image.game_thread(), "wrong_thread");
    auto time = now(); require(time + 2 >= request.requested_at && time <= request.requested_at + 3, "request_expired");
#ifdef LGSM_NATIVE_PROBE_CONTRACT
    if(request.operation=="probe") probe_target_contract(image.base,request.proxy,request.instance,request.world);
    else target_contract(image.base,request.proxy,request.instance,request.world);
#else
    target_contract(image.base, request.proxy, request.instance, request.world);
#endif
}
#ifndef LGSM_NATIVE_SAVE_ASYNC
const char* guarded_call(const Image& image, uintptr_t target) {
    __try {
#ifdef LGSM_NATIVE_SAVE_RESULT
        return native_save(image.base, target);
#else
        native_save(image.base, target); return "returned";
#endif
    }
    __except (GetExceptionCode() == EXCEPTION_ACCESS_VIOLATION ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH) { return nullptr; }
}
#endif

struct Receipt {
    std::string key, text;
#ifdef LGSM_NATIVE_SAVE_ASYNC
    bool pending{};
    uint64_t admitted_ms{};
#endif
};
std::array<Receipt, 512> recent;
#ifndef LGSM_NATIVE_SAVE_ASYNC
size_t next_receipt{};
#endif
std::string receipt(const Request& request, std::string_view status) {
    FILETIME creation{}, exit{}, kernel{}, user{};
    require(GetProcessTimes(GetCurrentProcess(), &creation, &exit, &kernel, &user) != FALSE, "process_identity");
    ULARGE_INTEGER value{}; value.LowPart = creation.dwLowDateTime; value.HighPart = creation.dwHighDateTime;
    return "schema=1\nnonce=" + request.nonce + "\nop=" + request.operation + "\nstatus=" + std::string(status)
        + "\npid=" + std::to_string(GetCurrentProcessId()) + "\ncreation_time=" + std::to_string(value.QuadPart) + "\n";
}
void write_receipt(HANDLE file, std::string_view text) {
    LARGE_INTEGER start{}; require(SetFilePointerEx(file, start, nullptr, FILE_BEGIN) && SetEndOfFile(file), "receipt_truncate");
    DWORD count{}; require(WriteFile(file, text.data(), static_cast<DWORD>(text.size()), &count, nullptr)
        && count == text.size() && FlushFileBuffers(file), "receipt_write");
}
struct Initialization {
    std::unique_ptr<Image> image;
    Initialization() { try { image = std::make_unique<Image>(); } catch (...) { OutputDebugStringA("[LgsmWorldSave] Fixed native contract unavailable.\n"); } }
};
Initialization& initialization() { static Initialization value; return value; }
// Unbound, optional diagnostic only: never a completion input or resend signal.
void verification_diagnostic(const std::exception& error) noexcept {
    try {
        // Emit only bounded internal reason codes, never paths or frame data.
        const char* reason = error.what(); size_t length = 0;
        while (reason[length] && length < 64 && ((reason[length] >= 'a' && reason[length] <= 'z') || reason[length] == '_')) ++length;
        if (length == 0 || length >= 64 || reason[length]) reason = "native_exception";
        std::fprintf(stderr, "[LgsmWorldSave] Native verification failed (%s); request was not resent.\n", reason);
        // Some native launchers detach stderr. Preserve the same finite code
        // in the authenticated exchange without changing the original result.
        const auto& state = initialization();
        if (state.image) {
            auto file = open_file(state.image->directory / L"native-diagnostic.txt", GENERIC_WRITE, OPEN_ALWAYS, 0);
            write_receipt(file.value, std::string(reason) + "\n");
        }
    } catch (...) { /* Diagnostics cannot change rejection or unknown results. */ }
}
#ifdef LGSM_NATIVE_SAVE_ASYNC
const char* guarded_async(const Image& image, const Request& request, bool observe) {
    __try { return observe ? native_observe_async(image.base, request) : native_save_async(image.base, request); }
    __except (GetExceptionCode() == EXCEPTION_ACCESS_VIOLATION ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH) { return nullptr; }
}
void async_result(Receipt& slot, const Request& request, const char* status) {
    require(status && (std::string_view(status) == "pending" || std::string_view(status) == "confirmed"
        || std::string_view(status) == "failed" || std::string_view(status) == "unknown"), "native_result");
    slot.pending = std::string_view(status) == "pending";
    slot.text = receipt(request, status);
}
Receipt* available_receipt(uint64_t tick) {
    for (auto& candidate : recent) {
        if (candidate.key.empty() || (tick >= candidate.admitted_ms && tick - candidate.admitted_ms > 32000)) return &candidate;
    }
    return nullptr;
}
void observe(Receipt& slot, const Request& request) {
#ifdef LGSM_NATIVE_SAVE_OBSERVATION_ABORT
    struct ObservationExit {
        const Request& request; bool completed{};
        ~ObservationExit() noexcept { if(!completed) native_observation_aborted(request); }
    } observation{request};
#endif
    // A replay is read-only observation of the original admitted operation.
    // Any lost/late observation stays unknown without releasing its callback.
    slot.pending = false;
    slot.text = receipt(request, "unknown");
    auto& initialized = initialization();
    require(initialized.image && initialized.image->game_thread(), "native_unavailable");
    const auto tick = GetTickCount64();
    require(tick >= slot.admitted_ms && tick - slot.admitted_ms <= 30000, "request_expired");
    const auto time = now();
    require(time + 2 >= request.requested_at && time <= request.requested_at + 30, "request_expired");
    prologue(initialized.image->base, SAVE_RVA, SAVE_PROLOGUE);
    target_contract(initialized.image->base, request.proxy, request.instance, request.world);
    async_result(slot, request, guarded_async(*initialized.image, request, true));
#ifdef LGSM_NATIVE_SAVE_OBSERVATION_ABORT
    observation.completed = true;
#endif
}
#endif
void invoke() {
    auto path = module_path(GetModuleHandleW(nullptr));
    require(equal_path(path.filename(), fs::path(GAME_NAME)), "game_module");
    auto directory = path.parent_path().parent_path().parent_path().parent_path() / L"langame_world_save";
    auto pins = pin_directories(directory);
    auto request = read_request(directory);
    auto response = open_file(directory / L"native-response.txt", GENERIC_WRITE, OPEN_ALWAYS, 0);
    auto key = request.nonce + "\n" + request.operation + "\n" + std::to_string(request.requested_at)
        + "\n" + std::to_string(request.proxy) + "\n" + std::to_string(request.instance) + "\n" + std::to_string(request.world);
    for (auto& prior : recent) {
        if (prior.key.starts_with(request.nonce + "\n")) {
            require(prior.key == key, "nonce_conflict");
#ifdef LGSM_NATIVE_SAVE_ASYNC
            if (prior.pending) {
                try { observe(prior, request); }
                catch (...) { OutputDebugStringA("[LgsmWorldSave] Observation unavailable; original result stays unknown.\n"); }
            }
#endif
            write_receipt(response.value, prior.text); return;
        }
    }
    auto& initialized = initialization();
#ifdef LGSM_NATIVE_SAVE_ASYNC
    // Preserve every unexpired nonce even under saturation. Evicting a pending
    // receipt could turn a retransmission into another native save.
    const auto tick = GetTickCount64();
    auto available = available_receipt(tick);
    require(available != nullptr, "receipt_capacity");
    auto& slot = *available; slot = {key, receipt(request, "rejected"), false, tick};
#else
    auto& slot = recent[next_receipt++ % recent.size()]; slot = {key, receipt(request, "rejected")};
#endif
    try {
        require(initialized.image != nullptr, "native_unavailable");
        validate(*initialized.image, request);
    } catch (const std::exception& error) {
        verification_diagnostic(error); write_receipt(response.value, slot.text); return;
    } catch (...) { write_receipt(response.value, slot.text); return; }
    if (request.operation == "probe") { slot.text = receipt(request, "ready"); write_receipt(response.value, slot.text); return; }
    // Durable conservative receipt and replay record precede entry. Any
    // exception, false result, or lost final receipt remains unknown.
    slot.text = receipt(request, "unknown"); write_receipt(response.value, slot.text);
    validate(*initialized.image, request);
#ifdef LGSM_NATIVE_SAVE_ASYNC
    async_result(slot, request, guarded_async(*initialized.image, request, false));
    write_receipt(response.value, slot.text);
#else
    if (const auto status = guarded_call(*initialized.image, request.proxy)) {
        // Current-world ownership is also rechecked by the owned Lua caller.
        require(std::string_view(status) == "returned" || std::string_view(status) == "confirmed"
            || std::string_view(status) == "failed", "native_result");
        slot.text = receipt(request, status); write_receipt(response.value, slot.text);
    }
#endif
}
} // namespace
extern "C" __declspec(dllexport) int LGSM_SAVE_EXPORT(void*) noexcept {
    static SRWLOCK lock = SRWLOCK_INIT;
    if (!TryAcquireSRWLockExclusive(&lock)) return 0;
    try { invoke(); }
    catch (const std::exception& error) { verification_diagnostic(error); }
    catch (...) { OutputDebugStringA("[LgsmWorldSave] Request rejected or result unknown; never retried.\n"); }
    ReleaseSRWLockExclusive(&lock); return 0;
}
