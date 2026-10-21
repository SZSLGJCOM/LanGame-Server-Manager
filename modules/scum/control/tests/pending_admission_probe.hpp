#pragma once
#include "../native_state.hpp"
struct ProbeRequest {
    std::string nonce;std::uint64_t requested_at{};std::uintptr_t system{},instance{},world{};
};
const char* start_request(const ProbeRequest&);
const char* query_request(const ProbeRequest&);
scum_save::Owner parked_owner();
void reset_request();
bool install_root_boundary();
unsigned root_boundary_calls();
void root_boundary_throws(bool);
void initialize_public_boundary(const scum_save::fs::path&);
void prime_cached_request(const ProbeRequest&);
void invoke_cached_request();
bool cached_terminal_unknown();
scum_save::fs::path admission_image(const scum_save::fs::path&);
scum_save::fs::path admission_scratch(const scum_save::fs::path&);
