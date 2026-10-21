#include "native_completion.h"
#include "native_unwind.h"
#include "../../enshrouded/control/vendor/minhook/include/MinHook.h"
#include <bcrypt.h>
#include <intrin.h>
#include <array>
#include <limits>

namespace vrising_save {
namespace {
SRWLOCK gate = SRWLOCK_INIT;
Completion pending;
std::uint64_t sequence{};
std::uintptr_t image{};
bool installed{};
int install_diagnostic{};
HANDLE image_file = INVALID_HANDLE_VALUE, mapping{};
const unsigned char* file_bytes{};
std::size_t file_length{};
struct Lock { Lock() noexcept { AcquireSRWLockExclusive(&gate); } ~Lock() { ReleaseSRWLockExclusive(&gate); } };
using Trigger = bool (*)(void*, int, const void*, const void*, const void*);
using Serialize = void (*)(void*, const void*, const void*, const void*);
using Finalize = void (*)(void*, const void*);
Trigger original_trigger{};
Serialize original_serialize{};
Finalize original_finalize{};
struct CodePin { std::uintptr_t rva{}; std::size_t count{}; const unsigned char* original{}; unsigned skip{}; unsigned char patch[6]{}; };
std::array<CodePin, 7> pins{};
std::array<UnwindRegistration, 3> unwind{};

bool read(const void* address, void* output, std::size_t count) noexcept {
    SIZE_T copied{};
    return address && ReadProcessMemory(GetCurrentProcess(), address, output, count, &copied) && copied == count;
}
const unsigned char* rva_file(DWORD rva, std::size_t count) noexcept {
    if (!file_bytes || file_length < sizeof(IMAGE_DOS_HEADER)) return nullptr;
    auto dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(file_bytes);
    if (dos->e_magic != IMAGE_DOS_SIGNATURE || dos->e_lfanew < 0 ||
        static_cast<std::size_t>(dos->e_lfanew) > file_length - sizeof(IMAGE_NT_HEADERS64)) return nullptr;
    auto nt = reinterpret_cast<const IMAGE_NT_HEADERS64*>(file_bytes + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE || nt->FileHeader.Machine != IMAGE_FILE_MACHINE_AMD64 ||
        nt->FileHeader.NumberOfSections > 128) return nullptr;
    const auto sections = IMAGE_FIRST_SECTION(nt);
    for (unsigned i = 0; i < nt->FileHeader.NumberOfSections; ++i) {
        const auto offset = reinterpret_cast<const unsigned char*>(&sections[i]) - file_bytes;
        if (offset < 0 || static_cast<std::size_t>(offset) > file_length - sizeof(IMAGE_SECTION_HEADER)) return nullptr;
        const auto& section = sections[i];
        if (rva < section.VirtualAddress) continue;
        const auto within = static_cast<std::size_t>(rva - section.VirtualAddress);
        if (within > section.SizeOfRawData || count > section.SizeOfRawData - within) continue;
        const auto raw = static_cast<std::size_t>(section.PointerToRawData) + within;
        if (raw > file_length || count > file_length - raw) return nullptr;
        return file_bytes + raw;
    }
    return nullptr;
}
bool identify_image() noexcept {
    image = reinterpret_cast<std::uintptr_t>(GetModuleHandleW(L"GameAssembly.dll"));
    if (!image) return false;
    wchar_t path[32768]{};
    const auto count = GetModuleFileNameW(reinterpret_cast<HMODULE>(image), path, static_cast<DWORD>(std::size(path)));
    if (!count || count >= std::size(path)) return false;
    image_file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT, nullptr);
    if (image_file == INVALID_HANDLE_VALUE) return false;
    FILE_ATTRIBUTE_TAG_INFO attributes{}; LARGE_INTEGER size{};
    if (!GetFileInformationByHandleEx(image_file, FileAttributeTagInfo, &attributes, sizeof attributes) ||
        (attributes.FileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY)) ||
        !GetFileSizeEx(image_file, &size) || size.QuadPart <= 0 || size.QuadPart > 512LL * 1024 * 1024) return false;
    BCRYPT_ALG_HANDLE algorithm{}; BCRYPT_HASH_HANDLE hash{};
    unsigned char object[1024]{}, digest[32]{}, bytes[64 * 1024]{}; DWORD object_size{}, returned{};
    bool ok = BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) >= 0 &&
        BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH, reinterpret_cast<PUCHAR>(&object_size), sizeof object_size, &returned, 0) >= 0 &&
        object_size <= sizeof object && BCryptCreateHash(algorithm, &hash, object, object_size, nullptr, 0, 0) >= 0;
    while (ok) {
        DWORD received{};
        if (!ReadFile(image_file, bytes, sizeof bytes, &received, nullptr)) { ok = false; break; }
        if (!received) break;
        if (BCryptHashData(hash, bytes, received, 0) < 0) { ok = false; break; }
    }
    ok = ok && BCryptFinishHash(hash, digest, sizeof digest, 0) >= 0;
    if (hash) BCryptDestroyHash(hash);
    if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0);
    constexpr unsigned char expected[] = {0xff,0x36,0x65,0x77,0xfb,0x0c,0x70,0x7c,0x42,0xb8,0xbd,0xe2,0x42,0x30,0x46,0x53,
        0xaa,0xa3,0x7f,0x6f,0xf0,0x66,0x5a,0xb9,0x52,0x01,0x50,0xce,0xb6,0x92,0xae,0xd1};
    if (!ok || std::memcmp(expected, digest, sizeof digest)) return false;
    mapping = CreateFileMappingW(image_file, nullptr, PAGE_READONLY, 0, 0, nullptr);
    if (!mapping) return false;
    file_bytes = static_cast<const unsigned char*>(MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 0));
    file_length = static_cast<std::size_t>(size.QuadPart);
    return file_bytes != nullptr;
}
bool code_matches() noexcept {
    unsigned char bytes[65536]{};
    for (const auto& pin : pins) {
        if (!pin.original || !pin.count || pin.count > sizeof bytes ||
            !read(reinterpret_cast<const void*>(image + pin.rva), bytes, pin.count) ||
            (pin.skip && std::memcmp(bytes, pin.patch, pin.skip)) ||
            std::memcmp(bytes + pin.skip, pin.original + pin.skip, pin.count - pin.skip)) return false;
    }
    return true;
}
bool prepare_pins() noexcept {
    constexpr std::uintptr_t rvas[] = {trigger_rva, serialize_rva, finalize_rva,
        0x7f339c0, 0x7f31ee0, 0x7f2a310, 0x7f3cb70};
    for (unsigned i = 0; i < std::size(rvas); ++i) {
        DWORD64 base{};
        auto function = RtlLookupFunctionEntry(image + rvas[i], &base, nullptr);
        if (!function || base != image || function->BeginAddress != rvas[i] ||
            function->EndAddress <= function->BeginAddress) return false;
        auto& pin = pins[i]; pin.rva = rvas[i]; pin.count = function->EndAddress - function->BeginAddress;
        pin.original = rva_file(static_cast<DWORD>(rvas[i]), pin.count);
        if (!pin.original || pin.count > 65536) return false;
    }
    return code_matches();
}
bool hook_trigger(void* instance, int reason, const void* name, const void* settings, const void* method) {
    {
        Lock lock;
        // The native autosave callsite, actual trigger instance and owner
        // thread jointly bind the minted native name to this request.
        pending.triggered(instance, reason, name, GetCurrentThreadId(),
            reinterpret_cast<std::uintptr_t>(_ReturnAddress()) == image + autosave_return_rva);
    }
    return original_trigger(instance, reason, name, settings, method);
}
void hook_serialize(void* instance, const void* event, const void* settings, const void* method) {
    { Lock lock; pending.serializing(instance, event, GetCurrentThreadId()); }
    // No managed trampoline and no exception handler surrounds the original.
    original_serialize(instance, event, settings, method);
}
void hook_finalize(void* job, const void* method) {
    std::uint64_t ticket{};
    unsigned char* expected_result{};
    { Lock lock; ticket = pending.finalizing(job); if (ticket) expected_result = pending.result; }
    original_finalize(job, method);
    if (!ticket) return;
    // Execute just wrote Result.Value after MoveCopyAndFinalizeSave returned.
    // This is still the job's owner thread, before returning to Unity jobs.
    // No IsCompleted polling or observer-initiated Complete call is involved.
    unsigned char* address{}; unsigned char value{};
    const bool valid = read(static_cast<unsigned char*>(job) + 0x410, &address, sizeof address) &&
        address == expected_result && read(address, &value, 1);
    Lock lock;
    if (!valid) { if (pending.ticket == ticket) pending.fail(FailureReason::ResultAddress); }
    else pending.finalized(ticket, value);
}
bool install() noexcept {
    if (installed) return code_matches();
    install_diagnostic = 1;
    if (!identify_image()) return false;
    install_diagnostic = 2;
    if (!prepare_pins()) return false;
    // Keep the module, trampolines and unwind records alive until process exit.
    HMODULE self{};
    install_diagnostic = 3;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<LPCWSTR>(&hook_trigger), &self)) return false;
    if (MH_Initialize() != MH_OK) return false;
    const auto target = [](std::uintptr_t rva) { return reinterpret_cast<void*>(image + rva); };
    const auto create = [](void* function, void* replacement, void** original, HookKind kind, unsigned index) {
        return MH_CreateHook(function, replacement, original) == MH_OK && register_unwind(unwind[index], *original, function, kind);
    };
    install_diagnostic = 4;
    if (!create(target(trigger_rva), reinterpret_cast<void*>(&hook_trigger), reinterpret_cast<void**>(&original_trigger), HookKind::Trigger, 0)) return false;
    install_diagnostic = 5;
    if (!create(target(serialize_rva), reinterpret_cast<void*>(&hook_serialize), reinterpret_cast<void**>(&original_serialize), HookKind::Serialize, 1)) return false;
    install_diagnostic = 6;
    if (!create(target(finalize_rva), reinterpret_cast<void*>(&hook_finalize), reinterpret_cast<void**>(&original_finalize), HookKind::Finalize, 2)) return false;
    install_diagnostic = 7;
    for (unsigned i = 0; i < 3; ++i) {
        if (MH_QueueEnableHook(target(pins[i].rva)) != MH_OK) return false;
    }
    install_diagnostic = 8;
    if (MH_ApplyQueued() != MH_OK) return false;
    for (unsigned i = 0; i < 3; ++i) {
        auto& pin = pins[i]; pin.skip = i == 2 ? 6 : 5;
        if (!read(target(pin.rva), pin.patch, pin.skip)) return false;
    }
    install_diagnostic = 9;
    installed = code_matches();
    if (installed) install_diagnostic = 0;
    return installed;
}
}
}

