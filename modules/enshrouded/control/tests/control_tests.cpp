#include "../control_protocol.h"
#include "../request_history.h"
#include "world_save_tests.h"
#include <vector>
#include <cstdio>
#include <string>
using namespace enshrouded_control;
namespace {
struct Fixture {
    static constexpr std::uintptr_t base = 0x100000;
    std::vector<unsigned char> bytes = std::vector<unsigned char>(0x20000);
    std::uintptr_t manager = base + 0x4000, slot = base + 0x1e0, output = base + 0x10000;
    IdentityCache cache{};
    Fixture() { set(base + 0x10, manager); BindContext(cache, base, read, this); }
    template<class T> void set(std::uintptr_t address, T value) {
        std::memcpy(bytes.data() + address - base, &value, sizeof value);
    }
    static bool read(void* context, std::uintptr_t address, void* out, std::size_t size) {
        auto& f = *static_cast<Fixture*>(context);
        if (address < base || size > f.bytes.size() || address - base > f.bytes.size() - size) return false;
        std::memcpy(out, f.bytes.data() + address - base, size); return true;
    }
    void player() {
        set(slot, std::uint32_t(64)); set(slot + 0xa, std::uint8_t(3));
        set(slot + 0x20, base + 0x18000); set(slot + 0x28, std::uint64_t(5));
        set(output, std::uint32_t(64)); set(output + 0x10, std::uint8_t(3));
        set(output + 0x18, std::uint64_t(0x0110000100000001ull));
        set(output + 0x20, std::uint64_t(9007199254740993ull));
        set(output + 0x28, base + 0x18000); set(output + 0x30, std::uint64_t(5));
        std::memcpy(bytes.data() + 0x18000, "Alice", 5);
    }
    const char* observe(std::uint64_t status = 0, std::uintptr_t caller = kCallerA) {
        const NativeLookupContext lookup{output, manager, 64, caller};
        return ObserveLocalCopy(cache, lookup, caller, status, 10000, read, this);
    }
    Snapshot capture(std::uint64_t now = 10001) { return Capture(cache, base, now, read, this); }
};
bool Is(const char* value, const char* expected) { return !std::strcmp(value, expected); }
std::string RequestText() {
    return std::string(kHeader) + "request_id=0123456789abcdef0123456789abcdef\n"
        "run_id=fedcba9876543210fedcba9876543210\naction=ban\nsnapshot_sequence=10\n"
        "session_handle=64\ngeneration=1\naccount_id=9007199254740993\nexpires_at_tick=11000\n";
}
}
int main() {
    if (!WorldSaveTests()) return 100;
    Fixture empty;
    if (!empty.capture().complete || empty.capture().count) return 1;
    Fixture valid; valid.player();
    if (valid.capture().complete || !Is(valid.observe(), "none")) return 2;
    auto current = valid.capture();
    if (!current.complete || current.count != 1 || current.players[0].generation != 1
        || current.players[0].accountId != 9007199254740993ull) return 3;
    Fixture hotfix; hotfix.player();
    hotfix.cache.callerA = 0x7b3b8f; hotfix.cache.callerB = 0x7b497e;
    if (!Is(hotfix.observe(), "unexpected_native_caller") || hotfix.capture().complete
        || !Is(hotfix.observe(0, hotfix.cache.callerA), "none") || !hotfix.capture().complete
        || !Is(hotfix.observe(0, hotfix.cache.callerB), "none")) return 38;
    const auto generation = current.players[0].generation;
    valid.set(valid.slot + 0x10, std::uint8_t(1));
    if (!valid.capture().players[0].moderationImmune) return 4;
    valid.set(valid.slot + 0x10, std::uint8_t(2));
    if (valid.capture().players[0].moderationImmune) return 5; // native mask is 1.
    valid.set(valid.slot + 0x10, std::uint8_t(0)); valid.set(valid.output + 0x148, std::uint8_t(1)); valid.observe();
    if (!valid.capture().players[0].moderationImmune) return 25;
    valid.set(valid.output + 0x148, std::uint8_t(2)); valid.observe();
    if (valid.capture().players[0].moderationImmune) return 26;
    valid.set(valid.slot + 0xa, std::uint8_t(4));
    if (valid.capture().complete || valid.cache.entries[0].player.generation) return 6;
    valid.set(valid.slot + 0xa, std::uint8_t(3)); valid.observe();
    if (valid.capture().players[0].generation <= generation) return 7;
    valid.set(valid.slot, std::uint32_t(0)); valid.capture(); valid.set(valid.slot, std::uint32_t(64)); valid.observe();
    if (valid.capture().players[0].generation <= generation + 1) return 8;
    if (valid.capture(11001).complete || valid.cache.entries[0].player.generation) return 9;
    valid.observe();
    if (!Is(valid.observe(0x1f), "native_lookup_failed") || valid.capture().complete) return 10;
    if (!Is(valid.observe(0, 0x123), "unexpected_native_caller")) return 11;
    valid.observe(); std::memcpy(valid.bytes.data() + 0x18000, "Other", 5);
    if (valid.capture().complete || valid.cache.entries[0].player.generation) return 12;
    Fixture managerChanged; managerChanged.player(); managerChanged.observe();
    managerChanged.set(Fixture::base + 0x10, managerChanged.manager + 8);
    if (managerChanged.capture().complete || !managerChanged.cache.degraded) return 13;
    Fixture duplicate; duplicate.player(); duplicate.observe();
    std::memcpy(duplicate.bytes.data() + duplicate.slot + 0x148 - Fixture::base,
        duplicate.bytes.data() + duplicate.slot - Fixture::base, 0x30);
    if (duplicate.capture().complete) return 14;
    if (ValidName("\xc0\x80", 2) || ValidName("\xed\xa0\x80", 3) || ValidName("\0", 1)) return 15;
    Request request{}; const auto raw = RequestText();
    if (!ParseRequest(raw, request) || request.accountId != 9007199254740993ull) return 16;
    const std::string bad[] = {raw + "\n", raw.substr(0, raw.size() - 1), raw + "action=kick\n",
        std::string(1025, 'a'), "LGSM_ENSHROUDED_CONTROL_1\r\n", raw + "\xff"};
    for (const auto& text : bad) if (ParseRequest(text, request)) return 17;
    auto overflow = raw; overflow.replace(overflow.find("account_id=9007199254740993"), 27,
        "account_id=18446744073709551616");
    if (ParseRequest(overflow, request)) return 18;
    ParseRequest(raw, request); ProcessIdentity identity{123, 456, {}};
    std::memcpy(identity.runId, request.runId, 33);
    Fixture actionFixture; actionFixture.player(); actionFixture.observe(); current = actionFixture.capture(); current.sequence = 11;
    if (!Is(ValidateRequest(request, identity, current, 10002), "none")) return 19;
    auto changed = request; ++changed.generation;
    if (!Is(ValidateRequest(changed, identity, current, 10002), "target_session_changed")) return 20;
    current.players[0].moderationImmune = true;
    if (!Is(ValidateRequest(request, identity, current, 10002), "native_target_immune")) return 21;
    if (!Is(ValidateRequest(request, identity, current, 12000), "request_expired")) return 22;
    changed = request; changed.action = Action::Unban; changed.sessionHandle = 0; changed.generation = 0; changed.sequence = 0;
    current.complete = false;
    if (!Is(ValidateRequest(changed, identity, current, 10002), "none")) return 23;
    const auto json = SerializeSnapshot(identity, current);
    if (json.find("\"account_id\":\"9007199254740993\"") == std::string::npos
        || json.find("\"generation\":\"1\"") == std::string::npos) return 24;
    RequestHistory history{}; HistoryRecord* record{}; const char* issue{};
    if (history.Admit(request, identity, 10000, record, issue) != HistoryAdmission::New
        || !history.Complete({request, true, "none"})) return 27;
    for (unsigned i = 1; i <= 16; ++i) {
        auto churn = request; std::snprintf(churn.requestId, sizeof churn.requestId, "%032x", i);
        const auto admission = history.Admit(churn, identity, 10000, record, issue);
        if (i < 16) {
            if (admission != HistoryAdmission::New || !history.Complete({churn, false, "request_queue_busy"})) return 28;
        } else if (admission != HistoryAdmission::Full) return 29;
    }
    if (history.Admit(request, identity, 10000, record, issue) != HistoryAdmission::Replay
        || !record->result.dispatched) return 30;
    auto conflict = request; ++conflict.accountId;
    if (history.Admit(conflict, identity, 10000, record, issue) != HistoryAdmission::Conflict) return 31;
    auto renewed = request; renewed.expiresAt = 12000; std::memset(renewed.requestId, 'b', 32);
    if (history.Admit(renewed, identity, 11001, record, issue) != HistoryAdmission::New) return 32;
    RequestHistory pendingHistory{};
    if (pendingHistory.Admit(request, identity, 10000, record, issue) != HistoryAdmission::New) return 33;
    auto invalidExpiry = request; invalidExpiry.expiresAt = UINT64_MAX; std::memset(invalidExpiry.requestId, 'c', 32);
    if (pendingHistory.Admit(invalidExpiry, identity, 10000, record, issue) != HistoryAdmission::InvalidEnvelope) return 34;
    for (unsigned i = 1; i < 16; ++i) {
        auto busy = request; std::snprintf(busy.requestId, sizeof busy.requestId, "%032x", i);
        if (pendingHistory.Admit(busy, identity, 10000, record, issue) != HistoryAdmission::New
            || !pendingHistory.Complete({busy, false, "request_queue_busy"})) return 35;
    }
    if (pendingHistory.Admit(request, identity, 10000, record, issue) != HistoryAdmission::Pending
        || !pendingHistory.Find(request.requestId) || pendingHistory.Find(request.requestId)->completed) return 36;
    // An expired pending/dispatching record is never replaced by new history.
    renewed.expiresAt = 13000;
    if (pendingHistory.Admit(renewed, identity, 11001, record, issue) != HistoryAdmission::New
        || !pendingHistory.Find(request.requestId)) return 37;
    std::puts("PASS identity cache and protocol: native-copy lifecycle, generations, expiry, mask1 immunity, context, duplicates, strict parsing, target authorization");
    std::puts("PASS request history: unexpired replay after 16 churns, pending retention, full rejection, expired completed reuse, conflict and invalid expiry");
    return 0;
}
