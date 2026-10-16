#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdint>
#include <cstdio>
extern "C" {
void ObserverBody(), ObserverFlagsPushed(), ObserverFlagsPopped();
void ObserverRestoreFlagsPushed(), ObserverRestoreFlagsPopped();
void ObserverEpilogueLea(), ObserverEpiloguePop(), ObserverEpilogueJump();

void* g_updateOriginal{};
std::uint64_t g_observedRcx{}, g_callbackCount{};
}
int main() {
    alignas(16) std::uint64_t stack[64]{};
    const auto entryRsp = reinterpret_cast<std::uint64_t>(&stack[33]);
    const auto frameRbp = entryRsp - 8;
    const auto localsRsp = frameRbp - 0xc0;
    constexpr std::uint64_t callerRbp = 0x22334455, returnRip = 0x12345678;
    *reinterpret_cast<std::uint64_t*>(entryRsp) = returnRip;
    *reinterpret_cast<std::uint64_t*>(frameRbp) = callerRbp;
    struct Case { void(*point)(); std::uint64_t rsp, rbp; const char* label; };
    const Case cases[] = {
        {ObserverBody, localsRsp, frameRbp, "body"},
        {ObserverFlagsPushed, localsRsp - 8, frameRbp, "first pushfq pending pop"},
        {ObserverFlagsPopped, localsRsp, frameRbp, "first pop completed"},
        {ObserverRestoreFlagsPushed, localsRsp - 8, frameRbp, "restore push pending popfq"},
        {ObserverRestoreFlagsPopped, localsRsp, frameRbp, "restore popfq completed"},
        {ObserverEpilogueLea, localsRsp, frameRbp, "epilogue lea"},
        {ObserverEpiloguePop, frameRbp, frameRbp, "epilogue pop rbp"},
        {ObserverEpilogueJump, entryRsp, callerRbp, "epilogue tail jump"},

    };
    for (const auto& test : cases) {
        CONTEXT context{};
        context.ContextFlags = CONTEXT_FULL;
        context.Rip = reinterpret_cast<DWORD64>(test.point);
        context.Rsp = test.rsp;
        context.Rbp = test.rbp;
        DWORD64 imageBase{}, establisher{};
        void* handlerData{};
        auto function = RtlLookupFunctionEntry(context.Rip, &imageBase, nullptr);
        if (!function) { std::printf("Missing function entry: %s\n", test.label); return 1; }
        RtlVirtualUnwind(UNW_FLAG_NHANDLER, imageBase, context.Rip, function, &context,
            &handlerData, &establisher, nullptr);
        if (context.Rip != returnRip || context.Rsp != entryRsp + 8 || context.Rbp != callerRbp) {
            std::printf("Unwind failed: %s rsp=%llx rbp=%llx rip=%llx\n", test.label,
                context.Rsp, context.Rbp, context.Rip); return 2;
        }
    }
    std::puts("PASS RtlVirtualUnwind: body, dynamic flag push/pop boundaries, all tail-epilogue points");
    return 0;
}
