#include "../save_native.hpp"
#include <cstdio>
using namespace dragon_save;
int main() {
    unsigned count{};
    for (const auto& hook : test_native_hooks()) {
        std::vector<std::pair<unsigned,unsigned>> pushes;
        if (hook.rva==0x5d98ba0) pushes={{2,5},{3,3},{4,6},{5,7}};
        if (hook.rva==0x5d99a60) pushes={{2,5},{3,3},{4,7},{6,14}};
        if (hook.rva==0x12ac590) pushes={{2,3}};
        const auto allocation_after=hook.rva==0x5d9a810 ? 4U : hook.rva==0x12ac590 ? 6U : 0U;
        const auto allocation_bytes=hook.rva==0x5d9a810 ? 0x38U : hook.rva==0x12ac590 ? 0x20U : 0U;
        if (!test_trampoline(hook,pushes,allocation_after,allocation_bytes)) return 1;
        ++count;
    }
    std::printf("PASS %u Dragonwilds actual MinHook trampoline/unwind components\n",count);
}
