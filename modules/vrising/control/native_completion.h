#pragma once
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdint>
#include <cstring>

namespace vrising_save {
enum class FailureReason : int {
    None = 0, TriggerInstance = 1, TriggerReason = 2, TriggerCaller = 3,
    TriggerRepeated = 4, TriggerNameEncoding = 5, TriggerNameFormat = 6,
    NativeRejected = 7, TriggerMissing = 8, SerializeLifecycle = 9,
    EventShape = 10, EventIdentity = 11, TargetIdentity = 12,
    FinalizerRepeated = 13, FinalResult = 14, ResultAddress = 15,
    OperationIdentity = 16, NativeCode = 17, OwnerThread = 18
};
// All addresses/layouts below belong to the exact GameAssembly/metadata pair
// verified by the managed entry point. No game-owned container is read by a
// polling thread: Result is copied only while its original finalizer owns it.
constexpr std::uintptr_t trigger_rva = 0x7f3d030;
constexpr std::uintptr_t serialize_rva = 0x7f25570;
constexpr std::uintptr_t finalize_rva = 0x7f33970;
constexpr std::uintptr_t autosave_return_rva = 0x7f3cfc0;
constexpr std::size_t final_result_offset = 0x170;
constexpr std::size_t dependency_offset = 0x198;

struct FixedName {
    char bytes[512]{};
    unsigned length{};
    bool read(const void* value, unsigned capacity, bool native_path = false) noexcept {
        if (!value || capacity < 4 || capacity > sizeof bytes) return false;
        const auto data = static_cast<const unsigned char*>(value);
        unsigned short count{}; std::memcpy(&count, data, sizeof count);
        if (!count || count > capacity - 3) return false;
        for (unsigned i = 0; i < count; ++i) {
            const auto c = data[i + 2];
            if (c < 32 || c == 127 || (!native_path && c > 126)) return false;
        }
        if (data[count + 2] != 0) return false;
        length = count; std::memcpy(bytes, data + 2, count); bytes[count] = 0;
        return true;
    }
    bool leaf_equals(const FixedName& name) const noexcept {
        unsigned at = length;
        while (at && bytes[at - 1] != '/' && bytes[at - 1] != '\\') --at;
        return length - at == name.length && !std::memcmp(bytes + at, name.bytes, name.length);
    }
    bool equals(const FixedName& other) const noexcept {
        return length == other.length && !std::memcmp(bytes, other.bytes, length);
    }
    bool native_autosave() const noexcept {
        // The pinned TriggerAutoSave uses the exact native format
        // "{0}{1}.save" with prefix "AutoSave_" and its integer counter.
        constexpr char prefix[] = "AutoSave_", suffix[] = ".save";
        constexpr unsigned start = sizeof prefix - 1, tail = sizeof suffix - 1;
        if (length <= start + tail || std::memcmp(bytes, prefix, start) ||
            std::memcmp(bytes + length - tail, suffix, tail)) return false;
        for (unsigned i = start; i < length - tail; ++i)
            if (bytes[i] < '0' || bytes[i] > '9') return false;
        return true;
    }
};

// Only a single SaveLoop operation owns this state. Ticket prevents late native
// callbacks from acknowledging a later IPC request after timeout or unload.
struct Completion {
    std::uint64_t ticket{};
    void* trigger{};
    void* serializer{};
    unsigned char* result{};
    DWORD owner{};
    unsigned trigger_calls{}, serializer_calls{}, finalizer_calls{};
    FixedName native_name{}, event_name{};
    bool invoking{}, admitted{}, serialized{}, invalid{}, finished{};
    unsigned char native_result{};
    FailureReason failure{};
    void fail(FailureReason reason) noexcept {
        invalid = true; if (failure == FailureReason::None) failure = reason;
    }

    bool begin(std::uint64_t identity, void* trigger_instance, void* serializer_instance,
        unsigned char* result_address, DWORD thread) noexcept {
        if (ticket || !identity || !trigger_instance || !serializer_instance || !result_address || !thread) return false;
        *this = {}; ticket = identity; trigger = trigger_instance; serializer = serializer_instance;
        result = result_address; owner = thread; invoking = true; return true;
    }
    void triggered(void* instance, int reason, const void* name, DWORD thread, bool native_caller) noexcept {
        if (!ticket || !invoking || thread != owner) return;
        if (instance != trigger) { fail(FailureReason::TriggerInstance); return; }
        if (reason != 2) { fail(FailureReason::TriggerReason); return; }
        if (!native_caller) { fail(FailureReason::TriggerCaller); return; }
        if (++trigger_calls != 1) { fail(FailureReason::TriggerRepeated); return; }
        if (!native_name.read(name, 128)) { fail(FailureReason::TriggerNameEncoding); return; }
        if (!native_name.native_autosave()) fail(FailureReason::TriggerNameFormat);
    }
    void end(bool accepted) noexcept {
        invoking = false; admitted = accepted;
        if (!accepted) fail(FailureReason::NativeRejected);
        else if (trigger_calls != 1 || !native_name.length) fail(FailureReason::TriggerMissing);
    }
    bool serializing(void* instance, const void* event, DWORD thread) noexcept {
        if (!ticket || instance != serializer) return false;
        // A second native generation must never inherit this request's result
        // pointer, even when the first finalizer has already returned.
        if (!admitted || invoking || thread != owner || ++serializer_calls != 1 || invalid) {
            fail(FailureReason::SerializeLifecycle); return false;
        }
        const auto data = static_cast<const unsigned char*>(event);
        int reason{}; if (!data) { fail(FailureReason::EventShape); return false; }
        std::memcpy(&reason, data, sizeof reason);
        if (reason != 2 || !event_name.read(data + 4, 512) || data[516] > 1) {
            fail(FailureReason::EventShape); return false;
        }
        FixedName expected = native_name;
        if (data[516]) {
            if (expected.length > sizeof expected.bytes - 4) { fail(FailureReason::EventShape); return false; }
            std::memcpy(expected.bytes + expected.length, ".gz", 4); expected.length += 3;
        }
        if (!expected.equals(event_name)) { fail(FailureReason::EventIdentity); return false; }
        serialized = true; return true;
    }
    std::uint64_t finalizing(void* job) noexcept {
        if (!ticket || !serialized || invalid || !job) return 0;
        auto data = static_cast<unsigned char*>(job);
        unsigned char* address{}; std::memcpy(&address, data + 0x410, sizeof address);
        if (address != result) return 0;
        FixedName target;
        if (!target.read(data + 0x200, 512, true) || !target.leaf_equals(event_name)) { fail(FailureReason::TargetIdentity); return 0; }
        if (++finalizer_calls != 1) { fail(FailureReason::FinalizerRepeated); return 0; }
        return ticket;
    }
    void finalized(std::uint64_t identity, unsigned char value) noexcept {
        if (!identity || ticket != identity || invalid) return;
        if (finalizer_calls != 1 || value < 3 || value > 10) { fail(FailureReason::FinalResult); return; }
        native_result = value; finished = true;
    }
    int observe(std::uint64_t identity) const noexcept {
        if (!identity || ticket != identity || invalid || !admitted) return -1;
        if (!finished) return 0;
        return native_result == 3 ? 1 : 2;
    }
};
}
