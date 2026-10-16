#include "../trampoline_unwind.h"
#include "../vendor/minhook/include/MinHook.h"
#include <cstdio>
extern "C" void UnwindUpdateTarget(), UnwindCopyTarget();
namespace {
enshrouded_control::TrampolineUnwind registrations[2]{};
std::uint64_t Detour() { return 0; }
bool Check(void* trampoline, bool update) {
    alignas(16) std::uint64_t stack[96]{};
    const auto entryRsp = reinterpret_cast<DWORD64>(&stack[65]);
    constexpr DWORD64 callerRip = 0x12345678, callerRbx = 0x34567890, callerRbp = 0x23456789;
    *reinterpret_cast<DWORD64*>(entryRsp) = callerRip;
    if (update) {
        *reinterpret_cast<DWORD64*>(entryRsp - 8) = callerRbp;
        *reinterpret_cast<DWORD64*>(entryRsp - 16) = callerRbx;
    } else *reinterpret_cast<DWORD64*>(entryRsp - 8) = callerRbx;
    const unsigned relocated = update ? 5 : 6;
    // Includes all instruction boundaries and every byte of the indirect tail
    // jump (including its address literal): no full-native prologue is assumed.
    for (unsigned offset = 0; offset < relocated + 14; ++offset) {
        CONTEXT context{}; context.ContextFlags = CONTEXT_FULL;
        context.Rip = reinterpret_cast<DWORD64>(trampoline) + offset;
        context.Rsp = entryRsp;
        if (update) context.Rsp -= offset >= 5 ? 16 : offset >= 4 ? 8 : 0;
        else context.Rsp -= offset >= 6 ? 40 : offset >= 2 ? 8 : 0;
        context.Rbp = callerRbp; context.Rbx = callerRbx;
        DWORD64 imageBase{}, establisher{}; void* handlerData{};
        const auto function = RtlLookupFunctionEntry(context.Rip, &imageBase, nullptr);
        if (!function) return false;
        RtlVirtualUnwind(UNW_FLAG_NHANDLER, imageBase, context.Rip, function, &context,
            &handlerData, &establisher, nullptr);
        if (context.Rip != callerRip || context.Rsp != entryRsp + 8
            || context.Rbp != callerRbp || context.Rbx != callerRbx) {
            std::printf("Trampoline unwind mismatch: update=%d offset=%u rsp=%llx rip=%llx\n",
                update, offset, context.Rsp, context.Rip); return false;
        }
    }
    return true;
}
}
int main() {
    using namespace enshrouded_control;
    if (MH_Initialize() != MH_OK) return 1;
    void* trampolines[2]{};
    void* targets[] = {reinterpret_cast<void*>(&UnwindUpdateTarget), reinterpret_cast<void*>(&UnwindCopyTarget)};
    const TrampolineKind kinds[] = {TrampolineKind::UpdatePlayers, TrampolineKind::LocalCopy};
    unsigned index{};
    for (const auto kind : kinds) {
        if (MH_CreateHook(targets[index], reinterpret_cast<void*>(&Detour), &trampolines[index]) != MH_OK
            || !RegisterTrampolineUnwind(registrations[index], trampolines[index], targets[index], kind)
            || !Check(trampolines[index], index == 0)) return 2;
        MEMORY_BASIC_INFORMATION info{};
        if (!VirtualQuery(registrations[index].metadata, &info, sizeof info) || info.Protect != PAGE_READONLY) return 3;
        ++index;
    }
    using Call = std::uint64_t(*)();
    if (reinterpret_cast<Call>(trampolines[0])() != 0x12345678
        || reinterpret_cast<Call>(trampolines[1])() != 0x87654321) return 4;
    unsigned char invalid[20]{}; TrampolineUnwind rejected{};
    if (RegisterTrampolineUnwind(rejected, invalid, targets[0], TrampolineKind::UpdatePlayers)) return 5;
    // Registered code/tables/metadata deliberately survive until this owned
    // synthetic process exits. No freed trampoline has a dangling function table.
    std::puts("PASS fixed-profile MinHook trampolines: actual 5/6 relocated bytes, 39 RIP offsets, native stack/register unwind, read-only metadata, prefix rejection");
    return 0;
}
