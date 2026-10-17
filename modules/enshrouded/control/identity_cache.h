#pragma once
#include <cstddef>
#include <cstdint>
#include <cstring>

namespace enshrouded_control {
constexpr std::size_t kMaxPlayers = 16, kMaxNameBytes = 256;
constexpr std::uint64_t kFreshMs = 1000;
constexpr std::uintptr_t kCallerA = 0x7b3aaf, kCallerB = 0x7b489e;
using ReadMemory = bool (*)(void*, std::uintptr_t, void*, std::size_t);
struct Player {
    std::uint32_t sessionHandle{};
    std::uint64_t generation{}, accountId{}, platformId{};
    std::uint8_t platform{};
    bool moderationImmune{}, nativeProjectionImmune{};
    std::uint16_t nameBytes{};
    char name[kMaxNameBytes + 1]{};
};
struct Snapshot {
    std::uint64_t sequence{}, tickMs{};
    bool complete{};
    const char* issue{"awaiting_native_update"};
    std::uint32_t count{};
    Player players[kMaxPlayers]{};
};
struct NativeLookupContext {
    std::uintptr_t output{}, manager{};
    std::uint64_t requestedHandle{}, caller{};
};
static_assert(sizeof(NativeLookupContext) == 32);
struct CacheEntry { Player player{}; std::uint64_t cachedTick{}; };
struct IdentityCache {
    std::uintptr_t server{}, manager{};
    std::uintptr_t callerA{kCallerA}, callerB{kCallerB};
    std::uint64_t nextGeneration{};
    bool degraded{};
    CacheEntry entries[kMaxPlayers]{};
};
inline bool ValidName(const char* data, std::size_t size) noexcept {
    if (!size || size > kMaxNameBytes) return false;
    for (std::size_t i = 0; i < size;) {
        const auto a = static_cast<unsigned char>(data[i++]);
        if (a < 0x80) { if (a < 0x20 || a == 0x7f) return false; continue; }
        const unsigned length = a >= 0xc2 && a <= 0xdf ? 1 : a >= 0xe0 && a <= 0xef ? 2
            : a >= 0xf0 && a <= 0xf4 ? 3 : 0;
        if (!length || i + length > size) return false;
        const auto b = static_cast<unsigned char>(data[i]);
        if ((a == 0xe0 && b < 0xa0) || (a == 0xed && b >= 0xa0)
            || (a == 0xf0 && b < 0x90) || (a == 0xf4 && b >= 0x90)) return false;
        for (unsigned n = 0; n < length; ++n)
            if ((static_cast<unsigned char>(data[i++]) & 0xc0) != 0x80) return false;
    }
    return true;
}
template<class T> bool Read(ReadMemory reader, void* context, std::uintptr_t base,
    std::uintptr_t offset, T& value) noexcept {
    return base <= UINTPTR_MAX - offset && reader(context, base + offset, &value, sizeof value);
}
inline void Invalidate(IdentityCache& cache, std::size_t index) noexcept {
    if (cache.entries[index].player.generation) {
        if (cache.nextGeneration == UINT64_MAX) cache.degraded = true;
        else ++cache.nextGeneration;
    }
    cache.entries[index] = {};
}
inline bool BindContext(IdentityCache& cache, std::uintptr_t server, ReadMemory reader,
    void* context) noexcept {
    std::uintptr_t manager{};
    if (!server || !Read(reader, context, server, 0x10, manager) || !manager) return false;
    if (cache.server && (cache.server != server || cache.manager != manager)) cache.degraded = true;
    if (cache.degraded) return false;
    cache.server = server; cache.manager = manager;
    return true;
}
inline const char* ObserveLocalCopy(IdentityCache& cache, const NativeLookupContext& lookup,
    std::uintptr_t callerRva, std::uint64_t nativeResult, std::uint64_t now,
    ReadMemory reader, void* context) noexcept {
    if (callerRva != cache.callerA && callerRva != cache.callerB) return "unexpected_native_caller";
    if (cache.server && lookup.manager != cache.manager) cache.degraded = true;
    if (cache.degraded || !cache.server) return "native_context_changed";
    const auto handle = static_cast<std::uint32_t>(lookup.requestedHandle), index = handle & 0x3fu;
    if (!handle || index >= kMaxPlayers) return "invalid_session_handle";
    if ((nativeResult & 0xff) != 0) { Invalidate(cache, index); return "native_lookup_failed"; }
    auto reject = [&](const char* issue) { Invalidate(cache, index); return issue; };
    Player row{};
    if (!Read(reader, context, lookup.output, 0, row.sessionHandle) || row.sessionHandle != handle)
        return reject("native_handle_mismatch");
    std::uint8_t nativeProtection{};
    if (!Read(reader, context, lookup.output, 0x10, row.platform)
        || !Read(reader, context, lookup.output, 0x18, row.platformId)
        || !Read(reader, context, lookup.output, 0x20, row.accountId)
        || !Read(reader, context, lookup.output, 0x148, nativeProtection)) return reject("native_copy_unreadable");
    row.nativeProjectionImmune = nativeProtection == 1;
    if (row.platform != 3 || !row.platformId || !row.accountId) return reject("unknown_native_identity");
    std::uintptr_t name{}; std::uint64_t size{};
    if (!Read(reader, context, lookup.output, 0x28, name)
        || !Read(reader, context, lookup.output, 0x30, size) || !name || size > kMaxNameBytes
        || !reader(context, name, row.name, static_cast<std::size_t>(size))
        || !ValidName(row.name, static_cast<std::size_t>(size))) return reject("invalid_native_name");
    row.nameBytes = static_cast<std::uint16_t>(size);
    auto& current = cache.entries[index];
    if (current.player.generation && (current.player.sessionHandle != handle
        || current.player.accountId != row.accountId || current.player.platform != row.platform
        || current.player.platformId != row.platformId || current.player.nameBytes != row.nameBytes
        || std::memcmp(current.player.name, row.name, row.nameBytes) != 0)) Invalidate(cache, index);
    if (!current.player.generation) {
        if (cache.nextGeneration == UINT64_MAX) { cache.degraded = true; return "generation_exhausted"; }
        row.generation = ++cache.nextGeneration;
    } else row.generation = current.player.generation;
    current.player = row; current.cachedTick = now;
    return "none";
}
inline Snapshot Capture(IdentityCache& cache, std::uintptr_t server, std::uint64_t now,
    ReadMemory reader, void* context) noexcept {
    Snapshot result{}; result.tickMs = now;
    if (!BindContext(cache, server, reader, context)) { result.issue = "native_context_changed"; return result; }
    bool used[kMaxPlayers]{};
    result.complete = true; result.issue = "none";
    auto reject = [&](const char* issue) { result.complete = false; result.issue = issue; };
    for (std::size_t i = 0; i < kMaxPlayers; ++i) {
        const auto slot = server + 0x1e0 + i * 0x148;
        std::uint32_t handle{}; std::uint8_t phase{}, permissions{};
        if (!Read(reader, context, slot, 0, handle)
            || !Read(reader, context, slot, 0xa, phase)) { reject("native_slot_unreadable"); continue; }
        if (!handle) continue;
        const auto index = handle & 0x3fu;
        if (index >= kMaxPlayers) { reject("invalid_session_handle"); continue; }
        if (used[index]) { reject("duplicate_session_handle"); continue; }
        used[index] = true;
        auto& entry = cache.entries[index];
        if (phase != 3) { Invalidate(cache, index); reject("native_transition"); continue; }
        if (entry.player.sessionHandle != handle || !entry.player.generation) {
            Invalidate(cache, index); reject("identity_not_observed"); continue;
        }
        if (now < entry.cachedTick || now - entry.cachedTick > kFreshMs) {
            Invalidate(cache, index); reject("identity_stale"); continue;
        }
        std::uintptr_t name{}; std::uint64_t size{};
        char currentName[kMaxNameBytes + 1]{};
        if (!Read(reader, context, slot, 0x20, name) || !Read(reader, context, slot, 0x28, size)
            || !name || size > kMaxNameBytes || !reader(context, name, currentName, static_cast<std::size_t>(size))
            || !ValidName(currentName, static_cast<std::size_t>(size))) {
            Invalidate(cache, index); reject("invalid_current_name"); continue;
        }
        if (size != entry.player.nameBytes || std::memcmp(currentName, entry.player.name, static_cast<std::size_t>(size))) {
            Invalidate(cache, index); reject("identity_name_changed"); continue;
        }
        if (!Read(reader, context, slot, 0x10, permissions)) { reject("native_slot_unreadable"); continue; }
        bool duplicate = false;
        for (std::uint32_t n = 0; n < result.count; ++n)
            if (result.players[n].accountId == entry.player.accountId) duplicate = true;
        if (duplicate) { reject("duplicate_account_identity"); continue; }
        entry.player.moderationImmune = entry.player.nativeProjectionImmune || (permissions & 1) != 0;
        result.players[result.count++] = entry.player;
    }
    for (std::size_t i = 0; i < kMaxPlayers; ++i) if (!used[i]) Invalidate(cache, i);
    return result;
}
} // namespace enshrouded_control
