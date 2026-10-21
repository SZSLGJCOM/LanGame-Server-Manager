#include <cassert>
#include <cstdio>
int main() {
    const std::string valid_request = "schema=1\nnonce=1234567890abcdef1234567890abcdef\nop=save\nrequested_at=123\nproxy_address=456\ninstance_address=789\nworld_address=999\n";
    auto value = parse(valid_request);
    assert(value.operation == "save" && value.requested_at == 123 && value.proxy == 456 && value.instance == 789 && value.world == 999);
    auto probe = valid_request; probe.replace(probe.find("op=save"), 7, "op=probe");
    assert(parse(probe).operation == "probe");
    std::vector<std::string> invalid{valid_request + "extra=1\n", valid_request.substr(0, valid_request.size() - 1)};
    auto alter = [&](std::string_view from, std::string_view to) {
        auto text = valid_request; text.replace(text.find(from), from.size(), to); invalid.push_back(text);
    };
    alter("schema=1", "schema=2"); alter("op=save", "op=quit");
    alter("requested_at=123", "requested_at=0123"); alter("requested_at=123", "requested_at=-1");
    alter("requested_at=123", "requested_at=18446744073709551616");
    alter("proxy_address=456", "proxy_address=0"); alter("world_address=999", "world_address=0");
    alter("instance_address=789", "instance_address=0");
    alter("nonce=1234567890abcdef1234567890abcdef", "nonce=1234567890abcdef1234567890abcdeF");
    alter("nonce=1234567890abcdef1234567890abcdef", "nonce=1234567890abcdef");
    for (const auto& request : invalid) {
        bool rejected = false;
        try { (void)parse(request); } catch (const std::exception&) { rejected = true; }
        assert(rejected);
    }
    assert(!readable(0, sizeof(void*)) && !readable(UINTPTR_MAX - 1, sizeof(void*)));
    bool rejected = false;
    try { target_contract(0, 0, 0, 0); } catch (const std::exception&) { rejected = true; }
    assert(rejected);
    auto returned = receipt(value, "returned");
    assert(returned.find("status=returned\npid=" + std::to_string(GetCurrentProcessId()) + "\ncreation_time=") != std::string::npos);
    assert(returned.find("confirmed") == std::string::npos);
    // A normal process cannot enter a game member through this exported entry.
    // The image name/path gate rejects before any request or native method read.
    assert(LGSM_SAVE_EXPORT(nullptr) == 0);
    std::printf("Native save contract: %zu malformed requests rejected; invocation receipt stays pending.\n", invalid.size());
    return 0;
}
