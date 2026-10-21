// Fixed-image prologues; synthetic unwind describes only copied instructions.
#include "native_state.hpp"
namespace scum_save {
Hook native_hook(std::uint32_t rva,void* replacement,void** original) {
    switch(rva) {
    case 0x1b94040:return {rva,replacement,original,"48895c2420",5,{}};
    case 0x1ba7060:return {rva,replacement,original,"48895c2418",5,{}};
    case 0x1b94ee0:return {rva,replacement,original,"4c8bdc554154",6,{2,6,2,0,6,192,4,80}};
    case 0x1b94250:return {rva,replacement,original,"4c8bdc49895b18",7,{}};
    case 0x1abced0:return {rva,replacement,original,"48895c2418",5,{}};
    case 0x1ba6690:return {rva,replacement,original,"48895c2418",5,{}};
    case 0x1e2c5b0:return {rva,replacement,original,"48895c2418",5,{}};
    case 0x1b976c0:return {rva,replacement,original,"48895c2410",5,{}};
    case 0x1b979f0:return {rva,replacement,original,"48895c2410",5,{}};
    case 0x1959490:return {rva,replacement,original,"48895c2410",5,{}};
    case 0x1b98080:return {rva,replacement,original,"40534883ec20",6,{2,6,2,0,6,50,2,48}};
    case 0x1b98210:return {rva,replacement,original,"48895c2418",5,{}};
    case 0x1ba6380:return {rva,replacement,original,"48895c2410",5,{}};
    case 0x1ad3aa0:return {rva,replacement,original,"4053574155",5,{2,5,3,0,5,208,3,112,2,48}};
    case 0x1acea90:return {rva,replacement,original,"40554155488dac24d8eaffff",12,{2,12,2,0,4,208,2,80}};
    case 0x1ab9df0:return {rva,replacement,original,"48895c2410",5,{}};
    case 0x1ab5d90:return {rva,replacement,original,"48895c2408",5,{}};
    case 0x1acc820:return {rva,replacement,original,"405553574157",6,{2,6,4,0,6,240,4,112,3,48,2,80}};
    case 0x296ab40:return {rva,replacement,original,"405556574883ec30",8,{2,8,4,0,8,82,4,112,3,96,2,80}};
    case 0x28eeec0:return {rva,replacement,original,"405641564157",6,{2,6,3,0,6,240,4,224,2,96}};
    case 0x28ea340:return {rva,replacement,original,"4c894c2420",5,{}};
    case 0x2956c20:return {rva,replacement,original,"44894c2420",5,{}};
    case 0x293ba90:return {rva,replacement,original,"48897c2420",5,{}};
    case 0x28ee0c0:return {rva,replacement,original,"48895c2408",5,{}};
    case 0x28ee190:return {rva,replacement,original,"48895c2408",5,{}};
    case 0x28ee320:return {rva,replacement,original,"48895c2408",5,{}};
    case 0x28ee3a0:return {rva,replacement,original,"48895c2408",5,{}};
    case 0x28ee430:return {rva,replacement,original,"48895c2408",5,{}};
    case 0x28ee4a0:return {rva,replacement,original,"4883ec38488b442460",9,{2,9,1,0,4,98}};
    case 0x28ee5b0:return {rva,replacement,original,"4883ec38488b442460",9,{2,9,1,0,4,98}};
    case 0x28ee5d0:return {rva,replacement,original,"0fb6442430",5,{}};
    case 0x28ee5f0:return {rva,replacement,original,"48895c2420",5,{}};
    case 0x28ee7e0:return {rva,replacement,original,"48895c2408",5,{}};
    case 0x28ee990:return {rva,replacement,original,"48895c2408",5,{}};
    case 0x28eea30:return {rva,replacement,original,"48895c2408",5,{}};
    case 0x28eebd0:return {rva,replacement,original,"40554883ec30",6,{2,6,2,0,6,82,2,80}};
    case 0x28f0130:return {rva,replacement,original,"48895c2410",5,{}};
    case 0x28f0230:return {rva,replacement,original,"48895c2410",5,{}};
    default:throw std::runtime_error("native_hook_profile");
    }
}
}
