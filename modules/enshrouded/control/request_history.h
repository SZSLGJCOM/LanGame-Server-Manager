#pragma once
#include "control_protocol.h"
namespace enshrouded_control {
enum class HistoryAdmission { New, Pending, Replay, Conflict, Full, InvalidEnvelope };
struct HistoryRecord { Request request{}; Response result{}; bool occupied{}, completed{}; };
struct RequestHistory {
    HistoryRecord records[16]{};
    HistoryRecord* Find(const char* id) noexcept {
        for (auto& record : records)
            if (record.occupied && !std::strcmp(record.request.requestId, id)) return &record;
        return nullptr;
    }
    HistoryAdmission Admit(const Request& request, const ProcessIdentity& identity, std::uint64_t now,
        HistoryRecord*& record, const char*& issue) noexcept {
        record = nullptr; issue = "none";
        if (std::strcmp(request.runId, identity.runId)) issue = "run_identity_mismatch";
        else if (request.expiresAt < now || request.expiresAt - now > 15000) issue = "request_expired";
        if (std::strcmp(issue, "none")) return HistoryAdmission::InvalidEnvelope;
        if (auto existing = Find(request.requestId)) {
            record = existing;
            if (!SameRequest(existing->request, request)) { issue = "request_id_payload_conflict"; return HistoryAdmission::Conflict; }
            return existing->completed ? HistoryAdmission::Replay : HistoryAdmission::Pending;
        }
        for (auto& candidate : records) {
            if (!candidate.occupied || (candidate.completed && candidate.request.expiresAt < now)) {
                candidate = {}; candidate.occupied = true; candidate.request = request; record = &candidate;
                return HistoryAdmission::New;
            }
        }
        issue = "request_history_full"; return HistoryAdmission::Full;
    }
    bool Complete(const Response& result) noexcept {
        auto record = Find(result.request.requestId);
        if (!record || !SameRequest(record->request, result.request)) return false;
        record->result = result; record->completed = true; return true;
    }
};
} // namespace enshrouded_control
