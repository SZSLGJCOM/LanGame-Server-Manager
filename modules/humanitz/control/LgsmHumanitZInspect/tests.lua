local production = assert(LGSM_INSPECT_TEST_SOURCE)
local tests = 0
local function check(value) tests=tests+1; assert(value) end
local run = string.rep('a',32)
local base='E:/physical-fixture\\langame_humanitz_inspect\\'
local function fixture(configured)
    local state={files={},queue={},scans=0}
    local parent={GetFullName=function()return 'BlueprintGeneratedClass /Game/Server/BP_Server.BP_Server_C' end}
    local fn={IsValid=function()return true end,IsA=function()return true end,
        GetFullName=function()return 'Function /Game/Server/BP_Server.BP_Server_C.BPDataReceived' end,
        GetAddress=function()return 140123456789 end,GetFunctionFlags=function()return 0x800 end,
        GetOuter=function()return parent end,ForEachProperty=function(_,callback)
            callback({GetFullName=function()return 'StrProperty BPDataReceived.Data' end}) end}
    local class={IsValid=function()return true end}
    local ioapi={open=function(path,mode)
        if mode=='rb' and not state.files[path] then return nil end
        return {read=function(_,n)return state.files[path]:sub(1,n)end,
            write=function(_,value)state.files[path]=value;return true end,close=function()return true end}
    end}
    local environment=setmetatable({io=ioapi,os={getenv=function()return configured end,
        remove=function(path)if state.files[path] then state.files[path]=nil;return true end return nil,'absent',2 end,
        rename=function(from,to)state.files[to]=state.files[from];state.files[from]=nil;return true end},
        print=function()end,StaticFindObject=function()return class end,
        LoopAsync=function(_,callback)state.tick=callback end,
        ExecuteInGameThread=function(callback)state.queue[#state.queue+1]=callback end,
        ForEachUObject=function(callback)state.scans=state.scans+1;callback(fn)end}, {__index=_G})
    assert(load(production,'@E:/physical-fixture/HumanitZServer/Binaries/Win64/ue4ss/Mods/LgsmHumanitZInspect/Scripts/main.lua','t',environment))()
    function state.request(nonce,other)
        state.files[base..'request.txt']='schema=1\nrun='..(other or run)..'\nnonce='..nonce..'\n';state.tick()
    end
    function state.drain()for _,callback in ipairs(state.queue)do callback()end;state.queue={}end
    return state
end
local state=fixture(run);local id=string.rep('b',32)
state.request(id);check(#state.queue==1);state.tick();check(#state.queue==1);state.drain()
local response=assert(state.files[base..'reflection.json']);check(response:find('"complete":true',1,true));check(response:find('"nonce":"'..id..'"',1,true));check(response:find('BPDataReceived',1,true));check(state.scans==1)
state.request(id);state.drain();check(state.scans==1)
state.request(string.rep('c',32),string.rep('d',32));state.drain();check(state.scans==1)
state.files[base..'request.txt']='schema=1\nrun='..run..'\nnonce='..string.rep('c',32)..'\nextra=x\n';state.tick();state.drain();check(state.scans==1)
for i=1,8 do state.request(string.format('%032x',i));state.drain()end
check(state.scans==8)
check(fixture('').tick==nil)
print('HumanitZ read-only reflection fixture passed: '..tests)
return tests
