#include "save_native.hpp"
#include <charconv>
#include <string_view>
#include <stdexcept>
bool lgsm_dragon_control_image(std::uintptr_t&,std::filesystem::path&,bool (*&)()) noexcept;
namespace dragon_save {
namespace {
constexpr std::size_t maximum=1024;
struct Request { std::string run,nonce,operation; Target target; bool operator==(const Request&) const = default; };
struct Receipt { Request request; std::shared_ptr<Record> record; std::string rejected; };
std::array<std::unique_ptr<Receipt>,512> receipts;
std::size_t next{};
std::string run;
fs::path directory;
bool initialized{},available{};
bool token(std::string_view text) noexcept { return text.size()==32 && text.find_first_not_of("0123456789abcdef")==text.npos; }
void need(bool value) { if (!value) throw std::runtime_error("native_frame"); }
std::uintptr_t decimal(std::string_view text) {
    need(!text.empty() && text.size()<=20 && (text.size()==1 || text.front()!='0'));
    std::uintptr_t value{}; const auto [end,error]=std::from_chars(text.data(),text.data()+text.size(),value);
    need(error==std::errc{} && end==text.data()+text.size() && value); return value;
}
Request parse(std::string_view text) {
    constexpr std::string_view keys[]={"schema=","run=","nonce=","op=","persistence=","spud=","instance=","world="};
    std::array<std::string_view,8> fields;
    need(!text.empty() && text.size()<=maximum);
    for (char value : text) need((value>=32 && value<=126) || value=='\n');
    for (std::size_t index=0;index<fields.size();++index) {
        const auto end=text.find('\n'); need(end!=text.npos && text.starts_with(keys[index]));
        fields[index]=text.substr(keys[index].size(),end-keys[index].size()); text.remove_prefix(end+1);
    }
    need(text.empty() && fields[0]=="1" && token(fields[1]) && token(fields[2])
        && (fields[3]=="probe" || fields[3]=="save" || fields[3]=="query"));
    return {std::string(fields[1]),std::string(fields[2]),std::string(fields[3]),
        {decimal(fields[4]),decimal(fields[5]),decimal(fields[6]),decimal(fields[7])}};
}
Handle regular(const fs::path& path,DWORD access,DWORD disposition,DWORD sharing) {
    Handle result(CreateFileW(path.c_str(),access,sharing,nullptr,disposition,FILE_FLAG_OPEN_REPARSE_POINT,nullptr));
    BY_HANDLE_FILE_INFORMATION info{};
    need(result.value!=INVALID_HANDLE_VALUE && GetFileInformationByHandle(result.value,&info)
        && !(info.dwFileAttributes&(FILE_ATTRIBUTE_DIRECTORY|FILE_ATTRIBUTE_REPARSE_POINT)) && info.nNumberOfLinks==1);
    return result;
}
std::string read_request(const fs::path& path) {
    auto file=regular(path,GENERIC_READ,OPEN_EXISTING,FILE_SHARE_READ); LARGE_INTEGER size{};
    need(GetFileSizeEx(file.value,&size) && size.QuadPart>0 && size.QuadPart<=maximum);
    std::array<char,maximum+1> bytes{}; DWORD count{};
    need(ReadFile(file.value,bytes.data(),static_cast<DWORD>(bytes.size()),&count,nullptr) && count==size.QuadPart);
    return std::string(bytes.data(),count);
}
std::uint64_t creation() noexcept {
    FILETIME begin{},end{},kernel{},user{};
    return GetProcessTimes(GetCurrentProcess(),&begin,&end,&kernel,&user)
        ? static_cast<std::uint64_t>(begin.dwHighDateTime)<<32|begin.dwLowDateTime : 0;
}
const char* issue(Issue value) noexcept {
    switch (value) {
    case Issue::none:return "none";
    case Issue::native_failed:return "native_write_failed";
    case Issue::identity_changed:return "native_target_changed";
    case Issue::timed_out:return "native_completion_timeout";
    default:return "native_closure_unavailable";
    }
}
std::string encode(const Request& request,const char* status,const char* error,bool invoked,bool returned) {
    return "schema=1\nrun="+run+"\nnonce="+request.nonce+"\nop="+request.operation+"\npid="+std::to_string(GetCurrentProcessId())
        +"\ncreation_time="+std::to_string(creation())+"\nstatus="+status+"\ninvoked="+(invoked ? "true" : "false")
        +"\nreturned="+(returned ? "true" : "false")+"\nissue="+error+"\n";
}
std::string observed(const Request& request,Receipt& receipt) {
    if (!receipt.record) return encode(request,"rejected",receipt.rejected.c_str(),false,false);
    refresh(receipt.record); Lock lock(receipt.record->lock); const auto& state=receipt.record->completion;
    const char* status="unknown";
    switch (state.status) {
    case Status::dispatching:status="dispatching";break;
    case Status::completed:status="completed";break;
    case Status::failed:status="failed";break;
    case Status::unknown:break;
    }
    return encode(request,status,issue(state.issue),state.entered,state.root_returned);
}
std::string execute(const Request& request) {
    reset_identity_diagnostic();
    if (request.run!=run) return encode(request,"rejected","native_run_changed",false,false);
    if (!available) return encode(request,"rejected","native_profile_unavailable",false,false);
    if (request.operation=="probe") {
        const auto* error=probe_target(request.target);
        return encode(request,error ? "rejected" : "ready",error ? error : "none",false,false);
    }
    for (const auto& receipt : receipts) if (receipt && receipt->request.nonce==request.nonce) {
        auto expected=receipt->request; expected.operation=request.operation;
        if (expected!=request) return encode(request,"rejected","native_nonce_conflict",false,false);
        return observed(request,*receipt);
    }
    if (request.operation=="query") return encode(request,"unknown","native_request_unknown",true,false);
    if (next>=receipts.size()) return encode(request,"rejected","native_request_capacity",false,false);
    // Cache before dispatch. An exception, receipt-write failure or later Query
    // cannot invoke a second root call for this process/nonce.
    auto receipt=std::make_unique<Receipt>(); receipt->request=request;
    const char* rejection{}; receipt->record=admit(request.target,request.nonce,rejection);
    if (!receipt->record) receipt->rejected=rejection ? rejection : "native_target_unavailable";
    auto* active=receipt.get(); receipts[next++]=std::move(receipt);
    if (active->record) try { dispatch(active->record); } catch (...) {
        Lock lock(active->record->lock); active->record->completion.unknown();
    }
    return observed(request,*active);
}
void publish_identity_diagnostic() noexcept {
    const auto error=GetLastError();
    try {
        const auto value=identity_diagnostic();
        const std::string body=std::string("schema=1\nstage=")+value.stage+"\nrole="+value.role+"\nreason="+value.reason+"\n";
        const auto path=directory/L"native-diagnostic.txt"; const auto pins=pin_parents(path);
        auto file=regular(path,GENERIC_WRITE,OPEN_ALWAYS,0); DWORD count{};
        need(SetEndOfFile(file.value) && WriteFile(file.value,body.data(),static_cast<DWORD>(body.size()),&count,nullptr)
            && count==body.size() && FlushFileBuffers(file.value));
    } catch (...) { OutputDebugStringA("[LgsmPlayerControl] Optional identity diagnostic unavailable.\n"); }
    SetLastError(error);
}
void invoke() {
    std::uintptr_t image{}; bool (*thread)(){}; fs::path verified;
    need(lgsm_dragon_control_image(image,verified,thread));
    if (!initialized) {
        initialized=true; directory=verified;
        std::array<char,34> configured{}; const auto count=GetEnvironmentVariableA("LGSM_DRAGON_SAVE_RUN",configured.data(),static_cast<DWORD>(configured.size()));
        if (count==32 && token(std::string_view(configured.data(),count))) {
            run.assign(configured.data(),count);
            const auto saved=directory.parent_path()/L"RSDragonwilds/Saved";
            available=initialize_native(image,saved,thread);
        }
    }
    need(!run.empty() && equal_path(directory,verified));
    const auto request_file=directory/L"native-save-request.txt",response_file=directory/L"native-save-response.txt";
    const auto pins=pin_parents(request_file); const auto request=parse(read_request(request_file));
    const auto response=execute(request);
    publish_identity_diagnostic();
    auto file=regular(response_file,GENERIC_WRITE,OPEN_ALWAYS,0); DWORD count{};
    need(SetEndOfFile(file.value) && WriteFile(file.value,response.data(),static_cast<DWORD>(response.size()),&count,nullptr)
        && count==response.size() && FlushFileBuffers(file.value));
}
}
}
extern "C" __declspec(dllexport) int luaopen_lgsm_dragonwilds_save(void*) noexcept {
    static SRWLOCK lock=SRWLOCK_INIT;
    if (!TryAcquireSRWLockExclusive(&lock)) return 0;
    try { dragon_save::invoke(); }
    catch (...) { OutputDebugStringA("[LgsmPlayerControl] Save request/receipt rejected.\n"); }
    ReleaseSRWLockExclusive(&lock); return 0;
}
