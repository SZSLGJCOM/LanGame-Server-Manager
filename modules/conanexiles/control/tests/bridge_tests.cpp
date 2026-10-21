#include "../bridge.cpp"
#include <cassert>
#include <thread>
#define main shared_contract_tests
#include "../../../../apps/desktop/src-tauri/native/tests/world_save_bridge_tests.hpp"
#undef main

void check_completion() {
    using namespace lgsm_conan;
    unsigned checks = 0;
    const auto check = [&](bool condition) { ++checks; assert(condition); };
    Completion valid(1000);
    check(valid.busy() && valid.observe(1001) == Outcome::Pending);
    valid.complete({0, 1, false, false}, 1002);
    check(valid.busy() && valid.observe(1003) == Outcome::Pending);
    std::thread worker([&] { valid.complete({1, 1, true, false}, 2000); });
    worker.join();
    check(!valid.busy() && valid.observe(2001) == Outcome::Confirmed);
    valid.complete({1, 1, true, false}, 2002);
    check(valid.observe(2003) == Outcome::Unknown);
    Completion unordered(1000);
    unordered.complete({2, 1, true, false}, 1001);
    check(unordered.busy() && unordered.observe(1002) == Outcome::Pending);
    unordered.complete({0, 1, false, false}, 1003);
    check(unordered.busy() && unordered.observe(1004) == Outcome::Pending);
    unordered.complete({1, 1, false, false}, 1005);
    check(!unordered.busy() && unordered.observe(1006) == Outcome::Confirmed);
    Completion sql_error(1000);
    sql_error.complete({0, 1, false, true}, 1001);
    check(sql_error.busy() && sql_error.observe(1002) == Outcome::Pending);
    sql_error.complete({1, 1, true, false}, 1003);
    check(!sql_error.busy() && sql_error.observe(1004) == Outcome::Failed);
    Completion duplicate(1000);
    duplicate.complete({0, 1, false, false}, 1001);
    duplicate.complete({0, 1, false, false}, 1002);
    check(duplicate.busy() && duplicate.observe(1003) == Outcome::Unknown);
    duplicate.complete({1, 1, true, false}, 1004);
    check(!duplicate.busy() && duplicate.observe(1005) == Outcome::Unknown);
    Completion timeout(1000);
    timeout.complete({0, 1, false, false}, 1001);
    check(timeout.observe(31001) == Outcome::Unknown && timeout.busy());
    timeout.complete({1, 1, true, false}, 31002);
    check(!timeout.busy() && timeout.observe(31003) == Outcome::Unknown);
    Completion changed_world(1000);
    check(changed_world.observe(1001, false) == Outcome::Unknown && changed_world.busy());
    changed_world.complete({0, 2, true, false}, 1002);
    check(!changed_world.busy() && changed_world.observe(1003) == Outcome::Unknown);
    Completion callback_error(1000);
    callback_error.invalid();
    check(callback_error.busy() && callback_error.observe(1001) == Outcome::Unknown);
    callback_error.complete({0, 2, true, false}, 1002);
    check(callback_error.observe(1003) == Outcome::Unknown);
    Completion late_callback(1000);
    late_callback.complete({0, 2, true, false}, 31001);
    check(late_callback.observe(2000) == Outcome::Unknown);
    Completion invalid_clock(1000);
    check(invalid_clock.observe(999) == Outcome::Unknown && invalid_clock.busy());
    Completion missing(1000);
    missing.complete({0, 1, false, false}, 1001);
    missing.complete({2, 1, true, false}, 1002);
    check(missing.busy() && missing.observe(1003) == Outcome::Pending);
    check(missing.busy() && missing.observe(31001) == Outcome::Unknown);
    Completion only_one(1000);
    only_one.complete({0, 1, true, false}, 1001);
    check(only_one.observe(1002) == Outcome::Unknown);
    Completion aggregate(1000);
    aggregate.complete({0, 3, true, false}, 1001);
    check(!aggregate.busy() && aggregate.observe(1002) == Outcome::Confirmed);
    Completion aggregate_error(1000);
    aggregate_error.complete({0, 3, true, true}, 1001);
    check(!aggregate_error.busy() && aggregate_error.observe(1002) == Outcome::Failed);
    Completion aggregate_early(1000);
    aggregate_early.complete({0, 3, false, false}, 1001);
    check(aggregate_early.busy() && aggregate_early.observe(1002) == Outcome::Unknown);
    Completion too_large(1000);
    too_large.complete({0, MAX_SQL_RESULTS + 1, true, false}, 1001);
    check(too_large.busy() && too_large.observe(1002) == Outcome::Unknown);
    Completion bad_index(1000);
    bad_index.complete({MAX_SQL_RESULTS, 1, true, false}, 1001);
    check(bad_index.busy() && bad_index.observe(1002) == Outcome::Unknown);
    Completion aggregate_after_partial(1000);
    aggregate_after_partial.complete({0, 1, false, false}, 1001);
    aggregate_after_partial.complete({0, 2, true, false}, 1002);
    check(aggregate_after_partial.busy() && aggregate_after_partial.observe(1003) == Outcome::Unknown);
    std::printf("Conan owned SQL completion: %u positive/negative assertions passed.\n", checks);
}

void check_replay() {
    for (auto& entry : recent) entry = {"owned", "pending", true, 1000};
    assert(available_receipt(1001) == nullptr);
    assert(available_receipt(33000) == nullptr);
    assert(available_receipt(999) == nullptr);
    assert(available_receipt(33001) == &recent[0]);
    recent[1] = {};
    assert(available_receipt(1001) == &recent[1]);
    Request first{"1234567890abcdef1234567890abcdef", "save", 1000, 2, 3, 4};
    assert(same_request(first, first));
    auto changed = first; changed.requested_at = 1001;
    assert(!same_request(first, changed));
    changed = first; changed.proxy = 5; assert(!same_request(first, changed));
    changed = first; changed.instance = 5; assert(!same_request(first, changed));
    changed = first; changed.world = 5; assert(!same_request(first, changed));
    changed = first; changed.operation = "probe"; assert(!same_request(first, changed));
    changed = first; changed.nonce[0] = 'f'; assert(!same_request(first, changed));
    Receipt result;
    async_result(result, first, "pending"); assert(result.pending);
    async_result(result, first, "confirmed"); assert(!result.pending && result.text.find("status=confirmed\n") != result.text.npos);
    async_result(result, first, "failed"); assert(!result.pending && result.text.find("status=failed\n") != result.text.npos);
    async_result(result, first, "unknown"); assert(!result.pending && result.text.find("status=unknown\n") != result.text.npos);
    bool rejected = false;
    try { async_result(result, first, "returned"); } catch (const std::exception&) { rejected = true; }
    assert(rejected);
    std::puts("Conan replay: 17 cache saturation, identity and closed-result assertions passed.");
}

int main() {
    for (const auto& profile : PROFILES) {
        assert(select_game_profile(profile.image_hash));
        assert(&game_profile() == &profile);
        assert(SAVE_RVA == profile.save && SAVE_PROLOGUE == profile.save_prologue);
        auto changed = std::string(profile.image_hash);
        changed[0] = changed[0] == '0' ? '1' : '0';
        assert(!select_game_profile(changed));
        bool rejected = false;
        try { (void)game_profile(); } catch (const std::exception&) { rejected = true; }
        assert(rejected);
    }
    check_completion();
    check_replay();
    return shared_contract_tests();
}
