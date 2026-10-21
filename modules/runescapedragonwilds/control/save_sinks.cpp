#include "save_native.hpp"
#include <intrin.h>
#include <stdexcept>
namespace dragon_save {
namespace {
using Create=std::uintptr_t(*)(std::uintptr_t,const wchar_t*,std::uint32_t,std::uint32_t);
using Reader=std::uintptr_t(*)(std::uintptr_t,const wchar_t*,std::uint32_t);
using Close=bool(*)(std::uintptr_t);
using Copy=std::uint32_t(*)(std::uintptr_t,const wchar_t*,const wchar_t*,bool,bool,bool,void*,std::uint32_t,std::uint32_t);
using Move=bool(*)(std::uintptr_t,const wchar_t*,const wchar_t*,bool,bool,bool,bool);
Create original_create{};
Reader original_reader{};
Close original_close{},original_reader_close{};
Copy original_copy{};
Move original_move{};
using OsClose=BOOL(WINAPI*)(HANDLE);
OsClose original_os_close{};
bool patched_close{};
struct LastError {
    DWORD value=GetLastError();
    void restore() const noexcept { SetLastError(value); }
    void capture() noexcept { value=GetLastError(); }
    ~LastError() { restore(); }
};
void unknown(const std::shared_ptr<Record>& record) noexcept { if (record) { Lock lock(record->lock); record->completion.unknown(); } }
void failed(const std::shared_ptr<Record>& record) noexcept { if (record) { Lock lock(record->lock); record->completion.fail(); } }
fs::path owned_path(const wchar_t* text) {
    if (!text) throw std::runtime_error("native_path");
    std::wstring copy;
    for (std::size_t index=0;index<32767;++index) {
        wchar_t value{};
        if (!read(reinterpret_cast<std::uintptr_t>(text)+index*sizeof(wchar_t),value)) throw std::runtime_error("native_path");
        if (!value) break;
        copy.push_back(value);
    }
    const auto path=full_path(copy); auto boundary=saved_root.native(); boundary+=L'\\';
    if (copy.empty() || copy.size()>=32766 || path.native().size()<=boundary.size()
        || CompareStringOrdinal(path.c_str(),static_cast<int>(boundary.size()),boundary.c_str(),static_cast<int>(boundary.size()),TRUE)!=CSTR_EQUAL)
        throw std::runtime_error("native_path");
    return path;
}
std::uintptr_t hook_create(std::uintptr_t manager,const wchar_t* name,std::uint32_t flags,std::uint32_t buffer) {
    LastError error;
    const auto record=scope_record; fs::path path; std::vector<Handle> parents; bool observed{};
    if (record) {
        try {
            path=owned_path(name); parents=pin_parents(path);
            const auto attributes=GetFileAttributesW(path.c_str());
            if (attributes!=INVALID_FILE_ATTRIBUTES && (attributes & (FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_DIRECTORY))) throw std::runtime_error("native_path");
            observed=true;
        } catch (...) { unknown(record); }
    }
    error.restore(); const auto writer=original_create(manager,name,flags,buffer);
    error.capture();
    if (record && observed) {
        try {
            std::uintptr_t vtable{},physical{},physical_vtable{},file{};
            BY_HANDLE_FILE_INFORMATION info{};
            if (!writer) { failed(record); return writer; }
            if (!read(writer,vtable) || vtable!=image_base+0x88fd2d0 || !read(writer+0xb0,physical)
                || !read(physical,physical_vtable) || physical_vtable!=image_base+0x8937788 || !read(physical+8,file)
                || !GetFileInformationByHandle(reinterpret_cast<HANDLE>(file),&info)
                || (info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_DIRECTORY)) || info.nNumberOfLinks!=1) throw std::runtime_error("native_writer");
            Lock lock(record->lock);
            if (record->writers.size()>=512) { record->completion.unknown(); return writer; }
            const bool world=!record->incomplete.empty() && equal_path(path,record->incomplete);
            if (world && (flags!=0x40 || buffer!=0x1000)) record->completion.unknown();
            for (const auto& known : record->writers) if (known.object==writer && !known.closed) record->completion.unknown();
            record->writers.push_back({writer,std::move(path),static_cast<std::uint64_t>(info.nFileIndexHigh)<<32|info.nFileIndexLow,
                info.dwVolumeSerialNumber,file,world,false,false,false,std::move(parents),nullptr});
            record->completion.enter_native();
            if (world) record->completion.writer();
        } catch (...) { unknown(record); }
    }
    return writer;
}
std::uintptr_t hook_reader(std::uintptr_t manager,const wchar_t* name,std::uint32_t flags) {
    LastError error;
    const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());
    const auto record=caller==image_base+0x5468731 ? scope_record : nullptr;
    fs::path path; std::unique_ptr<Snapshot> expected;
    if (record) try {
        path=owned_path(name);
        const auto cache=saved_root/L"SpudCache";
        if (flags || !equal_path(path.parent_path(),cache) || path.extension()!=L".lvl") throw std::runtime_error("native_cache");
        expected=std::make_unique<Snapshot>(snapshot(path));
    } catch (...) { unknown(record); }
    error.restore(); const auto reader=original_reader(manager,name,flags);
    error.capture();
    if (record) {
        if (!reader) { failed(record); return reader; }
        try {
            std::uintptr_t vtable{},physical{},physical_vtable{},file{}; BY_HANDLE_FILE_INFORMATION info{};
            if (!expected || !read(reader,vtable) || vtable!=image_base+0x88fd058 || !read(reader+0xb8,physical)
                || !read(physical,physical_vtable) || physical_vtable!=image_base+0x8937788 || !read(physical+8,file)
                || !GetFileInformationByHandle(reinterpret_cast<HANDLE>(file),&info)
                || info.nNumberOfLinks!=1 || (info.dwFileAttributes&(FILE_ATTRIBUTE_DIRECTORY|FILE_ATTRIBUTE_REPARSE_POINT))
                || info.dwVolumeSerialNumber!=expected->volume || (static_cast<std::uint64_t>(info.nFileIndexHigh)<<32|info.nFileIndexLow)!=expected->index)
                throw std::runtime_error("native_cache");
            Lock lock(record->lock);
            if (record->writers.size()>=512) { record->completion.unknown(); return reader; }
            record->writers.push_back({reader,std::move(path),expected->index,expected->volume,file,false,false,false,true,{},std::move(expected)});
            record->completion.enter_native();
        } catch (...) { unknown(record); }
    }
    return reader;
}
bool close_archive(Close original,std::uintptr_t writer) {
    LastError error;
    const auto record=scope_record; std::size_t selected=SIZE_MAX; fs::path path; bool world{};
    if (record) {
        Lock lock(record->lock);
        for (std::size_t index=record->writers.size();index>0;--index)
            if (record->writers[index-1].object==writer && !record->writers[index-1].closed) {
                selected=index-1; path=record->writers[selected].path; world=record->writers[selected].world; break;
            }
    }
    error.restore(); const auto success=original(writer);
    error.capture();
    if (record && selected!=SIZE_MAX) {
        std::unique_ptr<Snapshot> observed;
        std::uint8_t flags{};
        bool os_closed{};
        { Lock lock(record->lock); os_closed=record->writers[selected].os_closed; }
        const bool native_ok=success && os_closed && read(writer+0x29,flags) && !(flags&6);
        if (native_ok) try { observed=std::make_unique<Snapshot>(snapshot(path)); } catch (...) {}
        Lock lock(record->lock);
        auto& entry=record->writers[selected];
        if (entry.closed) record->completion.unknown();
        entry.closed=true;
        if (!native_ok) record->completion.fail();
        else if (!observed || observed->index!=entry.index || observed->volume!=entry.volume) record->completion.unknown(Issue::identity_changed);
        else if (entry.reader && (!entry.input || observed->digest!=entry.input->digest || observed->size!=entry.input->size))
            record->completion.unknown(Issue::identity_changed);
        if (world) {
            record->completion.close_writer(native_ok);
            if (record->closed_world) record->completion.unknown();
            record->closed_world=std::move(observed);
        }
        record->completion.leave_native();
    }
    return success;
}
bool hook_close(std::uintptr_t writer) { return close_archive(original_close,writer); }
bool hook_reader_close(std::uintptr_t reader) { return close_archive(original_reader_close,reader); }
std::uint32_t hook_copy(std::uintptr_t manager,const wchar_t* to,const wchar_t* from,bool replace,bool readonly,
    bool attributes,void* progress,std::uint32_t read_flags,std::uint32_t write_flags) {
    LastError error;
    const auto record=scope_record; const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());
    fs::path destination,source; std::unique_ptr<Snapshot> expected;
    if (record) try {
        destination=owned_path(to); source=owned_path(from);
        // This optional version copy is performed only if its old source
        // exists. Its nonzero failure is otherwise hidden by SaveToSlot.
        if (caller!=image_base+0x5483818 || !replace || readonly || attributes || progress || read_flags || write_flags
            || !equal_path(source,record->published) || !equal_path(destination,record->version_backup))
            throw std::runtime_error("native_copy");
        expected=std::make_unique<Snapshot>(snapshot(source));
    } catch (...) { unknown(record); }
    error.restore(); const auto result=original_copy(manager,to,from,replace,readonly,attributes,progress,read_flags,write_flags);
    error.capture();
    if (record) {
        if (result!=0) failed(record);
        else if (!expected || !matches(*expected,source,true) || !matches(*expected,destination,false)) unknown(record);
    }
    return result;
}
bool hook_move(std::uintptr_t manager,const wchar_t* to,const wchar_t* from,bool replace,bool readonly,bool attributes,bool no_retry) {
    LastError error;
    const auto record=scope_record; const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());
    fs::path destination,source; std::unique_ptr<Snapshot> expected; bool publication{};
    if (record) try {
        destination=owned_path(to); source=owned_path(from);
        if (!replace || readonly || attributes || !no_retry) throw std::runtime_error("native_move");
        publication=caller==image_base+0x5483973 && equal_path(source,record->incomplete) && equal_path(destination,record->published);
        const bool backup=caller==image_base+0x548390b && equal_path(source,record->published) && equal_path(destination,record->backup);
        const bool restore=caller==image_base+0x54839f9 && equal_path(source,record->backup) && equal_path(destination,record->published);
        if (!publication && !backup && !restore) throw std::runtime_error("native_move");
        expected=std::make_unique<Snapshot>(snapshot(source));
        if (publication && (!record->closed_world || expected->digest!=record->closed_world->digest
            || expected->size!=record->closed_world->size || expected->index!=record->closed_world->index
            || expected->volume!=record->closed_world->volume)) throw std::runtime_error("native_publication");
    } catch (...) { unknown(record); }
    error.restore(); const auto result=original_move(manager,to,from,replace,readonly,attributes,no_retry);
    error.capture();
    if (record) {
        const bool readback=expected && matches(*expected,destination,true);
        Lock lock(record->lock);
        if (publication) record->completion.publish(result,readback);
        else if (!result) record->completion.fail();
        else if (!readback) record->completion.unknown(Issue::identity_changed);
    }
    return result;
}
BOOL WINAPI hook_os_close(HANDLE file) {
    const auto record=scope_record; const auto caller=reinterpret_cast<std::uintptr_t>(_ReturnAddress());
    std::size_t selected=SIZE_MAX;
    if (record) {
        Lock lock(record->lock);
        for (std::size_t index=0;index<record->writers.size();++index)
            if (!record->writers[index].closed && record->writers[index].native_handle==reinterpret_cast<std::uintptr_t>(file)) {
                if (selected!=SIZE_MAX) record->completion.unknown();
                selected=index;
            }
    }
    const auto result=original_os_close(file); const auto error=GetLastError();
    if (record && selected!=SIZE_MAX) {
        Lock lock(record->lock); auto& writer=record->writers[selected];
        if (caller!=image_base+0x142dc33 || writer.os_closed) record->completion.unknown();
        if (!result) record->completion.fail();
        else writer.os_closed=true;
    }
    SetLastError(error); return result;
}
constexpr unsigned char save_rbx_8[]={2,5,2,0,5,0x34,1,0};
constexpr unsigned char save_rbx_32[]={2,5,2,0,5,0x34,4,0};
constexpr unsigned char save_rbp_24[]={2,5,2,0,5,0x54,3,0};
constexpr unsigned char reader_close_unwind[]={2,6,2,0,6,0x32,2,0x30};
}
std::vector<Hook> sink_hooks() {
    return {
        {0x12ad020,5,"48895c2420",reinterpret_cast<void*>(&hook_create),reinterpret_cast<void**>(&original_create),save_rbx_32},
        {0x12ac5c0,5,"48895c2408",reinterpret_cast<void*>(&hook_close),reinterpret_cast<void**>(&original_close),save_rbx_8},
        {0x12ac8e0,5,"48895c2408",reinterpret_cast<void*>(&hook_copy),reinterpret_cast<void**>(&original_copy),save_rbx_8},
        {0x12b0420,5,"48895c2408",reinterpret_cast<void*>(&hook_move),reinterpret_cast<void**>(&original_move),save_rbx_8}
        ,{0x12acea0,5,"48896c2418",reinterpret_cast<void*>(&hook_reader),reinterpret_cast<void**>(&original_reader),save_rbp_24}
        ,{0x12ac590,6,"40534883ec20",reinterpret_cast<void*>(&hook_reader_close),reinterpret_cast<void**>(&original_reader_close),reader_close_unwind}
    };
}
bool file_imports_match() noexcept {
    void* current{};
    return patched_close && read(image_base+0x8885370,current) && current==reinterpret_cast<void*>(&hook_os_close);
}
bool install_file_imports() noexcept {
    if (patched_close || !image_base) return false;
    auto* slot=reinterpret_cast<void**>(image_base+0x8885370);
    const auto expected=reinterpret_cast<void*>(GetProcAddress(GetModuleHandleW(L"kernel32.dll"),"CloseHandle"));
    void* current{}; if (!expected || !read(reinterpret_cast<std::uintptr_t>(slot),current) || current!=expected) return false;
    original_os_close=reinterpret_cast<OsClose>(expected); DWORD previous{};
    if (!VirtualProtect(slot,sizeof(void*),PAGE_READWRITE,&previous)) return false;
    const bool changed=InterlockedCompareExchangePointer(slot,reinterpret_cast<void*>(&hook_os_close),expected)==expected;
    DWORD ignored{}; const bool restored=VirtualProtect(slot,sizeof(void*),previous,&ignored)!=FALSE;
    patched_close=changed;
    return changed && restored && file_imports_match();
}
#ifdef LGSM_DRAGON_SAVE_TESTS
void* test_os_close(void* original) noexcept {
    original_os_close=reinterpret_cast<OsClose>(original); return reinterpret_cast<void*>(&hook_os_close);
}
#endif
}
