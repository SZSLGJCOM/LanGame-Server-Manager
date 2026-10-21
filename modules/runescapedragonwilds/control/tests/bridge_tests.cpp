// The actual closed parser and receipt encoder; external game dispatch is not
// replaced or exercised by these protocol boundary checks.
#include "../save_bridge.cpp"
#include <cstdio>
#include <cstdlib>
using namespace dragon_save;
namespace {
unsigned checks{};
void check(bool value,const char* name) { ++checks; if (!value) { std::fprintf(stderr,"FAIL %s\n",name); std::exit(1); } }
bool accepts(const std::string& text) { try { (void)dragon_save::parse(text); return true; } catch (...) { return false; } }
}
int main() {
    const std::string valid="schema=1\nrun="+std::string(32,'a')+"\nnonce="+std::string(32,'b')
        +"\nop=save\npersistence=123\nspud=456\ninstance=789\nworld=1000\n";
    check(accepts(valid),"closed Save accepted");
    for (const auto operation : {"query","probe"}) {
        auto value=valid; value.replace(value.find("op=save"),7,std::string("op=")+operation);
        check(accepts(value),"read-only operation accepted");
    }
    for (const auto suffix : {"\n","world=2\n","path=C:/other\n","\r\n"}) check(!accepts(valid+suffix),"extra field/suffix refused");
    for (const auto replacement : {"op=Save","op=save query","op=delete","op=save\r"}) {
        auto value=valid; value.replace(value.find("op=save"),7,replacement); check(!accepts(value),"unknown/non-ASCII operation refused");
    }
    for (const auto numeric : {"0","01","+1","-1","18446744073709551616","1e3","1/2"," 1"}) {
        auto value=valid; value.replace(value.find("world=1000"),10,std::string("world=")+numeric); check(!accepts(value),"invalid address refused");
    }
    for (const auto malformed : {std::string(31,'a'),std::string(33,'a'),std::string(32,'A'),std::string(31,'a')+"z"}) {
        auto value=valid; value.replace(value.find("run=")+4,32,malformed); check(!accepts(value),"invalid startup token refused");
        value=valid; value.replace(value.find("nonce=")+6,32,malformed); check(!accepts(value),"invalid operation nonce refused");
    }
    check(!accepts(std::string(1025,'a')),"oversized frame refused");
    { auto value=valid; value.insert(value.find("spud="),"persistence=124\n"); check(!accepts(value),"duplicate field refused"); }
    { auto value=valid; value[value.find("123")]='\0'; check(!accepts(value),"embedded nul refused"); }
    { auto value=valid; value[value.find("123")]=static_cast<char>(0x80); check(!accepts(value),"non ASCII refused"); }
    { auto value=valid; value.replace(0,8,"schema=2"); check(!accepts(value),"unknown version refused"); }
    const auto request=dragon_save::parse(valid);
    dragon_save::run=std::string(32,'a');
    const auto receipt=dragon_save::encode(request,"completed","none",true,true);
    check(receipt.find("\nnonce="+std::string(32,'b')+"\nop=save\npid=")!=receipt.npos,"receipt binds operation/process");
    check(receipt.find("\ncreation_time=")!=receipt.npos && receipt.ends_with("\nstatus=completed\ninvoked=true\nreturned=true\nissue=none\n"),"receipt exact completion state");
    std::printf("PASS %u Dragonwilds production frame/receipt checks\n",checks);
}
