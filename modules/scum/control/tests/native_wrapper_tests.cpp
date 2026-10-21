// Full fixed copied wrapper (437 bytes) and callable adapter (87 bytes).
// Only external allocation, weak-object, publication, cookie and functor
// boundaries are host functions. No game entry, Save, SQL or worker executes.
#include "native_fixture.hpp"
#include "../native_state.hpp"
#include <intrin.h>
#include "../native_tasks.cpp"
#include "../../../enshrouded/control/vendor/minhook/include/MinHook.h"
namespace {
struct Function {uintptr_t invoke{},pad{},storage{},pad2{};unsigned char inlined[32]{};};
struct Functor {uintptr_t* table{};unsigned cookie{0xabc123};};
static_assert(sizeof(Function)==64);
alignas(16) unsigned char system_object[0x80]{},outer[0x80]{},worker[0x40]{},native_worker[0x140]{};
uintptr_t table[3]{},opaque=0x1234567812345678ULL,event_value=0x88776655,constructed=0x11223344;
unsigned clones{},destroys{},allocs{},frees{},queues{},adapters{},getters{},cookies{};bool active=true,has_body=true;
uintptr_t node{};
void clone(Functor* self,void** output){auto error=GetLastError();auto* copy=(Functor*)_aligned_malloc(sizeof(Functor),16);require(copy,"clone allocation");*copy=*self;*output=copy;++clones;SetLastError(error);}
void* callable(Functor* self){check(self->cookie==0xabc123,"copied native callable identity");return self;}
void destroy(Functor* self){auto error=GetLastError();check(self->cookie==0xabc123,"copied native destroy identity");_aligned_free(self);++destroys;SetLastError(error);}
void* weak(uintptr_t field){++getters;if(field==(uintptr_t)system_object+0x58)return outer;require(active&&field==(uintptr_t)outer+0x58,"native wrapper weak receiver");return worker;}
void* allocation(size_t bytes,unsigned alignment){auto error=GetLastError();check(bytes==0x60&&alignment==0x10,"native wrapper allocation ABI");node=(uintptr_t)_aligned_malloc(bytes,alignment);require(node,"node allocation");std::memset((void*)node,0,bytes);++allocs;SetLastError(error);return (void*)node;}
void destroy_node(uintptr_t value){auto error=GetLastError();check(value==node,"native wrapper destructor receiver");if(*(uintptr_t*)(value+0x20))destroy(*(Functor**)(value+0x30));_aligned_free((void*)value);++frees;node=0;SetLastError(error);}
void cookie(uintptr_t value){check(value==0x6d3a91a5,"full native wrapper stack cookie and epilogue");++cookies;SetLastError(4321);}
void* construct(uintptr_t event,uintptr_t context){check(event==event_value&&context==opaque,"full native adapter opaque args");return &constructed;}
void invoke(Functor* self,void** context){check(self->cookie==0xabc123&&context&&*context==&constructed,"native adapter original callable args");++adapters;}
int original_queue(uintptr_t object,void* reference){
    check(object==(active?(uintptr_t)worker:0),"native wrapper queue receiver");auto* fn=(Function*)reference;
    check(fn&&fn->invoke==mapped+0x1b976c0&&fn->storage==node,"native wrapper original callable layout");
    check(*(uintptr_t*)node==mapped+0x571e488&&*(uintptr_t*)(node+0x10)==opaque,"native wrapper copied context/vtable");
    check((*(uintptr_t*)(node+0x20)!=0)==has_body,"native wrapper empty/nonempty body preserved");
    uintptr_t replacement=(uintptr_t)destroy_node;protect_copy(mapped+0x571e488+16,&replacement,8);
    if(has_body){Function output{};output.invoke=1;output.storage=2;
        auto* returned=((Function*(*)(Function*,void*,uintptr_t*))(mapped+0x1b976c0))(&output,(void*)(node+0x10),&event_value);
        check(returned==&output&&!output.invoke&&!output.storage,"full native adapter sret and empty continuation");}
    ++queues;SetLastError(4321);return 42;
}
__declspec(noinline) int publication(uintptr_t object,void* reference){
    check((uintptr_t)_ReturnAddress()==mapped+0x1ba64c9,"full copied wrapper actual queue return RVA");
    return active?scum_save::queue_hook(object,reference):original_queue(object,reference);
}
}
int wmain(int argc,wchar_t** argv){try{
    require(argc==2,"arguments");auto root=fixture_root(argv[1]);auto image=root/L"image/SCUMServer.exe";
    auto dirs=pin_directories(image.parent_path());auto file=open_file(image,GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ);
    check(hash_file(file.value)=="4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad","exact ordinary physical image");
    mapped=(uintptr_t)LoadLibraryExW(image.c_str(),nullptr,DONT_RESOLVE_DLL_REFERENCES);require(mapped,"data-only image mapping");SetUnhandledExceptionFilter(fault);
    check(scum_save::test_profile_copy(image,mapped),"full existing image profile before host boundaries");
    std::array<unsigned char,437> body{};std::array<unsigned char,87> adapter{};
    std::memcpy(body.data(),(void*)(mapped+0x1ba6380),body.size());std::memcpy(adapter.data(),(void*)(mapped+0x1b976c0),adapter.size());
    auto hooks=scum_save::task_hooks();auto found=std::find_if(hooks.begin(),hooks.end(),[](const auto& h){return h.rva==0x1ba6380;});
    require(found!=hooks.end(),"production wrapper hook missing");check(scum_save::install_hooks({*found},{}),"real production wrapper MinHook and original trampoline");
    check(scum_save::code_matches(),"original profile including tracked five-byte hook before fixture stubs");
    jump(mapped+0x2d230b0,(uintptr_t)weak);jump(mapped+0x2a1f270,(uintptr_t)allocation);jump(mapped+0x1abced0,(uintptr_t)publication);
    jump(mapped+0x50876e0,(uintptr_t)cookie);jump(mapped+0x1ac4d30,(uintptr_t)construct);
    uintptr_t cookie_value=0x6d3a91a5;protect_copy(mapped+0x73d94f8,&cookie_value,8);unsigned char log_level=0;protect_copy(mapped+0x742a0f0,&log_level,1);
    table[0]=(uintptr_t)clone;table[1]=(uintptr_t)callable;table[2]=(uintptr_t)destroy;
    auto address=(uintptr_t)native_worker;std::memcpy(worker+0x30,&address,8);InitializeCriticalSection((CRITICAL_SECTION*)(native_worker+0x18));
    scum_save::queue_original=original_queue;auto& state=scum_save::runtime();state.armed=true;state.broken=false;
    for(unsigned mode=0;mode<5;++mode){active=mode<3;has_body=mode!=2&&mode!=4;outer[0x30]=(unsigned char)active;
        clones=destroys=allocs=frees=queues=adapters=getters=cookies=0;
        auto owner=std::make_shared<scum_save::Epoch>();owner->system.pointer=(uintptr_t)system_object;owner->native_worker=(uintptr_t)native_worker;
        owner->main_thread=GetCurrentThreadId();state.active=owner;scum_save::scope.reset();state.task_inflight=state.io_inflight=state.sql_inflight=0;
        Functor original{table};Function input{};input.invoke=has_body?(uintptr_t)invoke:0;
        if(mode==1)std::memcpy(input.inlined,&original,sizeof(original));else input.storage=(uintptr_t)&original;
        SetLastError(883);((void(*)(void*,uintptr_t,Function*))(mapped+0x1ba6380))(system_object,opaque,&input);
        check(queues==1&&allocs==1&&frees==1&&!node&&cookies==1,"native wrapper original calls and complete cleanup");
        check(clones==(has_body?2U:0U)&&destroys==clones&&adapters==(has_body?1U:0U),"native wrapper heap/inline/empty callable lifetimes");
        check(getters==(active?2U:1U)&&GetLastError()==4321,"native wrapper active/inactive weak flow and return error");
        check(!scum_save::wrapper_frame.present&&!scum_save::wrapper_frame.system&&!scum_save::wrapper_frame.caller,"production wrapper TLS restored after full native body");
        if(active)check(owner->first_foreign_queue.captured&&owner->first_foreign_queue.parent_context==5&&owner->first_foreign_queue.parent_rva==0&&owner->completion.rejected(),"host caller bounded outside image without adopting foreign queue");
        check(!owner->confirmed&&state.task_inflight==0&&state.sql_inflight==0&&state.io_inflight==0,"diagnostic adds no ownership or confirmation");
    }
    check(!std::memcmp(body.data()+5,(void*)(mapped+0x1ba6385),body.size()-5)&&!std::memcmp(adapter.data(),(void*)(mapped+0x1b976c0),adapter.size()),"complete wrapper remainder and adapter bytes unchanged");
    check(MH_DisableHook(MH_ALL_HOOKS)==MH_OK&&MH_Uninitialize()==MH_OK,"owned test hook removed");
    check(!std::memcmp(body.data(),(void*)(mapped+0x1ba6380),body.size()),"complete original 437-byte wrapper restored");
    state.active.reset();DeleteCriticalSection((CRITICAL_SECTION*)(native_worker+0x18));
    std::cout<<"SCUM complete copied wrapper ABI checks="<<checks<<" wrapper_bytes=437 adapter_bytes=87 game_entry_Save_SQL_worker_executed=false callee_boundaries=host\n";return 0;
}catch(const std::exception& error){std::cerr<<error.what()<<'\n';return 1;}}
