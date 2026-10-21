#include "control_protocol.h"
#include <cstring>
namespace enshrouded_control {
namespace {
bool Field(std::string_view& remaining, std::string_view key, std::string_view& value) noexcept {
    if (!remaining.starts_with(key)) return false;
    remaining.remove_prefix(key.size());
    const auto end = remaining.find('\n');
    if (end == std::string_view::npos) return false;
    value = remaining.substr(0, end); remaining.remove_prefix(end + 1);
    return !value.empty();
}
bool Hex32(std::string_view value, char* out) noexcept {
    if (value.size() != 32) return false;
    for (const char ch : value) if (!((ch >= '0' && ch <= '9') || (ch >= 'a' && ch <= 'f'))) return false;
    std::memcpy(out, value.data(), 32); out[32] = 0; return true;
}
bool Decimal(std::string_view text, std::uint64_t& value) noexcept {
    if (text.empty() || text.size() > 20 || (text.size() > 1 && text[0] == '0')) return false;
    value = 0;
    for (const char ch : text) {
        if (ch < '0' || ch > '9' || value > (UINT64_MAX - (ch - '0')) / 10) return false;
        value = value * 10 + (ch - '0');
    }
    return true;
}
std::string IdentityJson(const ProcessIdentity& identity) {
    return "\"protocol\":1,\"pid\":" + std::to_string(identity.pid)
        + ",\"creation_time\":\"" + std::to_string(identity.creationTime)
        + "\",\"run_id\":\"" + identity.runId + '"';
}
std::string NameJson(const Player& row) {
    std::string out = "\"";
    for (std::size_t i = 0; i < row.nameBytes; ++i) {
        const char ch = row.name[i];
        if (ch == '"' || ch == '\\') out += '\\';
        out += ch;
    }
    return out + '"';
}
}
bool ParseRequest(std::string_view text, Request& request) noexcept {
    request = {};
    if (text.size() > kMaxRequestBytes || !text.starts_with(kHeader)) return false;
    for (const unsigned char ch : text) if ((ch < 0x20 && ch != '\n') || ch > 0x7e) return false;
    text.remove_prefix(kHeader.size()); std::string_view value;
    if (!Field(text, "request_id=", value) || !Hex32(value, request.requestId)
        || !Field(text, "run_id=", value) || !Hex32(value, request.runId)
        || !Field(text, "action=", value)) return false;
    if (value == "kick") request.action = Action::Kick;
    else if (value == "ban") request.action = Action::Ban;
    else if (value == "unban") request.action = Action::Unban;
    else if (value == "save_world") request.action = Action::SaveWorld;
    else return false;
    std::uint64_t handle{};
    if (!Field(text, "snapshot_sequence=", value) || !Decimal(value, request.sequence)
        || !Field(text, "session_handle=", value) || !Decimal(value, handle) || handle > UINT32_MAX
        || !Field(text, "generation=", value) || !Decimal(value, request.generation)
        || !Field(text, "account_id=", value) || !Decimal(value, request.accountId)
        || !Field(text, "expires_at_tick=", value) || !Decimal(value, request.expiresAt) || !text.empty()) return false;
    request.sessionHandle = static_cast<std::uint32_t>(handle);
    return true;
}
bool SameRequest(const Request& a, const Request& b) noexcept {
    return !std::strcmp(a.requestId, b.requestId) && !std::strcmp(a.runId, b.runId)
        && a.action == b.action && a.sequence == b.sequence && a.generation == b.generation
        && a.accountId == b.accountId && a.expiresAt == b.expiresAt && a.sessionHandle == b.sessionHandle;
}
const char* ActionName(Action action) noexcept {
    switch (action) { case Action::Kick: return "kick"; case Action::Ban: return "ban"; case Action::Unban: return "unban"; case Action::SaveWorld: return "save_world"; }
    return "invalid";
}
const char* ValidateRequest(const Request& request, const ProcessIdentity& identity,
    const Snapshot& current, std::uint64_t now) noexcept {
    if (std::strcmp(request.runId, identity.runId)) return "run_identity_mismatch";
    if (request.action == Action::SaveWorld) return "world_save_requires_native_context";
    if (!request.accountId) return "invalid_account_identity";
    if (request.expiresAt < now || request.expiresAt - now > 15000) return "request_expired";
    if (!current.sequence || now < current.tickMs || now - current.tickMs > kFreshMs)
        return "snapshot_not_current";
    if (request.action == Action::Unban) {
        return request.sessionHandle || request.generation || request.sequence ? "invalid_offline_target" : "none";
    }
    if (!current.complete) return "snapshot_not_current";
    if (!request.sequence || request.sequence > current.sequence || !request.sessionHandle || !request.generation)
        return "invalid_online_target";
    for (std::uint32_t i = 0; i < current.count; ++i) {
        const auto& row = current.players[i];
        if (row.sessionHandle != request.sessionHandle || row.generation != request.generation
            || row.accountId != request.accountId) continue;
        return row.moderationImmune ? "native_target_immune" : "none";
    }
    return "target_session_changed";
}
std::string SerializeSnapshot(const ProcessIdentity& identity, const Snapshot& snapshot) {
    std::string out = "{" + IdentityJson(identity) + ",\"sequence\":\"" + std::to_string(snapshot.sequence)
        + "\",\"tick_ms\":\"" + std::to_string(snapshot.tickMs) + "\",\"complete\":"
        + (snapshot.complete ? "true" : "false") + ",\"issue\":\"" + snapshot.issue + "\",\"players\":[";
    for (std::uint32_t i = 0; i < snapshot.count; ++i) {
        const auto& row = snapshot.players[i]; if (i) out += ',';
        out += "{\"session_handle\":" + std::to_string(row.sessionHandle)
            + ",\"generation\":\"" + std::to_string(row.generation) + "\",\"account_id\":\""
            + std::to_string(row.accountId) + "\",\"platform\":" + std::to_string(row.platform)
            + ",\"platform_id\":\"" + std::to_string(row.platformId) + "\",\"name\":" + NameJson(row)
            + ",\"moderation_immune\":" + (row.moderationImmune ? "true" : "false") + '}';
    }
    return out + "]}";
}
std::string SerializeResponse(const ProcessIdentity& identity, const Response& response) {
    return "{" + IdentityJson(identity) + ",\"request_id\":\"" + response.request.requestId
        + "\",\"action\":\"" + ActionName(response.request.action) + "\",\"account_id\":\""
        + std::to_string(response.request.accountId) + "\",\"status\":\""
        + (response.saveCompleted && response.request.action == Action::SaveWorld ? "completed"
            : response.dispatched ? "dispatched" : "rejected") + "\",\"issue\":\"" + response.issue + "\"}";
}
} // namespace enshrouded_control
