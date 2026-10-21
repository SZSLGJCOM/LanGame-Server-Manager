// Build 25811348 only. Native addresses below are RVAs in the SHA-256-pinned
// Shipping image; no client RPC, player privilege changes, or Lua ABI imports.
#include <windows.h>
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
constexpr std::string_view GAME_HASH = "89ed65335a46db067a361d35ae4e6be80857a4c1f2453510600d632879dfbeed";
constexpr std::string_view UE4SS_HASH = "91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958";
constexpr wchar_t GAME_NAME[] = L"RSDragonwildsServer-Win64-Shipping.exe";
constexpr size_t MAX_KNOWN = 4096, MAX_REQUEST = 512;
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
    require(GetFileSizeEx(file, &size) && size.QuadPart > 0 && size.QuadPart <= 256LL * 1024 * 1024, "module_size");
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
        auto game = GetModuleHandleW(nullptr);
        auto path = module_path(game);
        require(equal_path(path.filename(), fs::path(GAME_NAME)), "game_module");
        auto win64 = path.parent_path();
        auto core = GetModuleHandleW(L"UE4SS.dll");
        require(core && same_file(module_path(core), win64 / L"ue4ss/UE4SS.dll"), "loader_module");
        HMODULE bridge{};
        require(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCWSTR>(&module_path), &bridge) != FALSE, "bridge_module");
        require(same_file(module_path(bridge), win64 / L"ue4ss/Mods/LgsmPlayerControl/native/LgsmPlayerControl.dll"), "bridge_path");
        directory = win64.parent_path().parent_path().parent_path() / L"langame_player_control";
        pins = pin_directories(directory);
        // Handles below remain pinned for the process lifetime.
        auto shipping_file = open_file(path, GENERIC_READ, OPEN_EXISTING, FILE_SHARE_READ);
        auto core_file = open_file(module_path(core), GENERIC_READ, OPEN_EXISTING, FILE_SHARE_READ);
        require(hash_file(shipping_file.value) == GAME_HASH && hash_file(core_file.value) == UE4SS_HASH, "module_fingerprint");
        shipping.value = shipping_file.value; shipping_file.value = INVALID_HANDLE_VALUE;
        ue4ss.value = core_file.value; core_file.value = INVALID_HANDLE_VALUE;
        base = reinterpret_cast<uintptr_t>(game);
        prologue(base, 0x59a24c0, "48895c240848896c2410488974241848897c242041564883");
        prologue(base, 0x5d54050, "4055534154488d6c24e04881ec20010000488b05d816a705");
        prologue(base, 0x5d51960, "48895c241848896c2420565741564881ec90000000488b05");
        prologue(base, 0x596b480, "405741544155415641574883ec30488bb9d00000004532f6");
        prologue(base, 0x5d52600, "405553574155488d6c24d84881ec28010000488b052731a7");
        game_thread = reinterpret_cast<bool (*)()>(GetProcAddress(core, "?IsInGameThreadRaw@Unreal@RC@@YA_NXZ"));
        require(game_thread != nullptr, "thread_api");
    }
};
struct Initialization {
    std::unique_ptr<Image> image;
    std::string error;
    Initialization() {
        try { image = std::make_unique<Image>(); }
        catch (const std::exception& failure) { error = failure.what(); OutputDebugStringA("[LgsmPlayerControl] Initialization failed.\n"); }
        catch (...) { error = "initialization_exception"; OutputDebugStringA("[LgsmPlayerControl] Initialization exception.\n"); }
    }
};
Initialization& initialization() { static Initialization result; return result; }
struct Request {
    std::string nonce, operation;
    uintptr_t matchmaker{}, settings{};
    size_t index{};
};
uint64_t decimal(std::string_view text) {
    require(!text.empty() && text.size() <= 20 && (text.size() == 1 || text.front() != '0'), "request_decimal");
    uint64_t value{};
    auto [end, error] = std::from_chars(text.data(), text.data() + text.size(), value);
    require(error == std::errc{} && end == text.data() + text.size(), "request_decimal");
    return value;
}
Request parse(std::string_view text) {
    constexpr std::array<std::string_view, 6> fields{
        "schema=", "nonce=", "op=", "matchmaker_address=", "settings_address=", "known_player_index="};
    std::array<std::string_view, 6> values{};
    for (size_t i = 0; i < fields.size(); ++i) {
        auto end = text.find('\n');
        require(end != text.npos && text.substr(0, end).starts_with(fields[i]), "request_format");
        values[i] = text.substr(fields[i].size(), end - fields[i].size());
        text.remove_prefix(end + 1);
    }
    require(text.empty() && values[0] == "1" && values[1].size() == 32
        && values[1].find_first_not_of("0123456789abcdef") == values[1].npos
        && (values[2] == "ban" || values[2] == "unban" || values[2] == "probe"), "request_format");
    auto index = decimal(values[5]);
    require(index < MAX_KNOWN, "request_index");
    return {std::string(values[1]), std::string(values[2]), decimal(values[3]), decimal(values[4]), static_cast<size_t>(index)};
}
Request read_request(const fs::path& directory) {
    auto file = open_file(directory / L"native-request.txt", GENERIC_READ, OPEN_EXISTING, FILE_SHARE_READ);
    LARGE_INTEGER length{};
    require(GetFileSizeEx(file.value, &length) && length.QuadPart > 0 && length.QuadPart <= MAX_REQUEST, "request_size");
    std::array<char, MAX_REQUEST + 1> bytes{};
    DWORD count{};
    require(ReadFile(file.value, bytes.data(), static_cast<DWORD>(bytes.size()), &count, nullptr) && count == length.QuadPart, "request_read");
    return parse(std::string_view(bytes.data(), count));
}
struct SharedUser { void* object{}; void* controller{}; };
static_assert(sizeof(SharedUser) == 16);
const char* dispatch(const Image& image, const Request& request) {
    require(image.game_thread(), "wrong_thread");
    auto cls = read<uintptr_t>(image.base + 0xbb31eb0);
    auto settings_class = read<uintptr_t>(image.base + 0xbb1daf0);
    require(cls && settings_class, "native_classes");
    require(readable(request.matchmaker, 0x730) && read<uintptr_t>(request.matchmaker) == image.base + 0x9dfa7e0
        && read<uintptr_t>(request.matchmaker + 0x10) == cls
        && !(read<uint32_t>(request.matchmaker + 8) & 0x40000000), "matchmaker_object");
    require(readable(request.settings, 0x108) && request.settings == read<uintptr_t>(settings_class + 0x110)
        && read<uintptr_t>(request.settings + 0x10) == settings_class, "settings_object");
    auto state = read<uintptr_t>(request.matchmaker + 0x6c8);
    // 0x3f0c1c0 gets the subsystem's UObject.Outer (+0x20) as GameInstance.
    auto game_instance = read<uintptr_t>(request.matchmaker + 0x20);
    require(readable(state, 0x3d0) && read<uintptr_t>(state) == image.base + 0xa0dec90, "native_state_layout");
    require(game_instance && read<uintptr_t>(state + 8) == game_instance, "native_state_owner");
    require(read<uintptr_t>(state + 0x388) && read<uintptr_t>(state + 0x398), "native_session_unready");
    // 0x38bcc90 (GameInstance::GetWorld) performs these exact two loads:
    // GameInstance + 0x30 -> FWorldContext; FWorldContext + 0x2c0 -> UWorld.
    auto context = read<uintptr_t>(game_instance + 0x30);
    auto world = read<uintptr_t>(context + 0x2c0);
    // OwningGameInstance is a reflected UWorld property at +552 (0x228),
    // independently observed in the isolated live reflection fixture.
    require(world && read<uintptr_t>(world + 0x1a8)
        && read<uintptr_t>(world + 0x228) == game_instance, "server_world");
    auto entries = read<uintptr_t>(request.settings + 0xd0);
    auto count = read<int32_t>(request.settings + 0xd8);
    auto capacity = read<int32_t>(request.settings + 0xdc);
    require(count >= 0 && count <= static_cast<int32_t>(MAX_KNOWN) && capacity >= count && capacity <= 8192
        && (!count || readable(entries, static_cast<size_t>(count) * 88)), "known_player_array");
    if (request.operation == "probe") return nullptr;
    require(request.index < static_cast<size_t>(count), "known_player_index");
    auto source = entries + request.index * 88;
    auto name = read<uintptr_t>(source + 48);
    auto name_length = read<int32_t>(source + 56);
    auto name_capacity = read<int32_t>(source + 60);
    require(name_length >= 0 && name_length <= 513 && name_capacity >= name_length && name_capacity <= 4096
        && (!name_length || readable(name, static_cast<size_t>(name_length) * sizeof(wchar_t))), "known_player_name");
    // The real 0x59a24c0 member ABI is RCX=source, RDX=16-byte sret. It
    // copies the FUniqueNetIdRepl shared identity and FString into an owned
    // online-user allocation. Both action callees consume exactly one owned
    // SharedUser argument (verified against their original wrapper call sites).
    SharedUser user{};
    using Factory = SharedUser* (*)(void*, SharedUser*);
    reinterpret_cast<Factory>(image.base + 0x59a24c0)(reinterpret_cast<void*>(source), &user);
    // Invalid UID writes (0,0); allocation failure writes (0x10,0). Neither
    // owns a reference. The factory never returns a non-null controller with
    // a null object, so no manual allocator/refcount ABI is needed here.
    if (!user.controller) return user.object ? "factory_allocation" : "invalid_unique_id";
    if (request.operation == "ban") {
        using Ban = void (*)(void*, void*, SharedUser*, bool);
        reinterpret_cast<Ban>(image.base + 0x5d54050)(reinterpret_cast<void*>(state), nullptr, &user, false);
        // Online Ban(false) saves itself. Native offline administration uses a
        // temporary internal flag; avoid changing that flag and explicitly run
        // the same native persistence method. It updates KnownPlayerList from
        // the runtime ban map and only writes config when a flag changed.
        using SaveBans = void (*)(void*);
        reinterpret_cast<SaveBans>(image.base + 0x5d52600)(reinterpret_cast<void*>(state));
    } else {
        using Unban = void (*)(void*, SharedUser*);
        reinterpret_cast<Unban>(image.base + 0x5d51960)(reinterpret_cast<void*>(state), &user);
    }
    // Native APIs are void. SaveConfig is called by the native path; this
    // receipt only proves dispatch, never a successful or persisted ban.
    return nullptr;
}
const char* guarded_dispatch(const Image& image, const Request& request) {
    __try { return dispatch(image, request); }
    __except (GetExceptionCode() == EXCEPTION_ACCESS_VIOLATION ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH) {
        return "native_access_violation";
    }
}
struct Receipt { std::string request, text; };
std::array<Receipt, 512> recent;
size_t next_receipt{};
void write_receipt(HANDLE file, std::string_view text) {
    DWORD count{};
    require(WriteFile(file, text.data(), static_cast<DWORD>(text.size()), &count, nullptr) && count == text.size()
        && FlushFileBuffers(file), "receipt_write");
}
void invoke() {
    auto shipping_path = module_path(GetModuleHandleW(nullptr));
    require(equal_path(shipping_path.filename(), fs::path(GAME_NAME)), "game_module");
    auto directory = shipping_path.parent_path().parent_path().parent_path().parent_path() / L"langame_player_control";
    auto pins = pin_directories(directory);
    auto request = read_request(directory);
    auto response = open_file(directory / L"native-response.txt", GENERIC_WRITE, OPEN_ALWAYS, 0);
    require(SetEndOfFile(response.value) != FALSE, "receipt_truncate");
    auto key = request.nonce + "\n" + request.operation + "\n" + std::to_string(request.matchmaker)
        + "\n" + std::to_string(request.settings) + "\n" + std::to_string(request.index);
    for (const auto& receipt : recent) {
        if (receipt.request.starts_with(request.nonce + "\n")) {
            require(receipt.request == key, "nonce_conflict");
            write_receipt(response.value, receipt.text); return;
        }
    }
    // A failed initialization is cached for this process; query probes cannot
    // trigger repeated module hashing. Restart after repairing installation.
    auto& initialized = initialization();
    const char* error{};
    try { error = initialized.image ? guarded_dispatch(*initialized.image, request) : initialized.error.c_str(); }
    catch (const std::exception& failure) { error = failure.what();
        auto text = "schema=1\nnonce=" + request.nonce + "\nop=" + request.operation + "\nstatus=failed\nerror=" + error + "\n";
        recent[next_receipt++ % recent.size()] = {key, text}; write_receipt(response.value, text); return;
    }
    auto text = "schema=1\nnonce=" + request.nonce + "\nop=" + request.operation
        + "\nstatus=" + (error ? "failed" : request.operation == "probe" ? "ready" : "dispatched")
        + "\nerror=" + (error ? error : "") + "\n";
    recent[next_receipt++ % recent.size()] = {key, text};
    write_receipt(response.value, text);
}
} // namespace

// Shared by the save observer and moderation entry. The process image, loader,
// physical plugin path and full hashes have one cached initialization owner.
bool lgsm_dragon_control_image(std::uintptr_t& base, std::filesystem::path& directory, bool (*&thread)()) noexcept {
    const auto& state = initialization();
    if (!state.image) return false;
    base=state.image->base; directory=state.image->directory; thread=state.image->game_thread;
    return thread && thread();
}

// Lua's package.loadlib passes lua_State*. No Lua symbol is imported or called;
// return 0 means no Lua results. The synchronous bounded receipt is the API.
extern "C" __declspec(dllexport) int luaopen_lgsm_dragonwilds_control(void*) noexcept {
    static SRWLOCK lock = SRWLOCK_INIT;
    if (!TryAcquireSRWLockExclusive(&lock)) return 0;
    try { invoke(); }
    catch (const std::exception&) { OutputDebugStringA("[LgsmPlayerControl] Request/receipt rejected; no successful receipt.\n"); }
    catch (...) { OutputDebugStringA("[LgsmPlayerControl] Unexpected bridge exception; no successful receipt.\n"); }
    ReleaseSRWLockExclusive(&lock);
    return 0;
}