extern "C" __declspec(dllexport) int __cdecl lgsm_vr_save_install() noexcept {
    vrising_save::Lock lock;
    return vrising_save::install() ? 1 : 0;
}
extern "C" __declspec(dllexport) int __cdecl lgsm_vr_save_diagnostic() noexcept {
    vrising_save::Lock lock;
    return vrising_save::install_diagnostic;
}
extern "C" __declspec(dllexport) int __cdecl lgsm_vr_save_operation_diagnostic(std::uint64_t ticket) noexcept {
    vrising_save::Lock lock;
    using namespace vrising_save;
    return ticket && pending.ticket == ticket ? static_cast<int>(pending.failure) : static_cast<int>(FailureReason::OperationIdentity);
}
extern "C" __declspec(dllexport) std::uint64_t __cdecl lgsm_vr_save_begin(void* trigger, void* serializer) noexcept {
    using namespace vrising_save;
    Lock lock;
    unsigned char has_dependency{}; unsigned char* result{};
    if (!trigger || !serializer || !installed || !code_matches() || pending.ticket || sequence == UINT64_MAX ||
        !read(static_cast<unsigned char*>(serializer) + dependency_offset, &has_dependency, 1) || has_dependency ||
        !read(static_cast<unsigned char*>(serializer) + final_result_offset, &result, sizeof result)) return 0;
    return pending.begin(++sequence, trigger, serializer, result, GetCurrentThreadId()) ? sequence : 0;
}
extern "C" __declspec(dllexport) int __cdecl lgsm_vr_save_end(std::uint64_t ticket, int admitted) noexcept {
    using namespace vrising_save;
    Lock lock;
    if (!ticket || pending.ticket != ticket) return -1;
    if (pending.owner != GetCurrentThreadId()) { pending.fail(FailureReason::OwnerThread); return -1; }
    pending.end(admitted == 1); return pending.invalid ? -1 : 0;
}
extern "C" __declspec(dllexport) int __cdecl lgsm_vr_save_observe(std::uint64_t ticket) noexcept {
    using namespace vrising_save;
    Lock lock;
    if (!installed || !code_matches()) { pending.fail(FailureReason::NativeCode); return -1; }
    if (pending.owner != GetCurrentThreadId()) { pending.fail(FailureReason::OwnerThread); return -1; }
    return pending.observe(ticket);
}
extern "C" __declspec(dllexport) void __cdecl lgsm_vr_save_abandon(std::uint64_t ticket) noexcept {
    using namespace vrising_save;
    Lock lock;
    if (ticket && pending.ticket == ticket) pending = {};
}
