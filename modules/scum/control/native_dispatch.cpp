#include "native_state.hpp"
#include <intrin.h>

namespace scum_save {
thread_local BatchFrame* batch_frame{};
namespace {
struct FunctionShape {std::uint32_t invoker,table;};
constexpr FunctionShape functions[]={{0x1b976c0,0x571e488},{0x1b979f0,0x59795d8},
    {0x1b98080,0x571e488},{0x1b98210,0x59df9b8},{0x1959490,0x59795d8}};
std::uintptr_t storage(std::uintptr_t function) {
    const auto heap=require_read<std::uintptr_t>(function+0x10);return heap?heap:function+0x20;
}
std::uintptr_t function_payload(std::uintptr_t function,std::uint32_t invoke,std::uint32_t table,unsigned offset) {
    const auto base=image_base(),object=storage(function);
    if(require_read<std::uintptr_t>(function)!=base+invoke||require_read<std::uintptr_t>(object)!=base+table||
        require_read<std::uintptr_t>(base+table+8)!=base+(offset==16?0x964310:0x9108c0))
        throw std::runtime_error("native_task_callable_identity");
    return object+offset;
}
using Invoke=void*(*)(void*,std::uintptr_t,std::uintptr_t*);
template<std::uint32_t Address>struct Dispatch {
    static inline Invoke original{};
    static void* call(void* output,std::uintptr_t payload,std::uintptr_t* connection) {
        const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());
        // Other native uses retain their original ABI and source scope. The
        // sole batch invocation site must always have its exact active frame.
        if(caller!=image_base()+0x1aceb8c)return original(output,payload,connection);
        auto* frame=batch_frame;Owner owner;const auto error=GetLastError();
        struct LastError {DWORD value;~LastError(){SetLastError(value);}} last_error{error};
        try {
            std::lock_guard lock(runtime().mutex);
            if(!frame||frame->root!=runtime().active||frame->root->confirmed||scope||
                frame->root->worker_thread!=GetCurrentThreadId()||frame->cursor>=frame->records.size()||
                !connection||require_read<std::uintptr_t>(reinterpret_cast<std::uintptr_t>(connection))!=frame->connection)
                throw std::runtime_error("native_task_dispatch_context");
            const auto& expected=frame->records[frame->cursor];auto actual=task_record(expected.function);
            actual.sequence=expected.sequence;actual.binding.kind=expected.binding.kind;
            actual.binding.producer=expected.binding.producer;
            actual.binding.manager=expected.binding.manager;actual.binding.payload_hash=expected.binding.payload_hash;
            if(expected.binding.kind==TaskKind::item) {
                actual.binding.writer=expected.binding.writer;actual.binding.writer_table=expected.binding.writer_table;
                validate_item_function(expected.function,actual.binding);
            }
            if(expected.binding.kind==TaskKind::virtualized) {
                actual.binding.writer=expected.binding.writer;actual.binding.writer_table=expected.binding.writer_table;
                validate_virtualized_function(expected.function,actual.binding,frame->root);
            }
            if(expected.binding.invoker!=Address||actual.binding!=expected.binding||actual.payload!=payload||
                expected.payload!=payload||!same_identity(frame->root->worker)||
                require_read<std::uintptr_t>(frame->root->worker.pointer+0x30)!=frame->root->native_worker)
                throw std::runtime_error("native_task_dispatch_identity");
            owner=task_owner(frame->root,expected.sequence);++frame->cursor;
        }catch(...){unknown(frame?frame->root:runtime().active,"native_task_dispatch_identity");}
        void* result{};
        {Scope entered(owner,frame?frame->connection:0);SetLastError(error);
        try {result=original(output,payload,connection);}catch(...){last_error.value=GetLastError();unknown(owner,"native_task_exception");throw;}
        last_error.value=GetLastError();
        if(owner)try {
            if(result!=output)throw std::runtime_error("native_task_return");
            if(owner->item_scope||owner->virtualized_scope) {
            // Proven item and virtualization adapters both return an empty
            // continuation. Auxiliary receipts may only publish C0.
            if(result!=output||require_read<std::uintptr_t>(reinterpret_cast<std::uintptr_t>(output))||
                require_read<std::uintptr_t>(reinterpret_cast<std::uintptr_t>(output)+0x10))
                throw std::runtime_error("native_item_continuation");}
        }catch(...){unknown(owner,"native_task_return");}}
        return result;
    }
    static Hook hook(){return native_hook(Address,reinterpret_cast<void*>(call),reinterpret_cast<void**>(&original));}
};
}
TaskRecord task_record(std::uintptr_t function) {
    const auto invoke=require_read<std::uintptr_t>(function),base=image_base();
    for(const auto& shape:functions)if(invoke==base+shape.invoker) {
        TaskRecord value;value.function=function;value.binding.invoker=shape.invoker;value.binding.table=shape.table;
        value.payload=function_payload(function,shape.invoker,shape.table,16);return value;
    }
    throw std::runtime_error("native_task_callable_unknown");
}
void validate_item_function(std::uintptr_t function,TaskBinding& binding) {
    const auto outer=task_record(function);const auto base=image_base();
    if(binding.kind!=TaskKind::item||outer.binding.invoker!=0x1b976c0||outer.binding.table!=0x571e488||
        !binding.producer||!binding.writer||!binding.writer_table)
        throw std::runtime_error("native_item_callable");
    const auto descriptor=require_read<std::uintptr_t>(base+0x742a1d0);
    if(!descriptor||require_read<std::uintptr_t>(outer.payload)!=descriptor||
        require_read<std::uintptr_t>(descriptor)!=base+0x59d8488)
        throw std::runtime_error("native_item_cache");
    const auto wrapped=function_payload(outer.payload+0x10,0x1b5bca0,0x58c4b08,16);
    (void)function_payload(wrapped,binding.writer,binding.writer_table,8);
}
namespace {
struct VirtualizedShape {std::uint32_t producer,writer,table,bytes;};
constexpr VirtualizedShape virtualized_shapes[]={
    {0x1e25f1d,0x1e28e30,0x5a73b78,88},{0x1e32d14,0x1e28db0,0x56c6b10,40},
    {0x1e45225,0x1e28cb0,0x51e8bd8,8}};
Digest virtualized_hash(std::uintptr_t payload,const VirtualizedShape& shape) {
    std::vector<unsigned char> canonical;
    const auto append=[&](std::uintptr_t at,std::size_t count) {
        if(count>1048576||at>UINTPTR_MAX-count||canonical.size()>1114112-count)
            throw std::runtime_error("native_virtualized_payload_size");
        const auto size=canonical.size();canonical.resize(size+count);
        if(count&&!memory(at,canonical.data()+size,count))throw std::runtime_error("native_virtualized_payload");
    };
    if(shape.bytes!=88)append(payload,shape.bytes);
    else {
        // The actual clone deep-copies FString and byte-array storage. Hash
        // logical content, never heap addresses/capacity or inactive optional bytes.
        append(payload,8);append(payload+0x18,4);append(payload+0x20,8);
        for(const auto [offset,stride]:{std::pair{8u,2u},std::pair{0x28u,1u}}) {
            const auto at=payload+offset;const auto count=require_read<int>(at+8),capacity=require_read<int>(at+12);
            const auto address=require_read<std::uintptr_t>(at);
            if(count<0||count>capacity||capacity>1048576||(count&&!address)||
                (stride==2&&count>32768))throw std::runtime_error("native_virtualized_payload_size");
            append(at+8,4);append(address,static_cast<std::size_t>(count)*stride);
        }
        const auto present=require_read<unsigned char>(payload+0x50);
        if(present>1)throw std::runtime_error("native_virtualized_optional");
        append(payload+0x50,1);
        if(present){append(payload+0x38,4);append(payload+0x40,8);append(payload+0x48,1);}
    }
    Digest result{};if(!buffer_hash(canonical.data(),canonical.size(),result))throw std::runtime_error("native_virtualized_payload_hash");
    return result;
}
const VirtualizedShape& virtualized_shape(const TaskBinding& binding) {
    for(const auto& shape:virtualized_shapes)if(binding.producer==shape.producer&&binding.writer==shape.writer&&binding.writer_table==shape.table)return shape;
    throw std::runtime_error("native_virtualized_producer");
}
}
TaskBinding virtualized_input(std::uintptr_t function,std::uintptr_t manager,std::uint32_t producer) {
    TaskBinding binding;binding.kind=TaskKind::virtualized;binding.producer=producer;
    for(const auto& shape:virtualized_shapes)if(producer==shape.producer) {
        binding.writer=shape.writer;binding.writer_table=shape.table;
        const auto payload=function_payload(function,shape.writer,shape.table,8);
        binding.payload_hash=virtualized_hash(payload,shape);
        binding.manager=identity(manager,require_read<std::uintptr_t>(image_base()+0x744b648));return binding;
    }
    throw std::runtime_error("native_virtualized_producer");
}
void validate_virtualized_function(std::uintptr_t function,const TaskBinding& binding,const Owner& root) {
    const auto outer=task_record(function);const auto base=image_base();const auto& shape=virtualized_shape(binding);
    if(binding.kind!=TaskKind::virtualized||outer.binding.invoker!=0x1959490||outer.binding.table!=0x59795d8||
        !same_identity(binding.manager)||binding.manager.type!=require_read<std::uintptr_t>(base+0x744b648)||
        require_read<std::uintptr_t>(binding.manager.pointer)!=base+0x5a68b00||
        require_read<std::uintptr_t>(base+0x5a68b00+0x160)!=base+0x3ecd490||
        !same_identity(root->world)||!same_identity(root->instance)||!same_identity(root->worker)||
        reinterpret_cast<std::uintptr_t(*)(std::uintptr_t)>(base+0x3ecd490)(binding.manager.pointer)!=root->world.pointer||
        reinterpret_cast<std::uintptr_t(*)(std::uintptr_t)>(base+0x2d230b0)(binding.manager.pointer+0x520)!=root->worker.pointer||
        require_read<std::uintptr_t>(root->worker.pointer+0x30)!=root->native_worker)
        throw std::runtime_error("native_virtualized_world");
    const auto payload=function_payload(outer.payload,shape.writer,shape.table,8);
    if(virtualized_hash(payload,shape)!=binding.payload_hash)throw std::runtime_error("native_virtualized_payload_changed");
}
std::vector<Hook> dispatch_hooks() {
    return {Dispatch<0x1b976c0>::hook(),Dispatch<0x1b979f0>::hook(),
        Dispatch<0x1b98080>::hook(),Dispatch<0x1b98210>::hook(),Dispatch<0x1959490>::hook()};
}
}
