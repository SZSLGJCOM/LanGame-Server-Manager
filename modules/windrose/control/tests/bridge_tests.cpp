// Closed native protocol tests; include implementation to exercise its parser.
#include "../bridge.cpp"
#include <cassert>
int main() {
    const std::string request = "schema=1\nnonce=1234567890abcdef1234567890abcdef\nop=save\nrequested_at=123\nproxy_address=456\ninstance_address=789\n";
    auto value = parse(request);
    assert(value.operation == "save" && value.requested_at == 123 && value.proxy == 456 && value.instance == 789);
    for (auto invalid : {request + "extra=1\n", request.substr(0, request.size() - 1),
        std::string("schema=1\nnonce=1234567890abcdef1234567890abcdef\nop=quit\nrequested_at=123\nproxy_address=456\ninstance_address=789\n"),
        std::string("schema=1\nnonce=1234567890abcdef1234567890abcdef\nop=save\nrequested_at=0123\nproxy_address=456\ninstance_address=789\n")}) {
        bool rejected = false;
        try { (void)parse(invalid); } catch (const std::exception&) { rejected = true; }
        assert(rejected);
    }
    auto text = receipt(value, "confirmed");
    assert(text.find("status=confirmed\npid=" + std::to_string(GetCurrentProcessId()) + "\ncreation_time=") != std::string::npos);
    return 0;
}
