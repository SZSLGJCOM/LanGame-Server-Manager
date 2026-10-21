#pragma once
#include <windows.h>
#include <bitset>
#include <cstddef>
#include <cstdint>

namespace lgsm_conan {
constexpr uint64_t COMPLETION_WINDOW_MS = 30000;
constexpr uint64_t MAX_SQL_RESULTS = 262144;
enum class Outcome { Pending, Confirmed, Failed, Unknown };

// The default native builder calls back once per SQL statement; aggregate mode
// supplies the whole result once. IsDone is only accepted inside that owned
// post-execution callback, together with every native statement result/index.
struct QueryResult { uint64_t index{}, count{}; bool done{}, sql_error{}; };
inline const char* text(Outcome outcome) {
    switch (outcome) {
    case Outcome::Pending: return "pending";
    case Outcome::Confirmed: return "confirmed";
    case Outcome::Failed: return "failed";
    default: return "unknown";
    }
}
class Completion {
    SRWLOCK lock_ = SRWLOCK_INIT;
    uint64_t admitted_;
    std::bitset<MAX_SQL_RESULTS> seen_;
    uint64_t received_{}, highest_{}, final_{};
    bool final_seen_{}, sql_error_{}, malformed_{};
    bool settled_{};
    bool abandoned_{};
    Outcome result_ = Outcome::Pending;
    bool expired(uint64_t time) const {
        return time < admitted_ || time - admitted_ > COMPLETION_WINDOW_MS;
    }
public:
    explicit Completion(uint64_t admitted) : admitted_(admitted) {}
    Completion(const Completion&) = delete;
    Completion& operator=(const Completion&) = delete;
    void invalid() noexcept {
        AcquireSRWLockExclusive(&lock_);
        malformed_ = true;
        result_ = Outcome::Unknown;
        ReleaseSRWLockExclusive(&lock_);
    }
    void complete(QueryResult result, uint64_t time) noexcept {
        AcquireSRWLockExclusive(&lock_);
        if (expired(time)) abandoned_ = true;
        if (settled_ || !result.count || result.count > MAX_SQL_RESULTS || result.index >= MAX_SQL_RESULTS) {
            malformed_ = true;
        } else if (result.count > 1) {
            // Native aggregate mode only dispatches its complete merged result.
            if (received_ || result.index != 0 || !result.done) malformed_ = true;
            else { received_ = result.count; settled_ = true; sql_error_ = result.sql_error; }
        } else {
            if (seen_[static_cast<size_t>(result.index)]) malformed_ = true;
            else {
                seen_.set(static_cast<size_t>(result.index)); ++received_;
                if (result.index > highest_) highest_ = result.index;
                sql_error_ = sql_error_ || result.sql_error;
            }
            if (result.done) {
                if (final_seen_ && final_ != result.index) malformed_ = true;
                final_seen_ = true; final_ = result.index;
            }
            if (final_seen_ && highest_ <= final_ && received_ == final_ + 1) {
                settled_ = true;
                if (received_ < 2) malformed_ = true; // BEGIN and END must both execute.
            }
        }
        result_ = malformed_ || abandoned_ ? Outcome::Unknown : !settled_ ? Outcome::Pending
            : sql_error_ ? Outcome::Failed : Outcome::Confirmed;
        ReleaseSRWLockExclusive(&lock_);
    }
    Outcome observe(uint64_t time, bool same_owner = true) noexcept {
        AcquireSRWLockExclusive(&lock_);
        if (!same_owner || expired(time)) abandoned_ = true;
        auto result = abandoned_ ? Outcome::Unknown : result_;
        ReleaseSRWLockExclusive(&lock_);
        return result;
    }
    bool busy() noexcept {
        AcquireSRWLockExclusive(&lock_);
        auto result = !settled_;
        ReleaseSRWLockExclusive(&lock_);
        return result;
    }
};
} // namespace lgsm_conan
