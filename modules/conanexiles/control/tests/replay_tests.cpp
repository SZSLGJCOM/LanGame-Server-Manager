#include "../bridge.cpp"
#include <cassert>
#include <cstdio>
int main() {
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
