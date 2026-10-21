#pragma once
#include <array>
#include <cstddef>
#include <cstdint>

namespace dragon_save {
// The native root has two aggregates. Only +0x2d8 is the final save result;
// +0x310 is an earlier player stage and cannot finish this operation.
enum class Status { dispatching, completed, failed, unknown };
enum class Issue { none, native_failed, identity_changed, closure_invalid, timed_out };
enum class Kind { world, player };
struct Completion {
    static constexpr std::size_t maximum_players = 256;
    struct Result { std::int32_t handle{}; bool present{}, returned{}; };
    std::array<Result, maximum_players> players{};
    Result world;
    Status status = Status::dispatching;
    Issue issue = Issue::none;
    std::uint64_t deadline{};
    std::size_t player_count{}, player_results{}, native_debt{};
    std::size_t slot_entries{}, slot_returns{}, writer_entries{}, writer_closes{}, publications{};
    bool entered{}, root_returned{}, aggregate_entered{}, aggregate_returned{}, world_stage{};
    explicit Completion(std::uint64_t now) : deadline(now + 120000) {}
    void fail(Issue reason = Issue::native_failed) noexcept {
        if (status == Status::dispatching) { status = Status::failed; issue = reason; }
    }
    void unknown(Issue reason = Issue::closure_invalid) noexcept {
        // A later native success must not turn an ambiguous/expired operation
        // into a successful receipt, even after its outstanding debt drains.
        if (status != Status::completed && status != Status::failed) { status = Status::unknown; issue = reason; }
    }
    void enter_root() noexcept {
        if (entered || root_returned) { unknown(); return; }
        entered = true;
        ++native_debt;
    }
    void leave_root(std::size_t native_expected) noexcept {
        if (!entered || root_returned || native_expected != player_count + 1) unknown();
        root_returned = true;
        leave_native();
        finish();
    }
    void enter_native() noexcept {
        if (native_debt >= 1024) unknown();
        else ++native_debt;
    }
    void leave_native() noexcept {
        if (!native_debt) unknown();
        else --native_debt;
        finish();
    }
    void player(std::int32_t handle) noexcept {
        if (!entered || root_returned || handle <= 0 || player_count >= players.size()
            || (world.present && world.handle == handle)) { unknown(); return; }
        for (std::size_t index = 0; index < player_count; ++index)
            if (players[index].handle == handle) { unknown(); return; }
        players[player_count++] = {handle, true, false};
    }
    void begin_world_stage() noexcept {
        if (!entered || world_stage) { unknown(); return; }
        world_stage = true;
    }
    void world_handle(std::int32_t handle) noexcept {
        if (!world_stage || world.present || handle <= 0) { unknown(); return; }
        for (std::size_t index = 0; index < player_count; ++index)
            if (players[index].handle == handle) { unknown(); return; }
        world = {handle, true, false};
    }
    bool result(Kind kind, std::int32_t handle, bool success) noexcept {
        Result* selected{};
        if (kind == Kind::world) {
            if (world.present && world.handle == handle) selected = &world;
        } else {
            for (std::size_t index = 0; index < player_count; ++index)
                if (players[index].handle == handle) selected = &players[index];
        }
        if (!selected || selected->returned) { unknown(); return false; }
        selected->returned = true;
        if (kind == Kind::player) ++player_results;
        if (!success) fail();
        finish();
        return true;
    }
    void enter_slot() noexcept {
        if (!world_stage || slot_entries++) unknown();
        enter_native();
    }
    void leave_slot(bool success) noexcept {
        if (slot_returns++ || slot_entries != 1 || writer_entries != 1
            || writer_closes != 1 || publications != 1) unknown();
        if (!success) fail();
        leave_native();
    }
    void writer() noexcept {
        if (slot_entries != 1 || slot_returns || writer_entries++) unknown();
    }
    void close_writer(bool success) noexcept {
        if (writer_entries != 1 || writer_closes++) unknown();
        if (!success) fail();
    }
    void publish(bool native_success, bool readback_matches) noexcept {
        if (slot_entries != 1 || slot_returns || writer_closes != 1 || publications++) unknown();
        if (!native_success) fail();
        else if (!readback_matches) unknown(Issue::identity_changed);
    }
    void enter_aggregate(std::size_t native_results, bool all_success) noexcept {
        if (aggregate_entered || native_results != player_count + 1
            || player_results != player_count || !world.returned) unknown();
        aggregate_entered = true;
        if (!all_success) fail();
        enter_native();
    }
    void leave_aggregate() noexcept {
        if (!aggregate_entered || aggregate_returned) unknown();
        aggregate_returned = true;
        leave_native();
    }
    void tick(std::uint64_t now) noexcept {
        if (status == Status::dispatching && now >= deadline) unknown(Issue::timed_out);
    }
    bool quiescent() const noexcept {
        return root_returned && aggregate_returned && native_debt == 0;
    }
    void finish() noexcept {
        if (status == Status::dispatching && quiescent() && world.returned
            && player_results == player_count && slot_returns == 1
            && writer_closes == 1 && publications == 1) status = Status::completed;
    }
};
}
