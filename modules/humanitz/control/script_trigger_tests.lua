local source=assert(LGSM_INSPECT_TEST_SOURCE)
local checks=0
local function check(value)checks=checks+1;assert(value)end
local run=string.rep('a',32)
local function fixture()
 local state={files={},queue={},calls=0}
 local environment=setmetatable({os={getenv=function()return run end},io={open=function(path)
  if not state.files[path]then return nil end
  return{read=function(_,size)return state.files[path]:sub(1,size)end,close=function()return true end}
 end},print=function()end,LoopAsync=function(_,callback)state.tick=callback end,
 ExecuteInGameThread=function(callback)state.queue[#state.queue+1]=callback end,
 package={loadlib=function(path,name)check(path:find('LgsmHumanitZInspect',1,true)~=nil and name=='luaopen_lgsm_humanitz_inspect');return function()state.calls=state.calls+1 end end}}, {__index=_G})
 assert(load(source,'@E:/physical/HumanitZServer/Binaries/Win64/ue4ss/Mods/LgsmHumanitZScriptInspect/Scripts/main.lua','t',environment))()
 function state.request(nonce,other)
  state.files['E:/physical\\langame_humanitz_inspect\\scripts-request.txt']='schema=1\nrun='..(other or run)..'\nnonce='..nonce..'\nfunction=140123456789\n';state.tick()
 end
 function state.drain()for _,callback in ipairs(state.queue)do callback()end;state.queue={}end
 return state
end
local state=fixture();local id=string.rep('b',32)
state.request(id);check(state.calls==0 and #state.queue==1);state.tick();check(#state.queue==1)
state.drain();check(state.calls==1);state.request(id);state.drain();check(state.calls==1)
state.request(string.rep('c',32),string.rep('d',32));state.drain();check(state.calls==1)
for i=1,8 do state.request(string.format('%032x',i));state.drain()end
check(state.calls==8)
print('HumanitZ read-only Script trigger fixture passed: '..checks)
return checks
