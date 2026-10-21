#pragma once
#include "native_fixture.hpp"
#include "../native_state.hpp"
#include <atomic>
#include "../../../enshrouded/control/vendor/minhook/include/MinHook.h"
namespace {
struct Function {uintptr_t invoke{},pad{},storage{},storage_pad{};unsigned char inlined[32]{};};
static_assert(sizeof(Function)==64);
struct Functor {uintptr_t* table{};unsigned cookie{};bool continuation{};};
struct Event {uintptr_t* table{};HANDLE ready{};};
alignas(16) unsigned char runtime[0x128]{};
alignas(16) unsigned char worker[0x40]{};
uintptr_t functor_table[3]{},event_table[5]{},worker_table[45]{},allocator_table[16]{};
Event event;
std::atomic<unsigned> functors{},bodies{},continuations{},body_sql_ok{};
HANDLE body_done{},callback_done{};
DWORD worker_thread{};
uintptr_t* allocator{};
CRITICAL_SECTION* producer(){return (CRITICAL_SECTION*)(runtime+0x18);}
void* no_world(void*){return nullptr;}
void clone(Functor* self,void** storage) {
 auto* result=(Functor*)_aligned_malloc(sizeof(Functor),64);require(result!=nullptr,"functor_copy_alloc");
 *result=*self;*storage=result;++functors;
}
void* get_callable(Functor* self){return self;}
void destroy(Functor* self){require(functors>0,"functor_lifetime");--functors;_aligned_free(self);}
void callback(Functor* value,const int* sequence) {
 require(value->cookie==0xadd5&&sequence&&*sequence>0,"actual_continuation_identity");
 ++continuations;SetEvent(callback_done);
}
bool wait_event(Event* self,unsigned timeout,bool) {return WaitForSingleObject(self->ready,timeout)==WAIT_OBJECT_0;}
DWORD WINAPI run(void*) {worker_thread=GetCurrentThreadId();return ((DWORD(*)(void*))(mapped+0x1ad3aa0))(runtime);}
void wait_count(unsigned count) {
 const auto start=GetTickCount64();
 for(;;) {
  EnterCriticalSection((CRITICAL_SECTION*)(runtime+0x68));
  const unsigned available=*(unsigned*)(runtime+0xc8)+*(unsigned*)(runtime+0xb8);
  LeaveCriticalSection((CRITICAL_SECTION*)(runtime+0x68));
  if(available==count)return;
  require(GetTickCount64()-start<5000,"native_completion_publication_timeout");Sleep(1);
 }
}
}
