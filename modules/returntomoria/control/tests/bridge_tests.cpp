#include <cassert>
#include <cstring>
#include "../bridge.cpp"
int main() {
    assert(std::string_view(completion_status(true, 0, 0, 0)) == "confirmed");
    for (auto result : {completion_status(true, 1, 0, 0), completion_status(true, 0, 1, 0), completion_status(true, 0, 0, 1)})
        assert(std::string_view(result) == "returned");
    assert(std::string_view(completion_status(false, 0, 0, 0)) == "failed");
    const auto body = "schema=1\nnonce=1234567890abcdef1234567890abcdef\nop=save\nrequested_at=123\nproxy_address=456\ninstance_address=789\nworld_address=321\n";
    auto request = parse(body);
    assert(request.proxy == 456 && request.world == 321 && request.operation == "save");
    bool rejected = false; try { (void)parse(std::string(body) + "extra=1\n"); } catch (...) { rejected = true; }
    assert(rejected);
}
