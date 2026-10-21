#include "../native_unwind.h"
#include "../../../enshrouded/control/vendor/minhook/include/MinHook.h"
#include <cstdio>
#include <cstdlib>
#include <stdexcept>
extern "C" void lgsm_probe_trigger() noexcept(false);
extern "C" void lgsm_probe_serialize() noexcept(false);
extern "C" void lgsm_probe_finalize() noexcept(false);
namespace {
unsigned checks{}, calls{};
bool should_throw{};
void (*original[3])(){};
void hook0() { ++calls; original[0](); }
void hook1() { ++calls; original[1](); }
void hook2() { ++calls; original[2](); }
void check(bool condition, const char* message) {
    if (!condition) { std::fprintf(stderr, "FAIL: %s\n", message); std::exit(1); } ++checks;
}
}
extern "C" void lgsm_probe_body() noexcept(false) {
    ++calls;
    if (should_throw) throw std::runtime_error("original_native_error");
}
int main() {
    check(MH_Initialize() == MH_OK, "native detour engine initialized");
    void (*targets[])() = {lgsm_probe_trigger, lgsm_probe_serialize, lgsm_probe_finalize};
    void (*hooks[])() = {hook0, hook1, hook2};
    vrising_save::UnwindRegistration registrations[3]{};
    for (unsigned i = 0; i < 3; ++i) {
        check(MH_CreateHook(reinterpret_cast<void*>(targets[i]), reinterpret_cast<void*>(hooks[i]),
            reinterpret_cast<void**>(&original[i])) == MH_OK, "exact native prologue relocated");
        check(vrising_save::register_unwind(registrations[i], reinterpret_cast<void*>(original[i]),
            reinterpret_cast<void*>(targets[i]), static_cast<vrising_save::HookKind>(i)), "every trampoline PC unwinds to original caller");
        check(MH_EnableHook(reinterpret_cast<void*>(targets[i])) == MH_OK, "native hook enabled");
        calls = 0; targets[i](); check(calls == 2, "native wrapper calls original exactly once");
        should_throw = true; bool caught{};
        try { targets[i](); } catch (const std::runtime_error& exception) { caught = std::string(exception.what()) == "original_native_error"; }
        check(caught, "original native C++ exception identity crosses wrapper unchanged");
        should_throw = false;
    }
    std::printf("PASS: %u real MinHook/native ABI/unwind checks (synthetic native probes; no game invocation).\n", checks);
}
