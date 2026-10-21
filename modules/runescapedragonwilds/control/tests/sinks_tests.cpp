#include "../save_native.hpp"
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
using namespace dragon_save;
namespace {
unsigned checks{};
void check(bool value,const char* name) { ++checks; if (!value) { std::fprintf(stderr,"FAIL %s (%lu)\n",name,GetLastError()); std::exit(1); } }
alignas(16) unsigned char writer[0xe8]{},physical[0x50]{};
bool close_failure{},copy_failure{},move_failure{},corrupt_copy{},corrupt_move{},flush_failure{};
bool read_failure{};
using OsClose=BOOL(WINAPI*)(HANDLE);
OsClose close_at_native_return{};
template<class T> void put(unsigned char* data,std::size_t at,T value) { std::memcpy(data+at,&value,sizeof value); }
void write(const fs::path& path,const char* text) { std::ofstream stream(path,std::ios::binary); stream<<text; stream.close(); check(stream.good(),"fixture file write"); }
BOOL WINAPI os_close(HANDLE file) { const auto result=CloseHandle(file); if (close_failure) { SetLastError(ERROR_ACCESS_DENIED); return FALSE; } return result; }
std::uintptr_t create(std::uintptr_t,const wchar_t* path,std::uint32_t,std::uint32_t) {
    const auto file=CreateFileW(path,GENERIC_WRITE,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,CREATE_ALWAYS,FILE_ATTRIBUTE_NORMAL,nullptr);
    if (file==INVALID_HANDLE_VALUE) return 0;
    constexpr char bytes[]="native world payload, actual Windows file"; DWORD count{};
    check(WriteFile(file,bytes,sizeof bytes-1,&count,nullptr) && count==sizeof bytes-1,"real writer bytes");
    std::memset(writer,0,sizeof writer); std::memset(physical,0,sizeof physical);
    put(writer,0,image_base+0x88fd2d0); put(writer,0xb0,reinterpret_cast<std::uintptr_t>(physical));
    put(physical,0,image_base+0x8937788); put(physical,8,reinterpret_cast<std::uintptr_t>(file));
    SetLastError(0x7755); return reinterpret_cast<std::uintptr_t>(writer);
}
bool close_writer(std::uintptr_t object) {
    check(object==reinterpret_cast<std::uintptr_t>(writer),"actual observer forwards same archive");
    HANDLE file{}; std::memcpy(&file,physical+8,sizeof file);
    const bool flushed=FlushFileBuffers(file)!=FALSE;
    if (!flushed || flush_failure) writer[0x29]|=2;
    (void)close_at_native_return(file); put<std::uintptr_t>(writer,0xb0,0);
    // Mirrors the observed native CloseLowLevel which swallows CloseHandle's
    // BOOL. The production observer must independently catch that failure.
    SetLastError(0x7755); return !(writer[0x29]&2);
}
std::uintptr_t reader_factory(std::uintptr_t,const wchar_t* path,std::uint32_t) {
    const auto file=CreateFileW(path,GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_EXISTING,FILE_ATTRIBUTE_NORMAL,nullptr);
    if (file==INVALID_HANDLE_VALUE) return 0;
    std::array<char,128> bytes{}; DWORD count{};
    check(ReadFile(file,bytes.data(),static_cast<DWORD>(bytes.size()),&count,nullptr) && count>0,"real required cache bytes read");
    std::memset(writer,0,sizeof writer); std::memset(physical,0,sizeof physical);
    put(writer,0,image_base+0x88fd058); put(writer,0xb8,reinterpret_cast<std::uintptr_t>(physical));
    put(physical,0,image_base+0x8937788); put(physical,8,reinterpret_cast<std::uintptr_t>(file));
    SetLastError(0x7755); return reinterpret_cast<std::uintptr_t>(writer);
}
bool close_reader(std::uintptr_t) {
    HANDLE file{}; std::memcpy(&file,physical+8,sizeof file); (void)close_at_native_return(file);
    if (read_failure) writer[0x29]|=2;
    SetLastError(0x7755); return !(writer[0x29]&2);
}
std::uint32_t copy(std::uintptr_t,const wchar_t* to,const wchar_t* from,bool,bool,bool,void*,std::uint32_t,std::uint32_t) {
    if (copy_failure) { SetLastError(0x7755); return 1; }
    const auto copied=CopyFileW(from,to,FALSE)!=FALSE;
    if (copied && corrupt_copy) write(to,"wrong copied bytes");
    SetLastError(0x7755); return copied ? 0U : 1U;
}
bool move(std::uintptr_t,const wchar_t* to,const wchar_t* from,bool,bool,bool,bool) {
    if (move_failure) { SetLastError(0x7755); return false; }
    const auto moved=MoveFileExW(from,to,MOVEFILE_REPLACE_EXISTING)!=FALSE;
    if (moved && corrupt_move) write(to,"wrong published bytes");
    SetLastError(0x7755); return moved;
}
// Real x64 calls whose return addresses are the reviewed game call sites.
// Boundary inputs still execute the production sink observer unchanged.
void* caller(unsigned char* image,std::uint32_t return_rva,void* target,unsigned stack_arguments) {
    auto* page=image+(return_rva&~4095U);
    check(VirtualAlloc(page,4096,MEM_COMMIT,PAGE_EXECUTE_READWRITE)==page,"commit isolated caller page");
    std::vector<unsigned char> bytes={0x48,0x83,0xec,0x58};
    for (unsigned index=0;index<stack_arguments;++index) {
        // Incoming arg5 at old RSP+0x28 -> new RSP+0x80.
        const unsigned char load[]={0x48,0x8b,0x84,0x24}; bytes.insert(bytes.end(),std::begin(load),std::end(load));
        const std::uint32_t source=0x80+index*8; const auto* raw=reinterpret_cast<const unsigned char*>(&source); bytes.insert(bytes.end(),raw,raw+4);
        const unsigned char store[]={0x48,0x89,0x44,0x24,static_cast<unsigned char>(0x20+index*8)}; bytes.insert(bytes.end(),std::begin(store),std::end(store));
    }
    bytes.push_back(0x48); bytes.push_back(0xb8); const auto address=reinterpret_cast<std::uintptr_t>(target);
    const auto* raw=reinterpret_cast<const unsigned char*>(&address); bytes.insert(bytes.end(),raw,raw+8);
    bytes.push_back(0xff); bytes.push_back(0xd0);
    const auto start=return_rva-bytes.size(); std::memcpy(image+start,bytes.data(),bytes.size());
    constexpr unsigned char end[]={0x48,0x83,0xc4,0x58,0xc3}; std::memcpy(image+return_rva,end,sizeof end);
    FlushInstructionCache(GetCurrentProcess(),page,4096); return image+start;
}
std::shared_ptr<Record> record(const fs::path& root) {
    auto value=std::make_shared<Record>(0); value->slot=L"world";
    value->published=full_path(root/L"SaveGames/world.sav"); value->incomplete=value->published.native()+L".incomplete";
    value->backup=value->published.native()+L".backup"; value->version_backup=value->published.native()+L".3.verbackup";
    value->completion.enter_root(); value->completion.begin_world_stage(); value->completion.enter_slot();
    return value;
}
}
int wmain(int count,wchar_t** args) {
    if (count!=2) return 2;
    const auto output=full_path(args[1]); fs::create_directories(output);
    auto* image=static_cast<unsigned char*>(VirtualAlloc(nullptr,256ULL*1024*1024,MEM_RESERVE,PAGE_NOACCESS));
    check(image!=nullptr,"reserve physically isolated native caller fixture"); image_base=reinterpret_cast<std::uintptr_t>(image);
    using Create=std::uintptr_t(*)(std::uintptr_t,const wchar_t*,std::uint32_t,std::uint32_t);
    using Close=bool(*)(std::uintptr_t);
    using Reader=std::uintptr_t(*)(std::uintptr_t,const wchar_t*,std::uint32_t);
    using Copy=std::uint32_t(*)(std::uintptr_t,const wchar_t*,const wchar_t*,bool,bool,bool,void*,std::uint32_t,std::uint32_t);
    using Move=bool(*)(std::uintptr_t,const wchar_t*,const wchar_t*,bool,bool,bool,bool);
    Create observed_create{}; Close observed_close{},observed_reader_close{}; Copy observed_copy{}; Move observed_move{}; Reader observed_reader{};
    for (const auto& hook : sink_hooks()) {
        if (hook.rva==0x12ad020) { *hook.original=reinterpret_cast<void*>(&create); observed_create=reinterpret_cast<Create>(hook.replacement); }
        if (hook.rva==0x12ac5c0) { *hook.original=reinterpret_cast<void*>(&close_writer); observed_close=reinterpret_cast<Close>(hook.replacement); }
        if (hook.rva==0x12ac8e0) { *hook.original=reinterpret_cast<void*>(&copy); observed_copy=reinterpret_cast<Copy>(hook.replacement); }
        if (hook.rva==0x12b0420) { *hook.original=reinterpret_cast<void*>(&move); observed_move=reinterpret_cast<Move>(hook.replacement); }
        if (hook.rva==0x12acea0) { *hook.original=reinterpret_cast<void*>(&reader_factory); observed_reader=reinterpret_cast<Reader>(hook.replacement); }
        if (hook.rva==0x12ac590) { *hook.original=reinterpret_cast<void*>(&close_reader); observed_reader_close=reinterpret_cast<Close>(hook.replacement); }
    }
    close_at_native_return=reinterpret_cast<OsClose>(caller(image,0x142dc33,test_os_close(reinterpret_cast<void*>(&os_close)),0));
    const auto publish=reinterpret_cast<Move>(caller(image,0x5483973,reinterpret_cast<void*>(observed_move),3));
    const auto backup=reinterpret_cast<Move>(caller(image,0x548390b,reinterpret_cast<void*>(observed_move),3));
    const auto version_copy=reinterpret_cast<Copy>(caller(image,0x5483818,reinterpret_cast<void*>(observed_copy),5));
    const auto read_cache=reinterpret_cast<Reader>(caller(image,0x5468731,reinterpret_cast<void*>(observed_reader),0));
    // A shared page can contain multiple exact callers; caller() only writes
    // its small own instruction sequence and keeps prior executable stubs.
    for (unsigned scenario=0;scenario<8;++scenario) {
        const auto root=output/std::to_wstring(scenario); fs::create_directories(root/L"SaveGames"); saved_root=root;
        auto state=record(root); scope_record=state;
        close_failure=scenario==1; flush_failure=scenario==2; copy_failure=scenario==3;
        corrupt_copy=scenario==4; move_failure=scenario==5; corrupt_move=scenario==6;
        if (scenario>=3) write(state->published,"previous world bytes");
        const auto archive=observed_create(1,state->incomplete.c_str(),0x40,0x1000);
        check(GetLastError()==0x7755,"factory preserves original last error after identity checks");
        check(archive!=0,"production create observes actual ordinary file");
        const auto closed=observed_close(archive);
        check(GetLastError()==0x7755,"close preserves original last error after readback");
        check(closed==!flush_failure,"observer preserves native close result");
        if (scenario>=3 && scenario<=4) {
            const auto result=version_copy(1,state->version_backup.c_str(),state->published.c_str(),true,false,false,nullptr,0,0);
            check(GetLastError()==0x7755,"copy preserves original last error after readback");
            check(result==(copy_failure ? 1U : 0U),"observer preserves native copy result");
        }
        if (scenario>=3) (void)backup(1,state->backup.c_str(),state->published.c_str(),true,false,false,true);
        if (scenario==7) {
            write(state->incomplete,"tampered closed source");
        }
        const auto result=publish(1,state->published.c_str(),state->incomplete.c_str(),true,false,false,true);
        check(GetLastError()==0x7755,"move preserves original last error after publication readback");
        check(result==!move_failure,"observer preserves native publication result");
        if (scenario==0 && (state->completion.status!=Status::dispatching || state->completion.publications!=1))
            std::fprintf(stderr,"sink diagnostic state=%u issue=%u writers=%zu closed=%zu publications=%zu osclosed=%u\n",
                static_cast<unsigned>(state->completion.status),static_cast<unsigned>(state->completion.issue),
                state->completion.writer_entries,state->completion.writer_closes,state->completion.publications,
                state->writers.empty() ? 0U : static_cast<unsigned>(state->writers[0].os_closed));
        if (scenario==0) check(state->completion.status==Status::dispatching && state->completion.publications==1,"true native sink sequence remains owned until aggregate");
        else if (scenario==1 || scenario==2 || scenario==3 || scenario==5) check(state->completion.status==Status::failed,"actual native failure cannot be hidden by later true");
        else check(state->completion.status==Status::unknown,"readback/identity mismatch cannot complete");
        check(state->completion.native_debt==2,"actual writer close releases only writer debt");
        scope_record.reset();
    }
    for (unsigned scenario=0;scenario<5;++scenario) {
        const auto root=output/(L"cache-"+std::to_wstring(scenario)); fs::create_directories(root/L"SaveGames"); fs::create_directories(root/L"SpudCache");
        saved_root=root; auto state=record(root); scope_record=state;
        close_failure=scenario==2; read_failure=scenario==1;
        const auto cache=root/L"SpudCache"/L"level.lvl";
        if (scenario!=4) write(cache,"native persisted level cache");
        const auto archive=read_cache(1,cache.c_str(),0);
        if (scenario==4) { check(archive==0 && state->completion.status!=Status::completed,"missing required cached level cannot be silently skipped"); scope_record.reset(); continue; }
        check(archive!=0,"actual required cache reader observed");
        if (scenario==3) write(cache,"concurrent changed cache");
        const auto result=observed_reader_close(archive);
        check(GetLastError()==0x7755,"reader close preserves original last error");
        check(result==!read_failure,"required reader result preserved");
        if (scenario==0) check(state->completion.status==Status::dispatching,"required cache read+actual close succeeds");
        else if (scenario==3) check(state->completion.status==Status::unknown,"changed required input cannot certify a world file");
        else check(state->completion.status==Status::failed,"required cache read/OS close failure retained");
        check(state->completion.native_debt==2,"required cache debt drains at actual close"); scope_record.reset();
    }
    VirtualFree(image,0,MEM_RELEASE);
    std::printf("PASS %u Dragonwilds actual production file-observer components\n",checks);
}
