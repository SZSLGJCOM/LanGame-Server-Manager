#include "../save_files.hpp"
#include <cstdio>
#include <cstdlib>
#include <fstream>
using namespace dragon_save;
namespace {
unsigned checks{};
void check(bool value,const char* name) {
    ++checks;
    if (!value) { std::fprintf(stderr,"FAIL %s (%lu)\n",name,GetLastError()); std::exit(1); }
}
void write(const fs::path& path,const char* value) { std::ofstream stream(path,std::ios::binary); stream<<value; stream.close(); check(stream.good(),"fixture write"); }
bool refused(const fs::path& path) { try { (void)snapshot(path); return false; } catch (...) { return true; } }
}
int wmain(int count,wchar_t** args) {
    if (count!=2) return 2;
    const auto root=full_path(args[1]); fs::create_directories(root/L"save");
    const auto source=root/L"save/world.sav.incomplete",target=root/L"save/world.sav",backup=root/L"save/world.sav.backup";
    write(source,"SPUD owned native bytes");
    {
        const auto expected=snapshot(source);
        check(matches(expected,source,true),"same ordinary source read back");
        check(CopyFileW(source.c_str(),backup.c_str(),TRUE)!=FALSE,"native copy fixture");
        check(matches(expected,backup,false),"copy has exact bytes");
        check(!matches(expected,backup,true),"copy is not a rename identity");
        check(MoveFileW(source.c_str(),target.c_str())!=FALSE,"native move fixture");
        check(matches(expected,target,true),"publication keeps closed writer identity and bytes");
        const auto renamed=root/L"renamed";
        check(!MoveFileW((root/L"save").c_str(),renamed.c_str()) && GetLastError()==ERROR_SHARING_VIOLATION,
            "active snapshot pins ancestors against rename");
        write(backup,"foreign replacement bytes");
        check(!matches(expected,backup,false),"changed copy readback refused");
        write(target,"SPUD owned native bytex");
        check(!matches(expected,target,true),"same size overwritten publication refused");
    }
    const auto renamed=root/L"renamed";
    check(MoveFileW((root/L"save").c_str(),renamed.c_str())!=FALSE,"ancestor rename works after release");
    const auto ordinary=renamed/L"world.sav";
    check(CreateHardLinkW((renamed/L"hardlink.sav").c_str(),ordinary.c_str(),nullptr)!=FALSE,"real hardlink created");
    check(refused(ordinary),"multiply linked source refused");
    check(refused(renamed/L"hardlink.sav"),"multiply linked destination refused");
    write(renamed/L"empty.sav",""); check(refused(renamed/L"empty.sav"),"empty file cannot prove publication");
    check(refused(renamed/L"missing.sav"),"missing publication refused");
    std::printf("PASS %u Dragonwilds real Windows file checks\n",checks);
}
