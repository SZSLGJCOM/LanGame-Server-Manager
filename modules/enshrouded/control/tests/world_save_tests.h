#pragma once
#include "../world_save.h"
#include <vector>
#include <cstdio>
inline bool WorldSaveTests() {
    using namespace enshrouded_control;
    // A plain synthetic memory image keeps all native reads observable and bounded.
    std::vector<unsigned char> bytes(0x8000);
    const auto put = [&]<class T>(std::uintptr_t address, T value) {
        std::memcpy(bytes.data() + address - 0x100000, &value, sizeof value);
    };
    const auto read = +[](void* context, std::uintptr_t address, void* output, std::size_t size) -> bool {
        auto& data = *static_cast<std::vector<unsigned char>*>(context);
        if (address < 0x100000 || size > data.size() || address - 0x100000 > data.size() - size) return false;
        std::memcpy(output, data.data() + address - 0x100000, size); return true;
    };
    constexpr std::uintptr_t server = 0x100000, save = 0x104000, world = 0x105000;
    put(server + 0x20, save); put(server + 0x1d0, world);
    put(save + 0x70, std::uint8_t(6));
    WorldSaveCache cache{};
    auto snapshot = CaptureWorldSave(cache, server, false, 10, 10000, read, &bytes);
    if (snapshot.ready || std::strcmp(snapshot.issue, "unsupported_save_profile")) return false;
    snapshot = CaptureWorldSave(cache, server, true, 10, 10000, read, &bytes);
    if (!snapshot.ready || snapshot.generation != 1) return false;
    ProcessIdentity identity{123, 456, {}};
    std::memset(identity.runId, 'a', 32);
    Request request{}; request.action = Action::SaveWorld; request.sequence = 10;
    request.generation = 1; request.expiresAt = 11000;
    std::memcpy(request.runId, identity.runId, sizeof request.runId);
    const auto is = [](const char* actual, const char* expected) { return !std::strcmp(actual, expected); };
    if (!is(ValidateWorldSave(request, identity, snapshot, 10001), "none")) return false;
    Snapshot players{};
    if (!is(ValidateRequest(request, identity, players, 10001), "world_save_requires_native_context")) return false;
    const auto json = SerializeWorldSave(identity, snapshot);
    if (json.find("\"generation\":\"1\"") == std::string::npos
        || json.find("\"ready\":true") == std::string::npos) return false;
    auto changed = request; changed.accountId = 1;
    if (!is(ValidateWorldSave(changed, identity, snapshot, 10001), "invalid_world_save_target")) return false;
    changed = request; changed.sessionHandle = 64;
    if (!is(ValidateWorldSave(changed, identity, snapshot, 10001), "invalid_world_save_target")) return false;
    changed = request; changed.generation = 2;
    if (!is(ValidateWorldSave(changed, identity, snapshot, 10001), "world_context_changed")) return false;
    if (!is(ValidateWorldSave(request, identity, snapshot, 11001), "request_expired")) return false;
    if (!is(ValidateWorldSave(request, identity, snapshot, 9999), "snapshot_not_current")) return false;
    request.expiresAt = 13000;
    if (!is(ValidateWorldSave(request, identity, snapshot, 11001), "snapshot_not_current")) return false;
    for (const auto flag : {std::uint8_t(7), std::uint8_t(14)}) {
        put(save + 0x70, flag);
        snapshot = CaptureWorldSave(cache, server, true, 11, 10001, read, &bytes);
        if (snapshot.ready || !is(snapshot.issue, "native_save_busy")) return false;
    }
    put(save + 0x70, std::uint8_t(6)); put(save + 0x78, std::uint8_t(1));
    if (CaptureWorldSave(cache, server, true, 11, 10001, read, &bytes).ready) return false;
    put(save + 0x78, std::uint8_t(0)); put(server + 0x1820, std::uint8_t(1));
    snapshot = CaptureWorldSave(cache, server, true, 11, 10001, read, &bytes);
    if (snapshot.ready || !is(snapshot.issue, "server_stopping")) return false;
    put(server + 0x1820, std::uint8_t(0)); put(server + 0x18aa, std::uint8_t(1));
    if (CaptureWorldSave(cache, server, true, 11, 10001, read, &bytes).ready) return false;
    put(server + 0x18aa, std::uint8_t(0)); put(server + 0x1d0, std::uintptr_t(0));
    if (CaptureWorldSave(cache, server, true, 12, 10002, read, &bytes).ready) return false;
    put(server + 0x1d0, world);
    snapshot = CaptureWorldSave(cache, server, true, 13, 10003, read, &bytes);
    if (!snapshot.ready || snapshot.generation != 2
        || !is(ValidateWorldSave(request, identity, snapshot, 10003), "world_context_changed")) return false;
    put(server + 0x1d0, world + 0x1000);
    snapshot = CaptureWorldSave(cache, server, true, 14, 10004, read, &bytes);
    if (!snapshot.ready || snapshot.generation != 3) return false;
    cache.generation = UINT64_MAX; put(server + 0x1d0, world);
    snapshot = CaptureWorldSave(cache, server, true, 15, 10005, read, &bytes);
    if (snapshot.ready || !is(snapshot.issue, "world_generation_exhausted")) return false;
    const auto raw = std::string(kHeader) + "request_id=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\nrun_id=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\naction=save_world\nsnapshot_sequence=10\nsession_handle=0\ngeneration=1\naccount_id=0\nexpires_at_tick=11000\n";
    if (!ParseRequest(raw, changed) || changed.action != Action::SaveWorld
        || changed.accountId || changed.sessionHandle || std::strcmp(ActionName(changed.action), "save_world")) return false;
    // A return from the request entry alone is never completion. Rebind the
    // world normally, then emulate only the fixed native persistence boundary.
    cache = {}; put(server + 0x1d0, world); put(save + 0x70, std::uint8_t(6));
    put(save + 0x78, std::uint8_t(0)); put(save + 0x80, std::uint64_t(77));
    snapshot = CaptureWorldSave(cache, server, true, 20, 20000, read, &bytes);
    WorldSaveCompletion completion{};
    if (BeginWorldSaveCompletion(completion, server, snapshot, cache, 20000, read, &bytes)) return false;
    put(save + 0x70, std::uint8_t(7)); put(save + 0x78, std::uint8_t(1));
    if (!BeginWorldSaveCompletion(completion, server, snapshot, cache, 20000, read, &bytes)) return false;
    if (BeginWorldSaveCompletion(completion, server, snapshot, cache, 20000, read, &bytes)) return false;
    for (std::uint8_t phase = 1; phase <= 6; ++phase) {
        put(save + 0x78, phase);
        if (ObserveWorldSaveCompletion(completion, server, cache, 20001, read, &bytes) != WorldSaveResult::Waiting) return false;
    }
    put(save + 0x70, std::uint8_t(6)); put(save + 0x78, std::uint8_t(0));
    if (ObserveWorldSaveCompletion(completion, server, cache, 20002, read, &bytes) != WorldSaveResult::Completed || completion.active) return false;
    const auto begin = [&]() {
        put(save + 0x70, std::uint8_t(7)); put(save + 0x78, std::uint8_t(1));
        put(save + 0x80, std::uint64_t(77)); completion = {};
        return BeginWorldSaveCompletion(completion, server, snapshot, cache, 20000, read, &bytes);
    };
    if (!begin()) return false;
    put(save + 0x70, std::uint8_t(6)); put(save + 0x78, std::uint8_t(0));
    if (ObserveWorldSaveCompletion(completion, server, cache, 20002, read, &bytes) != WorldSaveResult::Unknown) return false;
    if (!begin()) return false;
    put(save + 0x70, std::uint8_t(14)); put(save + 0x78, std::uint8_t(0));
    if (ObserveWorldSaveCompletion(completion, server, cache, 20002, read, &bytes) != WorldSaveResult::Failed) return false;
    if (!begin()) return false;
    put(save + 0x78, std::uint8_t(6));
    if (ObserveWorldSaveCompletion(completion, server, cache, 20001, read, &bytes) != WorldSaveResult::Waiting) return false;
    put(save + 0x78, std::uint8_t(1));
    if (ObserveWorldSaveCompletion(completion, server, cache, 20002, read, &bytes) != WorldSaveResult::Unknown) return false;
    if (!begin()) return false;
    put(save + 0x80, std::uint64_t(78));
    if (ObserveWorldSaveCompletion(completion, server, cache, 20002, read, &bytes) != WorldSaveResult::Unknown) return false;
    if (!begin()) return false;
    if (ObserveWorldSaveCompletion(completion, server, cache, 140000, read, &bytes) != WorldSaveResult::Unknown) return false;
    if (!begin()) return false;
    ++cache.generation;
    if (ObserveWorldSaveCompletion(completion, server, cache, 20002, read, &bytes) != WorldSaveResult::Unknown) return false;
    Response completed{request, true, "none", true};
    if (SerializeResponse(identity, completed).find("\"status\":\"completed\"") == std::string::npos) return false;
    completed.saveCompleted = false;
    if (SerializeResponse(identity, completed).find("\"status\":\"dispatched\"") == std::string::npos) return false;
    std::puts("PASS world save: exact-profile gate, native readiness/busy/stop, opaque world generations, expiry and non-player authority");
    return true;
}
