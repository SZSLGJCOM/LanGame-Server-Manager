#pragma once
#include "control_protocol.h"
#include <cstring>

namespace enshrouded_control {
// These layouts and the native request entry are admitted only by the exact
// executable profile. The engine owns the save state; the bridge never writes it.
struct WorldSaveSnapshot {
    std::uint64_t sequence{}, tickMs{}, generation{};
    bool ready{};
    const char* issue{"awaiting_native_update"};
};
struct WorldSaveCache {
    std::uintptr_t server{}, context{}, world{};
    std::uint64_t generation{};
    bool exhausted{};
};
struct WorldSaveNativeState {
    std::uintptr_t context{}, world{};
    std::uint8_t flags{}, phase{}, shutdown{}, disabled{};
    std::uint64_t requestSequence{};
};
inline bool ReadWorldSaveState(std::uintptr_t server, WorldSaveNativeState& state,
    ReadMemory reader, void* memory) noexcept {
    return server && Read(reader, memory, server, 0x20, state.context) && state.context
        && Read(reader, memory, server, 0x1d0, state.world) && state.world
        && Read(reader, memory, server, 0x1820, state.shutdown)
        && Read(reader, memory, server, 0x18aa, state.disabled)
        && Read(reader, memory, state.context, 0x70, state.flags)
        && Read(reader, memory, state.context, 0x78, state.phase)
        && Read(reader, memory, state.context, 0x80, state.requestSequence);
}

// The admitted helper sets phase=1/pending synchronously before returning.
// Native processing alone advances it through the persistence state machine;
// only successful persistence returns to phase=0 with the pending/error bits
// clear. A failed native retry sequence leaves error bit 8 set. This observer
// owns one request until a terminal result and never requests another save.
struct WorldSaveCompletion {
    std::uintptr_t server{}, context{}, world{};
    std::uint64_t generation{}, requestSequence{}, started{};
    bool active{}, sawNativeCommit{};
};
enum class WorldSaveResult { Waiting, Completed, Failed, Unknown };
inline bool BeginWorldSaveCompletion(WorldSaveCompletion& completion, std::uintptr_t server,
    const WorldSaveSnapshot& before, const WorldSaveCache& cache, std::uint64_t now,
    ReadMemory reader, void* memory) noexcept {
    WorldSaveNativeState state{};
    if (completion.active || !before.ready || !before.generation || before.generation != cache.generation
        || cache.server != server || !ReadWorldSaveState(server, state, reader, memory)
        || state.context != cache.context || state.world != cache.world || state.shutdown || state.disabled
        || state.flags != 7 || state.phase != 1) return false;
    completion = {server, state.context, state.world, before.generation, state.requestSequence, now, true, false};
    return true;
}
inline WorldSaveResult ObserveWorldSaveCompletion(WorldSaveCompletion& completion,
    std::uintptr_t server, const WorldSaveCache& cache, std::uint64_t now,
    ReadMemory reader, void* memory) noexcept {
    WorldSaveNativeState state{};
    if (!completion.active || server != completion.server || cache.server != server
        || cache.generation != completion.generation || now < completion.started
        || now - completion.started >= 120000 || !ReadWorldSaveState(server, state, reader, memory)
        || state.context != completion.context || state.world != completion.world
        || state.requestSequence != completion.requestSequence || state.shutdown || state.disabled
        || (state.flags & ~15) || (state.flags & 6) != 6 || state.phase > 6
        || (completion.sawNativeCommit && state.phase != 6 && state.phase != 0)) {
        completion.active = false; return WorldSaveResult::Unknown;
    }
    // Phase 6 follows a successful native persistence check. Idle alone could
    // also be an unseen reset; skipped observations must remain unknown.
    if (state.phase == 6 && state.flags == 7) completion.sawNativeCommit = true;
    if (state.phase == 0) {
        completion.active = false;
        if (state.flags == 6 && completion.sawNativeCommit) return WorldSaveResult::Completed;
        if (state.flags & 8) return WorldSaveResult::Failed;
        return WorldSaveResult::Unknown;
    }
    return WorldSaveResult::Waiting;
}
inline WorldSaveSnapshot CaptureWorldSave(WorldSaveCache& cache, std::uintptr_t server,
    bool supported, std::uint64_t sequence, std::uint64_t now,
    ReadMemory reader, void* memory) noexcept {
    WorldSaveSnapshot result{sequence, now, cache.generation, false, "unsupported_save_profile"};
    if (!supported) return result;
    WorldSaveNativeState state{};
    if (!ReadWorldSaveState(server, state, reader, memory)) {
        // Retire the old binding even if this is only a transient world unload.
        cache.server = cache.context = cache.world = 0;
        result.issue = "world_not_ready"; return result;
    }
    if (cache.server != server || cache.context != state.context || cache.world != state.world) {
        if (cache.generation == UINT64_MAX) cache.exhausted = true;
        else ++cache.generation;
        cache.server = server; cache.context = state.context; cache.world = state.world;
    }
    result.generation = cache.generation;
    if (cache.exhausted) result.issue = "world_generation_exhausted";
    else if (state.shutdown || state.disabled) result.issue = "server_stopping";
    else if ((state.flags & 6) != 6 || (state.flags & ~15) != 0) result.issue = "world_not_ready";
    else if (state.phase || (state.flags & 9)) result.issue = "native_save_busy";
    else { result.ready = true; result.issue = "none"; }
    return result;
}
inline const char* ValidateWorldSave(const Request& request, const ProcessIdentity& identity,
    const WorldSaveSnapshot& current, std::uint64_t now) noexcept {
    if (std::strcmp(request.runId, identity.runId)) return "run_identity_mismatch";
    if (request.action != Action::SaveWorld || request.sessionHandle || request.accountId
        || !request.sequence || request.sequence > current.sequence || !request.generation)
        return "invalid_world_save_target";
    if (request.expiresAt < now || request.expiresAt - now > 15000) return "request_expired";
    if (!current.sequence || now < current.tickMs || now - current.tickMs > kFreshMs)
        return "snapshot_not_current";
    if (request.generation != current.generation) return "world_context_changed";
    return current.ready ? "none" : current.issue;
}
inline std::string SerializeWorldSave(const ProcessIdentity& identity, const WorldSaveSnapshot& snapshot) {
    return "{\"protocol\":1,\"pid\":" + std::to_string(identity.pid)
        + ",\"creation_time\":\"" + std::to_string(identity.creationTime)
        + "\",\"run_id\":\"" + identity.runId + "\",\"sequence\":\"" + std::to_string(snapshot.sequence)
        + "\",\"tick_ms\":\"" + std::to_string(snapshot.tickMs) + "\",\"generation\":\""
        + std::to_string(snapshot.generation) + "\",\"ready\":" + (snapshot.ready ? "true" : "false")
        + ",\"issue\":\"" + snapshot.issue + "\"}";
}
} // namespace enshrouded_control
