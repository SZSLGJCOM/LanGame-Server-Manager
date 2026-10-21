local source = assert(LGSM_TEST_SOURCE)
local hostio = io
local count = 0
local function check(value) count=count+1;assert(value) end
local run, id = string.rep("a",32),string.rep("b",32)
local directory = "E:/physical-fixture\\langame_humanitz_inspect\\"
local function fixture(config)
    config=config or {}
    local s={files={},queue={},main=false,scans=0,reads=0,logs={}}
    local function read()s.reads=s.reads+1;assert(s.main,"metadata read outside main thread")end
    local owner_name="BlueprintGeneratedClass /Game/TSS_Game/KaiAIMapTest/RCON/BP_HMZRCon.BP_HMZRCon_C"
    local property={GetFullName=function()read();return "IntProperty "..owner_name..":Local" end,
        GetAddress=function()read();if config.descriptor_error then error("SECRET/private-object-path") end;return 140111111111 end,
        GetOffset_Internal=function()read();return config.bad_offset and -1 or 48 end}
    local owner={IsValid=function()read();return true end,GetFullName=function()read();return owner_name end,
        GetAddress=function()read();return 140222222222 end,ForEachProperty=function(_,fn)fn(property)end}
    local plain_owner={IsValid=owner.IsValid,GetFullName=owner.GetFullName,GetAddress=owner.GetAddress}
    owner.IsAnyClass=function()read();return true end
    local function fn(native)
        local result={IsValid=function()read();return true end,IsA=function()read();return true end,
            GetFullName=function()read();return native and "Function /Script/Engine.GameplayStatics:SaveGameToSlot" or "Function /Game/TSS_Game/KaiAIMapTest/RCON/BP_HMZRCon.BP_HMZRCon_C:BPDataReceived" end,
            GetAddress=function()read();return native and 140444444444 or 140333333333 end,
            GetFName=function()read();return {GetComparisonIndex=function()read();return config.bad_index and -1 or (native and 43 or 42) end}end,
            GetFunctionFlags=function()read();return native and 0x400 or 0x800 end,
            GetOuter=function()read();return native and {IsValid=function()read();return true end,GetFullName=function()read();return "Class /Script/Engine.GameplayStatics" end} or plain_owner end,
            ForEachProperty=function(_,callback)for _=1,(config.properties or 130)do if callback(property) then break end end end}
        if config.missing_fname then result.GetFName=nil end
        return result
    end
    local instance={IsValid=function()read();return true end,IsA=function()read();return false end,
        GetAddress=function()read();return 140555555555 end,GetClass=function()read();return owner end,
        GetOuter=function()read();return nil end,GetFullName=function()read();return "Object secret-instance-name" end}
    local ioapi={open=function(path,mode)
        if config.output_error and mode=="wb" and path:find("\\names-",1,true) then return nil,"SECRET/output-path" end
        if mode=="rb" and not s.files[path]then return nil end
        return {read=function(_,n)return s.files[path]:sub(1,n)end,
            write=function(_,body)s.files[path]=body;return true end,close=function()return true end}
    end}
    local environment=setmetatable({io=ioapi,os={getenv=function()return config.run==nil and run or config.run end,
        rename=function(from,to)if s.files[to]then return nil end;s.files[to]=s.files[from];s.files[from]=nil;return true end},
        print=function(value)s.logs[#s.logs+1]=value end,StaticFindObject=function(path)read();
            if path=="/Script/CoreUObject.Function" then if config.no_function_class then return nil end;return {IsValid=function()read();return true end} end
            if path=="/Game/TSS_Game/KaiAIMapTest/RCON/BP_HMZRCon.BP_HMZRCon_C" then if config.no_owner_class then return nil end;return owner end
            error("unexpected object lookup")
        end,
        ForEachUObject=function(callback)read();if config.enumeration_error then error({secret="SECRET"})end;s.scans=s.scans+1;callback(fn(false));callback(fn(true));callback(instance)end,
        IsInGameThread=function()return s.main end,LoopAsync=function(_,callback)s.tick=callback end,
        ExecuteInGameThread=function(callback)s.queue[#s.queue+1]=callback end,
        SaveGameToSlot=function()error("forbidden save call")end,
        RegisterHook=function()error("forbidden hook")end,FName=function()error("forbidden FName construction")end}, {__index=_G})
    if config.no_thread_api then environment.IsInGameThread=false end
    assert(load(source,"@E:/physical-fixture/HumanitZServer/Binaries/Win64/ue4ss/Mods/LgsmHumanitZNameInspect/Scripts/main.lua","t",environment))()
    function s.request(nonce,launch)s.files[directory.."names-request.txt"]="schema=1\nrun="..(launch or run).."\nnonce="..nonce.."\n";s.tick()end
    function s.drain()s.main=true;for _,callback in ipairs(s.queue)do callback()end;s.main=false;s.queue={}end
    function s.response(nonce)return s.files[directory.."names-"..nonce..".json"]end
    return s
end
local s=fixture();s.request(id);check(s.reads==0);check(#s.queue==1);s.tick();check(#s.queue==1);s.drain()
local body=assert(s.response(id));check(body:find('"stage":"name_metadata_only"',1,true));check(body:find('"complete":true',1,true))
check(body:find('"comparison_index":42',1,true));check(body:find('"comparison_index":43',1,true));check(body:find('"property_count":131',1,true))
check(body:find('"offset":48',1,true));check(body:find('"classes":[{',1,true));check(not body:find('secret-instance-name',1,true))
s.request(id);s.drain();check(s.scans==1)
s.request(string.rep('c',32),string.rep('d',32));s.drain();check(s.scans==1)
s.files[directory.."names-request.txt"]="schema=1\nrun="..run.."\nnonce="..string.rep('c',32).."\nextra=x\n";s.tick();s.drain();check(s.scans==1)
local other=string.rep('c',32);s.request(other);s.drain();check(s.response(id)==body);check(s.response(other):find('"nonce":"'..other..'"',1,true))
local overflow=fixture({properties=8193});overflow.request(id);overflow.drain();check(overflow.response(id):find('"complete":false',1,true))
for i=1,8 do s.request(string.format('%032x',i));s.drain()end;check(s.scans==8)
check(fixture({run=""}).tick==nil)
local f=assert(hostio.open("E:/LanGameTemp/runtime-scratch/humanitz-save-completion-20261020/name-fixture-response.json","wb"));assert(f:write(body));assert(f:close())
local generic=fixture({generic_outer=true});generic.request(id);generic.drain()
local generic_error=generic.files[directory.."name-error-"..id..".json"]
if generic_error then
    local e=assert(hostio.open("E:/LanGameTemp/runtime-scratch/humanitz-save-completion-20261020/name-generic-outer-red.json","wb"));assert(e:write(generic_error));assert(e:close())
end
check(generic.response(id)~=nil)
for _,case in ipairs({
    {{no_thread_api=true},"game_thread","api_missing","string"},
    {{no_function_class=true},"function_class","function_class_unavailable","string"},
    {{missing_fname=true},"get_fname","call_error","string"},
    {{bad_index=true},"comparison_index","name_index_unavailable","string"},
    {{descriptor_error=true},"field_descriptor","api_error","string"},
    {{bad_offset=true},"field_offset","property_offset_unavailable","string"},
    {{enumeration_error=true},"object_enumeration","api_error","table"},
    {{no_owner_class=true},"owner_class_lookup","owner_class_unavailable","string"},
    {{output_error=true},"output_open","io_failed","string"}
}) do
    local failed=fixture(case[1]);failed.request(id);failed.drain()
    local error_body=assert(failed.files[directory.."name-error-"..id..".json"])
    check(failed.response(id)==nil)
    check(error_body:find('"failure_stage":"'..case[2]..'"',1,true))
    check(error_body:find('"issue":"'..case[3]..'"',1,true))
    check(error_body:find('"error_kind":"'..case[4]..'"',1,true))
    check(not error_body:find("SECRET",1,true) and not table.concat(failed.logs):find("SECRET",1,true))
end
print("HumanitZ read-only name metadata fixture passed: "..count)
return count
