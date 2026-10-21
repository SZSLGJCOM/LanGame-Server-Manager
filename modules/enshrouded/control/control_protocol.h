#pragma once
#include "identity_cache.h"
#include <string>
#include <string_view>
namespace enshrouded_control {
constexpr std::string_view kHeader = "LGSM_ENSHROUDED_CONTROL_1\n";
constexpr std::size_t kMaxRequestBytes = 1024;
enum class Action : std::uint8_t { Kick, Ban, Unban, SaveWorld };
struct ProcessIdentity {
    std::uint32_t pid{};
    std::uint64_t creationTime{};
    char runId[33]{};
};
struct Request {
    char requestId[33]{}, runId[33]{};
    Action action{};
    std::uint64_t sequence{}, generation{}, accountId{}, expiresAt{};
    std::uint32_t sessionHandle{};
};
struct Response { Request request{}; bool dispatched{}; const char* issue{"none"}; bool saveCompleted{}; };
bool ParseRequest(std::string_view text, Request& request) noexcept;
bool SameRequest(const Request& left, const Request& right) noexcept;
const char* ActionName(Action action) noexcept;
const char* ValidateRequest(const Request& request, const ProcessIdentity& identity,
    const Snapshot& current, std::uint64_t now) noexcept;
std::string SerializeSnapshot(const ProcessIdentity& identity, const Snapshot& snapshot);
std::string SerializeResponse(const ProcessIdentity& identity, const Response& response);
} // namespace enshrouded_control
