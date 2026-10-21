#include "../native_completion.h"
#include <array>
#include <cstdio>
#include <cstdlib>
using namespace vrising_save;
namespace {
unsigned checks{};
void check(bool value, const char* name) {
    if (!value) { std::fprintf(stderr, "FAIL: %s\n", name); std::exit(1); } ++checks;
}
void fixed(void* output, const char* text) {
    auto data = static_cast<unsigned char*>(output);
    const auto count = static_cast<unsigned short>(std::strlen(text));
    std::memcpy(data, &count, sizeof count); std::memcpy(data + 2, text, count + 1);
}
struct Fixture {
    Completion state;
    unsigned char result{};
    std::array<unsigned char, 128> name{};
    std::array<unsigned char, 520> event{};
    std::array<unsigned char, 0x460> job{};
    Fixture() {
        fixed(name.data(), "AutoSave_42.save");
        int reason = 2; std::memcpy(event.data(), &reason, sizeof reason);
        fixed(event.data() + 4, "AutoSave_42.save.gz"); event[516] = 1;
        fixed(job.data() + 0x200, "C:/private/Saves/AutoSave_42.save.gz");
        unsigned char* pointer = &result; std::memcpy(job.data() + 0x410, &pointer, sizeof pointer);
    }
    void start() {
        check(state.begin(1, this, job.data(), &result, 19), "begin isolated request");
        state.triggered(this, 2, name.data(), 19, true); state.end(true);
        check(state.observe(1) == 0, "native bool admission is pending");
        check(state.serializing(job.data(), event.data(), 19), "exact native event binds serializer");
    }
};
}
int main() {
    {
        Fixture test; test.start();
        check(test.state.observe(1) == 0, "serialization void is pending");
        auto ticket = test.state.finalizing(test.job.data());
        check(ticket == 1, "exact finalizer result container and filename bind request");
        check(test.state.observe(1) == 0, "job entry is pending");
        test.state.finalized(ticket, 3);
        check(test.state.observe(1) == 1, "only final native success completes");
    }
    for (unsigned char failure = 4; failure <= 10; ++failure) {
        Fixture test; test.start(); auto ticket = test.state.finalizing(test.job.data());
        test.state.finalized(ticket, failure);
        check(test.state.observe(1) == 2, "every native writer/publication/validation failure fails");
    }
    {
        Fixture test; test.start(); fixed(test.job.data() + 0x200, "C:/\xe4\xb8\xad\xe6\x96\x87/Saves/AutoSave_42.save.gz");
        auto ticket = test.state.finalizing(test.job.data()); test.state.finalized(ticket, 3);
        check(test.state.observe(1) == 1, "native UTF-8 parent path preserves exact ASCII save identity");
    }
    for (unsigned char unknown = 0; unknown < 3; ++unknown) {
        Fixture test; test.start(); auto ticket = test.state.finalizing(test.job.data());
        test.state.finalized(ticket, unknown);
        check(test.state.observe(1) == -1, "non-terminal native states never complete");
    }
    {
        Fixture test; test.start(); unsigned char other{}; unsigned char* address = &other;
        std::memcpy(test.job.data() + 0x410, &address, sizeof address);
        check(!test.state.finalizing(test.job.data()), "another writer container is ignored");
        check(test.state.observe(1) == 0, "unrelated successful autosave cannot complete");
    }
    {
        Fixture test; test.start(); fixed(test.job.data() + 0x200, "C:/private/Saves/AutoSave_41.save.gz");
        check(!test.state.finalizing(test.job.data()), "old save filename rejected even if container reused");
        check(test.state.observe(1) == -1, "ambiguous generation fails certification");
    }
    {
        Fixture test; test.start(); auto old = test.state.finalizing(test.job.data());
        test.state = {}; check(test.state.begin(2, &test, test.job.data(), &test.result, 19), "new request after abandonment");
        test.state.triggered(&test, 2, test.name.data(), 19, true); test.state.end(true);
        check(test.state.serializing(test.job.data(), test.event.data(), 19), "new accepted request uses same native slot and result container");
        test.state.finalized(old, 3); check(test.state.observe(2) == 0, "late original completion cannot acknowledge new pending request");
        auto fresh = test.state.finalizing(test.job.data()); test.state.finalized(fresh, 3);
        check(test.state.observe(2) == 1, "new request requires its own finalizer completion");
    }
    {
        Fixture test; test.start(); check(!test.state.serializing(test.job.data(), test.event.data(), 19), "second generation invalidates request");
        test.state.finalized(1, 3); check(test.state.observe(1) == -1, "duplicate serialization cannot complete");
    }
    {
        Fixture test; check(test.state.begin(1, &test, test.job.data(), &test.result, 19), "begin foreign caller case");
        test.state.triggered(&test, 2, test.name.data(), 19, false); test.state.end(true);
        check(test.state.observe(1) == -1, "unreviewed caller cannot mint request identity");
        check(test.state.failure == FailureReason::TriggerCaller, "first closed failure reason survives end validation");
    }
    {
        Fixture test; check(test.state.begin(1, &test, test.job.data(), &test.result, 19), "begin unrelated thread case");
        test.state.triggered(&test, 2, test.name.data(), 21, true); test.state.end(true);
        check(test.state.observe(1) == -1, "foreign thread does not bind trigger");
    }
    {
        Fixture test; test.start(); auto ticket = test.state.finalizing(test.job.data());
        check(!test.state.finalizing(test.job.data()), "duplicate finalizer invalidates request");
        test.state.finalized(ticket, 3); check(test.state.observe(1) == -1, "duplicate finalizer cannot complete");
    }
    for (const char* invalid : {"AutoSave_42", "AutoSave_42.save.gz", "../AutoSave_42.save", "AutoSave_42.save/evil",
        "AutoSave_.save", "Other_42.save", "AutoSave_a.save", "AutoSave_42..save"}) {
        Fixture test; fixed(test.name.data(), invalid);
        check(test.state.begin(1, &test, test.job.data(), &test.result, 19), "begin exact native filename grammar rejection");
        test.state.triggered(&test, 2, test.name.data(), 19, true); test.state.end(true);
        check(test.state.observe(1) == -1, "unreviewed save-name shape cannot acknowledge request");
        check(test.state.failure == FailureReason::TriggerNameFormat, "native name grammar failure has closed reason");
    }
    std::printf("PASS: %u native completion identity/state boundary checks (synthetic boundary inputs; no game invocation).\n", checks);
}
